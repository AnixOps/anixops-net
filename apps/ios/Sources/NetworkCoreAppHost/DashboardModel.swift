import Foundation
import Network
import NetworkCoreBridge
import NetworkExtension

/// The live data behind the home screen dashboard: speed and its last minute, the busiest hosts, the policy groups,
/// the device's network, the exit address on request, and a short event log. Polls the engine only while the
/// tunnel is connected and the home screen is showing.
@MainActor
final class DashboardModel: ObservableObject {
    enum ExitLookup: Equatable {
        case idle
        case loading
        case done(ExitAddress)
        case failed
    }

    /// The device's current path, as the network card shows it.
    struct NetworkState: Equatable {
        enum Interface: Equatable {
            case wifi
            case cellular
            case wired
            case other
            case none
        }

        var interface: Interface = .other
        var isConstrained = false
        var isExpensive = false
        /// Interface switches seen since the app started.
        var switches = 0
    }

    @Published private(set) var traffic: RuntimeControl.Traffic?
    @Published private(set) var history = TrafficHistory()
    @Published private(set) var topHosts: [HostTraffic] = []
    @Published private(set) var groups: [RuntimeControl.Groups.Group] = []
    @Published private(set) var network = NetworkState()
    @Published private(set) var exitLookup = ExitLookup.idle
    @Published private(set) var events: DashboardEventLog

    private static let eventsKey = "dashboard-events"
    private var monitor: NWPathMonitor?

    init() {
        let saved = UserDefaults.standard.data(forKey: Self.eventsKey)
        events = saved.flatMap { try? JSONDecoder().decode(DashboardEventLog.self, from: $0) } ?? DashboardEventLog()
    }

    /// Polls the engine once a second for speed, and every third second for connections and groups, until the
    /// task is cancelled or the tunnel leaves the connected state.
    func poll(_ tunnel: TunnelController) async {
        var tick = 0
        while !Task.isCancelled, tunnel.connection == .connected {
            if let data = await tunnel.control(.traffic), let next = RuntimeControl.decodeTraffic(data) {
                traffic = next
                history.append(upload: next.uploadRate, download: next.downloadRate)
            }
            if tick % 3 == 0 {
                if let list = RuntimeControl.decode(RuntimeControl.Connections.self, from: await tunnel.control(.connections)) {
                    topHosts = HostTraffic.top(list)
                }
                if let answer = RuntimeControl.decode(RuntimeControl.Groups.self, from: await tunnel.control(.groups)) {
                    groups = answer.groups
                }
            }
            tick += 1
            try? await Task.sleep(for: .seconds(1))
        }
        if tunnel.connection != .connected {
            traffic = nil
            history.reset()
            topHosts = []
            groups = []
        }
    }

    func startNetworkMonitor() {
        guard monitor == nil else {
            return
        }
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { [weak self] path in
            let interface: NetworkState.Interface
            if path.status != .satisfied {
                interface = .none
            } else if path.usesInterfaceType(.wifi) {
                interface = .wifi
            } else if path.usesInterfaceType(.cellular) {
                interface = .cellular
            } else if path.usesInterfaceType(.wiredEthernet) {
                interface = .wired
            } else {
                interface = .other
            }
            let constrained = path.isConstrained
            let expensive = path.isExpensive
            Task { @MainActor in
                self?.updateNetwork(interface, constrained: constrained, expensive: expensive)
            }
        }
        monitor.start(queue: DispatchQueue(label: "net.anixops.dashboard.path"))
        self.monitor = monitor
    }

    private func updateNetwork(_ interface: NetworkState.Interface, constrained: Bool, expensive: Bool) {
        var next = network
        // Our own tunnel shows up as `.other`; it is not a switch of the physical network.
        let physical = interface != .other
        if physical, next.interface != interface, next.interface != .other {
            next.switches += 1
            record(.network, String(localized: "网络切换到\(Self.name(of: interface))"))
        }
        if physical || next.interface == .other {
            next.interface = interface
        }
        next.isConstrained = constrained
        next.isExpensive = expensive
        network = next
    }

    static func name(of interface: NetworkState.Interface) -> String {
        switch interface {
        case .wifi:
            return "Wi-Fi"
        case .cellular:
            return String(localized: "蜂窝网络")
        case .wired:
            return String(localized: "有线网络")
        case .other:
            return String(localized: "已联网")
        case .none:
            return String(localized: "无网络")
        }
    }

    /// Looks up the public address once, on request. The request follows the tunnel's current mode and rules, so
    /// it shows the address sites see for this kind of traffic.
    func lookUpExitAddress() async {
        guard exitLookup != .loading else {
            return
        }
        exitLookup = .loading
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 8
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        let session = URLSession(configuration: configuration)
        defer {
            session.finishTasksAndInvalidate()
        }
        guard let url = URL(string: "https://www.cloudflare.com/cdn-cgi/trace") else {
            exitLookup = .failed
            return
        }
        do {
            let (data, response) = try await session.data(from: url)
            guard (response as? HTTPURLResponse)?.statusCode == 200,
                  let address = ExitAddress.parseTrace(String(decoding: data, as: UTF8.self))
            else {
                exitLookup = .failed
                return
            }
            exitLookup = .done(address)
        } catch {
            exitLookup = .failed
        }
    }

    /// The tunnel's status changed: log connects and drops, and forget the exit address of the old route.
    func tunnelChanged(from previous: NEVPNStatus, to status: NEVPNStatus, node: String?) {
        if status == .connected, previous != .connected {
            let text = node.map { String(localized: "已连接 · \($0)") } ?? String(localized: "已连接")
            record(.connected, text)
            exitLookup = .idle
        } else if status == .disconnected, previous == .connected || previous == .reasserting || previous == .disconnecting {
            record(.disconnected, String(localized: "已断开"))
            exitLookup = .idle
        } else if status == .reasserting {
            record(.network, String(localized: "网络变化，正在恢复隧道"))
        }
    }

    func messageShown(_ message: String?) {
        if let message, !message.isEmpty {
            record(.error, message)
        }
    }

    func clearEvents() {
        events.clear()
        save()
    }

    private func record(_ kind: DashboardEventLog.Kind, _ text: String) {
        events.record(kind, text)
        save()
    }

    private func save() {
        if let data = try? JSONEncoder().encode(events) {
            UserDefaults.standard.set(data, forKey: Self.eventsKey)
        }
    }
}
