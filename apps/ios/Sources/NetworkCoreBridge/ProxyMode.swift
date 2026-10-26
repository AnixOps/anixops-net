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
}
