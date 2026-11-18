import Foundation

/// Captured connections the request list has no request from, by host. Most are HTTPS to a domain that is not
/// decrypted, so the engine kept only TLS records; the request list shows them under its requests so "0 requests"
/// never hides a capture that did see traffic, and offers to decrypt each domain.
public struct CaptureHostGroup: Identifiable, Equatable {
    /// The domain, or the destination address when the connection had none.
    public let name: String
    /// `name` is a domain name, so HTTPS decryption can be turned on for it.
    public let isDomain: Bool
    /// Newest first.
    public private(set) var connections: [RuntimeControl.Connection]
    /// Both directions of every connection.
    public private(set) var bytes: UInt64
    /// `tcp`, `udp`, in that order, as seen.
    public private(set) var networks: [String]

    public var id: String {
        name
    }

    public var latest: Date {
        connections.first?.startedAt ?? .distantPast
    }

    init(name: String, isDomain: Bool, connection: RuntimeControl.Connection) {
        self.name = name
        self.isDomain = isDomain
        connections = [connection]
        bytes = connection.upload &+ connection.download
        networks = [connection.network]
    }

    mutating func add(_ connection: RuntimeControl.Connection) {
        let at = connections.firstIndex { $0.startedAt < connection.startedAt } ?? connections.endIndex
        connections.insert(connection, at: at)
        bytes &+= connection.upload &+ connection.download
        if !networks.contains(connection.network) {
            networks.append(connection.network)
            networks.sort()
        }
    }
}

public enum CaptureCoverage {
    /// The connections capture kept (`captured`) that no request row came from (`withRequests`, the rows'
    /// connection ids), grouped by domain or address, the most recently used first.
    public static func hostsWithoutRequests(
        _ connections: [RuntimeControl.Connection],
        withRequests: Set<UInt64>
    ) -> [CaptureHostGroup] {
        var groups: [String: CaptureHostGroup] = [:]
        var seen = Set<UInt64>()
        for connection in connections where connection.captured && !withRequests.contains(connection.id) {
            // The engine lists a connection once, but an answer read while it closed may carry it twice.
            guard seen.insert(connection.id).inserted else {
                continue
            }
            let domain = connection.host.flatMap(DebugMappings.normalizeHost)
            let name = domain ?? address(of: connection.destination)
            if groups[name] == nil {
                groups[name] = CaptureHostGroup(name: name, isDomain: domain != nil, connection: connection)
            } else {
                groups[name]?.add(connection)
            }
        }
        return groups.values.sorted { lhs, rhs in
            lhs.latest != rhs.latest ? lhs.latest > rhs.latest : lhs.name < rhs.name
        }
    }

    /// `destination` without its port: `1.2.3.4:443` → `1.2.3.4`, `[2001:db8::1]:443` → `2001:db8::1`.
    static func address(of destination: String) -> String {
        if destination.hasPrefix("["), let close = destination.firstIndex(of: "]") {
            return String(destination[destination.index(after: destination.startIndex)..<close])
        }
        if destination.filter({ $0 == ":" }).count == 1, let colon = destination.lastIndex(of: ":") {
            return String(destination[..<colon])
        }
        return destination
    }
}

/// The domains HTTPS decryption is asked for: the decryption list, the active mappings' hosts and the enabled
/// plugins' `[MITM]` hostnames, less the exclusions of the list and the plugins (an exclusion wins over every
/// include, as the engine merges them). Only for what the app shows; the engine decides from the same lists.
public struct DecryptionDomains: Equatable {
    public let patterns: [String]
    public let excluded: [String]

    public init(mappings: DebugMappings, plugins: MitmPluginLibrary) {
        let enabled = plugins.isEnabled ? plugins.plugins.filter(\.isEnabled) : []
        self.init(
            patterns: mappings.hosts + mappings.active.compactMap(\.host) + enabled.flatMap(\.summary.hostnames),
            excluded: mappings.excludedHosts + enabled.flatMap(\.summary.excludedHostnames)
        )
    }

    public init(patterns: [String], excluded: [String] = []) {
        self.patterns = Self.unique(patterns)
        self.excluded = Self.unique(excluded)
    }

    private static func unique(_ patterns: [String]) -> [String] {
        var unique: [String] = []
        for pattern in patterns {
            let pattern = clean(pattern)
            if !pattern.isEmpty, !unique.contains(pattern) {
                unique.append(pattern)
            }
        }
        return unique
    }

    public var isEmpty: Bool {
        patterns.isEmpty
    }

    /// Whether `host` is decrypted: it matches one of the patterns, an exact name or a `*` / `?` wildcard as in
    /// `*.example.com`, and none of the exclusions.
    public func covers(_ host: String) -> Bool {
        guard let name = DebugMappings.normalizeHost(host) else {
            return false
        }
        return patterns.contains { Self.glob(Array($0), Array(name)) }
            && !excluded.contains { Self.glob(Array($0), Array(name)) }
    }

    /// Whether an exclusion names `host`.
    public func excludes(_ host: String) -> Bool {
        guard let name = DebugMappings.normalizeHost(host) else {
            return false
        }
        return excluded.contains { Self.glob(Array($0), Array(name)) }
    }

    /// Whether `pattern` matches `host`, by the same rules as `covers`.
    public static func matches(_ pattern: String, _ host: String) -> Bool {
        guard let name = DebugMappings.normalizeHost(host) else {
            return false
        }
        return glob(Array(clean(pattern)), Array(name))
    }

    /// Lower-cased, without a `:port` (Loon allows `example.com:8443`) or a trailing dot.
    private static func clean(_ pattern: String) -> String {
        var text = pattern.trimmingCharacters(in: .whitespaces).lowercased()
        if let colon = text.lastIndex(of: ":"), text[text.index(after: colon)...].allSatisfy(\.isNumber) {
            text = String(text[..<colon])
        }
        while text.hasSuffix(".") {
            text.removeLast()
        }
        return text
    }

    /// `*` matches any run of characters, `?` one character.
    static func glob(_ pattern: [Character], _ text: [Character]) -> Bool {
        var p = 0
        var t = 0
        var star: Int?
        var mark = 0
        while t < text.count {
            if p < pattern.count, pattern[p] == "?" || pattern[p] == text[t] {
                p += 1
                t += 1
            } else if p < pattern.count, pattern[p] == "*" {
                star = p
                mark = t
                p += 1
            } else if let star {
                p = star + 1
                mark += 1
                t = mark
            } else {
                return false
            }
        }
        while p < pattern.count, pattern[p] == "*" {
            p += 1
        }
        return p == pattern.count
    }
}
