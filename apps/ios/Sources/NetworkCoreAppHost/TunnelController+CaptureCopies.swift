import Foundation
import NetworkCoreBridge

/// Capture that outlives the tunnel. Capture lives in the tunnel's engine and its saved sessions in the tunnel's own
/// container, sealed with a key the app never sees; so while connected the app keeps copies sealed with its own key
/// (`CaptureViewer`): the live capture every few seconds while it changes, under the run's session id, and every
/// session the tunnel saved. When the tunnel goes down, the request and connection lists switch to the app's copy of
/// the run, and the tunnel's own seal of it (`saveCapture(run: true)` in `stopTunnel`) replaces that copy at the next
/// connect. A disconnect from the app takes one last copy first, so nothing caught before it is missing.
extension TunnelController {
    /// How often the live capture is copied while connected.
    static let captureCopyInterval: Duration = .seconds(5)
    /// The longest a disconnect from the app waits for the last copy.
    static let captureCopyBeforeStopLimit: Duration = .seconds(3)
    /// Sessions deleted in the app, so the next sync does not copy them back from the tunnel: id → deleted at.
    private static let deletedCaptureKey = "networkcore.capture.deleted-sessions"

    /// The commands the request and connection lists read capture with; they go to the capture viewer while a saved
    /// session is shown.
    static func readsCapture(_ command: RuntimeControl) -> Bool {
        switch command {
        case .captureStatus, .captureRequests, .captureConnection, .captureExchange, .captureH2Frames, .exportCapture,
             .exportRequest, .connections:
            return true
        default:
            return false
        }
    }

    /// The name of the run's session: the tunnel names its seal on stop the same way.
    static var captureRunName: String {
        String(localized: "断开时自动保存")
    }

    // MARK: - What the lists show

    /// Shows saved session `session` in the request and connection lists, connected or not.
    func openSavedCapture(_ session: PacketCapture.Session) async throws {
        try await CaptureViewer.shared.reload(session.id)
        viewedCapture = session
        viewedOnStop = false
    }

    /// Back to the live capture of the running tunnel.
    func showLiveCapture() {
        viewedCapture = nil
        viewedOnStop = false
    }

    /// The app's copies, newest first; empty when the viewer has none or cannot open its folder.
    func savedCaptures() async -> [PacketCapture.Session] {
        (try? await CaptureViewer.shared.sessions()) ?? []
    }

    /// Deletes a saved session from the app and, when connected, from the tunnel; a later sync does not bring it back.
    func deleteSavedCapture(_ session: PacketCapture.Session) async throws {
        _ = try await CaptureViewer.shared.send(.deleteCapture(session.id))
        rememberDeleted(session.id)
        if connection == .connected {
            _ = await tunnelControl(.deleteCapture(session.id))
        }
        if viewedCapture?.id == session.id {
            showLiveCapture()
        }
    }

    /// Seals what the tunnel captures now as a new session and keeps a copy: `nil` when it could not.
    func saveLiveCapture() async -> PacketCapture.Session? {
        guard let saved = RuntimeControl.decode(
            PacketCapture.SavedAnswer.self, from: await tunnelControl(.saveCapture(name: nil))
        )?.session else {
            return nil
        }
        _ = await copySession(saved.id)
        return saved
    }

    // MARK: - Copies while connected

    /// The tunnel is up: the lists go back to live capture if the app had switched them at the stop, sessions the
    /// tunnel saved meanwhile are copied, and the live capture is copied from now on.
    func captureConnected() {
        if viewedOnStop {
            showLiveCapture()
        }
        captureRun = nil
        copiedCapture = nil
        captureMirror?.cancel()
        #if DEBUG
        if Self.isDemo {
            return
        }
        #endif
        captureMirror = Task { [weak self] in
            await self?.syncSavedCaptures()
            while !Task.isCancelled {
                await self?.copyLiveCapture()
                try? await Task.sleep(for: Self.captureCopyInterval)
            }
        }
    }

    /// The tunnel went down: copying stops, and the lists show the app's copy of the run, if there is one and the
    /// user is not looking at another saved session.
    func captureStopped() {
        captureMirror?.cancel()
        captureMirror = nil
        guard let run = captureRun, copiedCapture != nil, viewedCapture == nil else {
            return
        }
        Task { [weak self] in
            guard let self,
                  let session = await self.savedCaptures().first(where: { $0.id == run }),
                  self.viewedCapture == nil, self.connection != .connected
            else {
                return
            }
            try? await self.openSavedCapture(session)
            self.viewedOnStop = true
        }
    }

    /// A disconnect from the app: one last copy of the live capture, waiting at most `captureCopyBeforeStopLimit`.
    func copyLiveCaptureBeforeStop() async {
        guard connection == .connected else {
            return
        }
        captureMirror?.cancel()
        captureMirror = nil
        await withTaskGroup(of: Void.self) { group in
            group.addTask { [weak self] in
                await self?.copyLiveCapture()
            }
            group.addTask {
                try? await Task.sleep(for: Self.captureCopyBeforeStopLimit)
            }
            _ = await group.next()
            group.cancelAll()
        }
    }

    /// Copies what the tunnel captures now into the app, under the run's session id, unless nothing changed since the
    /// last copy. `true` when the app's copy is up to date.
    @discardableResult
    func copyLiveCapture() async -> Bool {
        guard connection == .connected,
              let status = RuntimeControl.decode(PacketCapture.Status.self, from: await tunnelControl(.captureStatus)),
              let run = status.runSession
        else {
            return false
        }
        captureRun = run
        guard status.connections > 0 else {
            return copiedCapture != nil
        }
        if status == copiedCapture {
            return true
        }
        guard let key = try? await CaptureViewer.shared.keyBase64(),
              let copy = RuntimeControl.decode(
                  PacketCapture.CopyAnswer.self,
                  from: await tunnelControl(.copyCapture(session: nil, keyBase64: key, name: Self.captureRunName))
              ),
              copy.session == run,
              (try? await CaptureViewer.shared.importCopy(copy)) != nil
        else {
            return false
        }
        copiedCapture = status
        if viewedCapture?.id == run {
            // The run being looked at (a reconnect the app did not cause) shows the newer copy.
            viewedCapture = await savedCaptures().first { $0.id == run } ?? viewedCapture
        }
        return true
    }

    /// Copies each session the tunnel saved that the app has no copy of, or only an older one of (the run the
    /// tunnel sealed when it stopped replaces the app's last copy of it). Sessions deleted in the app stay deleted.
    func syncSavedCaptures() async {
        guard let list = RuntimeControl.decode(
            PacketCapture.SessionsAnswer.self, from: await tunnelControl(.captureSessions)
        ) else {
            return
        }
        let mine = Dictionary(await savedCaptures().map { ($0.id, $0.savedAt) }) { first, _ in first }
        let deleted = deletedCaptures()
        for session in list.sessions where !session.unreadable && deleted[session.id] == nil {
            if let copied = mine[session.id], copied >= session.savedAt {
                continue
            }
            _ = await copySession(session.id)
        }
        if let viewed = viewedCapture {
            viewedCapture = await savedCaptures().first { $0.id == viewed.id } ?? viewed
        }
    }

    /// Copies saved session `id` from the tunnel into the app.
    private func copySession(_ id: String) async -> PacketCapture.Session? {
        guard let key = try? await CaptureViewer.shared.keyBase64(),
              let copy = RuntimeControl.decode(
                  PacketCapture.CopyAnswer.self,
                  from: await tunnelControl(.copyCapture(session: id, keyBase64: key, name: nil))
              )
        else {
            return nil
        }
        return try? await CaptureViewer.shared.importCopy(copy)
    }

    // MARK: - Deleted sessions

    private func deletedCaptures() -> [String: Double] {
        let stored = UserDefaults.standard.dictionary(forKey: Self.deletedCaptureKey) as? [String: Double] ?? [:]
        // The tunnel deletes its sessions after the retention; the record of a deletion can go then too.
        let horizon = Date().timeIntervalSince1970 - Double(CaptureSessionStorage.retentionDays + 1) * 86_400
        return stored.filter { $0.value > horizon }
    }

    private func rememberDeleted(_ id: String) {
        var deleted = deletedCaptures()
        deleted[id] = Date().timeIntervalSince1970
        UserDefaults.standard.set(deleted, forKey: Self.deletedCaptureKey)
    }
}
