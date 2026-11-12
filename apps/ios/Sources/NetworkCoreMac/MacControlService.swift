import Foundation
import NetworkCoreBridge
import NetworkExtension

/// Answers the `anixops` command-line tool (docs/macos-cli.md) with the app's own tunnel controller, so a command
/// does exactly what the same button in the window does.
///
/// The socket lives in the app group container (`ControlProtocol.socketPath`); the tool finds it there through the
/// same group identifier in its own Info.plist.
@MainActor
final class MacControlService {
    private let tunnel: TunnelController
    private var server: ControlServer?

    init(tunnel: TunnelController) {
        self.tunnel = tunnel
    }

    /// Starts listening once; later calls do nothing. A failure leaves the app working without the command line.
    func start() {
        guard server == nil, let path = Self.socketPath() else {
            return
        }
        let server = ControlServer(path: path) { [weak self] request in
            guard let self else {
                return .failure(.unavailable, "AnixOps Net 正在退出")
            }
            return await self.handle(request)
        }
        do {
            try server.start()
            self.server = server
        } catch {
            NSLog("AnixOps Net: command-line socket unavailable: \(error)")
        }
    }

    func stop() {
        server?.stop()
        server = nil
    }

    /// The socket in the app group container; nil when the build has no group (the sandbox allows nothing else).
    static func socketPath() -> String? {
        guard let group = Bundle.main.object(forInfoDictionaryKey: "AnixOpsAppGroup") as? String, !group.isEmpty,
              let container = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group)
        else {
            return nil
        }
        return container.appendingPathComponent(ControlProtocol.socketName).path
    }

    func handle(_ request: ControlRequest) async -> ControlResponse {
        guard tunnel.loaded else {
            return .failure(.unavailable, "AnixOps Net 正在启动，请稍后再试")
        }
        switch request.command {
        case .status:
            return ControlResponse(status: status())
        case .connect:
            return await connect()
        case .disconnect:
            return await disconnect()
        case .mode:
            return await mode(request.mode)
        case .nodes:
            return groups(filter: request.group)
        case .select:
            return await select(node: request.node ?? "", group: request.group)
        case .test:
            if let filter = request.group, findGroup(filter) == nil {
                return .failure(.notFound, "没有叫 \(filter) 的订阅")
            }
            await tunnel.testLatency()
            return groups(filter: request.group)
        case .importNodes:
            return await importNodes(request.text ?? "")
        }
    }

    // MARK: Commands

    private func connect() async -> ControlResponse {
        if tunnel.connection == .connected {
            return ControlResponse(message: "已经连接", status: status())
        }
        if tunnel.phase == .unconfigured {
            return .failure(.unavailable, "还没有导入节点，先运行 anixops import <链接>")
        }
        if !tunnel.isConnected {
            tunnel.toggle()
        }
        let started = Date()
        while Date().timeIntervalSince(started) < 40 {
            try? await Task.sleep(for: .milliseconds(250))
            switch tunnel.connection {
            case .connected:
                return ControlResponse(message: "已连接", status: status())
            case .disconnected, .invalid:
                // Saving the profile or activating the system extension comes first; give it a moment.
                if Date().timeIntervalSince(started) > 5 {
                    return .failure(.failed, tunnel.message ?? "连接失败")
                }
            default:
                break
            }
        }
        return .failure(.timeout, tunnel.message ?? "等待连接超时")
    }

    private func disconnect() async -> ControlResponse {
        if tunnel.isConnected {
            tunnel.toggle()
        }
        let started = Date()
        while tunnel.connection != .disconnected, tunnel.connection != .invalid {
            guard Date().timeIntervalSince(started) < 20 else {
                return .failure(.timeout, "等待断开超时")
            }
            try? await Task.sleep(for: .milliseconds(250))
        }
        return ControlResponse(message: "已断开", status: status())
    }

    private func mode(_ next: ProxyMode?) async -> ControlResponse {
        guard let next else {
            return ControlResponse(status: status())
        }
        if tunnel.phase == .unconfigured {
            return .failure(.unavailable, "还没有导入节点")
        }
        if next == .rule, tunnel.library == nil {
            return .failure(.usage, "单个节点没有规则，无法用规则模式。导入订阅或多个节点后可用")
        }
        await tunnel.setMode(next)
        guard tunnel.mode == next else {
            return .failure(.failed, tunnel.message ?? "切换模式失败")
        }
        return ControlResponse(message: "已切换", status: status())
    }

    private func select(node query: String, group filter: String?) async -> ControlResponse {
        let candidates: [SubscriptionGroup]
        if let filter {
            guard let group = findGroup(filter) else {
                return .failure(.notFound, "没有叫 \(filter) 的订阅")
            }
            candidates = [group]
        } else {
            candidates = tunnel.subscriptions.groups
        }
        var matches: [(group: SubscriptionGroup, node: NativeImportReport.Node)] = []
        for group in candidates {
            if let node = Self.findNode(query, in: group.library.nodes) {
                matches.append((group, node))
            }
        }
        if matches.count > 1, let active = matches.first(where: { $0.group.id == tunnel.subscriptions.activeID }) {
            matches = [active]
        }
        guard let match = matches.first else {
            return .failure(.notFound, "没有叫 \(query) 的节点")
        }
        guard matches.count == 1 else {
            return .failure(.usage, "多个订阅里都有 \(query)，请用 --group 指定订阅")
        }
        await tunnel.selectNode(match.node.id, inGroup: match.group.id)
        let current = tunnel.subscriptions
        guard current.activeID == match.group.id, current.group(id: match.group.id)?.library.selectedID == match.node.id else {
            return .failure(.failed, tunnel.message ?? "切换节点失败")
        }
        return ControlResponse(message: "已切换到 \(match.node.name)", status: status())
    }

    private func importNodes(_ text: String) async -> ControlResponse {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return .failure(.invalidData, "没有可导入的内容")
        }
        let isSubscription = SubscriptionLink.url(from: trimmed) != nil
        guard await tunnel.importLink(trimmed) else {
            // A subscription that does not download is a failure of the attempt, not of the input.
            return .failure(isSubscription ? .failed : .invalidData, tunnel.message ?? "导入失败")
        }
        return ControlResponse(message: "已导入", status: status())
    }

    // MARK: Model

    private func status() -> ControlStatus {
        let connection: String
        switch tunnel.connection {
        case .connected:
            connection = "connected"
        case .connecting:
            connection = "connecting"
        case .reasserting:
            connection = "reasserting"
        case .disconnecting:
            connection = "disconnecting"
        case .invalid:
            connection = tunnel.phase == .unconfigured ? "unconfigured" : "disconnected"
        default:
            connection = "disconnected"
        }
        return ControlStatus(
            connection: connection,
            node: tunnel.nodeName,
            group: tunnel.subscriptions.active?.name,
            mode: tunnel.mode,
            connectedSince: tunnel.connectedSince,
            appVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "—"
        )
    }

    private func groups(filter: String?) -> ControlResponse {
        var groups = tunnel.subscriptions.groups
        if let filter {
            guard let group = findGroup(filter) else {
                return .failure(.notFound, "没有叫 \(filter) 的订阅")
            }
            groups = [group]
        }
        let activeID = tunnel.subscriptions.activeID
        return ControlResponse(groups: groups.map { group in
            ControlGroup(
                id: group.id,
                name: group.name,
                active: group.id == activeID,
                nodes: group.library.nodes.map { node in
                    let latency = tunnel.latencies[TunnelController.latencyKey(group: group.id, node: node.id)]
                    return ControlNode(
                        id: node.id,
                        name: node.name,
                        protocol: node.protocol,
                        selected: group.library.selectedID == node.id,
                        latencyMs: latency.flatMap(Self.millis),
                        latency: latency.flatMap(Self.latencyWord)
                    )
                }
            )
        })
    }

    /// A group by id, then by exact name, then by name ignoring case when only one matches.
    private func findGroup(_ query: String) -> SubscriptionGroup? {
        let groups = tunnel.subscriptions.groups
        if let group = groups.first(where: { $0.id == query }) ?? groups.first(where: { $0.name == query }) {
            return group
        }
        let folded = groups.filter { $0.name.caseInsensitiveCompare(query) == .orderedSame }
        return folded.count == 1 ? folded[0] : nil
    }

    /// A node by id, then by exact name, then by name ignoring case when only one matches.
    static func findNode(_ query: String, in nodes: [NativeImportReport.Node]) -> NativeImportReport.Node? {
        if let node = nodes.first(where: { $0.id == query }) ?? nodes.first(where: { $0.name == query }) {
            return node
        }
        let folded = nodes.filter { $0.name.caseInsensitiveCompare(query) == .orderedSame }
        return folded.count == 1 ? folded[0] : nil
    }

    private static func millis(_ latency: NodeLatency) -> Int? {
        if case let .millis(value) = latency {
            return value
        }
        return nil
    }

    private static func latencyWord(_ latency: NodeLatency) -> String? {
        switch latency {
        case .measuring:
            return "measuring"
        case .millis:
            return nil
        case .timeout:
            return "timeout"
        case .failed:
            return "failed"
        case .unsupported:
            return "unsupported"
        }
    }
}
