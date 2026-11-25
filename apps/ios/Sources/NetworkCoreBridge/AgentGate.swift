import Foundation

// AI tools' access to the app (docs/mcp-desktop.md). The MCP program `anixops-mcp`, bundled in the Mac app, asks
// over the command-line channel (`ControlRequest.Command.agent`). Any process of the user can reach that channel and
// call itself the MCP program, so the app decides here, not the program: nothing while the user has not allowed AI
// tools, of the engine only the read-only requests below, with redaction and dry runs forced on, and an action
// (`AgentAction`) only while the user allows actions and only once the user has confirmed it in the app.

/// What the MCP program asks with `ControlRequest.Command.agent`.
public enum AgentCall: String, Codable, Sendable {
    /// The user's settings (`AgentAccess`), answered whatever they are.
    case access
    /// One engine request (`payload`), through `AgentGate.engineRequest`.
    case engine
    /// The tunnel's recent diagnostic events.
    case logs
    /// One action (`payload`, `AgentAction`), taken once the user confirms it in the app.
    case act
}

/// How much of a captured request AI tools see.
public enum AgentContentLevel: String, Codable, CaseIterable, Identifiable, Sendable {
    /// Method, host, path, status, sizes and timing.
    case summary
    /// The summary and the headers, masked.
    case headers
    /// The summary, the headers and text bodies, masked.
    case bodies

    public var id: String {
        rawValue
    }
}

/// The user's settings for AI tools (设置 › 高级 › 外部工具访问). Everything is off until the user turns it on.
public struct AgentAccess: Codable, Equatable, Sendable {
    /// AI tools may read the client's state.
    public var read: Bool
    /// AI tools may ask for actions, each confirmed by the user in the app. Only together with `read`.
    public var act: Bool
    public var content: AgentContentLevel

    public init(read: Bool, act: Bool = false, content: AgentContentLevel = .summary) {
        self.read = read
        self.act = read && act
        self.content = content
    }

    public static let off = AgentAccess(read: false)

    static let readKey = "agentAccess.read"
    static let actKey = "agentAccess.act"
    static let contentKey = "agentAccess.content"

    public static func load(from defaults: UserDefaults = .standard) -> AgentAccess {
        AgentAccess(
            read: defaults.bool(forKey: readKey),
            act: defaults.bool(forKey: actKey),
            content: defaults.string(forKey: contentKey).flatMap(AgentContentLevel.init(rawValue:)) ?? .summary
        )
    }

    /// Saves the settings; turning reading off turns actions off with it.
    public func save(to defaults: UserDefaults = .standard) {
        defaults.set(read, forKey: Self.readKey)
        defaults.set(read && act, forKey: Self.actKey)
        defaults.set(content.rawValue, forKey: Self.contentKey)
    }
}

/// Which engine requests AI tools may make. The same table is `crates/client-mcp/policy.json`, which the MCP
/// program's tests check its requests against; `AgentGateTests` checks this one against it.
public enum AgentGate {
    public struct Rule: Equatable, Sendable {
        /// Fields the request may carry, besides `cmd`.
        public let fields: Set<String>
        /// Fields set to these values whatever the request says.
        public let forced: [String: Bool]

        public init(fields: Set<String> = [], forced: [String: Bool] = [:]) {
            self.fields = fields
            self.forced = forced
        }
    }

    public static let engine: [String: Rule] = [
        "traffic": Rule(),
        "connections": Rule(),
        "groups": Rule(),
        "test_route": Rule(fields: ["host", "port", "network", "process", "url"]),
        "rule_hits": Rule(),
        "reload_rules": Rule(fields: ["rules"], forced: ["dry_run": true]),
        "outbound_health": Rule(),
        "rule_sets": Rule(),
        "mitm_hosts": Rule(),
        "capture": Rule(),
        "capture_requests": Rule(fields: ["since_seq", "limit", "filter"], forced: ["redact": true]),
        "capture_exchange": Rule(fields: ["id", "index"], forced: ["redact": true]),
    ]

    public enum Refusal: Error, Equatable {
        /// Not a JSON object with a `cmd`, or a field of a kind no allowed request has.
        case malformed
        /// A command, or a field of an allowed command, that AI tools may not use.
        case notAllowed(String)

        /// For the MCP program, which passes it to the AI tool.
        public var message: String {
            switch self {
            case .malformed:
                return "The engine request is malformed."
            case let .notAllowed(name):
                return "AI tools may not use \(name)."
            }
        }
    }

    /// The request the engine gets for `text`: only an allowed command, only its allowed fields, each a string, a
    /// number, a switch or a list of strings, and the forced fields set.
    public static func engineRequest(_ text: String) -> Result<Data, Refusal> {
        guard let data = text.data(using: .utf8),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let command = object["cmd"] as? String
        else {
            return .failure(.malformed)
        }
        guard let rule = engine[command] else {
            return .failure(.notAllowed(command))
        }
        var request: [String: Any] = ["cmd": command]
        for (key, value) in object where key != "cmd" {
            guard rule.fields.contains(key) || rule.forced[key] != nil else {
                return .failure(.notAllowed(key))
            }
            guard isPlain(value) else {
                return .failure(.malformed)
            }
            request[key] = value
        }
        for (key, value) in rule.forced {
            request[key] = value
        }
        guard let gated = try? JSONSerialization.data(withJSONObject: request, options: [.sortedKeys]) else {
            return .failure(.malformed)
        }
        return .success(gated)
    }

    private static func isPlain(_ value: Any) -> Bool {
        if value is String || value is NSNumber {
            return true
        }
        if let list = value as? [Any] {
            return list.allSatisfy { $0 is String }
        }
        return false
    }
}

/// One line of `AgentCall.logs`, as the MCP program reads it.
public struct AgentLogLine: Codable, Equatable, Sendable {
    public var atMs: UInt64
    /// error, warn, info or debug.
    public var level: String
    public var message: String

    enum CodingKeys: String, CodingKey {
        case atMs = "at_ms"
        case level
        case message
    }

    public init(_ event: TunnelDiagnosticEvent) {
        atMs = UInt64(max(0, event.time.timeIntervalSince1970 * 1000))
        switch event.level {
        case .error:
            level = "error"
        case .warning:
            level = "warn"
        case .info:
            level = "info"
        case .debug:
            level = "debug"
        }
        message = event.message
    }
}

/// An action an AI tool asks for (`AgentCall.act`, docs/mcp-desktop.md §5.2): the same list as `Action` in
/// `crates/client-mcp/src/backend.rs`, and nothing else. The app takes it with the same code as the window's own
/// buttons, after the user confirmed it.
public enum AgentAction: Equatable, Sendable {
    case connect
    case disconnect
    case setMode(ProxyMode)
    /// A node by name, in the subscription `group` names when several have it.
    case selectNode(node: String, group: String?)
    /// Every subscription's nodes, as the window's test button.
    case testNodeDelays
    case setCapture(Bool)
    case clearCapture
    case closeConnection(UInt64)

    /// The action's name, as the MCP program sends it: the kind the user may allow for 15 minutes at once.
    public var kind: String {
        switch self {
        case .connect:
            return "connect"
        case .disconnect:
            return "disconnect"
        case .setMode:
            return "set_mode"
        case .selectNode:
            return "select_node"
        case .testNodeDelays:
            return "test_node_delays"
        case .setCapture:
            return "set_capture"
        case .clearCapture:
            return "clear_capture"
        case .closeConnection:
            return "close_connection"
        }
    }

    /// Every kind, for the tests.
    public static let kinds: Set<String> = [
        "connect", "disconnect", "set_mode", "select_node", "test_node_delays", "set_capture", "clear_capture",
        "close_connection",
    ]

    /// The action in `text` (`{"action": "...", ...}`): only a known action, with exactly its fields.
    public static func parse(_ text: String) -> Result<AgentAction, AgentGate.Refusal> {
        guard let data = text.data(using: .utf8),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let kind = object["action"] as? String
        else {
            return .failure(.malformed)
        }
        let fields = object.filter { $0.key != "action" }
        func only(_ allowed: Set<String>, required: Set<String> = []) -> AgentGate.Refusal? {
            if let extra = fields.keys.sorted().first(where: { !allowed.contains($0) }) {
                return .notAllowed(extra)
            }
            return required.isSubset(of: Set(fields.keys)) ? nil : .malformed
        }
        switch kind {
        case "connect", "disconnect", "test_node_delays", "clear_capture":
            if let refusal = only([]) {
                return .failure(refusal)
            }
            switch kind {
            case "connect":
                return .success(.connect)
            case "disconnect":
                return .success(.disconnect)
            case "test_node_delays":
                return .success(.testNodeDelays)
            default:
                return .success(.clearCapture)
            }
        case "set_mode":
            if let refusal = only(["mode"], required: ["mode"]) {
                return .failure(refusal)
            }
            guard let mode = (fields["mode"] as? String).flatMap(ProxyMode.init(rawValue:)) else {
                return .failure(.malformed)
            }
            return .success(.setMode(mode))
        case "select_node":
            if let refusal = only(["node", "group"], required: ["node"]) {
                return .failure(refusal)
            }
            guard let node = fields["node"] as? String,
                  !node.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            else {
                return .failure(.malformed)
            }
            let group = fields["group"]
            if let group, !(group is String), !(group is NSNull) {
                return .failure(.malformed)
            }
            return .success(.selectNode(node: node, group: group as? String))
        case "set_capture":
            if let refusal = only(["enabled"], required: ["enabled"]) {
                return .failure(refusal)
            }
            guard let enabled = fields["enabled"] as? NSNumber, CFGetTypeID(enabled) == CFBooleanGetTypeID() else {
                return .failure(.malformed)
            }
            return .success(.setCapture(enabled.boolValue))
        case "close_connection":
            if let refusal = only(["id"], required: ["id"]) {
                return .failure(refusal)
            }
            guard let id = fields["id"] as? NSNumber, CFGetTypeID(id) != CFBooleanGetTypeID(),
                  id.int64Value >= 0, id.doubleValue == Double(id.int64Value)
            else {
                return .failure(.malformed)
            }
            return .success(.closeConnection(UInt64(id.int64Value)))
        default:
            return .failure(.notAllowed(kind))
        }
    }
}

/// Kinds of action the user allowed for a while at once, with 「15 分钟内允许此类操作」. Kept in memory only: a
/// restart of the app, or turning actions off, ends every grant.
public struct AgentGrants: Equatable, Sendable {
    /// How long a grant lasts.
    public static let duration: TimeInterval = 15 * 60
    /// How long the confirmation waits for the user before it counts as a refusal.
    public static let confirmationTimeout: TimeInterval = 60

    private var until: [String: Date] = [:]

    public init() {}

    public func allows(_ kind: String, at now: Date = Date()) -> Bool {
        guard let end = until[kind] else {
            return false
        }
        return now < end
    }

    public mutating func grant(_ kind: String, at now: Date = Date()) {
        until[kind] = now.addingTimeInterval(Self.duration)
    }

    public mutating func revokeAll() {
        until.removeAll()
    }
}

/// The last calls of AI tools and what came of them (设置 › 高级 › 外部工具访问 › 访问记录). Kept on this Mac only, in
/// the app's preferences; never sent anywhere, not part of a diagnostics report.
public struct AgentAccessLog: Equatable, Sendable {
    public enum Outcome: String, Codable, Sendable {
        /// Answered, or the action was taken.
        case allowed
        /// Taken without asking, under a 15-minute grant.
        case granted
        /// The user refused it in the confirmation.
        case denied
        /// The user did not answer the confirmation in time.
        case noAnswer
        /// The user's settings do not allow it, or the gate refused it.
        case refused
    }

    public struct Entry: Codable, Equatable, Identifiable, Sendable {
        public var id: UUID
        public var at: Date
        /// The request: an engine command, `logs`, or an action's kind.
        public var call: String
        public var outcome: Outcome

        public init(id: UUID = UUID(), at: Date = Date(), call: String, outcome: Outcome) {
            self.id = id
            self.at = at
            self.call = call
            self.outcome = outcome
        }
    }

    /// The most entries kept; older ones are dropped.
    public static let limit = 200
    static let key = "agentAccess.log"

    /// Oldest first.
    public private(set) var entries: [Entry]

    public init(entries: [Entry] = []) {
        self.entries = Array(entries.suffix(Self.limit))
    }

    public static func load(from defaults: UserDefaults = .standard) -> AgentAccessLog {
        guard let data = defaults.data(forKey: key),
              let entries = try? JSONDecoder().decode([Entry].self, from: data)
        else {
            return AgentAccessLog()
        }
        return AgentAccessLog(entries: entries)
    }

    public mutating func append(_ entry: Entry) {
        entries.append(entry)
        if entries.count > Self.limit {
            entries.removeFirst(entries.count - Self.limit)
        }
    }

    public func save(to defaults: UserDefaults = .standard) {
        if let data = try? JSONEncoder().encode(entries) {
            defaults.set(data, forKey: Self.key)
        }
    }

    /// Adds one entry to the saved log.
    public static func record(_ call: String, _ outcome: Outcome, defaults: UserDefaults = .standard) {
        var log = load(from: defaults)
        log.append(Entry(call: call, outcome: outcome))
        log.save(to: defaults)
    }

    public static func clear(in defaults: UserDefaults = .standard) {
        defaults.removeObject(forKey: key)
    }
}
