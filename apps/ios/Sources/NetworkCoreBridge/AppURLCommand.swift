import Foundation

/// What another app, a Shortcut or a web page asks for with an `anixops://` link. The six actions:
///
/// - `anixops://connect`, `anixops://disconnect`, `anixops://toggle`
/// - `anixops://mode?set=rule|global|direct`
/// - `anixops://select?node=<name>[&group=<subscription name or id>]` picks a node of a subscription group (the
///   active one when `group` is left out); `anixops://select?policy=<policy group>&node=<member>` picks a member of
///   a policy group in the subscription's rules
/// - `anixops://import?url=<subscription or node link>` opens the import sheet with that link; nothing is saved
///   until the user confirms it there
///
/// `anixops:///connect` and `anixops://x-callback-url/connect` are accepted too, the two spellings Shortcuts and
/// x-callback-url callers produce. Parsing never touches the tunnel, so it is tested on its own.
public enum AppURLCommand: Equatable, Sendable {
    case connect
    case disconnect
    case toggle
    case mode(ProxyMode)
    case selectNode(name: String, group: String?)
    case selectMember(policy: String, member: String)
    case importLink(String)

    public static let scheme = "anixops"
    /// Links longer than this are refused, so a page cannot hand the import sheet megabytes of text.
    public static let maxLinkLength = 8 * 1024

    public enum Failure: Error, Equatable {
        case notOurScheme
        case unknownAction(String)
        case missing(String)
        case invalid(String)
    }

    public static func parse(_ url: URL) -> Result<AppURLCommand, Failure> {
        guard url.scheme?.lowercased() == scheme,
              let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        else {
            return .failure(.notOurScheme)
        }
        guard url.absoluteString.utf8.count <= maxLinkLength + 256 else {
            return .failure(.invalid("url"))
        }
        let action = Self.action(host: components.host, path: components.path)
        var query: [String: String] = [:]
        for item in components.queryItems ?? [] where query[item.name.lowercased()] == nil {
            query[item.name.lowercased()] = item.value ?? ""
        }
        func value(_ name: String) -> String? {
            query[name].map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.flatMap { $0.isEmpty ? nil : $0 }
        }

        switch action {
        case "connect", "start", "on":
            return .success(.connect)
        case "disconnect", "stop", "off":
            return .success(.disconnect)
        case "toggle":
            return .success(.toggle)
        case "mode":
            guard let raw = value("set") ?? value("mode") else {
                return .failure(.missing("set"))
            }
            guard let mode = ProxyMode(rawValue: raw.lowercased()) else {
                return .failure(.invalid("set"))
            }
            return .success(.mode(mode))
        case "select":
            guard let node = value("node") else {
                return .failure(.missing("node"))
            }
            if let policy = value("policy") {
                return .success(.selectMember(policy: policy, member: node))
            }
            return .success(.selectNode(name: node, group: value("group")))
        case "import", "install-config", "add":
            guard let link = value("url") else {
                return .failure(.missing("url"))
            }
            guard link.utf8.count <= maxLinkLength else {
                return .failure(.invalid("url"))
            }
            return .success(.importLink(link))
        case "":
            return .failure(.missing("action"))
        default:
            return .failure(.unknownAction(action))
        }
    }

    /// `anixops://connect` keeps the action in the host; `anixops:///connect` and
    /// `anixops://x-callback-url/connect` keep it in the path.
    private static func action(host: String?, path: String) -> String {
        let segments = path.split(separator: "/").map(String.init)
        let host = host?.lowercased() ?? ""
        if host.isEmpty || host == "x-callback-url" {
            return segments.first?.lowercased() ?? ""
        }
        return host
    }

    /// The group and node a `selectNode` names: the group by id, else by name (case-insensitive), else the active
    /// group; the node by name, else by id. Nil when either is not found.
    public static func resolveNode(
        name: String,
        group: String?,
        in subscriptions: SubscriptionGroups
    ) -> (groupID: String, nodeID: String)? {
        let target: SubscriptionGroup?
        if let group {
            target = subscriptions.group(id: group)
                ?? subscriptions.groups.first { $0.name.caseInsensitiveCompare(group) == .orderedSame }
        } else {
            target = subscriptions.active ?? subscriptions.groups.first
        }
        guard let target else {
            return nil
        }
        let nodes = target.library.nodes
        guard let node = nodes.first(where: { $0.name == name })
            ?? nodes.first(where: { $0.name.caseInsensitiveCompare(name) == .orderedSame })
            ?? nodes.first(where: { $0.id == name })
        else {
            return nil
        }
        return (target.id, node.id)
    }
}
