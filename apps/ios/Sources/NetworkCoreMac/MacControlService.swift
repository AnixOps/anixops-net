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
        case .agent:
            return await agent(request)
        }
    }

    // MARK: AI tools (docs/mcp-desktop.md)

    /// The MCP program's requests. Its answers go to an AI tool, so they are English and carry no node address or
    /// credential; the engine sees only what `AgentGate` lets through, and an action happens only once the user has
    /// confirmed it (`MacAgentConfirmation`). Every request but `access` goes into the access log.
    private func agent(_ request: ControlRequest) async -> ControlResponse {
        let access = AgentAccess.load()
        guard let call = request.agent else {
            return .failure(.usage, "The request names no agent call.")
        }
        if call == .access {
            return ControlResponse(access: access)
        }
        let name = Self.logName(call, payload: request.payload)
        guard access.read else {
            AgentAccessLog.record(name, .refused)
            return .failure(.accessOff, "AnixOps Net has not allowed access by AI tools.")
        }
        switch call {
        case .access:
            return ControlResponse(access: access)
        case .engine:
            let gated: Data
            switch AgentGate.engineRequest(request.payload ?? "") {
            case let .success(data):
                gated = data
            case let .failure(refusal):
                AgentAccessLog.record(name, .refused)
                return .failure(.usage, refusal.message)
            }
            AgentAccessLog.record(name, .allowed)
            guard tunnel.connection == .connected else {
                return .failure(.notConnected, "AnixOps Net is not connected.")
            }
            guard let answer = await tunnel.agentControl(gated) else {
                return .failure(.failed, "The tunnel gave no answer.")
            }
            return ControlResponse(payload: String(decoding: answer, as: UTF8.self))
        case .logs:
            AgentAccessLog.record(name, .allowed)
            guard tunnel.connection == .connected else {
                return .failure(.notConnected, "AnixOps Net is not connected.")
            }
            guard let diagnostics = await tunnel.agentDiagnostics() else {
                return .failure(.failed, "The tunnel gave no answer.")
            }
            let lines = diagnostics.events.map(AgentLogLine.init)
            guard let data = try? JSONEncoder().encode(lines) else {
                return .failure(.failed, "The log could not be read.")
            }
            return ControlResponse(payload: String(decoding: data, as: UTF8.self))
        case .act:
            return await act(request.payload ?? "", access: access, name: name)
        }
    }

    /// An action of an AI tool: the user's switch, the action itself, the user's confirmation, and then the same code
    /// as the window's own button or the `anixops` command.
    private func act(_ payload: String, access: AgentAccess, name: String) async -> ControlResponse {
        guard access.act else {
            AgentAccessLog.record(name, .refused)
            return .failure(.actOff, "AnixOps Net has not allowed actions by AI tools.")
        }
        let action: AgentAction
        switch AgentAction.parse(payload) {
        case let .success(parsed):
            action = parsed
        case let .failure(refusal):
            AgentAccessLog.record(name, .refused)
            return .failure(.usage, refusal.message)
        }
        // What needs the engine is refused before the user is asked about it.
        switch action {
        case .setCapture, .clearCapture, .closeConnection:
            guard tunnel.connection == .connected else {
                return .failure(.notConnected, "AnixOps Net is not connected.")
            }
        default:
            break
        }
        let answer = await MacAgentConfirmation.shared.confirm(action)
        switch answer {
        case .allowed:
            AgentAccessLog.record(name, .allowed)
        case .granted:
            AgentAccessLog.record(name, .granted)
        case .denied:
            AgentAccessLog.record(name, .denied)
            return .failure(.denied, "The user denied this action.")
        case .noAnswer:
            AgentAccessLog.record(name, .noAnswer)
            return .failure(.noAnswer, "The user did not answer in time.")
        case .busy:
            AgentAccessLog.record(name, .refused)
            return .failure(.unavailable, "Another action is waiting for the user's confirmation. Try again later.")
        }
        // The user may have turned actions off while the confirmation was open.
        guard AgentAccess.load().act else {
            return .failure(.actOff, "AnixOps Net has not allowed actions by AI tools.")
        }
        let done: ControlResponse
        switch action {
        case .connect:
            done = await connect()
        case .disconnect:
            done = await disconnect()
        case let .setMode(mode):
            done = await self.mode(mode)
        case let .selectNode(node, group):
            done = await select(node: node, group: group)
        case .testNodeDelays:
            await tunnel.testLatency()
            done = ControlResponse()
        case let .setCapture(on):
            done = await engineAction(.setCapture(on))
        case .clearCapture:
            done = await engineAction(.clearCapture)
        case let .closeConnection(id):
            done = await engineAction(.closeConnection(id))
        }
        // The window's answers are for people; the AI tool gets English.
        guard done.ok else {
            return .failure(done.error ?? .failed, Self.agentFailure(done.error))
        }
        return ControlResponse()
    }

    /// An engine request of an action, sent as the window sends it, so the app's own view of capture stays right.
    private func engineAction(_ command: RuntimeControl) async -> ControlResponse {
        guard tunnel.connection == .connected else {
            return .failure(.notConnected, "")
        }
        guard await tunnel.tunnelControl(command) != nil else {
            return .failure(.failed, "")
        }
        return ControlResponse()
    }

    private static func agentFailure(_ error: ControlErrorCode?) -> String {
        switch error {
        case .notFound:
            return "No subscription or node has that name."
        case .usage:
            return "The action is not possible now: several subscriptions have that node, or the mode needs rules this subscription does not have."
        case .unavailable:
            return "The action is not possible now: nothing is imported yet."
        case .timeout:
            return "AnixOps Net did not finish the action in time."
        case .notConnected:
            return "AnixOps Net is not connected."
        default:
            return "AnixOps Net could not take the action."
        }
    }

    /// The request as the access log shows it: an engine command, `logs`, or an action's kind.
    private static func logName(_ call: AgentCall, payload: String?) -> String {
        let object = payload
            .flatMap { $0.data(using: .utf8) }
            .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
        switch call {
        case .access:
            return "access"
        case .logs:
            return "logs"
        case .engine:
            return (object?["cmd"] as? String).map { String($0.prefix(40)) } ?? "engine"
        case .act:
            return (object?["action"] as? String).map { String($0.prefix(40)) } ?? "act"
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
