import Foundation

/// Every node of the last subscription or multi-node import, for the node list, with the subscription's rules and
/// policy groups when it had them. It holds the nodes' credentials, so the app keeps it in the Keychain
/// (`NodeSecretStore`), never in the tunnel profile; the profile carries the selected node, and the extension reads
/// this library only to route by the subscription's rules (`TunnelProfileKeys.routing`).
public struct NodeLibrary: Codable, Equatable {
    public static let keychainAccount = "node-library"

    /// The subscription link the nodes came from, for refreshing; nil for pasted text.
    public var subscriptionURL: String?
    public var nodes: [NativeImportReport.Node]
    public var selectedID: String?
    public var updatedAt: Date
    public var routing: NativeImportReport.Routing?
    /// Whether the tunnel follows the subscription's rules and groups rather than sending everything to the selected
    /// node. Only meaningful with `routing`.
    public var followsRules: Bool
    /// The member picked in each `select` group, by group id.
    public var groupSelections: [String: String]

    enum CodingKeys: String, CodingKey {
        case subscriptionURL
        case nodes
        case selectedID
        case updatedAt
        case routing
        case followsRules
        case groupSelections
    }

    public init(
        subscriptionURL: String?,
        nodes: [NativeImportReport.Node],
        selectedID: String?,
        updatedAt: Date,
        routing: NativeImportReport.Routing? = nil,
        followsRules: Bool = false,
        groupSelections: [String: String] = [:]
    ) {
        self.subscriptionURL = subscriptionURL
        self.nodes = nodes
        self.selectedID = selectedID
        self.updatedAt = updatedAt
        self.routing = routing
        self.followsRules = followsRules
        self.groupSelections = groupSelections
    }

    /// A library from a fresh report. The previous selection survives a refresh when that node is still listed;
    /// otherwise the default node is selected (`defaultNode(in:)`). Subscription rules are followed whenever the
    /// report has them.
    public init(report: NativeImportReport, subscriptionURL: String?, keeping previousID: String?, at date: Date = Date()) {
        let selected = report.nodes.first { $0.id == previousID } ?? Self.defaultNode(in: report.nodes)
        self.init(
            subscriptionURL: subscriptionURL,
            nodes: report.nodes,
            selectedID: selected?.id,
            updatedAt: date,
            routing: report.routing,
            followsRules: report.routing != nil
        )
    }

    /// Libraries saved before rules existed have no routing fields.
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            subscriptionURL: try container.decodeIfPresent(String.self, forKey: .subscriptionURL),
            nodes: try container.decode([NativeImportReport.Node].self, forKey: .nodes),
            selectedID: try container.decodeIfPresent(String.self, forKey: .selectedID),
            updatedAt: try container.decode(Date.self, forKey: .updatedAt),
            routing: try container.decodeIfPresent(NativeImportReport.Routing.self, forKey: .routing),
            followsRules: try container.decodeIfPresent(Bool.self, forKey: .followsRules) ?? false,
            groupSelections: try container.decodeIfPresent([String: String].self, forKey: .groupSelections) ?? [:]
        )
    }

    /// The same subscription after a refresh to `report`: the selected node, the rules switch and the group picks
    /// carry over where they still apply.
    public func refreshed(with report: NativeImportReport, at date: Date = Date()) -> NodeLibrary {
        var next = NodeLibrary(report: report, subscriptionURL: subscriptionURL, keeping: selectedID, at: date)
        next.followsRules = report.routing != nil && (routing == nil || followsRules)
        let groups = report.routing?.groups ?? []
        next.groupSelections = groupSelections.filter { id, member in
            groups.first { $0.id == id }?.members.contains(member) == true
        }
        return next
    }

    /// The node picked when the user has not picked one: the first trunk node (a domestic ingress on a shared
    /// link), otherwise the first node. A single user dialing an overseas node directly shows a cross-layer RTT
    /// that many users on one trunk do not (anx FUTURE-IMPROVEMENTS F-51, 默认路径规则); direct nodes stay
    /// available to pick by hand.
    public static func defaultNode(in nodes: [NativeImportReport.Node]) -> NativeImportReport.Node? {
        nodes.first(where: \.isTrunk) ?? nodes.first
    }

    public var selected: NativeImportReport.Node? {
        nodes.first { $0.id == selectedID }
    }

    public func node(id: String) -> NativeImportReport.Node? {
        nodes.first { $0.id == id }
    }

    /// Whether the tunnel runs the subscription's rules and groups.
    public var routesByRules: Bool {
        followsRules && routing != nil
    }

    /// The member a group uses: the user's pick, then the subscription's default, then the first member.
    public func selection(in group: NativeImportReport.Routing.Group) -> String? {
        if let picked = groupSelections[group.id], group.members.contains(picked) {
            return picked
        }
        if let initial = group.default, group.members.contains(initial) {
            return initial
        }
        return group.members.first
    }

    /// What to show for a group member: a node's name (or server), otherwise the group or policy name.
    public func displayName(ofMember member: String) -> String {
        guard let node = node(id: member) else {
            return member
        }
        return node.name.isEmpty ? node.server : node.name
    }

    /// The runtime `routing` section: every node by id, the rules, and the groups with each `select` group starting
    /// on its current selection. Nil when the tunnel does not follow rules.
    public func runtimeRouting() -> EmbeddedRuntimeConfig.Routing? {
        followsRules ? routingForModes() : nil
    }

    /// The `routing` section whatever the rules switch says, so the engine can move between the rule, global and
    /// direct modes without a restart: the global mode goes to the selected node. With subscription rules the user's
    /// `localRules` go in front of them; without, `localRules` alone (its preset after the user's rules) make the rule
    /// mode, sending `PROXY` to the selected node. Nil without subscription rules and without `localRules`.
    public func routingForModes(localRules: LocalRules? = nil) -> EmbeddedRuntimeConfig.Routing? {
        let outbounds = Dictionary(nodes.map { ($0.id, $0.outbound) }) { first, _ in first }
        guard let routing else {
            guard let localRules, let selected else {
                return nil
            }
            let local = localRules.engineLines(proxy: selected.id)
            let ruleSets = Self.capped(local.ruleSets)
            return EmbeddedRuntimeConfig.Routing(
                outbounds: outbounds,
                rules: RuleSetCatalog.runnableRules(local.lines, sets: ruleSets),
                groups: [],
                global: selected.id,
                ruleSets: ruleSets
            )
        }
        let groups = routing.groups.map { group -> NativeImportReport.Routing.Group in
            var group = group
            if group.isSelectable {
                group.default = selection(in: group)
            }
            return group
        }
        let proxy = routing.groups.first?.id ?? selected?.id ?? "DIRECT"
        let local = localRules?.userLines(proxy: proxy) ?? (lines: [], ruleSets: [:])
        var defined = routing.ruleSets ?? [:]
        for (name, set) in local.ruleSets where defined[name] == nil {
            defined[name] = set
        }
        let lines = local.lines + routing.rules
        let own = Set((routing.ruleSets ?? [:]).keys)
        let ruleSets = RuleSetCatalog.ruleSets(for: lines, defined: defined.isEmpty ? nil : defined).flatMap { sets in
            Self.capped(sets, keeping: own)
        }
        return EmbeddedRuntimeConfig.Routing(
            outbounds: outbounds,
            rules: RuleSetCatalog.runnableRules(lines, sets: ruleSets),
            groups: groups,
            global: selected?.id,
            ruleSets: ruleSets
        )
    }

    /// The engine refuses more than this many rule sets.
    public static let maxRuleSets = 32

    /// `sets` cut to `maxRuleSets`, dropping the user's own lists (`local-…`, `sub-…`) as needed, never a name in
    /// `keeping`; the rules naming a dropped set are then left out (`RuleSetCatalog.runnableRules`).
    static func capped(
        _ sets: [String: EmbeddedRuntimeConfig.RuleSet],
        keeping: Set<String> = []
    ) -> [String: EmbeddedRuntimeConfig.RuleSet]? {
        var sets = sets
        if sets.count > maxRuleSets {
            let local = sets.keys
                .filter { !keeping.contains($0) && ($0.hasPrefix("local-") || $0.hasPrefix("sub-")) }
                .sorted()
            for name in local.reversed() where sets.count > maxRuleSets {
                sets.removeValue(forKey: name)
            }
        }
        return sets.isEmpty ? nil : sets
    }

    /// QUIC protocols: a TCP connect time to their port says nothing about them.
    public static func usesUDP(_ protocolName: String) -> Bool {
        ["hysteria2", "tuic"].contains(protocolName)
    }

    public func keychainValue() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        encoder.dateEncodingStrategy = .millisecondsSince1970
        return String(decoding: try encoder.encode(self), as: UTF8.self)
    }

    public static func fromKeychainValue(_ value: String) throws -> NodeLibrary {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .millisecondsSince1970
        return try decoder.decode(NodeLibrary.self, from: Data(value.utf8))
    }
}

/// Recognizes a pasted subscription link: one http(s) URL and nothing else.
public enum SubscriptionLink {
    public static func url(from text: String) -> URL? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.contains(where: \.isWhitespace),
              let url = URL(string: trimmed),
              let scheme = url.scheme?.lowercased(), scheme == "https" || scheme == "http",
              let host = url.host, !host.isEmpty
        else {
            return nil
        }
        return url
    }

    /// The host alone, for showing which subscription is in use without its token.
    public static func displayHost(_ url: String) -> String {
        URL(string: url)?.host ?? url
    }
}
