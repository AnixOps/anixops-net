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
    /// Refuses UDP to port 443 for new flows when on, so apps fall back from QUIC to TCP (`{"block_quic": bool}`).
    case setBlockQuic(Bool)
    case groups
    /// Picks the member of a `select` group.
    case select(group: String, member: String)
    case ruleSets
    /// Downloads every remote rule set now.
    case updateRuleSets
    /// Plugin scripts that run outside HTTP flows (`cron`, `network-changed`, `generic`).
    case scripts
    /// What scripts posted since the last call; each notification comes once.
    case scriptNotifications
    /// Runs the `network-changed` scripts; the extension sends it when the device's network changes.
    case networkChanged
    /// Drops the sessions the outbounds keep (shared QUIC connections, idle pools) and, with `closeConnections`, the
    /// open connections, after the network under the tunnel changed or the device woke from a long sleep
    /// (`{"reset": count, "closed": count}`).
    case resetNetwork(closeConnections: Bool)
    /// The physical network under the tunnel, for `subnet` policy groups (`{"changed": bool}`); nil fields stay
    /// unknown. The extension sends it with every path, before `resetNetwork`.
    case setNetwork(NetworkFacts?)
    /// Runs one scheduled script now.
    case runScript(tag: String)
    /// Dial health of the nodes that keep it (`OutboundHealth`).
    case outboundHealth
    /// Lets a node that stopped after repeated rejections dial again (`{"resumed": bool}`).
    case resumeOutbound(node: String)
    /// The engine's metrics in the Prometheus text format (`{"content_type", "text"}`). `anonymous` replaces
    /// policy group, member and rule set names with their positions, for text that leaves the device.
    case metrics(anonymous: Bool)
    /// Swaps the engine's rule lines in place (`reload_rules`): the whole new list ending with `FINAL`, naming only
    /// the running nodes, groups and rule sets. Open connections keep going; `dryRun` only checks the list.
    case reloadRules([String], dryRun: Bool)
    /// Whether capture (抓包) is on and how much of its memory budget it uses (`PacketCapture.Status`).
    case captureStatus
    /// Turns capture on or off for new connections; what was captured stays until `clearCapture`.
    case setCapture(Bool)
    /// Forgets everything captured.
    case clearCapture
    /// The bytes and decrypted HTTP messages kept of one connection (`{"capture": … | null}`).
    case captureConnection(UInt64)
    /// Everything captured, or one connection, as a pcapng or HAR file (`PacketCapture.ExportAnswer`).
    case exportCapture(PacketCapture.ExportFormat, id: UInt64?)
    /// One captured request (`exchange` of connection `id`) as cURL, code, Postman, Insomnia or HAR; `redact` hides
    /// cookies, tokens and passwords.
    case exportRequest(PacketCapture.RequestExport, id: UInt64, exchange: Int, redact: Bool)
    /// Where saved sessions go and the key that seals them; set by the extension after the engine starts.
    case setCaptureStorage(dir: String, keyBase64: String, retentionDays: Int)
    /// Seals what is captured now into a session file.
    case saveCapture(name: String?)
    case captureSessions
    /// Replaces what is captured with a saved session (and stops live capture).
    case loadCapture(String)
    case deleteCapture(String)
    /// One row per decrypted HTTP exchange matching `filter` (`CaptureFilter`): ended after `sinceSeq`, or, with
    /// `sinceRev`, every exchange that changed after it, including those still under way.
    case captureRequests(sinceSeq: UInt64, sinceRev: UInt64? = nil, limit: Int, filter: String)
    /// Exchange `index` of connection `id` with its event-stream or WebSocket messages from `sinceMessage` on
    /// (`PacketCapture.ExchangeAnswer`).
    case captureExchange(id: UInt64, index: Int, sinceMessage: UInt64)
    /// The HTTP/2 frame headers kept of connection `id`, from frame `since` on (`PacketCapture.H2FramesAnswer`).
    case captureH2Frames(id: UInt64, since: UInt64)
    /// The engine opened to phones on the local network that set this Mac as their Wi-Fi proxy (`LanShare`).
    case lanShare
    /// Starts (or restarts) the LAN share on `port`; `0` picks a free one.
    case startLanShare(port: UInt16)
    case stopLanShare
    /// The `127.0.0.1` listener the tunnel registers as the system proxy (`TrafficTakeover`).
    case systemProxy
    /// Starts it on `port`, or on a free port when that one is taken; the answer says which.
    case startSystemProxy(port: UInt16)
    case stopSystemProxy

    /// The port the system proxy listener asks for first.
    public static let systemProxyPort: UInt16 = 8890

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
        case let .setBlockQuic(on):
            object = ["cmd": "set_block_quic", "enabled": on]
        case .groups:
            object = ["cmd": "groups"]
        case let .select(group, member):
            object = ["cmd": "select", "group": group, "member": member]
        case .ruleSets:
            object = ["cmd": "rule_sets"]
        case .updateRuleSets:
            object = ["cmd": "update_rule_sets"]
        case .scripts:
            object = ["cmd": "scripts"]
        case .scriptNotifications:
            object = ["cmd": "script_notifications"]
        case .networkChanged:
            object = ["cmd": "network_changed"]
        case let .resetNetwork(closeConnections):
            object = ["cmd": "reset_network", "close_connections": closeConnections]
        case let .setNetwork(facts):
            var request = facts?.fields ?? [:]
            request["cmd"] = "set_network"
            object = request
        case let .runScript(tag):
            object = ["cmd": "run_script", "tag": tag]
        case .outboundHealth:
            object = ["cmd": "outbound_health"]
        case let .resumeOutbound(node):
            object = ["cmd": "resume_outbound", "node": node]
        case let .metrics(anonymous):
            object = ["cmd": "metrics", "anonymous": anonymous]
        case let .reloadRules(rules, dryRun):
            object = ["cmd": "reload_rules", "rules": rules, "dry_run": dryRun]
        case .captureStatus:
            object = ["cmd": "capture"]
        case let .setCapture(on):
            object = ["cmd": "set_capture", "enabled": on]
        case .clearCapture:
            object = ["cmd": "clear_capture"]
        case let .captureConnection(id):
            object = ["cmd": "capture_connection", "id": id]
        case let .exportCapture(format, id):
            var request: [String: Any] = ["cmd": "export_capture", "format": format.rawValue]
            if let id {
                request["id"] = id
            }
            object = request
        case let .exportRequest(export, id, exchange, redact):
            var request: [String: Any] = [
                "cmd": "export_capture", "format": export.format, "id": id, "exchange": exchange, "redact": redact,
            ]
            if let language = export.language {
                request["language"] = language.rawValue
            }
            object = request
        case let .setCaptureStorage(dir, keyBase64, retentionDays):
            object = ["cmd": "set_capture_storage", "dir": dir, "key_base64": keyBase64, "retention_days": retentionDays]
        case let .saveCapture(name):
            var request: [String: Any] = ["cmd": "save_capture"]
            if let name, !name.isEmpty {
                request["name"] = name
            }
            object = request
        case .captureSessions:
            object = ["cmd": "capture_sessions"]
        case let .loadCapture(session):
            object = ["cmd": "load_capture", "session": session]
        case let .deleteCapture(session):
            object = ["cmd": "delete_capture", "session": session]
        case let .captureRequests(sinceSeq, sinceRev, limit, filter):
            var request: [String: Any] = ["cmd": "capture_requests", "since_seq": sinceSeq, "limit": limit]
            if let sinceRev {
                request["since_rev"] = sinceRev
            }
            if !filter.isEmpty {
                request["filter"] = filter
            }
            object = request
        case let .captureExchange(id, index, sinceMessage):
            object = ["cmd": "capture_exchange", "id": id, "index": index, "since_message": sinceMessage]
        case let .captureH2Frames(id, since):
            object = ["cmd": "capture_h2_frames", "id": id, "since": since]
        case .lanShare:
            object = ["cmd": "lan_share"]
        case let .startLanShare(port):
            object = ["cmd": "start_lan_share", "port": Int(port)]
        case .stopLanShare:
            object = ["cmd": "stop_lan_share"]
        case .systemProxy:
            object = ["cmd": "system_proxy"]
        case let .startSystemProxy(port):
            object = ["cmd": "start_system_proxy", "port": Int(port)]
        case .stopSystemProxy:
            object = ["cmd": "stop_system_proxy"]
        }
        return (try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])) ?? Data()
    }

    /// The answer to the LAN share commands: whether it runs, on which port, the Mac's private IPv4 addresses (the
    /// likeliest first) and whether the phone setup page offers the HTTPS decryption root.
    public struct LanShare: Decodable, Equatable {
        public let running: Bool
        public let port: UInt16?
        public let addresses: [String]
        public let url: String?
        public let pageHost: String
        public let httpsDecryption: Bool
        public let certificate: Bool
        public let active: UInt64
        public let pages: UInt64

        enum CodingKeys: String, CodingKey {
            case running, port, addresses, url, certificate, active, pages
            case pageHost = "page_host"
            case httpsDecryption = "https_decryption"
        }

        /// The setup page address on `address`, what the QR code holds.
        public func setupURL(address: String) -> String? {
            guard running, let port else {
                return nil
            }
            return "http://\(address):\(port)/"
        }
    }

    /// The answer to the system proxy commands: whether the listener runs and where.
    public struct SystemProxy: Decodable, Equatable {
        public let running: Bool
        public let host: String?
        public let port: UInt16?
        public let httpsDecryption: Bool
        public let accepted: UInt64
        public let active: UInt64

        enum CodingKeys: String, CodingKey {
            case running, host, port, accepted, active
            case httpsDecryption = "https_decryption"
        }
    }

    /// The `metrics` answer.
    public struct Metrics: Decodable, Equatable {
        public let contentType: String
        public let text: String

        enum CodingKeys: String, CodingKey {
            case contentType = "content_type"
            case text
        }
    }

    /// The `set_network` answer: whether the network the `subnet` groups match changed.
    public struct NetworkSet: Decodable, Equatable {
        public let changed: Bool

        public init(changed: Bool) {
            self.changed = changed
        }
    }

    /// The `reset_network` answer.
    public struct NetworkReset: Decodable, Equatable {
        public let reset: Int
        public let closed: Int

        public init(reset: Int, closed: Int) {
            self.reset = reset
            self.closed = closed
        }
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
        /// Where a redirect sent the connection instead (`TrafficRedirects`), and the line that matched.
        public let redirect: Redirect?
        /// Capture was on as the connection opened, so `captureConnection` may have its bytes.
        public let captured: Bool

        public struct Redirect: Decodable, Equatable {
            public let destination: String
            public let rule: String
        }

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
            case redirect
            case captured
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
            redirect = try container.decodeIfPresent(Redirect.self, forKey: .redirect)
            captured = try container.decodeIfPresent(Bool.self, forKey: .captured) ?? false
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
            let fields = [target, destination, rule, outbound, network] + chain + [redirect?.destination].compactMap { $0 }
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

    /// The `set_block_quic` answer; an engine without the command answers nothing.
    public struct BlockQuicAnswer: Decodable, Equatable {
        public let blockQuic: Bool

        enum CodingKeys: String, CodingKey {
            case blockQuic = "block_quic"
        }
    }

    /// The `reload_rules` answer. `reloaded` is true once the new rules are in force; a refused list carries the
    /// 0-based index of its first bad line and a code (`syntax`, `unknown_target`, `unknown_rule_set`,
    /// `final_not_last`, `final_missing`, `too_many_rules`, `line_too_long`, `invalid`) and leaves the running rules.
    public struct ReloadAnswer: Codable, Equatable {
        public struct Rejection: Codable, Equatable {
            public let line: Int?
            public let code: String

            public init(line: Int?, code: String) {
                self.line = line
                self.code = code
            }
        }

        public let reloaded: Bool
        public let rules: Int?
        public let generation: UInt64?
        public let error: Rejection?
        /// Set by the extension, never by the engine: the change needs a restart (nodes, groups or rule sets
        /// differ, or the running engine has no rules to swap).
        public let restart: Bool?

        public init(reloaded: Bool, rules: Int? = nil, generation: UInt64? = nil, error: Rejection? = nil, restart: Bool? = nil) {
            self.reloaded = reloaded
            self.rules = rules
            self.generation = generation
            self.error = error
            self.restart = restart
        }
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

    /// The `rule_sets` answer.
    public struct RuleSets: Decodable, Equatable {
        public struct Status: Decodable, Equatable, Identifiable {
            public let name: String
            /// `inline` or `remote`.
            public let source: String
            public let entries: Int
            public let updatedAt: Date?
            public let lastError: String?

            public var id: String {
                name
            }

            enum CodingKeys: String, CodingKey {
                case name
                case source
                case entries
                case updatedAtMs = "updated_at_ms"
                case lastError = "last_error"
            }

            public init(from decoder: Decoder) throws {
                let container = try decoder.container(keyedBy: CodingKeys.self)
                name = try container.decode(String.self, forKey: .name)
                source = try container.decode(String.self, forKey: .source)
                entries = try container.decode(Int.self, forKey: .entries)
                updatedAt = try container.decodeIfPresent(UInt64.self, forKey: .updatedAtMs)
                    .map { Date(timeIntervalSince1970: Double($0) / 1000) }
                lastError = try container.decodeIfPresent(String.self, forKey: .lastError)
            }
        }

        public let ruleSets: [Status]

        enum CodingKeys: String, CodingKey {
            case ruleSets = "rule_sets"
        }
    }

    /// The `scripts` answer.
    public struct ScheduledScripts: Decodable, Equatable {
        public struct Script: Decodable, Equatable, Identifiable {
            public let tag: String
            /// `cron`, `network-changed` or `generic`.
            public let trigger: String
            public let cron: String?
            /// Local time of the next cron run, RFC 9557.
            public let nextRun: String?
            /// `done`, `finished_without_done` or a failure code, since the tunnel started.
            public let lastStatus: String?

            public var id: String {
                tag
            }

            enum CodingKeys: String, CodingKey {
                case tag, trigger, cron
                case nextRun = "next_run"
                case lastStatus = "last_status"
            }
        }

        public let scripts: [Script]
    }

    /// The `script_notifications` answer.
    public struct ScriptNotifications: Decodable, Equatable {
        public struct Notification: Decodable, Equatable {
            public let title: String
            public let subtitle: String
            public let content: String
            public let openURL: String?

            enum CodingKeys: String, CodingKey {
                case title, subtitle, content
                case openURL = "open_url"
            }
        }

        public let notifications: [Notification]
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
