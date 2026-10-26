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
            outbound: outbound
        )
    }

    /// The name the list shows: the node's name, otherwise its server.
    public var displayName: String {
        name.isEmpty ? server : name
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

    /// Whether a node can be deleted: the subscription's rules and groups must not name it, and one node must remain.
    public func canRemove(_ id: String) -> Bool {
        guard nodes.count > 1, node(id: id) != nil else {
            return false
        }
        guard let routing else {
            return true
        }
        let named = routing.groups.contains { $0.members.contains(id) }
            || routing.rules.contains { $0.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.contains(id) }
        return !named
    }

    /// The library without the node; the selection moves to the first node when it was the selected one.
    public func removing(_ id: String) -> NodeLibrary {
        guard canRemove(id) else {
            return self
        }
        var copy = self
        copy.nodes.removeAll { $0.id == id }
        if copy.selectedID == id {
            copy.selectedID = copy.nodes.first?.id
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
