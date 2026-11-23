import Foundation
import NetworkCoreBridge

/// Sends the engine's per-node dial counts to the panel of the subscription about once an hour (`ClientReport`),
/// only after the user turned "report connection quality" on in Settings (`ClientReportStore`, off by default).
/// Counts only, never destinations; only ANX nodes. Runs while the app polls the dashboard. A reporter belongs to
/// one setting: the controller retires it when the user changes the setting and makes a new one.
@MainActor
final class ClientReporter {
    private let session: URLSession
    private let planner: ClientReportPlanner
    private var sending = false
    private var retired = false

    init(session: URLSession = .shared, store: ClientReportStore = ClientReportStore()) {
        self.session = session
        planner = ClientReportPlanner(store: store)
    }

    /// Stops this reporter: a report in flight no longer moves the saved baseline.
    func retire() {
        retired = true
    }

    /// Reports when due. `network` is `wifi`, `cellular`, `ethernet` or `other`.
    func offer(_ health: OutboundHealth, groups: SubscriptionGroups, network: String, now: Date = Date()) {
        guard !sending, !retired else {
            return
        }
        guard case let .send(bodies, counts) = planner.next(
            health: health,
            groups: groups,
            network: network,
            client: Self.client,
            now: now
        ) else {
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
            guard !retired else {
                return
            }
            if delivered {
                planner.delivered(counts, at: now)
            } else {
                planner.failed(at: now)
            }
        }
    }

    private static var client: String {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
        return "anixops-net/\(version)"
    }
}
