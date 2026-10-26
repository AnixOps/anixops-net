import Foundation

/// Requests to the running engine (`networkcore_ios_runtime_control`, feature set 2) and its answers: live traffic
/// and the connection list. The app sends a request as `TunnelAppMessage.control`, the extension hands it to the
/// runtime and returns the runtime's JSON unchanged.
public enum RuntimeControl {
    case traffic
    case connections
    case closeConnection(UInt64)
    case closeAll
    case mode
    /// New connections only; open ones keep their route until `closeAll`.
    case setMode(ProxyMode)
    case groups
    /// Picks the member of a `select` group.
    case select(group: String, member: String)

    public var request: Data {
        let object: [String: Any]
        switch self {
        case .traffic:
            object = ["cmd": "traffic"]
        case .connections:
            object = ["cmd": "connections"]
        case let .closeConnection(id):
            object = ["cmd": "close_connection", "id": id]
        case .closeAll:
            object = ["cmd": "close_all"]
        case .mode:
            object = ["cmd": "mode"]
        case let .setMode(mode):
            object = ["cmd": "set_mode", "mode": mode.rawValue]
        case .groups:
            object = ["cmd": "groups"]
        case let .select(group, member):
            object = ["cmd": "select", "group": group, "member": member]
        }
        return (try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])) ?? Data()
    }

    /// The `traffic` answer: totals since the tunnel started and the current rates in bytes per second.
    public struct Traffic: Decodable, Equatable {
        public let uploadTotal: UInt64
        public let downloadTotal: UInt64
        public let uploadRate: UInt64
        public let downloadRate: UInt64
        public let active: Int

        enum CodingKeys: String, CodingKey {
            case uploadTotal = "upload_total"
            case downloadTotal = "download_total"
            case uploadRate = "upload_rate"
            case downloadRate = "download_rate"
            case active
        }

        public init(uploadTotal: UInt64 = 0, downloadTotal: UInt64 = 0, uploadRate: UInt64 = 0, downloadRate: UInt64 = 0, active: Int = 0) {
            self.uploadTotal = uploadTotal
            self.downloadTotal = downloadTotal
            self.uploadRate = uploadRate
            self.downloadRate = downloadRate
            self.active = active
        }
    }

    /// One connection of the `connections` answer.
    public struct Connection: Decodable, Equatable, Identifiable {
        public let id: UInt64
        /// `tcp` or `udp`.
        public let network: String
        /// The requested domain, or the TLS SNI / HTTP Host sniffed from an address.
        public let host: String?
        /// `address:port` or `domain:port` as requested.
        public let destination: String
        /// The rule that matched, for example `DOMAIN-SUFFIX,google.com`, `FINAL` or `MODE,global`.
        public let rule: String
        /// The policy groups passed through, ending with the outbound.
        public let chain: [String]
        /// A node id, `DIRECT` or `REJECT`.
        public let outbound: String
        public let startedAt: Date
        public let endedAt: Date?
        public let upload: UInt64
        public let download: UInt64
        /// `rejected` or `connect_failed`.
        public let error: String?

        enum CodingKeys: String, CodingKey {
            case id
            case network
            case host
            case destination
            case rule
            case chain
            case outbound
            case startedAtMs = "started_at_ms"
            case endedAtMs = "ended_at_ms"
            case upload
            case download
            case error
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            id = try container.decode(UInt64.self, forKey: .id)
            network = try container.decode(String.self, forKey: .network)
            host = try container.decodeIfPresent(String.self, forKey: .host)
            destination = try container.decode(String.self, forKey: .destination)
            rule = try container.decode(String.self, forKey: .rule)
            chain = try container.decodeIfPresent([String].self, forKey: .chain) ?? []
            outbound = try container.decode(String.self, forKey: .outbound)
            startedAt = Self.date(try container.decode(UInt64.self, forKey: .startedAtMs))
            endedAt = try container.decodeIfPresent(UInt64.self, forKey: .endedAtMs).map(Self.date)
            upload = try container.decode(UInt64.self, forKey: .upload)
            download = try container.decode(UInt64.self, forKey: .download)
            error = try container.decodeIfPresent(String.self, forKey: .error)
        }

        private static func date(_ millis: UInt64) -> Date {
            Date(timeIntervalSince1970: Double(millis) / 1000)
        }

        /// What the list shows as the target: the host name with the destination's port, otherwise the destination.
        public var target: String {
            guard let host, !host.isEmpty, !destination.hasPrefix(host) else {
                return destination
            }
            guard let colon = destination.lastIndex(of: ":") else {
                return host
            }
            return host + destination[colon...]
        }

        public var isActive: Bool {
            endedAt == nil
        }

        /// The search box of the connection list: every word must appear in the target, rule, chain or outbound.
        public func matches(_ query: String) -> Bool {
            let words = query.split(whereSeparator: \.isWhitespace)
            guard !words.isEmpty else {
                return true
            }
            let fields = [target, destination, rule, outbound, network] + chain
            return words.allSatisfy { word in
                fields.contains { $0.localizedCaseInsensitiveContains(word) }
            }
        }
    }

    /// The `connections` answer: active connections oldest first, then up to 128 closed ones newest first.
    public struct Connections: Decodable, Equatable {
        public let traffic: Traffic
        public let active: [Connection]
        public let closed: [Connection]

        public init(traffic: Traffic = Traffic(), active: [Connection] = [], closed: [Connection] = []) {
            self.traffic = traffic
            self.active = active
            self.closed = closed
        }
    }

    /// The `mode` and `set_mode` answers.
    public struct ModeAnswer: Decodable, Equatable {
        public let mode: ProxyMode
    }

    /// The `select` answer: the member the group uses now.
    public struct SelectAnswer: Decodable, Equatable {
        public let now: String
    }

    /// The `groups` answer: every policy group with the member it uses now and the members' last measured delay.
    public struct Groups: Decodable, Equatable {
        public struct Group: Decodable, Equatable, Identifiable {
            public struct Member: Decodable, Equatable {
                public let name: String
                public let latencyMs: UInt32?

                enum CodingKeys: String, CodingKey {
                    case name
                    case latencyMs = "latency_ms"
                }
            }

            public let id: String
            /// `select`, `url-test` or `fallback`.
            public let type: String
            public let now: String?
            public let members: [Member]
        }

        public let groups: [Group]
    }

    public static func decode<Answer: Decodable>(_ type: Answer.Type, from data: Data?) -> Answer? {
        data.flatMap { try? JSONDecoder().decode(type, from: $0) }
    }

    public static func decodeTraffic(_ data: Data) -> Traffic? {
        try? JSONDecoder().decode(Traffic.self, from: data)
    }

    public static func decodeConnections(_ data: Data) -> Connections? {
        try? JSONDecoder().decode(Connections.self, from: data)
    }
}
