import Foundation

/// Destination rewrites (the engine's `redirects`): matching TCP connections and UDP flows go to another address,
/// for example a game's server to a development server. Not secret; the app keeps them in `redirects.json` and copies
/// the enabled lines into the tunnel profile (`TunnelProfileKeys.redirects`). The engine applies them before routing,
/// so the routing rules then see the new address; UDP replies still come from the address the app asked for.
public struct TrafficRedirects: Codable, Equatable {
    /// The engine accepts at most 256 lines.
    public static let maxEntries = 256

    public enum Network: String, Codable, CaseIterable, Identifiable {
        case any
        case tcp
        case udp

        public var id: String {
            rawValue
        }
    }

    public struct Entry: Codable, Equatable, Identifiable {
        public var id: UUID
        /// The rule part of the engine line, such as `IP-CIDR,203.0.113.10/32` or
        /// `AND,((DOMAIN-SUFFIX,game.example.com),(PROTOCOL,UDP))`.
        public var match: String
        /// `host`, `host:port`, `[v6]:port` or `:port`.
        public var target: String
        public var enabled: Bool

        public init(id: UUID = UUID(), match: String, target: String, enabled: Bool = true) {
            self.id = id
            self.match = match
            self.target = target
            self.enabled = enabled
        }

        /// The engine line.
        public var line: String {
            "\(match),\(target)"
        }
    }

    public var entries: [Entry]

    public init(entries: [Entry] = []) {
        self.entries = entries
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        entries = try container.decodeIfPresent([Entry].self, forKey: .entries) ?? []
    }

    /// The lines the engine gets: the enabled entries, in order.
    public var lines: [String] {
        entries.filter(\.enabled).map(\.line)
    }

    public var isEmpty: Bool {
        lines.isEmpty
    }

    // MARK: - Editing

    /// Adds an entry from the editor's fields, or replaces the entry `replacing`. `source` is an address (`203.0.113.10`),
    /// a network (`203.0.113.0/24`), a domain (`game.example.com`, which also matches its subdomains), or a whole rule
    /// with its type (`DOMAIN,login.example.com`, `AND,((…),(…))`); `ports` is empty, a port or a `start-end` range.
    public func adding(
        source: String,
        ports: String,
        network: Network,
        target: String,
        replacing old: UUID? = nil
    ) throws -> TrafficRedirects {
        let match = try Self.match(source: source, ports: ports, network: network)
        let target = try Self.target(target)
        var copy = self
        if let old, let index = copy.entries.firstIndex(where: { $0.id == old }) {
            copy.entries[index].match = match
            copy.entries[index].target = target
        } else {
            guard copy.entries.count < Self.maxEntries else {
                throw TrafficRedirectsError.tooMany
            }
            copy.entries.append(Entry(match: match, target: target))
        }
        return copy
    }

    /// The rule part for the editor's fields.
    public static func match(source: String, ports: String, network: Network) throws -> String {
        var conditions: [String] = []
        let source = source.trimmingCharacters(in: .whitespaces)
        if !source.isEmpty {
            conditions.append(try condition(source))
        }
        let ports = ports.replacingOccurrences(of: " ", with: "")
        if !ports.isEmpty {
            guard isPortRange(ports) else {
                throw TrafficRedirectsError.invalidPort
            }
            conditions.append("DST-PORT,\(ports)")
        }
        switch network {
        case .any:
            break
        case .tcp:
            conditions.append("PROTOCOL,TCP")
        case .udp:
            conditions.append("PROTOCOL,UDP")
        }
        switch conditions.count {
        case 0:
            throw TrafficRedirectsError.emptyMatch
        case 1:
            return conditions[0]
        default:
            return "AND,(" + conditions.map { "(\($0))" }.joined(separator: ",") + ")"
        }
    }

    /// Rule types a typed rule may start with: those the engine matches without data only routing loads.
    static let ruleTypes: Set<String> = [
        "DOMAIN", "DOMAIN-SUFFIX", "DOMAIN-KEYWORD", "DOMAIN-WILDCARD", "IP-CIDR", "IP-CIDR6", "DST-PORT", "DEST-PORT",
        "PROTOCOL", "AND", "OR", "NOT",
    ]

    /// One condition from the source field.
    static func condition(_ source: String) throws -> String {
        if source.contains(",") {
            // A whole rule typed by hand; the engine checks the rest when the tunnel starts.
            let kind = source.split(separator: ",", maxSplits: 1)[0].trimmingCharacters(in: .whitespaces).uppercased()
            guard ruleTypes.contains(kind), !source.contains("\n") else {
                throw TrafficRedirectsError.invalidSource
            }
            if kind == "AND" || kind == "OR" || kind == "NOT" {
                guard source.filter({ $0 == "(" }).count == source.filter({ $0 == ")" }).count,
                      source.hasSuffix(")")
                else {
                    throw TrafficRedirectsError.invalidSource
                }
            } else {
                // `TYPE,value` only: a third field would be read as the target.
                let fields = source.split(separator: ",", omittingEmptySubsequences: false)
                guard fields.count == 2, !fields[1].trimmingCharacters(in: .whitespaces).isEmpty else {
                    throw TrafficRedirectsError.invalidSource
                }
            }
            return source
        }
        let parts = source.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        if DnsSettings.isAddress(parts[0]) {
            let v6 = parts[0].contains(":")
            let max = v6 ? 128 : 32
            let prefix: Int
            if parts.count == 1 {
                prefix = max
            } else if parts.count == 2, let value = Int(parts[1]), (0...max).contains(value) {
                prefix = value
            } else {
                throw TrafficRedirectsError.invalidSource
            }
            return "\(v6 ? "IP-CIDR6" : "IP-CIDR"),\(parts[0])/\(prefix)"
        }
        guard parts.count == 1, let domain = try? DnsSettings.domainName(source.hasPrefix(".") ? String(source.dropFirst()) : source) else {
            throw TrafficRedirectsError.invalidSource
        }
        return "DOMAIN-SUFFIX,\(domain)"
    }

    /// A normalized target: `host`, `host:port`, `[v6]`, `[v6]:port` or `:port`.
    public static func target(_ text: String) throws -> String {
        let value = text.trimmingCharacters(in: .whitespaces)
        if value.hasPrefix(":"), !value.dropFirst().contains(":") {
            guard isPort(String(value.dropFirst())) else {
                throw TrafficRedirectsError.invalidTarget
            }
            return value
        }
        if DnsSettings.isAddress(value) {
            return value
        }
        if value.hasPrefix("[") {
            guard let close = value.firstIndex(of: "]") else {
                throw TrafficRedirectsError.invalidTarget
            }
            let host = String(value[value.index(after: value.startIndex)..<close])
            let after = value[value.index(after: close)...]
            guard host.contains(":"), DnsSettings.isAddress(host),
                  after.isEmpty || (after.hasPrefix(":") && isPort(String(after.dropFirst())))
            else {
                throw TrafficRedirectsError.invalidTarget
            }
            return value
        }
        let pieces = value.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
        guard (1...2).contains(pieces.count), pieces.count == 1 || isPort(pieces[1]) else {
            throw TrafficRedirectsError.invalidTarget
        }
        if DnsSettings.isAddress(pieces[0]) {
            return value
        }
        guard let domain = try? DnsSettings.domainName(pieces[0]) else {
            throw TrafficRedirectsError.invalidTarget
        }
        return pieces.count == 2 ? "\(domain):\(pieces[1])" : domain
    }

    static func isPort(_ text: String) -> Bool {
        guard let port = UInt16(text) else {
            return false
        }
        return port > 0
    }

    static func isPortRange(_ text: String) -> Bool {
        let parts = text.split(separator: "-", omittingEmptySubsequences: false).map(String.init)
        guard (1...2).contains(parts.count), parts.allSatisfy(isPort) else {
            return false
        }
        return parts.count == 1 || UInt16(parts[0])! <= UInt16(parts[1])!
    }

    /// The editor's fields for an entry the editor made; nil for a rule it cannot show field by field.
    public static func fields(of entry: Entry) -> (source: String, ports: String, network: Network)? {
        var conditions = [entry.match]
        if entry.match.hasPrefix("AND,((") && entry.match.hasSuffix("))") {
            let inner = entry.match.dropFirst("AND,((".count).dropLast(2)
            conditions = inner.components(separatedBy: "),(")
        }
        var source = ""
        var ports = ""
        var network = Network.any
        for condition in conditions {
            let parts = condition.split(separator: ",", maxSplits: 1).map(String.init)
            guard parts.count == 2 else {
                return nil
            }
            switch parts[0] {
            case "IP-CIDR" where parts[1].hasSuffix("/32"), "IP-CIDR6" where parts[1].hasSuffix("/128"):
                source = String(parts[1].split(separator: "/")[0])
            case "IP-CIDR", "IP-CIDR6":
                source = parts[1]
            case "DOMAIN-SUFFIX":
                source = parts[1]
            case "DST-PORT":
                ports = parts[1]
            case "PROTOCOL" where parts[1] == "TCP":
                network = .tcp
            case "PROTOCOL" where parts[1] == "UDP":
                network = .udp
            default:
                return nil
            }
        }
        return (source, ports, network)
    }

    // MARK: - Storage

    public static var defaultURL: URL {
        get throws {
            try FileManager.default
                .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("redirects.json")
        }
    }

    public static func load(from url: URL) throws -> TrafficRedirects {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return TrafficRedirects()
        }
        return try JSONDecoder().decode(TrafficRedirects.self, from: Data(contentsOf: url))
    }

    public func save(to url: URL) throws {
        try JSONEncoder().encode(self).write(to: url, options: [.atomic])
    }

    /// The tunnel profile value under `TunnelProfileKeys.redirects`: a JSON array of the engine lines; nil when no
    /// entry is enabled.
    public func profileValue() throws -> Data? {
        isEmpty ? nil : try JSONEncoder().encode(lines)
    }

    /// The engine lines a profile value holds; empty when there are none.
    public static func lines(fromProfileValue value: Data?) -> [String] {
        value.flatMap { try? JSONDecoder().decode([String].self, from: $0) } ?? []
    }
}

/// Why a redirect was refused. The messages are fixed text without the values typed.
public enum TrafficRedirectsError: Error, Equatable {
    case invalidSource
    case invalidPort
    case invalidTarget
    case emptyMatch
    case tooMany

    public var message: String {
        switch self {
        case .invalidSource:
            return String(localized: "原地址应为 IP、网段、域名，或以 DOMAIN、IP-CIDR、AND 等开头的规则")
        case .invalidPort:
            return String(localized: "端口应为 1–65535，或 7000-7100 这样的范围")
        case .invalidTarget:
            return String(localized: "新地址应为 IP 或域名，可带 :端口；只改端口写 :端口")
        case .emptyMatch:
            return String(localized: "原地址、端口和协议至少填一项")
        case .tooMany:
            return String(localized: "已达到数量上限")
        }
    }
}
