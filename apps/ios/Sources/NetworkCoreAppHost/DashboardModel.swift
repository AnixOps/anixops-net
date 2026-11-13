import Foundation
import Network
import NetworkCoreBridge
import NetworkExtension
import Observation

/// The live data behind the home screen dashboard: speed and its last minute, the busiest hosts, the policy groups,
/// the device's network, the exit address on request, and a short event log. Polls the engine only while the
/// tunnel is connected and the home screen is showing.
@MainActor
@Observable
final class DashboardModel {
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

        /// The network type as the panel failure report names it.
        var reportName: String {
            switch interface {
            case .wifi:
                return "wifi"
            case .cellular:
                return "cellular"
            case .wired:
                return "ethernet"
            case .other, .none:
                return "other"
            }
        }
    }

    private(set) var traffic: RuntimeControl.Traffic?
    private(set) var history = TrafficHistory()
    private(set) var topHosts: [HostTraffic] = []
    private(set) var groups: [RuntimeControl.Groups.Group] = []
    private(set) var network = NetworkState()
    private(set) var exitLookup = ExitLookup.idle
    private(set) var events: DashboardEventLog
    /// The last connection's totals and busiest hosts, shown while the tunnel is off.
    private(set) var lastSession: LastSessionSummary?

    private static let eventsKey = "dashboard-events"
    private static let lastSessionKey = "dashboard-last-session"
    /// How often, in one-second ticks, the poll also refreshes the cached seven-day statistics.
    private static let statsTicks = 15
    /// How often, in one-second ticks, the poll reads the nodes' dial health.
    private static let healthTicks = 30
    @ObservationIgnored private var monitor: NWPathMonitor?
    /// Views showing this model through `observe(_:)`; the one shared poll runs while there is at least one.
    @ObservationIgnored private var viewers = 0
    @ObservationIgnored private var sampler: Task<Void, Never>?

    init() {
        let saved = UserDefaults.standard.data(forKey: Self.eventsKey)
        events = saved.flatMap { try? JSONDecoder().decode(DashboardEventLog.self, from: $0) } ?? DashboardEventLog()
        let last = UserDefaults.standard.data(forKey: Self.lastSessionKey)
        lastSession = last.flatMap { try? JSONDecoder().decode(LastSessionSummary.self, from: $0) }
    }

    /// Shows this model in one more view until the calling task is cancelled. However many views (the Mac's main
    /// window and its dashboard window) observe it, one poll reads the engine: the first view starts it and the
    /// last one to go stops it. While the tunnel is down the poll waits for it to connect again.
    func observe(_ tunnel: TunnelController) async {
        viewers += 1
        if sampler == nil {
            sampler = Task { [weak self] in
                while !Task.isCancelled {
                    guard let self else {
                        return
                    }
                    await self.poll(tunnel)
                    try? await Task.sleep(for: .seconds(1))
                }
            }
        }
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(3600))
        }
        viewers -= 1
        if viewers == 0 {
            sampler?.cancel()
            sampler = nil
        }
    }

    /// Polls the engine once a second for speed, every third second for connections and groups, and every
    /// fifteenth for the seven-day statistics, until the task is cancelled or the tunnel leaves the connected
    /// state. When the tunnel goes down the session's totals and busiest hosts stay on as `lastSession`.
    func poll(_ tunnel: TunnelController) async {
        var tick = 0
        var startedAt: Date?
        while !Task.isCancelled, tunnel.connection == .connected {
            startedAt = tunnel.connectedSince ?? startedAt
            if let data = await tunnel.control(.traffic), let next = RuntimeControl.decodeTraffic(data) {
                traffic = next
                history.append(upload: next.uploadRate, download: next.downloadRate)
            }
            if tick % 3 == 0 {
                if let list = RuntimeControl.decode(RuntimeControl.Connections.self, from: await tunnel.control(.connections)) {
                    topHosts = HostTraffic.top(list)
                    tunnel.remember(list)
                }
                if let answer = RuntimeControl.decode(RuntimeControl.Groups.self, from: await tunnel.control(.groups)) {
                    groups = answer.groups
                }
            }
            if tick % Self.healthTicks == 0 {
                // Nodes that stopped after repeated rejections, and the hourly failure report.
                await tunnel.checkOutboundHealth(network: network.reportName)
            }
            if tick % Self.statsTicks == 0 {
                // Keeps the statistics page's copy fresh, so it has something to show once the tunnel is off.
                _ = await tunnel.trafficStats()
            }
            tick += 1
            try? await Task.sleep(for: .seconds(1))
        }
        if tunnel.connection != .connected {
            if let ended = LastSessionSummary.ended(traffic: traffic, topHosts: topHosts, startedAt: startedAt) {
                lastSession = ended
                if let data = try? JSONEncoder().encode(ended) {
                    UserDefaults.standard.set(data, forKey: Self.lastSessionKey)
                }
            }
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

#if DEBUG
/// Demo data for screenshots (`TunnelController.loadDemo()`): a minute of traffic, the busiest hosts and three
/// policy groups, as the engine would answer.
extension DashboardModel {
    func loadDemo() {
        traffic = RuntimeControl.Traffic(
            uploadTotal: 86_400_000,
            downloadTotal: 1_932_000_000,
            uploadRate: 184_000,
            downloadRate: 3_870_000,
            active: 27
        )
        history.reset()
        for step in 0..<history.capacity {
            let wave = sin(Double(step) / 5) + sin(Double(step) / 2.3) * 0.5
            let download = 2_600_000 + wave * 1_300_000
            let upload = 150_000 + sin(Double(step) / 3.1) * 90_000
            history.append(upload: UInt64(max(20_000, upload)), download: UInt64(max(200_000, download)))
        }
        topHosts = [
            HostTraffic(host: "youtube.com", bytes: 1_240_000_000, connections: 9, outbound: "香港 01"),
            HostTraffic(host: "github.com", bytes: 312_000_000, connections: 6, outbound: "日本 01"),
            HostTraffic(host: "icloud.com", bytes: 148_000_000, connections: 4, outbound: "DIRECT"),
            HostTraffic(host: "openai.com", bytes: 96_000_000, connections: 5, outbound: "美国 01"),
            HostTraffic(host: "telegram.org", bytes: 41_000_000, connections: 3, outbound: "新加坡 01"),
        ]
        let answer = """
        {"groups":[
        {"id":"节点选择","type":"select","now":"香港 01","members":[{"name":"香港 01","latency_ms":38},{"name":"日本 01","latency_ms":72},{"name":"新加坡 01","latency_ms":86}]},
        {"id":"自动选择","type":"url-test","now":"香港 02","members":[{"name":"香港 02","latency_ms":45},{"name":"香港 01","latency_ms":38}]},
        {"id":"流媒体","type":"select","now":"美国 01","members":[{"name":"美国 01","latency_ms":164},{"name":"德国 01","latency_ms":213}]}
        ]}
        """
        groups = RuntimeControl.decode(RuntimeControl.Groups.self, from: Data(answer.utf8))?.groups ?? []
    }
}
#endif
