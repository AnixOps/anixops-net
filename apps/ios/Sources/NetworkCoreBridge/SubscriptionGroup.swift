import Foundation

/// Traffic and expiry a subscription server reports in its `subscription-userinfo` header, for example
/// `upload=1234; download=5678; total=107374182400; expire=1798761600`.
public struct SubscriptionUserInfo: Codable, Equatable {
    public var upload: Int64?
    public var download: Int64?
    public var total: Int64?
    public var expire: Date?

    public init(upload: Int64? = nil, download: Int64? = nil, total: Int64? = nil, expire: Date? = nil) {
        self.upload = upload
        self.download = download
        self.total = total
        self.expire = expire
    }

    /// Nil when the header carries none of the four fields. `expire=0` means no expiry.
    public static func parse(_ header: String) -> SubscriptionUserInfo? {
        var info = SubscriptionUserInfo()
        for field in header.split(separator: ";") {
            let pair = field.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            guard pair.count == 2, let number = Double(pair[1]), number.isFinite, number >= 0 else {
                continue
            }
            let value = number >= 9.2e18 ? Int64.max : Int64(number)
            switch pair[0].lowercased() {
            case "upload":
                info.upload = value
            case "download":
                info.download = value
            case "total":
                info.total = value
            case "expire":
                info.expire = value > 0 ? Date(timeIntervalSince1970: TimeInterval(value)) : nil
            default:
                continue
            }
        }
        return info == SubscriptionUserInfo() ? nil : info
    }

    /// Upload plus download, when the server sent either.
    public var used: Int64? {
        guard upload != nil || download != nil else {
            return nil
        }
        let (sum, overflow) = (upload ?? 0).addingReportingOverflow(download ?? 0)
        return overflow ? Int64.max : sum
    }

    /// The share of `total` used, 0...1, when both are known and `total` is not zero.
    public var usedFraction: Double? {
        guard let used, let total, total > 0 else {
            return nil
        }
        return min(1, Double(used) / Double(total))
    }
}

/// What a subscription download returned besides the body: the headers Loon, Clash and Shadowrocket read.
public struct SubscriptionResponseInfo: Equatable {
    public var userInfo: SubscriptionUserInfo?
    /// `profile-update-interval`, in hours.
    public var updateIntervalHours: Int?
    /// `profile-title`, or the file name from `content-disposition`.
    public var title: String?

    public init(userInfo: SubscriptionUserInfo? = nil, updateIntervalHours: Int? = nil, title: String? = nil) {
        self.userInfo = userInfo
        self.updateIntervalHours = updateIntervalHours
        self.title = title
    }

    /// `header` looks a header up by name, ignoring case (`HTTPURLResponse.value(forHTTPHeaderField:)`).
    public init(header: (String) -> String?) {
        self.init(
            userInfo: header("subscription-userinfo").flatMap(SubscriptionUserInfo.parse),
            updateIntervalHours: header("profile-update-interval").flatMap(Self.hours),
            title: header("profile-title").flatMap(Self.profileTitle)
                ?? header("content-disposition").flatMap(Self.fileName)
        )
    }

    static func hours(_ value: String) -> Int? {
        guard let hours = Int(value.trimmingCharacters(in: .whitespaces)), hours > 0 else {
            return nil
        }
        return min(hours, SubscriptionGroup.maxUpdateIntervalHours)
    }

    /// `profile-title` is plain text or `base64:` followed by base64 UTF-8.
    static func profileTitle(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespaces)
        if trimmed.lowercased().hasPrefix("base64:") {
            let encoded = String(trimmed.dropFirst("base64:".count))
            guard let data = Data(base64Encoded: Self.padded(encoded)) else {
                return nil
            }
            return cleanTitle(String(decoding: data, as: UTF8.self))
        }
        return cleanTitle(trimmed)
    }

    /// `filename*=UTF-8''<percent-encoded>` wins over `filename="<name>"`. The extension is dropped.
    static func fileName(_ value: String) -> String? {
        var plain: String?
        var extended: String?
        for part in value.split(separator: ";") {
            let pair = part.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            guard pair.count == 2 else {
                continue
            }
            switch pair[0].lowercased() {
            case "filename*":
                let encoded = pair[1].components(separatedBy: "''").last ?? pair[1]
                extended = encoded.removingPercentEncoding
            case "filename":
                plain = pair[1].trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            default:
                continue
            }
        }
        guard let name = extended ?? plain else {
            return nil
        }
        let stem = (name as NSString).deletingPathExtension
        return cleanTitle(stem.isEmpty ? name : stem)
    }

    private static func cleanTitle(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : String(trimmed.prefix(SubscriptionGroup.maxNameLength))
    }

    private static func padded(_ base64: String) -> String {
        let normalized = base64.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        let remainder = normalized.count % 4
        return remainder == 0 ? normalized : normalized + String(repeating: "=", count: 4 - remainder)
    }
}

/// One subscription, Loon or Shadowrocket style: its own name, nodes, rules and picks, the last update and the
/// server's traffic and expiry, updated by hand or every `updateIntervalHours`.
public struct SubscriptionGroup: Codable, Equatable, Identifiable {
    public static let defaultUpdateIntervalHours = 24
    public static let maxUpdateIntervalHours = 24 * 30
    public static let maxNameLength = 64

    public let id: String
    public var name: String
    /// The nodes, rules and picks of this subscription; `library.subscriptionURL` is the link, nil for pasted text.
    public var library: NodeLibrary
    public var userInfo: SubscriptionUserInfo?
    /// 0 updates by hand only.
    public var updateIntervalHours: Int
    /// When the last update failed; cleared by the next successful one.
    public var lastFailureAt: Date?

    public init(
        id: String = UUID().uuidString,
        name: String,
        library: NodeLibrary,
        userInfo: SubscriptionUserInfo? = nil,
        updateIntervalHours: Int = SubscriptionGroup.defaultUpdateIntervalHours,
        lastFailureAt: Date? = nil
    ) {
        self.id = id
        self.name = name
        self.library = library
        self.userInfo = userInfo
        self.updateIntervalHours = max(0, min(updateIntervalHours, Self.maxUpdateIntervalHours))
        self.lastFailureAt = lastFailureAt
    }

    /// A new group from a downloaded subscription: named by the server's title, else the link's host.
    public init(report: NativeImportReport, url: String, response: SubscriptionResponseInfo, at date: Date = Date()) {
        self.init(
            name: response.title ?? SubscriptionLink.displayHost(url),
            library: NodeLibrary(report: report, subscriptionURL: url, keeping: nil, at: date),
            userInfo: response.userInfo,
            updateIntervalHours: response.updateIntervalHours ?? Self.defaultUpdateIntervalHours
        )
    }

    public var url: String? {
        library.subscriptionURL
    }

    /// Whether this group can be updated at all: pasted text has no link.
    public var isUpdatable: Bool {
        url != nil
    }

    /// When the next automatic update is due, nil when it updates by hand only.
    public var nextUpdate: Date? {
        guard isUpdatable, updateIntervalHours > 0 else {
            return nil
        }
        return library.updatedAt.addingTimeInterval(TimeInterval(updateIntervalHours) * 3600)
    }

    public func isDue(at date: Date) -> Bool {
        nextUpdate.map { $0 <= date } ?? false
    }

    /// The group after a successful update: nodes and picks carry over as in `NodeLibrary.refreshed`. The server's
    /// traffic replaces the old figures; the name and interval stay the user's.
    public func refreshed(with report: NativeImportReport, response: SubscriptionResponseInfo, at date: Date = Date()) -> SubscriptionGroup {
        var next = self
        next.library = library.refreshed(with: report, at: date)
        next.userInfo = response.userInfo ?? userInfo
        next.lastFailureAt = nil
        return next
    }
}

/// Every subscription group, and which one the tunnel runs. Like `NodeLibrary` it holds credentials, so the app
/// keeps it in the Keychain. The extension never reads it: the app writes the active group's `activeLibrary()` to
/// `NodeLibrary.keychainAccount` for the extension.
public struct SubscriptionGroups: Codable, Equatable {
    public static let keychainAccount = "subscription-groups"
    /// Prefix of the policy group a subscription group becomes inside another subscription's rules.
    public static let policyGroupPrefix = "订阅·"

    public var groups: [SubscriptionGroup]
    /// The group whose selected node or rules the tunnel runs; nil when a single pasted node is in use.
    public var activeID: String?

    public init(groups: [SubscriptionGroup] = [], activeID: String? = nil) {
        self.groups = groups
        self.activeID = activeID
    }

    /// The store for a library saved before groups existed: that library becomes the only group, and it is active.
    public static func migrating(_ library: NodeLibrary?, name: String) -> SubscriptionGroups {
        guard let library, !library.nodes.isEmpty else {
            return SubscriptionGroups()
        }
        let group = SubscriptionGroup(
            name: library.subscriptionURL.map(SubscriptionLink.displayHost) ?? name,
            library: library,
            updateIntervalHours: library.subscriptionURL == nil ? 0 : SubscriptionGroup.defaultUpdateIntervalHours
        )
        return SubscriptionGroups(groups: [group], activeID: group.id)
    }

    public var active: SubscriptionGroup? {
        groups.first { $0.id == activeID }
    }

    public func group(id: String) -> SubscriptionGroup? {
        groups.first { $0.id == id }
    }

    /// The group already holding this link, so importing it again updates that group instead of adding another.
    public func group(url: String) -> SubscriptionGroup? {
        groups.first { $0.url == url }
    }

    public mutating func upsert(_ group: SubscriptionGroup) {
        if let index = groups.firstIndex(where: { $0.id == group.id }) {
            groups[index] = group
        } else {
            groups.append(group)
        }
    }

    /// Removes a group. When it was active, the next group with nodes becomes active.
    public mutating func remove(id: String) {
        groups.removeAll { $0.id == id }
        if activeID == id {
            activeID = groups.first { $0.library.selected != nil }?.id
        }
    }

    public func due(at date: Date) -> [SubscriptionGroup] {
        groups.filter { $0.isDue(at: date) }
    }

    /// The earliest automatic update of any group, for scheduling a background refresh.
    public var nextUpdate: Date? {
        groups.compactMap(\.nextUpdate).min()
    }

    /// Another subscription group offered as a policy group inside the active group's rules.
    public struct LinkedGroup: Equatable {
        /// The policy group id in the routing section.
        public let policyID: String
        public let groupID: String
        /// Node ids in the routing section are `prefix + node id`, so they cannot clash with the active group's.
        public let prefix: String
    }

    /// Other groups the active group's rules can use, as long as the routing section stays within the engine's
    /// limits (64 nodes, 64 policy groups); groups that would not fit are left out, in list order.
    public func linkedGroups() -> [LinkedGroup] {
        guard let active, let routing = active.library.routing else {
            return []
        }
        var usedIDs = Set(active.library.nodes.map(\.id)).union(routing.groups.map(\.id))
        var nodeCount = active.library.nodes.count
        var groupCount = routing.groups.count
        var linked: [LinkedGroup] = []
        for (index, group) in groups.enumerated() where group.id != active.id {
            let prefix = "s\(index)."
            let nodes = Self.linkedNodes(of: group.library.nodes, prefix: prefix)
            guard !nodes.isEmpty,
                  nodeCount + nodes.count <= Self.maxRoutedOutbounds,
                  groupCount + 1 <= Self.maxPolicyGroups
            else {
                continue
            }
            var policyID = Self.policyGroupPrefix + Self.policyName(group.name)
            var suffix = 2
            while usedIDs.contains(policyID) {
                policyID = Self.policyGroupPrefix + Self.policyName(group.name) + " \(suffix)"
                suffix += 1
            }
            usedIDs.insert(policyID)
            nodeCount += nodes.count
            groupCount += 1
            linked.append(LinkedGroup(policyID: policyID, groupID: group.id, prefix: prefix))
        }
        return linked
    }

    /// The library the tunnel runs: the active group's, and when it routes by rules, every other group that fits
    /// as a `select` policy group of its nodes, offered as a member of each of the subscription's `select` groups.
    /// A linked group starts on that group's selected node.
    public func activeLibrary() -> NodeLibrary? {
        guard let active else {
            return nil
        }
        var library = active.library
        guard let routing = library.routing else {
            return library
        }
        let linked = linkedGroups()
        guard !linked.isEmpty else {
            return library
        }
        var nodes = library.nodes
        var extraGroups: [NativeImportReport.Routing.Group] = []
        for link in linked {
            guard let group = self.group(id: link.groupID) else {
                continue
            }
            let members = Self.linkedNodes(of: group.library.nodes, prefix: link.prefix)
            nodes += members
            extraGroups.append(NativeImportReport.Routing.Group(
                id: link.policyID,
                type: "select",
                members: members.map(\.id),
                default: group.library.selectedID.map { link.prefix + $0 }.flatMap { id in
                    members.contains { $0.id == id } ? id : nil
                },
                url: nil,
                intervalSecs: nil,
                toleranceMs: nil
            ))
        }
        let policyIDs = linked.map(\.policyID)
        let groups = routing.groups.map { group -> NativeImportReport.Routing.Group in
            guard group.isSelectable else {
                return group
            }
            return NativeImportReport.Routing.Group(
                id: group.id,
                type: group.type,
                members: group.members + policyIDs,
                default: group.default,
                url: group.url,
                intervalSecs: group.intervalSecs,
                toleranceMs: group.toleranceMs
            )
        }
        library.nodes = nodes
        library.routing = NativeImportReport.Routing(rules: routing.rules, groups: groups + extraGroups, ruleSets: routing.ruleSets)
        return library
    }

    // Same limits as the engine's routing section (crates/node-import).
    static let maxRoutedOutbounds = 64
    static let maxPolicyGroups = 64

    /// A linked group's nodes under `prefix + node id`, each `via` renamed to match. A node whose id does not fit
    /// the engine's rule is left out, and so is every node that connects through a node left out.
    static func linkedNodes(of nodes: [NativeImportReport.Node], prefix: String) -> [NativeImportReport.Node] {
        var linked = nodes.map { node in node.renamed { prefix + $0 } }.filter { isOutboundID($0.id) }
        while true {
            let ids = Set(linked.map(\.id))
            let kept = linked.filter { node in node.via.map { ids.contains($0) } ?? true }
            if kept.count == linked.count {
                return linked
            }
            linked = kept
        }
    }

    /// The engine's outbound id rule: 1-128 of `[A-Za-z0-9._-]`.
    static func isOutboundID(_ id: String) -> Bool {
        (1...128).contains(id.utf8.count) && id.utf8.allSatisfy { byte in
            (byte >= 0x30 && byte <= 0x39) || (byte >= 0x41 && byte <= 0x5A) || (byte >= 0x61 && byte <= 0x7A)
                || byte == 0x2E || byte == 0x5F || byte == 0x2D
        }
    }

    /// Policy group ids must not contain commas (they separate rule fields).
    static func policyName(_ name: String) -> String {
        let cleaned = name.replacingOccurrences(of: ",", with: " ").trimmingCharacters(in: .whitespaces)
        return cleaned.isEmpty ? "subscription" : cleaned
    }

    public func keychainValue() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        encoder.dateEncodingStrategy = .millisecondsSince1970
        return String(decoding: try encoder.encode(self), as: UTF8.self)
    }

    public static func fromKeychainValue(_ value: String) throws -> SubscriptionGroups {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .millisecondsSince1970
        return try decoder.decode(SubscriptionGroups.self, from: Data(value.utf8))
    }
}
