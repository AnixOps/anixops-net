import Foundation

/// How much a line of the extension's event log matters. Ordered, so a filter can show one level and above.
public enum DiagnosticLevel: String, Codable, CaseIterable, Comparable, Sendable, Identifiable {
    case debug
    case info
    case warning
    case error

    public var id: String {
        rawValue
    }

    private var rank: Int {
        switch self {
        case .debug: return 0
        case .info: return 1
        case .warning: return 2
        case .error: return 3
        }
    }

    public static func < (lhs: DiagnosticLevel, rhs: DiagnosticLevel) -> Bool {
        lhs.rank < rhs.rank
    }
}

/// Which part of the tunnel a line of the event log is about.
public enum DiagnosticModule: String, Codable, CaseIterable, Sendable, Identifiable {
    /// Start, stop, the profile, the node key and start secrets, sleep and wake.
    case tunnel
    /// The embedded engine itself.
    case runtime
    /// The network under the tunnel: path changes and the bound interface on macOS.
    case network
    /// Subscription rules, the mode and redirects.
    case rules
    case dns
    /// HTTPS decryption and its plugins.
    case mitm
    /// Plugin scripts and notifications.
    case scripts
    /// Memory pressure and footprint.
    case memory

    public var id: String {
        rawValue
    }

    /// The level and module of a fixed event message. The extension's messages are fixed English phrases, so a
    /// prefix and a few words decide; anything unknown is an info line of the tunnel.
    public static func classify(_ message: String) -> (level: DiagnosticLevel, module: DiagnosticModule) {
        let text = message.lowercased()
        return (level(of: text), module(of: text))
    }

    private static func module(of text: String) -> DiagnosticModule {
        let prefixes: [(String, DiagnosticModule)] = [
            ("https decryption", .mitm),
            ("scripts off", .scripts),
            ("notifications", .scripts),
            ("subscription rules", .rules),
            ("mode ", .rules),
            ("redirects", .rules),
            ("rules reload", .rules),
            ("local rules", .rules),
            ("dns ", .dns),
            ("memory pressure", .memory),
            ("network changed", .network),
            ("network settings", .network),
            ("bound interface", .network),
            ("runtime", .runtime),
            ("embedded runtime", .runtime),
        ]
        for (prefix, module) in prefixes where text.hasPrefix(prefix) {
            return module
        }
        if text.hasSuffix("scheduled scripts") {
            return .scripts
        }
        if text.contains("embedded runtime") {
            return .runtime
        }
        return .tunnel
    }

    private static func level(of text: String) -> DiagnosticLevel {
        let errors = ["failed", "refused", "malformed", "unavailable", "could not", "missing", "error"]
        if errors.contains(where: text.contains) {
            return .error
        }
        let warnings = [" off", ": none", "not kept", ": restart", "memory pressure", "unpinned", "stoptunnel reason"]
        if warnings.contains(where: text.contains) {
            // A user stop (reason 1) is routine; every other reason is worth a look.
            if text.hasPrefix("stoptunnel reason ") {
                return text == "stoptunnel reason 1" ? .info : .warning
            }
            return .warning
        }
        let debug = ["profile loaded", "bound interface ", "notifications ", "device sleep", "device wake", "start secrets from"]
        if debug.contains(where: text.hasPrefix) {
            return .debug
        }
        return .info
    }
}

extension Array where Element == TunnelDiagnosticEvent {
    /// The lines at `minimum` or above, of the given modules (all when the set is empty).
    public func filtered(minimum: DiagnosticLevel, modules: Set<DiagnosticModule> = []) -> [TunnelDiagnosticEvent] {
        filter { $0.level >= minimum && (modules.isEmpty || modules.contains($0.module)) }
    }
}
