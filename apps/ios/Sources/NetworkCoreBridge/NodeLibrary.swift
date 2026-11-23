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
    /// What importing the subscription's rules left out (`NativeImportReport.routingWarnings`), shown on the rules
    /// page so a skipped rule never fails silently.
    public var routingWarnings: [String]
    /// The importer revision the nodes come from (`currentImportRevision`); 0 for a library saved before revisions,
    /// whose outbounds lack the subscription's certificate pins until it is updated or imported again.
    public var importRevision: Int

    /// Revision 1: outbounds carry the subscription's certificate pins (`pinned_certificate_sha256`,
    /// `pinned_public_key_sha256`). Revision 2: policy groups keep the importer's `network_rules` and
    /// `evaluate_before_use`, which earlier versions dropped when decoding.
    public static let currentImportRevision = 2

    enum CodingKeys: String, CodingKey {
        case subscriptionURL
        case nodes
        case selectedID
        case updatedAt
        case routing
        case followsRules
        case groupSelections
        case routingWarnings
        case importRevision
    }

    public init(
        subscriptionURL: String?,
        nodes: [NativeImportReport.Node],
        selectedID: String?,
        updatedAt: Date,
        routing: NativeImportReport.Routing? = nil,
        followsRules: Bool = false,
        groupSelections: [String: String] = [:],
        routingWarnings: [String] = [],
        importRevision: Int = NodeLibrary.currentImportRevision
    ) {
        self.subscriptionURL = subscriptionURL
        self.nodes = nodes
        self.selectedID = selectedID
        self.updatedAt = updatedAt
        self.routing = routing
        self.followsRules = followsRules
        self.groupSelections = groupSelections
        self.routingWarnings = routingWarnings
        self.importRevision = importRevision
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
            followsRules: report.routing != nil,
            routingWarnings: report.routingWarnings ?? []
        )
    }

    /// Libraries saved before rules existed have no routing fields; those saved before import revisions are
    /// revision 0.
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            subscriptionURL: try container.decodeIfPresent(String.self, forKey: .subscriptionURL),
            nodes: try container.decode([NativeImportReport.Node].self, forKey: .nodes),
            selectedID: try container.decodeIfPresent(String.self, forKey: .selectedID),
            updatedAt: try container.decode(Date.self, forKey: .updatedAt),
            routing: try container.decodeIfPresent(NativeImportReport.Routing.self, forKey: .routing),
            followsRules: try container.decodeIfPresent(Bool.self, forKey: .followsRules) ?? false,
            groupSelections: try container.decodeIfPresent([String: String].self, forKey: .groupSelections) ?? [:],
            routingWarnings: try container.decodeIfPresent([String].self, forKey: .routingWarnings) ?? [],
            importRevision: try container.decodeIfPresent(Int.self, forKey: .importRevision) ?? 0
        )
    }

    /// Whether the nodes were imported by an earlier version that left the subscription's certificate pins out.
    public var predatesCertificatePins: Bool {
        importRevision < 1
    }

    /// Whether the subscription's groups that pick by network were imported by an earlier version, which dropped
    /// their network rules: such a group always uses its default until the subscription is imported again.
    public var predatesNetworkRules: Bool {
        importRevision < 2 && (routing?.groups.contains(where: \.picksByNetwork) ?? false)
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

    /// The member a group uses: the user's pick, then the subscription's default, then the first member. A group
    /// that picks by network starts on its default (`displayMembers`); the running engine says what it uses now.
    public func selection(in group: NativeImportReport.Routing.Group) -> String? {
        let members = group.displayMembers
        if let picked = groupSelections[group.id], members.contains(picked) {
            return picked
        }
        if let initial = group.default, members.contains(initial) {
            return initial
        }
        return members.first
    }

    /// What to show for a group member: a node's name (or server), otherwise the group or policy name.
    public func displayName(ofMember member: String) -> String {
        guard let node = node(id: member) else {
            return member
        }
        return node.name.isEmpty ? node.server : node.name
    }

    /// The runtime `routing` section: the nodes by id, the rules, and the groups with each `select` group starting on
    /// its current selection. Nil when the tunnel does not follow rules.
    public func runtimeRouting() -> EmbeddedRuntimeConfig.Routing? {
        followsRules ? routingForModes() : nil
    }

    /// A routing section and the rule sets left out of it to stay within the engine's `maxRuleSets`, most
    /// expendable first, so the app can say which rules do not apply.
    public struct RoutingBuild: Equatable {
        public var routing: EmbeddedRuntimeConfig.Routing
        public var droppedRuleSets: [String]
    }

    /// The `routing` section whatever the rules switch says, so the engine can move between the rule, global and
    /// direct modes without a restart: the global mode goes to the selected node. With subscription rules the user's
    /// `localRules` go in front of them; without, `localRules` alone (the user's rules, then `FINAL` to the node) make the rule
    /// mode, sending `PROXY` to the selected node. `dnsRuleSets` are the sets the DNS rules name
    /// (`DnsSettings.ruleSetNames`), counted against the same limit. Nil without subscription rules and without
    /// `localRules`.
    public func routingForModes(localRules: LocalRules? = nil, dnsRuleSets: [String] = []) -> EmbeddedRuntimeConfig.Routing? {
        routingBuild(localRules: localRules, dnsRuleSets: dnsRuleSets)?.routing
    }

    /// `routingForModes(localRules:dnsRuleSets:)` with the rule sets the limit left out.
    public func routingBuild(localRules: LocalRules? = nil, dnsRuleSets: [String] = []) -> RoutingBuild? {
        let dataSources = Set((localRules?.dataSources ?? []).map(\.name))
        guard let routing else {
            guard let localRules, let selected else {
                return nil
            }
            let local = localRules.engineLines(proxy: selected.id)
            let capped = Self.capped(
                Self.adding(dnsRuleSets, to: local.ruleSets),
                dataSources: dataSources,
                dnsOnly: Self.dnsOnly(dnsRuleSets, lines: local.lines)
            )
            // Only the selected node and the nodes it connects through: the rules name no other, the global mode
            // goes to the selected node and picking another node restarts the tunnel. A list of more than the
            // engine's 64 routed outbounds would make it refuse the whole section.
            return RoutingBuild(
                routing: EmbeddedRuntimeConfig.Routing(
                    outbounds: chain(from: selected),
                    rules: RuleSetCatalog.runnableRules(local.lines, sets: capped.sets),
                    groups: [],
                    global: selected.id,
                    ruleSets: capped.sets,
                    devices: localRules.devices.isEmpty ? nil : localRules.devices
                ),
                droppedRuleSets: capped.dropped
            )
        }
        let outbounds = Dictionary(nodes.map { ($0.id, $0.outbound) }) { first, _ in first }
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
        // The user's GEOIP / IP-ASN data lists replace the subscription's own set of their name.
        for source in localRules?.dataSources ?? [] {
            defined[source.name] = .init(url: source.url, via: proxy)
        }
        let lines = local.lines + routing.rules
        let own = Set((routing.ruleSets ?? [:]).keys)
        let referenced = RuleSetCatalog.ruleSets(for: lines, defined: defined.isEmpty ? nil : defined) ?? [:]
        let capped = Self.capped(
            Self.adding(dnsRuleSets, to: referenced),
            keeping: own,
            dataSources: dataSources,
            dnsOnly: Self.dnsOnly(dnsRuleSets, lines: lines)
        )
        return RoutingBuild(
            routing: EmbeddedRuntimeConfig.Routing(
                outbounds: outbounds,
                rules: RuleSetCatalog.runnableRules(lines, sets: capped.sets),
                groups: groups,
                global: selected?.id,
                ruleSets: capped.sets,
                devices: localRules.flatMap { $0.devices.isEmpty ? nil : $0.devices }
            ),
            droppedRuleSets: capped.dropped
        )
    }

    /// `node` and every node it connects through (`via`), by id. A `via` naming a node that is not listed, or a
    /// loop, ends the chain; the engine then refuses the node rather than connect it directly.
    func chain(from node: NativeImportReport.Node) -> [String: [String: JSONValue]] {
        var chain: [String: [String: JSONValue]] = [:]
        var next: NativeImportReport.Node? = node
        while let current = next, chain[current.id] == nil {
            chain[current.id] = current.outbound
            next = current.via.flatMap { self.node(id: $0) }
        }
        return chain
    }

    /// The engine refuses more than this many rule sets.
    public static let maxRuleSets = 32

    /// `sets` with the catalog's list for each name in `dnsRuleSets` it lacks, as the DNS rules use them.
    static func adding(
        _ dnsRuleSets: [String],
        to sets: [String: EmbeddedRuntimeConfig.RuleSet]
    ) -> [String: EmbeddedRuntimeConfig.RuleSet] {
        var sets = sets
        for name in dnsRuleSets where sets[name] == nil {
            if let list = RuleSetCatalog.known[name] {
                sets[name] = list
            }
        }
        return sets
    }

    /// The DNS rules' sets that no rule line names.
    static func dnsOnly(_ dnsRuleSets: [String], lines: [String]) -> Set<String> {
        Set(dnsRuleSets).subtracting(RuleSetCatalog.referencedNames(in: lines))
    }

    /// `sets` cut to `maxRuleSets`, most expendable first: the user's own lists (`local-…`, `sub-…`), the user's
    /// GEOIP / IP-ASN data lists, the catalog's lists only the DNS rules use, the catalog's other lists, and only
    /// then the subscription's own sets (`keeping`); within each kind by name, last first, so the same input always
    /// gives the same result. The rules naming a dropped set are then left out (`RuleSetCatalog.runnableRules`): a
    /// `GEOIP` or `IP-ASN` rule without its set never matches. Returns the dropped names in the order dropped.
    static func capped(
        _ sets: [String: EmbeddedRuntimeConfig.RuleSet],
        keeping: Set<String> = [],
        dataSources: Set<String> = [],
        dnsOnly: Set<String> = []
    ) -> (sets: [String: EmbeddedRuntimeConfig.RuleSet]?, dropped: [String]) {
        var sets = sets
        var dropped: [String] = []
        if sets.count > maxRuleSets {
            let names = sets.keys.sorted(by: >)
            let others = names.filter { !keeping.contains($0) }
            let userLists = others.filter { $0.hasPrefix("local-") || $0.hasPrefix("sub-") }
            let data = others.filter { dataSources.contains($0) && !userLists.contains($0) }
            let dns = others.filter { dnsOnly.contains($0) && !userLists.contains($0) && !data.contains($0) }
            let catalog = others.filter { !userLists.contains($0) && !data.contains($0) && !dns.contains($0) }
            let subscription = names.filter { keeping.contains($0) }
            for name in userLists + data + dns + catalog + subscription where sets.count > maxRuleSets {
                sets.removeValue(forKey: name)
                dropped.append(name)
            }
        }
        return (sets.isEmpty ? nil : sets, dropped)
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

/// Recognizes a pasted subscription link: one http(s) URL and nothing else. An `http://` link is recognized so the
/// import can say why it cannot use it (`isPlainHTTP`) instead of trying it as a node link.
public enum SubscriptionLink {
    /// Whether `text` is a subscription link without TLS. The Apple apps refuse those before downloading: App
    /// Transport Security blocks plain HTTP loads, so the download would always fail and look like a dead link.
    public static func isPlainHTTP(_ text: String) -> Bool {
        url(from: text)?.scheme?.lowercased() == "http"
    }

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

extension NodeLibrary {
    /// The library with each ANX node marked with the origin of `subscriptionURL`, so its outbound gets that origin's
    /// ANX license and no other (anx spec §2.4.1). Whatever was there is replaced; pasted text marks none.
    public func markingLicenseOrigins() -> NodeLibrary {
        let origin = AnxLicense.origin(ofSubscription: subscriptionURL)
        var copy = self
        copy.nodes = nodes.map { node in
            var node = node
            node.licenseOrigin = node.protocol == "anx" ? origin : nil
            return node
        }
        return copy
    }
}
