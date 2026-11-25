import Foundation

// AI tools' access to the app (docs/mcp-desktop.md). The MCP program `anixops-mcp`, bundled in the Mac app, asks
// over the command-line channel (`ControlRequest.Command.agent`). Any process of the user can reach that channel and
// call itself the MCP program, so the app decides here, not the program: nothing while the user has not allowed AI
// tools, and of the engine only the read-only requests below, with redaction and dry runs forced on.

/// What the MCP program asks with `ControlRequest.Command.agent`.
public enum AgentCall: String, Codable, Sendable {
    /// The user's settings (`AgentAccess`), answered whatever they are.
    case access
    /// One engine request (`payload`), through `AgentGate.engineRequest`.
    case engine
    /// The tunnel's recent diagnostic events.
    case logs
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
    /// AI tools may ask for actions; not offered yet, always false.
    public var act: Bool
    public var content: AgentContentLevel

    public init(read: Bool, act: Bool = false, content: AgentContentLevel = .summary) {
        self.read = read
        self.act = act
        self.content = content
    }

    public static let off = AgentAccess(read: false)

    static let readKey = "agentAccess.read"
    static let contentKey = "agentAccess.content"

    public static func load(from defaults: UserDefaults = .standard) -> AgentAccess {
        AgentAccess(
            read: defaults.bool(forKey: readKey),
            content: defaults.string(forKey: contentKey).flatMap(AgentContentLevel.init(rawValue:)) ?? .summary
        )
    }

    public func save(to defaults: UserDefaults = .standard) {
        defaults.set(read, forKey: Self.readKey)
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
