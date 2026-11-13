import AppKit
import Foundation
import NetworkCoreBridge
import Observation

/// In-app updates of the Developer ID build (docs/macos-updates.md).
///
/// The app downloads the signed feed, checks it against the public keys built into Info.plist
/// (`UpdateVerifier`), downloads the newest installer package it may run, checks the package's size, SHA-256 and
/// signature, and only then opens it in Installer. Gatekeeper checks the package's Developer ID signature and
/// notarization once more when Installer opens it. Anything that does not verify is deleted and never opened.
///
/// A build without a feed URL or public key (every CI and developer build) has updates turned off.
@MainActor
@Observable
final class AppUpdater {
    enum State: Equatable {
        /// This build has no feed URL or no valid public key.
        case disabled
        case idle
        case checking
        case upToDate(Date)
        case available(UpdateItem)
        case downloading(UpdateItem, Double)
        /// The verified package, ready for Installer.
        case ready(UpdateItem, URL)
        case failed(String)
    }

    static let product = "anixops-net-macos"
    private static let automaticKey = "AnixOpsUpdateAutomatic"
    private static let lastCheckKey = "AnixOpsUpdateLastCheck"
    private static let sequenceKey = "AnixOpsUpdateFeedSequence"
    /// A feed is a few kilobytes; anything far larger is not one.
    private static let maxFeedBytes = 1 << 20
    private static let checkInterval: TimeInterval = 24 * 60 * 60

    private(set) var state: State
    /// Checks once a day on its own; on by default.
    private(set) var automatic: Bool

    private let feedURL: URL?
    private let verifier: UpdateVerifier?

    init(bundle: Bundle = .main) {
        let info = bundle.infoDictionary ?? [:]
        let feed = (info["AnixOpsUpdateFeedURL"] as? String)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .flatMap(URL.init(string:))
            .flatMap { $0.scheme == "https" ? $0 : nil }
        let keys = ((info["AnixOpsUpdatePublicKeys"] as? String) ?? "")
            .split(whereSeparator: { $0 == "," || $0 == " " })
            .map(String.init)
        let verifier = try? UpdateVerifier(product: Self.product, publicKeys: keys)
        feedURL = feed
        self.verifier = feed == nil ? nil : verifier
        state = feed != nil && verifier != nil ? .idle : .disabled
        automatic = UserDefaults.standard.object(forKey: Self.automaticKey) as? Bool ?? true
    }

    func setAutomatic(_ on: Bool) {
        automatic = on
        UserDefaults.standard.set(on, forKey: Self.automaticKey)
    }

    var isEnabled: Bool {
        state != .disabled
    }

    var currentVersion: String {
        let info = Bundle.main.infoDictionary
        return info?["CFBundleShortVersionString"] as? String ?? "—"
    }

    private var currentBuild: Int {
        Int(Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "") ?? 0
    }

    /// Checks once a day while automatic checks are on; called at launch and every few hours by the app.
    func checkIfDue(now: Date = Date()) async {
        guard isEnabled, automatic else {
            return
        }
        if let last = UserDefaults.standard.object(forKey: Self.lastCheckKey) as? Date,
           now.timeIntervalSince(last) < Self.checkInterval {
            return
        }
        await check()
    }

    /// Downloads and verifies the feed, then shows the newest update this Mac can run.
    func check() async {
        guard let feedURL, let verifier else {
            return
        }
        switch state {
        case .checking, .downloading:
            return
        default:
            break
        }
        state = .checking
        do {
            var request = URLRequest(url: feedURL, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 30)
            request.setValue("application/json", forHTTPHeaderField: "Accept")
            let (data, response) = try await URLSession.shared.data(for: request)
            guard (response as? HTTPURLResponse)?.statusCode == 200, data.count <= Self.maxFeedBytes else {
                state = .failed(String(localized: "无法获取更新信息"))
                return
            }
            let last = UserDefaults.standard.object(forKey: Self.sequenceKey) as? Int64
            let feed = try verifier.verifyFeed(data, lastAccepted: last)
            UserDefaults.standard.set(feed.sequence, forKey: Self.sequenceKey)
            UserDefaults.standard.set(Date(), forKey: Self.lastCheckKey)
            let newest = verifier.newestUpdate(
                in: feed,
                currentBuild: currentBuild,
                systemVersion: ProcessInfo.processInfo.operatingSystemVersion
            )
            if let newest, newest.url.pathExtension.lowercased() == "pkg" {
                state = .available(newest)
            } else {
                state = .upToDate(Date())
            }
        } catch is UpdateVerificationError {
            // Signed by someone else, changed on the way, or an old feed played back: never act on it.
            state = .failed(String(localized: "更新信息未通过签名校验，已忽略"))
        } catch {
            state = .failed(String(localized: "无法获取更新信息"))
        }
    }

    /// Downloads the package of an available update and verifies it.
    func download() async {
        guard case let .available(item) = state, let verifier else {
            return
        }
        state = .downloading(item, 0)
        let target: URL
        do {
            target = try Self.destination(for: item)
        } catch {
            state = .failed(String(localized: "无法保存更新安装包"))
            return
        }
        do {
            let (temporary, response) = try await URLSession.shared.download(from: item.url, delegate: DownloadProgress(item: item) { [weak self] fraction in
                Task { @MainActor in
                    guard let self else {
                        return
                    }
                    if case .downloading = self.state {
                        self.state = .downloading(item, fraction)
                    }
                }
            })
            guard (response as? HTTPURLResponse)?.statusCode == 200 else {
                try? FileManager.default.removeItem(at: temporary)
                state = .failed(String(localized: "下载更新失败"))
                return
            }
            try? FileManager.default.removeItem(at: target)
            try FileManager.default.moveItem(at: temporary, to: target)
        } catch {
            state = .failed(String(localized: "下载更新失败"))
            return
        }
        do {
            try verifier.verifyArchive(at: target, item: item)
        } catch {
            try? FileManager.default.removeItem(at: target)
            state = .failed(String(localized: "更新安装包未通过校验，已删除"))
            return
        }
        state = .ready(item, target)
    }

    /// Opens the verified package in Installer and quits, so Installer can replace the app.
    func install() {
        guard case let .ready(item, file) = state, let verifier else {
            return
        }
        // The file sat in Downloads since it was checked: check it again right before it is opened.
        do {
            try verifier.verifyArchive(at: file, item: item)
        } catch {
            try? FileManager.default.removeItem(at: file)
            state = .failed(String(localized: "更新安装包未通过校验，已删除"))
            return
        }
        guard NSWorkspace.shared.open(file) else {
            state = .failed(String(localized: "无法打开安装程序"))
            return
        }
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(1))
            NSApp.terminate(nil)
        }
    }

    /// `~/Downloads/AnixOps Net <version> (<build>).pkg`; the sandbox allows Downloads for this.
    private static func destination(for item: UpdateItem) throws -> URL {
        let downloads = try FileManager.default.url(
            for: .downloadsDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        let safeVersion = item.version.filter { $0.isLetter || $0.isNumber || $0 == "." || $0 == "-" }
        return downloads.appendingPathComponent("AnixOps Net \(safeVersion) (\(item.build)).pkg")
    }

    /// Reports download progress.
    private final class DownloadProgress: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
        private let total: Int64
        private let report: @Sendable (Double) -> Void
        private var observation: NSKeyValueObservation?

        init(item: UpdateItem, report: @escaping @Sendable (Double) -> Void) {
            total = item.length
            self.report = report
        }

        func urlSession(_ session: URLSession, didCreateTask task: URLSessionTask) {
            let total = total
            let report = report
            observation = task.observe(\.countOfBytesReceived) { task, _ in
                guard total > 0 else {
                    return
                }
                report(min(1, Double(task.countOfBytesReceived) / Double(total)))
            }
        }
    }
}
