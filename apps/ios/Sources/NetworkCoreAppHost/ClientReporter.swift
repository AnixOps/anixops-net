import Foundation
import NetworkCoreBridge

/// Sends the engine's per-node dial counts to the panel about once an hour (F-55, `ClientReport`), straight from
/// the app rather than through the tunnel, so a node that is blocked still gets reported. Counts only, never
/// destinations; only ANX nodes from our own panel. Runs while the app polls the dashboard.
@MainActor
final class ClientReporter {
    private static let snapshotKey = "client-report-snapshot"
    private static let lastReportKey = "client-report-last"

    private let session: URLSession
    private let defaults: UserDefaults
    /// Counts per engine node at the last report.
    private var snapshot: [String: OutboundHealth.Counts]
    private var lastReport: Date?
    private var nextReport: Date
    private var sending = false

    init(session: URLSession = .shared, defaults: UserDefaults = .standard) {
        self.session = session
        self.defaults = defaults
        snapshot = defaults.data(forKey: Self.snapshotKey)
            .flatMap { try? JSONDecoder().decode([String: OutboundHealth.Counts].self, from: $0) } ?? [:]
        let last = defaults.object(forKey: Self.lastReportKey) as? Date
        lastReport = last
        nextReport = (last ?? Date()).addingTimeInterval(Self.randomInterval())
    }

    private static func randomInterval() -> TimeInterval {
        TimeInterval.random(in: ClientReport.interval)
    }

    /// Reports when due. `network` is `wifi`, `cellular`, `ethernet` or `unknown`.
    func offer(_ health: OutboundHealth, groups: SubscriptionGroups, network: String, now: Date = Date()) {
        guard !sending, now >= nextReport, !health.outbounds.isEmpty else {
            return
        }
        let window = lastReport.map { now.timeIntervalSince($0) } ?? ClientReport.interval.lowerBound
        let bodies = ClientReport.bodies(
            health: health,
            groups: groups,
            previous: snapshot,
            window: window,
            client: Self.client,
            platform: "ios",
            network: network
        )
        let current = Dictionary(uniqueKeysWithValues: health.outbounds.map { ($0.node, $0.health.counts) })
        guard !bodies.isEmpty else {
            // Nothing new: move the baseline and look again next round.
            remember(current, at: now)
            return
        }
        sending = true
        Task {
            var delivered = true
            for (endpoint, body) in bodies {
                var request = URLRequest(url: endpoint, timeoutInterval: 15)
                request.httpMethod = "POST"
                request.setValue("application/json", forHTTPHeaderField: "Content-Type")
                request.httpBody = body.encoded()
                do {
                    // 204 taken; 404 (unknown token) and 429 (too soon) are dropped, as the panel asks.
                    _ = try await session.data(for: request)
                } catch {
                    delivered = false
                }
            }
            sending = false
            if delivered {
                remember(current, at: now)
            } else {
                // Offline or the panel unreachable: keep the counts and try again after the panel's minimum.
                nextReport = now.addingTimeInterval(ClientReport.minimumInterval)
            }
        }
    }

    private func remember(_ counts: [String: OutboundHealth.Counts], at date: Date) {
        snapshot = counts
        lastReport = date
        nextReport = date.addingTimeInterval(Self.randomInterval())
        if let data = try? JSONEncoder().encode(counts) {
            defaults.set(data, forKey: Self.snapshotKey)
        }
        defaults.set(date, forKey: Self.lastReportKey)
    }

    private static var client: String {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
        return "anixops-net/\(version)"
    }
}
