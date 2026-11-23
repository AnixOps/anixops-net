import Foundation

/// The app's VPN configuration in the system settings, as the disconnected status line explains it. The platform
/// decision (`IosPlatformBridgeMapper`) reports both "not saved" and "turned off" as one developer diagnostic in
/// English; the user sees what to do instead, and the diagnostic stays in developer mode.
public enum TunnelProfileState: Equatable, Sendable {
    /// No configuration yet (or it was removed in Settings): the first connect saves it, and the system asks to add it.
    case missing
    /// Saved but turned off, usually because another VPN app was turned on: a connect turns it on again.
    case disabled
    case ready

    public static func of(hasConfiguration: Bool, isEnabled: Bool) -> TunnelProfileState {
        guard hasConfiguration else {
            return .missing
        }
        return isEnabled ? .ready : .disabled
    }

    /// What the status line says while disconnected, instead of the platform diagnostic.
    public enum OffHint: Equatable, Sendable {
        /// Connecting the first time asks to add the VPN configuration.
        case firstConnect
        /// Connecting turns the configuration on again.
        case reenable
    }

    /// Nil when the configuration is ready, and the usual "tap to connect" applies.
    public var offHint: OffHint? {
        switch self {
        case .missing:
            return .firstConnect
        case .disabled:
            return .reenable
        case .ready:
            return nil
        }
    }
}
