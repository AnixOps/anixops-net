import Foundation

/// The two ways AnixOps Net is used (plan A3/A4): a proxy client, or a network debugging tool that captures, decrypts
/// and rewrites traffic, with the proxy only as its base. One app; the user picks on first launch and can switch in
/// Settings. Kept in the app's defaults under `storageKey`.
public enum AppMode: String, CaseIterable, Identifiable, Sendable {
    case proxy
    case debug

    public static let storageKey = "app-mode"
    /// The iOS welcome screen's key (`OnboardingView.storageKey`): set once it was closed.
    public static let onboardingDoneKey = "onboarding-done"

    public var id: String {
        rawValue
    }

    /// The mode to use; nil until the user has picked one. Someone who already imported nodes or closed the old
    /// welcome screen was using the proxy client before modes existed, so they keep it without being asked.
    public static func resolve(stored: String?, hasNodes: Bool, onboardingDone: Bool) -> AppMode? {
        if let stored, let mode = AppMode(rawValue: stored) {
            return mode
        }
        return hasNodes || onboardingDone ? .proxy : nil
    }

    /// The mode to switch to when an `anixops://` link arrives: someone who has not picked a mode yet uses the proxy
    /// client and is not asked (plan A4). Nil leaves the mode alone: while the saved choice has not been read yet
    /// (`loaded` false, so a nil `current` does not mean "never picked"; a link that launched the app arrives before
    /// `load()` finished), once a mode is picked, and for a link that is not a command the app runs.
    public static func forLinkArrival(current: AppMode?, loaded: Bool, isCommand: Bool) -> AppMode? {
        guard loaded, isCommand, current == nil else {
            return nil
        }
        return .proxy
    }
}

/// Where debug mode sends traffic once it has been captured (plan G2): straight to the destination, or through the
/// selected node of the subscriptions, which debug mode keeps under Settings › Exit. Kept in the app's defaults under
/// `storageKey`; direct by default.
public enum DebugExit: String, CaseIterable, Identifiable, Sendable {
    case direct
    case node

    public static let storageKey = "debug-exit"

    public var id: String {
        rawValue
    }

    /// Whether the tunnel runs the direct profile (`TunnelProfile.direct`): debug mode with the direct exit, or with
    /// no node to go through.
    public static func usesDirect(mode: AppMode?, exit: DebugExit, hasNodes: Bool) -> Bool {
        mode == .debug && (exit == .direct || !hasNodes)
    }
}

extension TunnelProfile {
    /// The engine's outbound type that connects straight to the destination.
    public static let directOutboundType = "direct"

    /// A profile without any node: the engine's `direct` outbound in the direct mode, so debug mode can capture,
    /// decrypt and rewrite traffic before anything is imported. It has no secret; the address fields only satisfy
    /// the profile format, the engine never dials them.
    public static func direct(profileID: String, displayName: String) -> TunnelProfile {
        let outbound = EmbeddedRuntimeConfig.NativeOutbound(
            type: directOutboundType,
            server: "127.0.0.1",
            port: 1,
            settings: [:]
        )
        return TunnelProfile(
            config: EmbeddedRuntimeConfig(outbound: .native(outbound), profileID: profileID),
            interface: .proxy,
            serverAddress: "127.0.0.1",
            displayName: displayName
        )
    }

    /// Whether this is the profile `direct(profileID:displayName:)` makes.
    public var isDirect: Bool {
        config.outboundKind == Self.directOutboundType
    }
}
