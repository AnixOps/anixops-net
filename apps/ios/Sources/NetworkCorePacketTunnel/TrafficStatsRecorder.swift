import Foundation
import NetworkCoreBridge

/// Builds `TrafficStats` while the tunnel runs: every few seconds it reads the engine's connection list and adds
/// what each connection moved since the last read. The stats live in this extension's own container, so they
/// outlast a restart of the tunnel; the app reads and clears them through app messages.
final class TrafficStatsRecorder: @unchecked Sendable {
    private let queue = DispatchQueue(label: "networkcore.packet-tunnel.traffic-stats")
    private var timer: DispatchSourceTimer?
    private var stats: TrafficStats
    private var counter = TrafficStatsCounter()
    private var dirty = false
    private var lastSave = Date.distantPast
    private let fileURL: URL?

    private static let pollSeconds = 5
    private static let saveSeconds: TimeInterval = 60

    init() {
        let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
        fileURL = directory?.appendingPathComponent("traffic-stats.json")
        if let fileURL, let data = try? Data(contentsOf: fileURL), let saved = try? TrafficStats.decoded(from: data) {
            stats = saved
        } else {
            stats = TrafficStats()
        }
        stats.prune(now: Date())
    }

    /// Starts reading `control`, the running engine's control entry point.
    func start(control: @escaping @Sendable (Data) -> Data?) {
        queue.async { [self] in
            counter.reset()
            timer?.cancel()
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(deadline: .now() + .seconds(Self.pollSeconds), repeating: .seconds(Self.pollSeconds), leeway: .seconds(1))
            timer.setEventHandler { [weak self] in
                self?.poll(control)
            }
            timer.resume()
            self.timer = timer
        }
    }

    /// Stops reading and saves what was counted.
    func stop() {
        queue.sync {
            timer?.cancel()
            timer = nil
            save(force: true)
        }
    }

    func snapshot() -> Data? {
        queue.sync {
            stats.prune(now: Date())
            return try? stats.encoded()
        }
    }

    func clear() -> Data? {
        queue.sync {
            stats.clear()
            dirty = true
            save(force: true)
            return try? stats.encoded()
        }
    }

    private func poll(_ control: (Data) -> Data?) {
        guard let list = RuntimeControl.decode(RuntimeControl.Connections.self, from: control(RuntimeControl.connections.request)) else {
            return
        }
        let before = stats
        counter.ingest(list, into: &stats)
        if stats != before {
            dirty = true
        }
        save(force: false)
    }

    private func save(force: Bool) {
        guard dirty, let fileURL else {
            return
        }
        let now = Date()
        guard force || now.timeIntervalSince(lastSave) >= Self.saveSeconds else {
            return
        }
        do {
            try FileManager.default.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            #if os(iOS)
            let options: Data.WritingOptions = [.atomic, .completeFileProtectionUntilFirstUserAuthentication]
            #else
            let options: Data.WritingOptions = [.atomic]
            #endif
            try stats.encoded().write(to: fileURL, options: options)
            dirty = false
            lastSave = now
        } catch {
            // Kept in memory; the next save tries again.
        }
    }
}
