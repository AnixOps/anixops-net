import Foundation

/// Editing, sorting and filtering the node list. Edits change the saved node only: a subscription update brings back
/// what the provider serves.
extension NativeImportReport.Node {
    /// The same node with a new name, server and port; the runtime outbound gets the same server and port.
    public func edited(name: String, server: String, port: UInt16) -> NativeImportReport.Node {
        var outbound = outbound
        outbound["server"] = .string(server)
        outbound["port"] = .int(Int64(port))
        return NativeImportReport.Node(
            id: id,
            name: name.trimmingCharacters(in: .whitespacesAndNewlines),
            protocol: self.protocol,
            server: server,
            port: port,
            outbound: outbound,
            path: path,
            licenseOrigin: licenseOrigin
        )
    }

    /// The name the list shows: the node's name, otherwise its server.
    public var displayName: String {
        name.isEmpty ? server : name
    }

    /// The input of the importer's share-link export (`networkcore_import_share_link`): the name and the full
    /// outbound, credentials included.
    public func shareLinkRequest() throws -> Data {
        struct Request: Encodable {
            let name: String
            let outbound: [String: JSONValue]
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(Request(name: displayName, outbound: outbound))
    }
}

/// Why a node could not be shared. The messages are fixed text without node values.
public enum NodeShareFailure: Error, Equatable {
    /// The protocol has no share-link form the importer writes, such as Mieru.
    case unsupported
    case unavailable

    public var message: String {
        switch self {
        case .unsupported:
            return String(localized: "这个协议没有通用的分享链接格式，无法导出")
        case .unavailable:
            return String(localized: "无法生成分享链接")
        }
    }
}

/// Why a node edit was refused. The messages are fixed text without node values.
public enum NodeEditError: Error, Equatable {
    case emptyServer
    case invalidServer
    case invalidPort

    public var message: String {
        switch self {
        case .emptyServer:
            return String(localized: "请填写服务器地址")
        case .invalidServer:
            return String(localized: "服务器地址不能包含空格或 /")
        case .invalidPort:
            return String(localized: "端口应为 1–65535")
        }
    }
}

public enum NodeEdit {
    /// Checks what the user typed in the edit form and returns the edited node.
    public static func apply(
        to node: NativeImportReport.Node,
        name: String,
        server: String,
        port: String
    ) -> Result<NativeImportReport.Node, NodeEditError> {
        let host = server.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !host.isEmpty else {
            return .failure(.emptyServer)
        }
        guard !host.contains(where: { $0.isWhitespace || $0 == "/" }) else {
            return .failure(.invalidServer)
        }
        guard let value = UInt16(port.trimmingCharacters(in: .whitespaces)), value > 0 else {
            return .failure(.invalidPort)
        }
        return .success(node.edited(name: name, server: host, port: value))
    }
}

extension NodeLibrary {
    /// The library with `node` in place of the node with the same id.
    public func replacing(_ node: NativeImportReport.Node) -> NodeLibrary {
        var copy = self
        if let index = copy.nodes.firstIndex(where: { $0.id == node.id }) {
            copy.nodes[index] = node
        }
        return copy
    }

    /// Whether a node can be deleted: no other node may connect through it (`via`), the subscription's rules and groups
    /// must not name it, and one node must remain.
    public func canRemove(_ id: String) -> Bool {
        guard nodes.count > 1, node(id: id) != nil else {
            return false
        }
        // Deleting it would leave the other node's `via` naming nothing: the engine refuses the routing section that
        // holds both, and that node on its own, rather than connect it directly. Lists of links and JSON outbound
        // configs, where such chains come from, have no subscription rules, so this goes before the rules.
        guard !nodes.contains(where: { $0.id != id && $0.via == id }) else {
            return false
        }
        guard let routing else {
            return true
        }
        return !Self.names(routing, id)
    }

    /// Whether the subscription's rules or groups name the node.
    private static func names(_ routing: NativeImportReport.Routing, _ id: String) -> Bool {
        routing.groups.contains { $0.referencedNames.contains(id) }
            || routing.rules.contains { $0.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.contains(id) }
    }

    /// The nodes that connect through `id` (`via`), directly or along a chain, in list order: deleting `id` alone
    /// would leave their `via` naming nothing.
    public func dependents(of id: String) -> [NativeImportReport.Node] {
        var found: Set<String> = []
        var frontier = [id]
        while let current = frontier.popLast() {
            for node in nodes where node.via == current && node.id != id && !found.contains(node.id) {
                found.insert(node.id)
                frontier.append(node.id)
            }
        }
        return nodes.filter { found.contains($0.id) }
    }

    /// What deleting the node takes with it: nil when it cannot be deleted at all, else the nodes that connect
    /// through it (`dependents(of:)`), which go too once the user confirms. It cannot go when the subscription's
    /// rules or groups name it or one of those nodes, or when nothing would remain.
    public func removal(of id: String) -> [NativeImportReport.Node]? {
        guard node(id: id) != nil else {
            return nil
        }
        let dependents = self.dependents(of: id)
        let ids = Set([id] + dependents.map(\.id))
        guard nodes.count > ids.count else {
            return nil
        }
        if let routing, ids.contains(where: { Self.names(routing, $0) }) {
            return nil
        }
        return dependents
    }

    /// The library without the node and the nodes that connect through it (`removal(of:)`); the selection moves to
    /// the default node when it was one of them. Unchanged when `removal(of:)` is nil.
    public func removingWithDependents(_ id: String) -> NodeLibrary {
        guard let dependents = removal(of: id) else {
            return self
        }
        let ids = Set([id] + dependents.map(\.id))
        var copy = self
        copy.nodes.removeAll { ids.contains($0.id) }
        if let selected = copy.selectedID, ids.contains(selected) {
            copy.selectedID = NodeLibrary.defaultNode(in: copy.nodes)?.id
        }
        return copy
    }

    /// The library without the node; the selection moves to the default node when it was the selected one.
    public func removing(_ id: String) -> NodeLibrary {
        guard canRemove(id) else {
            return self
        }
        var copy = self
        copy.nodes.removeAll { $0.id == id }
        if copy.selectedID == id {
            copy.selectedID = NodeLibrary.defaultNode(in: copy.nodes)?.id
        }
        return copy
    }
}

/// How the node list is ordered.
public enum NodeSort: String, CaseIterable, Identifiable {
    /// The order the subscription lists them in.
    case original
    case name
    /// Fastest first; untested and failed nodes last, in their original order.
    case latency

    public var id: String {
        rawValue
    }

    /// `nodes` in this order. `latency` gives a node's measured delay in milliseconds, nil when unknown or failed.
    public func sorted(
        _ nodes: [NativeImportReport.Node],
        latency: (NativeImportReport.Node) -> Int? = { _ in nil }
    ) -> [NativeImportReport.Node] {
        let indexed = Array(nodes.enumerated())
        switch self {
        case .original:
            return nodes
        case .name:
            return indexed.sorted { left, right in
                let order = left.element.displayName.localizedStandardCompare(right.element.displayName)
                return order == .orderedSame ? left.offset < right.offset : order == .orderedAscending
            }
            .map(\.element)
        case .latency:
            let delays = indexed.map { latency($0.element) }
            return indexed.sorted { left, right in
                switch (delays[left.offset], delays[right.offset]) {
                case let (l?, r?) where l != r:
                    return l < r
                case (.some, .none):
                    return true
                case (.none, .some):
                    return false
                default:
                    return left.offset < right.offset
                }
            }
            .map(\.element)
        }
    }
}

/// The search box of the node list. Words must all appear in the node's name, server or protocol, ignoring case;
/// `/pattern/` is a regular expression on the name, as Loon and Clash filters are written (for example `/香港|HK/`).
public struct NodeFilter: Equatable {
    public let query: String

    public init(_ query: String) {
        self.query = query.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public var isEmpty: Bool {
        query.isEmpty
    }

    /// The regular expression of a `/pattern/` query, nil for words or an invalid pattern.
    public var pattern: NSRegularExpression? {
        guard query.count > 2, query.hasPrefix("/"), query.hasSuffix("/") else {
            return nil
        }
        return try? NSRegularExpression(pattern: String(query.dropFirst().dropLast()), options: [.caseInsensitive])
    }

    public func matches(_ node: NativeImportReport.Node) -> Bool {
        guard !isEmpty else {
            return true
        }
        if let pattern {
            let name = node.displayName
            return pattern.firstMatch(in: name, range: NSRange(name.startIndex..., in: name)) != nil
        }
        let fields = [node.name, node.server, node.protocol]
        return query.split(whereSeparator: \.isWhitespace).allSatisfy { word in
            fields.contains { $0.localizedCaseInsensitiveContains(word) }
        }
    }

    public func apply(_ nodes: [NativeImportReport.Node]) -> [NativeImportReport.Node] {
        isEmpty ? nodes : nodes.filter(matches)
    }
}

/// A node deletion that waits for the user's confirmation, because other nodes connect through the node and go with
/// it (`NodeLibrary.removal(of:)`).
public struct PendingNodeDeletion: Identifiable {
    public let groupID: String
    public let node: NativeImportReport.Node
    public let dependents: [NativeImportReport.Node]

    public init(groupID: String, node: NativeImportReport.Node, dependents: [NativeImportReport.Node]) {
        self.groupID = groupID
        self.node = node
        self.dependents = dependents
    }

    public var id: String {
        "\(groupID)\u{1F}\(node.id)"
    }

    /// The dependent nodes' names, for the confirmation.
    public var dependentNames: String {
        dependents.map(\.displayName).joined(separator: "、")
    }
}
