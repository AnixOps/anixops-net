import Foundation

/// How the tunnel takes over the device's traffic.
///
/// - `tunnelAndProxy`: the tunnel interface takes every connection, and the tunnel also registers the engine's
///   `127.0.0.1` listener as the system HTTP/HTTPS proxy (`NEProxySettings`). Apps that honor the proxy, most of
///   what goes through the system's URL loading, skip the userspace TCP/IP stack and their requests reach the rules
///   by name; everything else still takes the tunnel. The default on iOS and macOS.
/// - `tunnel`: the tunnel interface alone, as before the proxy existed.
/// - `systemProxy` (macOS only): the system proxy alone, no routes into the tunnel. Apps that ignore the proxy go
///   straight out, untouched by rules; lighter, and the usual mode on a Mac.
///
/// The app keeps the choice in its own defaults and carries a copy in the tunnel profile
/// (`TunnelProfileKeys.takeover`); a change restarts a running tunnel, since the interface settings change.
public enum TrafficTakeover: String, Codable, CaseIterable, Identifiable, Sendable {
    case tunnelAndProxy = "tun-proxy"
    case tunnel = "tun"
    case systemProxy = "proxy"

    public static let storageKey = "networkcore.takeover"
    public static let `default` = TrafficTakeover.tunnelAndProxy

    public var id: String {
        rawValue
    }

    /// The modes a platform offers, the lightest first. iOS has no proxy-only mode: a VPN without routes would
    /// leave apps that ignore the proxy outside the rules with no way to see why.
    public static func choices(macOS: Bool) -> [TrafficTakeover] {
        macOS ? [.systemProxy, .tunnelAndProxy, .tunnel] : [.tunnelAndProxy, .tunnel]
    }

    /// Whether the tunnel interface takes the device's routes and DNS.
    public var routesTraffic: Bool {
        self != .systemProxy
    }

    /// Whether the tunnel registers the engine's listener as the system proxy.
    public var usesSystemProxy: Bool {
        self != .tunnel
    }

    /// The mode on this platform: a mode it does not offer is the default.
    public func supported(macOS: Bool) -> TrafficTakeover {
        Self.choices(macOS: macOS).contains(self) ? self : .default
    }

    /// The mode a saved tunnel profile starts in. Profiles saved before the choice existed get the default.
    public static func saved(in providerConfiguration: [String: Any]?, macOS: Bool) -> TrafficTakeover {
        let raw = providerConfiguration?[TunnelProfileKeys.takeover] as? String
        return (raw.flatMap(TrafficTakeover.init(rawValue:)) ?? .default).supported(macOS: macOS)
    }
}

/// Hosts and networks the system proxy leaves alone (`NEProxySettings.exceptionList`): the local network and
/// names that must not depend on the proxy. Connections to them still take the tunnel when it routes traffic.
///
/// Each entry is a host name, a `*.` wildcard, an IPv4 or IPv6 address, or an address with a prefix
/// (`192.168.0.0/16`). The user can edit the list; the profile carries it as `TunnelProfileKeys.proxyExceptions`.
public struct ProxyExceptions: Codable, Equatable, Sendable {
    public static let storageKey = "networkcore.proxy-exceptions"
    /// Longer lists are cut; the system reads the whole list for every connection.
    public static let maximumCount = 256

    public static let defaults = [
        "localhost",
        "*.local",
        "captive.apple.com",
        "127.0.0.0/8",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "169.254.0.0/16",
        "100.64.0.0/10",
        "::1",
        "fc00::/7",
        "fe80::/10",
    ]

    public var entries: [String]

    public init(entries: [String] = ProxyExceptions.defaults) {
        self.entries = entries
    }

    /// Parses one entry per line. Blank lines and `#` comments are skipped, duplicates kept once (ignoring case);
    /// the lines that are not a host, wildcard or address come back in `rejected` with their line number.
    public static func parse(_ text: String) -> (exceptions: ProxyExceptions, rejected: [Int]) {
        var entries: [String] = []
        var seen = Set<String>()
        var rejected: [Int] = []
        for (index, raw) in text.components(separatedBy: .newlines).enumerated() {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") {
                continue
            }
            guard isValid(line) else {
                rejected.append(index + 1)
                continue
            }
            if seen.insert(line.lowercased()).inserted, entries.count < maximumCount {
                entries.append(line)
            }
        }
        return (ProxyExceptions(entries: entries), rejected)
    }

    /// One entry per line, for editing.
    public var text: String {
        entries.joined(separator: "\n")
    }

    public var isDefault: Bool {
        entries == Self.defaults
    }

    /// A host name or `*.` wildcard of letters, digits, `-` and `.`, or an address with an optional prefix.
    public static func isValid(_ entry: String) -> Bool {
        if let cidr = CIDR.parse(entry) {
            return !entry.contains("/") || !cidr.isDefaultRoute
        }
        var host = Substring(entry)
        if host.hasPrefix("*.") {
            host = host.dropFirst(2)
        }
        guard !host.isEmpty, host.count <= 253, !host.hasPrefix("."), !host.hasSuffix("."), !host.contains("..") else {
            return false
        }
        return host.unicodeScalars.allSatisfy { scalar in
            (scalar.isASCII && CharacterSet.alphanumerics.contains(scalar)) || scalar == "-" || scalar == "."
        }
    }

    public var profileValue: Data {
        (try? JSONEncoder().encode(entries)) ?? Data()
    }

    /// The list in a profile; nil or unreadable data is the default list.
    public static func fromProfileValue(_ data: Data?) -> ProxyExceptions {
        guard let data, let entries = try? JSONDecoder().decode([String].self, from: data) else {
            return ProxyExceptions()
        }
        return ProxyExceptions(entries: Array(entries.filter(isValid).prefix(maximumCount)))
    }
}
