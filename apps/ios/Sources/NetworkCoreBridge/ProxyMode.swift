import Foundation

/// How the tunnel sends traffic, as Surge, Loon and Clash clients name the modes: by the subscription's rules,
/// everything through the selected node, or everything direct. The engine switches between them without a restart.
public enum ProxyMode: String, Codable, CaseIterable, Identifiable, Sendable {
    case rule
    case global
    case direct

    public var id: String {
        rawValue
    }

    /// The mode a saved tunnel profile starts in. Profiles saved before modes existed follow their rules flag.
    public static func saved(in providerConfiguration: [String: Any]?) -> ProxyMode {
        if let raw = providerConfiguration?[TunnelProfileKeys.mode] as? String, let mode = ProxyMode(rawValue: raw) {
            return mode
        }
        return providerConfiguration?[TunnelProfileKeys.routing] as? Bool == true ? .rule : .global
    }

    /// The mode a newly saved tunnel profile (another node, a refreshed subscription) starts in. `current` is the
    /// mode saved now, read from the system settings right before the save, so a mode Shortcuts or Control Center
    /// picked carries over. The direct mode and the node-less profile run direct; a profile that follows the
    /// subscription's rules runs by them; a node list without subscription rules keeps the rule mode of its local
    /// rules (`localRuleList`); anything else runs global.
    public static func forNewProfile(current: ProxyMode, routed: Bool, direct: Bool, localRuleList: Bool) -> ProxyMode {
        if current == .direct || direct {
            return .direct
        }
        if routed || (current == .rule && localRuleList) {
            return .rule
        }
        return .global
    }
}
