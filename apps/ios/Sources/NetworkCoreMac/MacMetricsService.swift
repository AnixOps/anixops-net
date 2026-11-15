import Foundation
import Network
import NetworkCoreBridge

/// Settings > Prometheus metrics: `GET http://127.0.0.1:9477/metrics`, as on Windows and Linux. The packet tunnel
/// runs in a System Extension whose sockets other apps cannot rely on, so the app serves the endpoint and asks the
/// running engine for its text (`RuntimeControl.metrics`) on each scrape. Off by default; loopback only.
@MainActor
final class MacMetricsService: ObservableObject {
    static let enabledKey = "metrics-endpoint-enabled"

    /// What the settings row shows: the address while listening, or why it is not.
    @Published private(set) var address: String?
    @Published private(set) var failure: String?

    private let tunnel: TunnelController
    private var listener: NWListener?
    private let queue = DispatchQueue(label: "net.anixops.metrics")

    init(tunnel: TunnelController) {
        self.tunnel = tunnel
    }

    var isEnabled: Bool {
        UserDefaults.standard.bool(forKey: Self.enabledKey)
    }

    func setEnabled(_ enabled: Bool) {
        UserDefaults.standard.set(enabled, forKey: Self.enabledKey)
        apply()
    }

    /// Starts or stops the listener to match the saved switch.
    func apply() {
        if isEnabled {
            start()
        } else {
            stop()
        }
    }

    private func start() {
        guard listener == nil, let port = NWEndpoint.Port(rawValue: MetricsEndpoint.defaultPort) else {
            return
        }
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: port)
        parameters.allowLocalEndpointReuse = true
        let listener: NWListener
        do {
            listener = try NWListener(using: parameters)
        } catch {
            failure = String(localized: "端口被占用")
            return
        }
        listener.stateUpdateHandler = { [weak self] state in
            Task { @MainActor in
                self?.update(state)
            }
        }
        listener.newConnectionHandler = { [weak self, queue] connection in
            connection.start(queue: queue)
            Self.readHead(connection, received: Data()) { head in
                let route = MetricsEndpoint.route(head)
                guard route == .metrics else {
                    Self.reply(connection, MetricsEndpoint.response(route, body: ""))
                    return
                }
                Task { @MainActor in
                    let body = await self?.render() ?? ""
                    Self.reply(connection, MetricsEndpoint.response(.metrics, body: body))
                }
            }
        }
        listener.start(queue: queue)
        self.listener = listener
    }

    private func stop() {
        listener?.cancel()
        listener = nil
        address = nil
        failure = nil
    }

    private func update(_ state: NWListener.State) {
        switch state {
        case .ready:
            address = "127.0.0.1:\(MetricsEndpoint.defaultPort)"
            failure = nil
        case .failed:
            listener?.cancel()
            listener = nil
            address = nil
            failure = String(localized: "端口被占用")
        default:
            break
        }
    }

    /// The engine's metrics while connected, then the app's own gauges.
    private func render() async -> String {
        let connected = tunnel.connection == .connected
        let engine = connected ? MetricsEndpoint.engineText(await tunnel.control(.metrics(anonymous: false))) : nil
        let health = ClientHealthMonitor.shared
        let client = MetricsEndpoint.clientGauges(
            platform: "macos",
            version: Self.version,
            connected: connected,
            crashReportsEnabled: health.crashReports.isEnabled,
            crashReportsKept: health.crashReports.pending().count,
            integrity: health.integrity?.state ?? ""
        )
        return MetricsEndpoint.combine(engine: engine, client: client)
    }

    private static var version: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "unknown"
    }

    /// Reads until the blank line ending the request head, or answers 400 once it grows too long.
    private nonisolated static func readHead(_ connection: NWConnection, received: Data, then handle: @escaping @Sendable (Data) -> Void) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 4096) { data, _, isComplete, error in
            var head = received
            if let data {
                head.append(data)
            }
            if MetricsEndpoint.isComplete(head) {
                handle(head)
            } else if error != nil || isComplete || head.count > MetricsEndpoint.maxHeadBytes {
                reply(connection, MetricsEndpoint.response(.badRequest, body: ""))
            } else {
                readHead(connection, received: head, then: handle)
            }
        }
    }

    private nonisolated static func reply(_ connection: NWConnection, _ response: Data) {
        connection.send(content: response, completion: .contentProcessed { _ in
            connection.cancel()
        })
    }
}
