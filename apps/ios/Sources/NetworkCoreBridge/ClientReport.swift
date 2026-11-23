import Foundation

/// Per-node dial health from the engine (`outbound_health`): how dials ended, whether a node stopped after its
/// server kept rejecting this client, and the device clock offset it measured. Only outbounds that keep such
/// counters answer (today the private ANX outbound). Counters only; never destinations.
public struct OutboundHealth: Decodable, Equatable {
    public let outbounds: [Entry]

    public struct Entry: Decodable, Equatable {
        /// The engine's node id (`proxy` for a config with a single outbound).
        public let node: String
        public let `protocol`: String
        public let health: Health
    }

    public struct Health: Decodable, Equatable {
        public let counts: Counts
        public let consecutiveFallbacks: UInt32
        /// The server kept rejecting this client, so the node stopped dialing until the user retries.
        public let stopped: Bool
        /// Server time minus device time, in seconds, when the node measured it.
        public let clockSkewSecs: Int64?
        /// `credentials_or_clock` or `clock` while something needs the user; nil otherwise.
        public let hint: String?

        enum CodingKeys: String, CodingKey {
            case counts
            case consecutiveFallbacks = "consecutive_fallbacks"
            case stopped
            case clockSkewSecs = "clock_skew_secs"
            case hint
        }

        public init(counts: Counts, consecutiveFallbacks: UInt32 = 0, stopped: Bool = false, clockSkewSecs: Int64? = nil, hint: String? = nil) {
            self.counts = counts
            self.consecutiveFallbacks = consecutiveFallbacks
            self.stopped = stopped
            self.clockSkewSecs = clockSkewSecs
            self.hint = hint
        }
    }

    /// Dial outcomes since the node started.
    public struct Counts: Codable, Equatable {
        public var ok: UInt64
        public var fallback: UInt64
        public var timeout: UInt64
        public var refused: UInt64
        public var reset: UInt64
        public var afterHandshake: UInt64
        public var keepaliveLost: UInt64
        public var other: UInt64

        enum CodingKeys: String, CodingKey {
            case ok
            case fallback
            case timeout
            case refused
            case reset
            case afterHandshake = "after_handshake"
            case keepaliveLost = "keepalive_lost"
            case other
        }

        public init(ok: UInt64 = 0, fallback: UInt64 = 0, timeout: UInt64 = 0, refused: UInt64 = 0, reset: UInt64 = 0,
                    afterHandshake: UInt64 = 0, keepaliveLost: UInt64 = 0, other: UInt64 = 0) {
            self.ok = ok
            self.fallback = fallback
            self.timeout = timeout
            self.refused = refused
            self.reset = reset
            self.afterHandshake = afterHandshake
            self.keepaliveLost = keepaliveLost
            self.other = other
        }

        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            func value(_ key: CodingKeys) throws -> UInt64 {
                try c.decodeIfPresent(UInt64.self, forKey: key) ?? 0
            }
            self.init(
                ok: try value(.ok),
                fallback: try value(.fallback),
                timeout: try value(.timeout),
                refused: try value(.refused),
                reset: try value(.reset),
                afterHandshake: try value(.afterHandshake),
                keepaliveLost: try value(.keepaliveLost),
                other: try value(.other)
            )
        }

        var all: [UInt64] {
            [ok, fallback, timeout, refused, reset, afterHandshake, keepaliveLost, other]
        }

        public var isZero: Bool {
            all.allSatisfy { $0 == 0 }
        }

        /// What happened since `earlier`. The engine's counters restart with the tunnel, so a counter that went
        /// down counts from zero.
        public func since(_ earlier: Counts?) -> Counts {
            guard let earlier else {
                return self
            }
            func delta(_ now: UInt64, _ then: UInt64) -> UInt64 {
                now >= then ? now - then : now
            }
            // A restart resets every counter; if any went down, take the whole current value.
            if zip(all, earlier.all).contains(where: { $0 < $1 }) {
                return self
            }
            return Counts(
                ok: delta(ok, earlier.ok),
                fallback: delta(fallback, earlier.fallback),
                timeout: delta(timeout, earlier.timeout),
                refused: delta(refused, earlier.refused),
                reset: delta(reset, earlier.reset),
                afterHandshake: delta(afterHandshake, earlier.afterHandshake),
                keepaliveLost: delta(keepaliveLost, earlier.keepaliveLost),
                other: delta(other, earlier.other)
            )
        }

        /// Each counter capped at the panel's limit.
        func capped(at limit: UInt64) -> Counts {
            Counts(
                ok: min(ok, limit), fallback: min(fallback, limit), timeout: min(timeout, limit),
                refused: min(refused, limit), reset: min(reset, limit), afterHandshake: min(afterHandshake, limit),
                keepaliveLost: min(keepaliveLost, limit), other: min(other, limit)
            )
        }
    }

    /// Nodes that stopped and need the user: a new subscription, the right time, or a retry.
    public var stopped: [Entry] {
        outbounds.filter(\.health.stopped)
    }
}

extension SubscriptionGroups {
    /// Where an engine node came from: its subscription link and the node as imported. The single-outbound
    /// config names its node `proxy`; linked groups' nodes run as `prefix + id` (`linkedGroups()`).
    public func origin(ofRuntimeNode id: String) -> (group: SubscriptionGroup, node: NativeImportReport.Node)? {
        guard let active else {
            return nil
        }
        if let node = active.library.node(id: id) {
            return (active, node)
        }
        if id == ClientReport.singleNode, let node = active.library.selected {
            return (active, node)
        }
        for link in linkedGroups() where id.hasPrefix(link.prefix) {
            if let group = group(id: link.groupID), let node = group.library.node(id: String(id.dropFirst(link.prefix.count))) {
                return (group, node)
            }
        }
        return nil
    }
}

/// The panel report of client-side dial failures: `POST <subscription link without query and extension>/report`,
/// for example `https://panel.example.com/s/abc123/report` for `https://panel.example.com/s/abc123.yaml?flag=x`
/// (the token is in the path). Counts only, at most one report per link every `minimumInterval`; the panel answers
/// 204, 404 for an unknown token, or 429 when it is too soon (the report is then dropped). Only nodes of the
/// private ANX protocol are reported, since only panels that run the ANX side hand them out: links of other
/// providers never get a report. Nothing is sent unless the user turned the report on (`ClientReportStore`).
public enum ClientReport {
    /// The engine's node id for a config with one outbound.
    public static let singleNode = "proxy"
    /// The protocols whose health goes to the panel.
    public static let reportedProtocols: Set<String> = ["anx"]
    /// The panel accepts at most one report per token per ten minutes; the app aims for about one an hour.
    public static let minimumInterval: TimeInterval = 10 * 60
    public static let interval: ClosedRange<TimeInterval> = 50 * 60 ... 70 * 60
    public static let maxNodes = 200
    public static let maxCount: UInt64 = 1_000_000
    public static let windowSecs: ClosedRange<Int> = 60 ... 86_400

    /// The report endpoint for a subscription link: the link without query, fragment and the last segment's
    /// extension, plus `/report`. Nil for links that are not http(s) or have no path to carry a token.
    // network-behavior: 连接质量报告
    public static func endpoint(forSubscription link: String) -> URL? {
        guard var components = URLComponents(string: link),
              let scheme = components.scheme?.lowercased(), scheme == "https" || scheme == "http",
              components.host?.isEmpty == false
        else {
            return nil
        }
        var segments = components.percentEncodedPath.split(separator: "/", omittingEmptySubsequences: true).map(String.init)
        guard var last = segments.popLast() else {
            return nil
        }
        if let dot = last.lastIndex(of: "."), dot != last.startIndex {
            last = String(last[..<dot])
        }
        guard !last.isEmpty else {
            return nil
        }
        segments.append(last)
        components.percentEncodedPath = "/" + (segments + ["report"]).joined(separator: "/")
        components.query = nil
        components.fragment = nil
        components.user = nil
        components.password = nil
        return components.url
    }

    public struct Body: Encodable, Equatable {
        public let client: String
        public let platform: String
        public let network: String
        public let windowSecs: Int
        public let nodes: [Node]

        enum CodingKeys: String, CodingKey {
            case client
            case platform
            case network
            case windowSecs = "window_secs"
            case nodes
        }

        public struct Node: Encodable, Equatable {
            public let node: String
            public let server: String
            public let proto: String
            public let counts: Counts
            public let stopped: Bool
            public let clockSkewSecs: Int64?

            enum CodingKeys: String, CodingKey {
                case node
                case server
                case proto
                case stopped
                case clockSkewSecs = "clock_skew_secs"
            }

            public func encode(to encoder: Encoder) throws {
                var c = encoder.container(keyedBy: CodingKeys.self)
                try c.encode(node, forKey: .node)
                try c.encode(server, forKey: .server)
                try c.encode(proto, forKey: .proto)
                try c.encode(stopped, forKey: .stopped)
                // Explicit null, as the panel's schema lists it.
                if let clockSkewSecs {
                    try c.encode(clockSkewSecs, forKey: .clockSkewSecs)
                } else {
                    try c.encodeNil(forKey: .clockSkewSecs)
                }
                try counts.encode(to: encoder)
            }
        }

        public typealias Counts = OutboundHealth.Counts

        public func encoded() -> Data {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            return (try? encoder.encode(self)) ?? Data()
        }
    }

    /// One report per subscription link: the nodes of that link with anything new since `previous` (counters
    /// restart with the tunnel), or still stopped. Nodes the app cannot place in a subscription are left out.
    public static func bodies(
        health: OutboundHealth,
        groups: SubscriptionGroups,
        previous: [String: OutboundHealth.Counts],
        window: TimeInterval,
        client: String,
        platform: String,
        network: String
    ) -> [URL: Body] {
        var byEndpoint: [URL: [Body.Node]] = [:]
        for entry in health.outbounds where reportedProtocols.contains(entry.protocol) {
            guard let origin = groups.origin(ofRuntimeNode: entry.node),
                  let link = origin.group.url,
                  let endpoint = endpoint(forSubscription: link)
            else {
                continue
            }
            let delta = entry.health.counts.since(previous[entry.node])
            guard !delta.isZero || entry.health.stopped else {
                continue
            }
            let node = Body.Node(
                node: origin.node.name.isEmpty ? origin.node.id : origin.node.name,
                server: "\(origin.node.server):\(origin.node.port)",
                proto: entry.protocol,
                counts: delta.capped(at: maxCount),
                stopped: entry.health.stopped,
                clockSkewSecs: entry.health.clockSkewSecs
            )
            byEndpoint[endpoint, default: []].append(node)
        }
        let seconds = min(max(Int(window.rounded()), windowSecs.lowerBound), windowSecs.upperBound)
        return byEndpoint.mapValues { nodes in
            Body(client: client, platform: platform, network: network, windowSecs: seconds, nodes: Array(nodes.prefix(maxNodes)))
        }
    }

    /// The text for a stopped node, in the user's terms.
    public static func stoppedText(node: String, health: OutboundHealth.Health) -> String {
        if health.hint == "clock", let skew = health.clockSkewSecs {
            let minutes = max(1, Int((Double(abs(skew)) / 60).rounded()))
            return skew > 0
                ? String(localized: "节点「\(node)」连不上：手机时间慢了约 \(minutes) 分钟。请在设置里打开「自动设置」时间后重试。")
                : String(localized: "节点「\(node)」连不上：手机时间快了约 \(minutes) 分钟。请在设置里打开「自动设置」时间后重试。")
        }
        return String(localized: "节点「\(node)」被服务器拒绝：订阅可能已到期或被重置，也可能是手机时间不准。请更新订阅，或检查时间后重试。")
    }
}

extension ClientReport {
    /// The `platform` field: the iOS and macOS apps share the reporter.
    public static var platform: String {
        #if os(macOS)
        return "macos"
        #else
        return "ios"
        #endif
    }
}

/// The user's choice about the panel report, and the reporter's saved state, in `UserDefaults`. The report is off
/// until the user turns it on in Settings. Turning it on starts from the counts at that moment, so nothing from
/// before is sent; turning it off deletes the saved state.
public struct ClientReportStore {
    public static let enabledKey = "panel-report-enabled"
    public static let snapshotKey = "client-report-snapshot"
    public static let lastReportKey = "client-report-last"

    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    /// Off unless the user turned it on.
    public var isEnabled: Bool {
        defaults.object(forKey: Self.enabledKey) as? Bool ?? false
    }

    /// On: the next look at the counts becomes the baseline and the first window starts `now`. Off: the
    /// baseline and the time of the last report are deleted.
    public func setEnabled(_ enabled: Bool, now: Date = Date()) {
        defaults.set(enabled, forKey: Self.enabledKey)
        defaults.removeObject(forKey: Self.snapshotKey)
        if enabled {
            defaults.set(now, forKey: Self.lastReportKey)
        } else {
            defaults.removeObject(forKey: Self.lastReportKey)
        }
    }

    /// Deletes the state of a reporter that is off, such as the one an earlier version kept before the report
    /// became a setting.
    public func forgetIfDisabled() {
        guard !isEnabled else {
            return
        }
        defaults.removeObject(forKey: Self.snapshotKey)
        defaults.removeObject(forKey: Self.lastReportKey)
    }

    /// Counts per engine node at the last report; nil until the first look after the report was turned on.
    public var snapshot: [String: OutboundHealth.Counts]? {
        defaults.data(forKey: Self.snapshotKey)
            .flatMap { try? JSONDecoder().decode([String: OutboundHealth.Counts].self, from: $0) }
    }

    public var lastReport: Date? {
        defaults.object(forKey: Self.lastReportKey) as? Date
    }

    func remember(_ counts: [String: OutboundHealth.Counts], at date: Date) {
        if let data = try? JSONEncoder().encode(counts) {
            defaults.set(data, forKey: Self.snapshotKey)
        }
        defaults.set(date, forKey: Self.lastReportKey)
    }
}

/// When the app reports and what, for its reporter; the sending is the app's. Does nothing while the report is off,
/// and the first look after it was turned on only takes the counts as the baseline.
public final class ClientReportPlanner {
    public enum Step: Equatable {
        /// Not due, nothing new, or the report is off.
        case wait
        /// The counts at this moment became the baseline; nothing is sent.
        case baseline
        /// Send these bodies, then call `delivered(_:at:)` with `counts`, or `failed(at:)`.
        case send([URL: ClientReport.Body], counts: [String: OutboundHealth.Counts])
    }

    private let store: ClientReportStore
    private let interval: () -> TimeInterval
    private var snapshot: [String: OutboundHealth.Counts]?
    private var lastReport: Date?
    private var nextReport: Date

    public init(
        store: ClientReportStore,
        now: Date = Date(),
        interval: @escaping () -> TimeInterval = { TimeInterval.random(in: ClientReport.interval) }
    ) {
        self.store = store
        self.interval = interval
        snapshot = store.snapshot
        let last = store.lastReport
        lastReport = last
        nextReport = (last ?? now).addingTimeInterval(interval())
    }

    /// The next step for the engine's current health. `network` is `wifi`, `cellular`, `ethernet` or `other`.
    public func next(
        health: OutboundHealth,
        groups: SubscriptionGroups,
        network: String,
        client: String,
        platform: String = ClientReport.platform,
        now: Date = Date()
    ) -> Step {
        guard store.isEnabled, !health.outbounds.isEmpty else {
            return .wait
        }
        let current = Dictionary(health.outbounds.map { ($0.node, $0.health.counts) }, uniquingKeysWith: { _, last in last })
        guard let previous = snapshot else {
            // First look since the report was turned on: what happened before is not reported.
            remember(current, at: lastReport ?? now)
            return .baseline
        }
        guard now >= nextReport else {
            return .wait
        }
        let window = lastReport.map { now.timeIntervalSince($0) } ?? ClientReport.interval.lowerBound
        let bodies = ClientReport.bodies(
            health: health,
            groups: groups,
            previous: previous,
            window: window,
            client: client,
            platform: platform,
            network: network
        )
        guard !bodies.isEmpty else {
            // Nothing new: move the baseline and look again next round.
            remember(current, at: now)
            return .wait
        }
        return .send(bodies, counts: current)
    }

    /// The bodies of a `.send` reached the panel (or it answered).
    public func delivered(_ counts: [String: OutboundHealth.Counts], at date: Date) {
        guard store.isEnabled else {
            return
        }
        remember(counts, at: date)
    }

    /// Offline or the panel unreachable: keep the counts and try again after the panel's minimum.
    public func failed(at date: Date) {
        nextReport = date.addingTimeInterval(ClientReport.minimumInterval)
    }

    private func remember(_ counts: [String: OutboundHealth.Counts], at date: Date) {
        snapshot = counts
        lastReport = date
        nextReport = date.addingTimeInterval(interval())
        store.remember(counts, at: date)
    }
}
