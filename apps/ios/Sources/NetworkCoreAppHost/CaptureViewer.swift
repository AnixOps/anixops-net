import Foundation
import NetworkCoreBridge
import NetworkCoreRuntimeC

/// The app's own copies of saved capture sessions, opened by the kernel's capture viewer
/// (crates/engine-embedded `networkcore_ios_capture_viewer`) in the app's process, so a capture can be read, filtered
/// and exported while the tunnel is off.
///
/// The tunnel keeps capture in its engine and seals it with a key the app never sees (`CaptureSessionStorage.key()`).
/// The app therefore asks the running tunnel for copies sealed with its own key (`copy_capture`,
/// `CaptureSessionStorage.appKey()`) and keeps them in its own Application Support folder: what is captured now,
/// under the tunnel run's session id, every few seconds while it changes, and each session the tunnel saved
/// (including the run it sealed when it stopped, which replaces the app's last copy of that run).
actor CaptureViewer {
    static let shared = CaptureViewer()

    /// Why the viewer could not answer.
    enum Failure: Error, Equatable {
        /// The key or the folder is not available (the Keychain refused, the disk is full).
        case storage
        /// The kernel refused the request; its fixed message (no paths, no key bytes).
        case refused(String)
    }

    /// The session that is loaded now, so loading it again is skipped.
    private var loaded: String?
    private var ready = false

    /// The answer to `command`, as the engine would give it.
    func control(_ command: RuntimeControl) -> Data? {
        try? send(command)
    }

    /// The answer to `command`, or why there is none.
    func send(_ command: RuntimeControl) throws -> Data {
        try prepare()
        let answer = try call(command.request)
        switch command {
        case let .loadCapture(session):
            loaded = session
        case let .deleteCapture(session) where session == loaded:
            loaded = nil
        case .clearCapture:
            loaded = nil
        default:
            break
        }
        return answer
    }

    /// Loads `session` unless it is loaded already.
    func open(_ session: String) throws {
        guard loaded != session else {
            return
        }
        _ = try send(.loadCapture(session))
    }

    /// Forgets the loaded session; the next `open` reads it again (after a newer copy replaced it).
    func reload(_ session: String) throws {
        if loaded == session {
            loaded = nil
        }
        try open(session)
    }

    /// The app's copies, newest first.
    func sessions() throws -> [PacketCapture.Session] {
        let answer = try send(.captureSessions)
        guard let list = RuntimeControl.decode(PacketCapture.SessionsAnswer.self, from: answer) else {
            throw Failure.refused("capture_sessions")
        }
        return list.sessions.sorted { $0.savedAt > $1.savedAt }
    }

    /// Keeps a copy from the tunnel (`copy_capture`); a copy of the loaded session is read again.
    @discardableResult
    func importCopy(_ copy: PacketCapture.CopyAnswer) throws -> PacketCapture.Session? {
        let answer = try send(.importCapture(session: copy.session, dataBase64: copy.dataBase64))
        if loaded == copy.session {
            loaded = nil
            try open(copy.session)
        }
        return RuntimeControl.decode(PacketCapture.SavedAnswer.self, from: answer)?.session
    }

    /// The app's key as the tunnel needs it for `copy_capture`.
    func keyBase64() throws -> String {
        do {
            return try CaptureSessionStorage.appKey().base64EncodedString()
        } catch {
            throw Failure.storage
        }
    }

    /// Points the viewer at the app's folder and key once per launch; old copies past the retention are deleted then.
    private func prepare() throws {
        guard !ready else {
            return
        }
        let request: RuntimeControl
        do {
            request = .setCaptureStorage(
                dir: try CaptureSessionStorage.directory().path,
                keyBase64: try CaptureSessionStorage.appKey().base64EncodedString(),
                retentionDays: CaptureSessionStorage.retentionDays
            )
        } catch {
            throw Failure.storage
        }
        _ = try call(request.request)
        ready = true
    }

    private func call(_ request: Data) throws -> Data {
        var buffer = NetworkCoreBuffer(ptr: nil, len: 0, cap: 0)
        let code = request.withUnsafeBytes { (input: UnsafeRawBufferPointer) -> Int32 in
            networkcore_ios_capture_viewer(input.bindMemory(to: UInt8.self).baseAddress, input.count, &buffer)
        }
        defer { networkcore_ios_runtime_free_buffer(buffer) }
        let data = buffer.ptr.map { Data(bytes: $0, count: buffer.len) } ?? Data()
        guard code == NETWORKCORE_IOS_OK else {
            let message = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["error"] as? String
            throw Failure.refused(message ?? "error \(code)")
        }
        return data
    }
}
