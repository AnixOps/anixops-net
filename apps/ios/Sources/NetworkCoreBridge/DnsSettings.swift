import Foundation

/// The engine's `dns` section as the user sets it: named upstreams (UDP, TCP, DNS over TLS, HTTPS or QUIC, through
/// the proxy or directly, or the system's own resolver), the default ones that race, the fallback, local hosts,
/// per-domain servers and the Fake-IP switch. Not secret; the app keeps it in
/// `dns-settings.json` and copies it into the tunnel profile (`TunnelProfileKeys.dns`). Empty leaves the engine's
/// default: one resolver over TCP through the proxy.
public struct DnsSettings: Codable, Equatable {
    public static let maxServers = 16
    public static let maxRules = 64
    /// The engine refuses the whole config when its DNS rules list more domains, suffixes and keywords than this in
    /// all (`MAX_DNS_RULE_DOMAINS`); rule sets do not count.
    public static let maxRuleDomains = 4096
    public static let maxHosts = 1024
    public static let maxHostAddresses = 8
    /// Most servers that race as the default, as the engine takes them.
    public static let maxRace = 4
    /// The `url` of the device's own resolver (`getaddrinfo` in the tunnel extension, which the tunnel excludes).
    public static let systemURL = "system"

    public enum Via: String, Codable, CaseIterable, Identifiable {
        /// Through the proxy: the node a rule picks for the server or else the global target, never directly
        /// because a rule says DIRECT; directly only in the direct mode.
        case proxy
        /// Always directly, for a resolver in the user's own country.
        case direct

        public var id: String {
            rawValue
        }
    }

    public struct Server: Codable, Equatable, Identifiable {
        public var name: String
        /// `1.1.1.1`, `1.1.1.1:53`, `udp://host[:53]`, `tcp://host[:53]`, `tls://host[:853]`,
        /// `https://host[:443][/dns-query]`, `quic://host[:853]` or `system`.
        public var url: String
        /// The TLS name when `url` gives an address, such as `cloudflare-dns.com` for `tls://1.1.1.1`.
        public var serverName: String?
        public var via: Via

        public var id: String {
            name
        }

        enum CodingKeys: String, CodingKey {
            case name
            case url
            case serverName = "server_name"
            case via
        }

        public init(name: String, url: String, serverName: String? = nil, via: Via = .proxy) {
            self.name = name
            self.url = url
            self.serverName = serverName
            self.via = via
        }

        /// `UDP`, `TCP`, `DoT`, `DoH`, `DoQ` or the system's, from the URL's scheme.
        public var kind: String {
            if isSystem {
                return String(localized: "系统")
            }
            switch url.lowercased().components(separatedBy: "://").first {
            case "udp" where url.contains("://"):
                return "UDP"
            case "tls" where url.contains("://"):
                return "DoT"
            case "https" where url.contains("://"):
                return "DoH"
            case "quic" where url.contains("://"):
                return "DoQ"
            default:
                return "TCP"
            }
        }

        /// The device's own resolver: always direct, no TLS name.
        public var isSystem: Bool {
            url == DnsSettings.systemURL
        }
    }

    /// Names one server answers: listed domains, their subdomains, names containing a keyword, or the names of a
    /// routing rule set such as `geosite-cn`. The first matching rule wins.
    public struct Rule: Codable, Equatable {
        public var domain: [String]?
        public var domainSuffix: [String]?
        public var domainKeyword: [String]?
        public var ruleSet: String?
        public var server: String

        enum CodingKeys: String, CodingKey {
            case domain
            case domainSuffix = "domain_suffix"
            case domainKeyword = "domain_keyword"
            case ruleSet = "rule_set"
            case server
        }

        public init(
            domain: [String]? = nil,
            domainSuffix: [String]? = nil,
            domainKeyword: [String]? = nil,
            ruleSet: String? = nil,
            server: String
        ) {
            self.domain = domain
            self.domainSuffix = domainSuffix
            self.domainKeyword = domainKeyword
            self.ruleSet = ruleSet
            self.server = server
        }

        /// The rule's conditions as one line, `RULE-SET,geosite-cn · DOMAIN-SUFFIX,cn`, for the list.
        public var summary: String {
            var parts: [String] = []
            if let ruleSet {
                parts.append("RULE-SET,\(ruleSet)")
            }
            parts += (domainSuffix ?? []).map { "DOMAIN-SUFFIX,\($0)" }
            parts += (domain ?? []).map { "DOMAIN,\($0)" }
            parts += (domainKeyword ?? []).map { "DOMAIN-KEYWORD,\($0)" }
            return parts.joined(separator: " · ")
        }

        /// What the rule counts against `DnsSettings.maxRuleDomains`: its domains, suffixes and keywords, as the
        /// engine's `entry_count`.
        public var entryCount: Int {
            (domain ?? []).count + (domainSuffix ?? []).count + (domainKeyword ?? []).count
        }

        var matchesSomething: Bool {
            ruleSet != nil || !(domain ?? []).isEmpty || !(domainSuffix ?? []).isEmpty || !(domainKeyword ?? []).isEmpty
        }
    }

    public var servers: [Server]
    /// The servers for names no rule picks, asked at once with the first good answer winning (at most `maxRace`);
    /// the first server when empty.
    public var defaultServers: [String]
    /// The server asked when the default ones (or a rule's) fail, such as the system's for a company VPN's names.
    public var fallbackServer: String?
    /// `example.lan` or `*.example.lan` to 1–8 addresses, answered without asking a server.
    public var hosts: [String: [String]]
    public var rules: [Rule]
    /// Fake-IP: the tunnel answers names at once with an address from `fakeIPRange` and maps it back to the name
    /// when the app connects, so domain rules match every protocol and the proxy resolves the name. LAN, time,
    /// STUN, captive-portal and push names keep their real answers (the engine's built-in filter).
    public var fakeIP: Bool

    /// The engine's default range; its first /24 holds the tunnel's own addresses (`TunnelInterface`).
    public static let fakeIPRange = "198.18.0.0/15"
    /// The IPv6 range: AAAA queries get an address from the same slot. The tunnel routes `::/0`, so these reach it.
    public static let fakeIPRange6 = "fc00::/18"

    /// The engine's `fake_ip` section, written only when it is on.
    struct FakeIPSection: Codable, Equatable {
        var enabled: Bool
        var range6: String?
    }

    enum CodingKeys: String, CodingKey {
        case servers
        case defaultServers = "default"
        case fallbackServer = "fallback"
        case hosts
        case rules
        case fakeIP = "fake_ip"
    }

    public init(
        servers: [Server] = [],
        defaultServers: [String] = [],
        fallbackServer: String? = nil,
        hosts: [String: [String]] = [:],
        rules: [Rule] = [],
        fakeIP: Bool = false
    ) {
        self.servers = servers
        self.defaultServers = defaultServers
        self.fallbackServer = fallbackServer
        self.hosts = hosts
        self.rules = rules
        self.fakeIP = fakeIP
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        servers = try container.decodeIfPresent([Server].self, forKey: .servers) ?? []
        // One name, or a list that races.
        if let one = try? container.decodeIfPresent(String.self, forKey: .defaultServers) {
            defaultServers = [one]
        } else {
            defaultServers = try container.decodeIfPresent([String].self, forKey: .defaultServers) ?? []
        }
        fallbackServer = try container.decodeIfPresent(String.self, forKey: .fallbackServer)
        hosts = try container.decodeIfPresent([String: [String]].self, forKey: .hosts) ?? [:]
        rules = try container.decodeIfPresent([Rule].self, forKey: .rules) ?? []
        fakeIP = try container.decodeIfPresent(FakeIPSection.self, forKey: .fakeIP)?.enabled ?? false
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        // The engine refuses a default or rules without servers.
        if !servers.isEmpty {
            try container.encode(servers, forKey: .servers)
            // A single name as a string, as engines before the race read it.
            if defaultServers.count == 1 {
                try container.encode(defaultServers[0], forKey: .defaultServers)
            } else if !defaultServers.isEmpty {
                try container.encode(defaultServers, forKey: .defaultServers)
            }
            try container.encodeIfPresent(fallbackServer, forKey: .fallbackServer)
            if !rules.isEmpty {
                try container.encode(rules, forKey: .rules)
            }
        }
        if !hosts.isEmpty {
            try container.encode(hosts, forKey: .hosts)
        }
        // Absent when off, so a profile without Fake-IP also suits an engine that predates it.
        if fakeIP {
            try container.encode(FakeIPSection(enabled: true, range6: Self.fakeIPRange6), forKey: .fakeIP)
        }
    }

    /// Nothing set: the engine keeps its default resolver.
    public var isEmpty: Bool {
        servers.isEmpty && hosts.isEmpty && !fakeIP
    }

    /// The settings with Fake-IP on or off; refused while a host answers with an address in the fake range, which
    /// the engine would refuse.
    public func settingFakeIP(_ on: Bool) throws -> DnsSettings {
        if on, hosts.values.joined().contains(where: Self.isFakeIPAddress) {
            throw DnsSettingsError.fakeIPRange
        }
        var copy = self
        copy.fakeIP = on
        return copy
    }

    /// Whether `text` is an address in `fakeIPRange` (198.18.0.0/15) or `fakeIPRange6` (fc00::/18).
    public static func isFakeIPAddress(_ text: String) -> Bool {
        var v4 = in_addr()
        if inet_pton(AF_INET, text, &v4) == 1 {
            let address = UInt32(bigEndian: v4.s_addr)
            return address & 0xFFFE_0000 == 0xC612_0000
        }
        var v6 = in6_addr()
        guard inet_pton(AF_INET6, text, &v6) == 1 else {
            return false
        }
        let bytes = withUnsafeBytes(of: &v6) { Array($0) }
        // fc00::/18: the first two bytes, then the top two bits of the third.
        return bytes[0] == 0xFC && bytes[1] == 0 && bytes[2] & 0xC0 == 0
    }

    /// The servers that answer names no rule picks: `defaultServers`, otherwise the first server.
    public var effectiveDefaults: [String] {
        if !defaultServers.isEmpty {
            return defaultServers
        }
        return servers.first.map { [$0.name] } ?? []
    }

    /// The first of `effectiveDefaults`, for a one-line summary.
    public var effectiveDefault: String? {
        effectiveDefaults.first
    }

    /// The settings with `name` as the only default server.
    public func settingDefault(_ name: String) throws -> DnsSettings {
        guard servers.contains(where: { $0.name == name }) else {
            throw DnsSettingsError.unknownServer
        }
        var copy = self
        copy.defaultServers = [name]
        return copy
    }

    /// The settings with `name` racing alongside the default servers, or no longer among them. The last default
    /// cannot leave: the first server would silently take its place.
    public func settingRacing(_ name: String, _ on: Bool) throws -> DnsSettings {
        guard servers.contains(where: { $0.name == name }) else {
            throw DnsSettingsError.unknownServer
        }
        var copy = self
        var defaults = effectiveDefaults
        if on {
            guard !defaults.contains(name) else {
                return self
            }
            guard defaults.count < Self.maxRace else {
                throw DnsSettingsError.tooManyDefaults
            }
            defaults.append(name)
        } else {
            guard defaults.count > 1 else {
                return self
            }
            defaults.removeAll { $0 == name }
        }
        copy.defaultServers = defaults
        return copy
    }

    /// The settings with `name` as the fallback server, or none.
    public func settingFallback(_ name: String?) throws -> DnsSettings {
        if let name, !servers.contains(where: { $0.name == name }) {
            throw DnsSettingsError.unknownServer
        }
        var copy = self
        copy.fallbackServer = name
        return copy
    }

    /// The rule-set names the rules use, for the routing section to define.
    public var ruleSetNames: [String] {
        var names: [String] = []
        for name in rules.compactMap(\.ruleSet) where !names.contains(name) {
            names.append(name)
        }
        return names
    }

    // MARK: - Editing

    /// The settings with `server` added, or in place of the server named `replacing`; rules and the default that
    /// named the old name follow the new one.
    public func saving(_ server: Server, replacing old: String? = nil) throws -> DnsSettings {
        var server = server
        if server.isSystem {
            // The engine always asks the system's resolver directly and without TLS.
            server.via = .direct
            server.serverName = nil
        }
        try Self.check(server)
        var copy = self
        if let old, let index = copy.servers.firstIndex(where: { $0.name == old }) {
            guard server.name == old || !copy.servers.contains(where: { $0.name == server.name }) else {
                throw DnsSettingsError.duplicateName
            }
            copy.servers[index] = server
            if old != server.name {
                copy.rules = copy.rules.map { rule in
                    var rule = rule
                    if rule.server == old {
                        rule.server = server.name
                    }
                    return rule
                }
                copy.defaultServers = copy.defaultServers.map { $0 == old ? server.name : $0 }
                if copy.fallbackServer == old {
                    copy.fallbackServer = server.name
                }
            }
        } else {
            guard !copy.servers.contains(where: { $0.name == server.name }) else {
                throw DnsSettingsError.duplicateName
            }
            guard copy.servers.count < Self.maxServers else {
                throw DnsSettingsError.tooMany
            }
            copy.servers.append(server)
        }
        return copy
    }

    /// The settings without the server, its rules, and its place among the default servers or as the fallback.
    public func removingServer(_ name: String) -> DnsSettings {
        var copy = self
        copy.servers.removeAll { $0.name == name }
        copy.rules.removeAll { $0.server == name }
        copy.defaultServers.removeAll { $0 == name }
        if copy.fallbackServer == name {
            copy.fallbackServer = nil
        }
        if copy.servers.isEmpty {
            copy.rules = []
        }
        return copy
    }

    /// Adds a rule from what the user typed: one domain suffix, domain, keyword or rule set per line, as
    /// `DOMAIN-SUFFIX,cn`, `DOMAIN,a.example`, `DOMAIN-KEYWORD,ads`, `RULE-SET,geosite-cn` or a bare suffix.
    public func addingRule(_ text: String, server: String) throws -> DnsSettings {
        guard servers.contains(where: { $0.name == server }) else {
            throw DnsSettingsError.unknownServer
        }
        guard rules.count < Self.maxRules else {
            throw DnsSettingsError.tooMany
        }
        var rule = Rule(server: server)
        var domain: [String] = []
        var suffix: [String] = []
        var keyword: [String] = []
        for line in text.split(whereSeparator: \.isNewline) {
            let parts = line.split(separator: ",", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            let (kind, value) = parts.count == 2 ? (parts[0].uppercased(), parts[1]) : ("DOMAIN-SUFFIX", parts[0])
            guard !value.isEmpty else {
                continue
            }
            switch kind {
            case "DOMAIN":
                domain.append(try Self.domainName(value))
            case "DOMAIN-SUFFIX":
                suffix.append(try Self.domainName(value.hasPrefix(".") ? String(value.dropFirst()) : value))
            case "DOMAIN-KEYWORD":
                keyword.append(value.lowercased())
            case "RULE-SET":
                guard rule.ruleSet == nil, Self.isIdentifier(value) else {
                    throw DnsSettingsError.invalidRule
                }
                rule.ruleSet = value
            case "GEOSITE":
                guard rule.ruleSet == nil else {
                    throw DnsSettingsError.invalidRule
                }
                rule.ruleSet = "geosite-" + value.lowercased()
            default:
                throw DnsSettingsError.invalidRule
            }
        }
        rule.domain = domain.isEmpty ? nil : domain
        rule.domainSuffix = suffix.isEmpty ? nil : suffix
        rule.domainKeyword = keyword.isEmpty ? nil : keyword
        guard rule.matchesSomething else {
            throw DnsSettingsError.invalidRule
        }
        guard ruleDomainCount + rule.entryCount <= Self.maxRuleDomains else {
            throw DnsSettingsError.tooManyDomains
        }
        var copy = self
        copy.rules.append(rule)
        return copy
    }

    /// The domains, suffixes and keywords of every rule, against `maxRuleDomains`.
    public var ruleDomainCount: Int {
        rules.reduce(0) { $0 + $1.entryCount }
    }

    /// The settings as the engine takes them: rules in order while their domains fit `maxRuleDomains`, the rest
    /// left out, with how many rules were. Settings an earlier version saved over the limit would otherwise make the
    /// engine refuse the whole config.
    public func withinDomainLimit() -> (settings: DnsSettings, droppedRules: Int) {
        var copy = self
        var total = 0
        copy.rules = rules.filter { rule in
            guard total + rule.entryCount <= Self.maxRuleDomains else {
                return false
            }
            total += rule.entryCount
            return true
        }
        return (copy, rules.count - copy.rules.count)
    }

    /// Adds or replaces the host `name` with the addresses in `text`, separated by commas, spaces or lines.
    public func settingHost(_ name: String, addresses text: String) throws -> DnsSettings {
        let host = name.trimmingCharacters(in: .whitespaces).lowercased()
        let bare = host.hasPrefix("*.") ? String(host.dropFirst(2)) : host
        _ = try Self.domainName(bare)
        let addresses = text.split(whereSeparator: { $0 == "," || $0.isWhitespace }).map(String.init)
        guard !addresses.isEmpty, addresses.count <= Self.maxHostAddresses, addresses.allSatisfy(Self.isAddress) else {
            throw DnsSettingsError.invalidAddress
        }
        guard !fakeIP || !addresses.contains(where: Self.isFakeIPAddress) else {
            throw DnsSettingsError.fakeIPRange
        }
        guard hosts[host] != nil || hosts.count < Self.maxHosts else {
            throw DnsSettingsError.tooMany
        }
        var copy = self
        copy.hosts[host] = addresses
        return copy
    }

    // MARK: - Checks

    /// Refuses what the engine would refuse, so a bad entry never stops the tunnel from starting.
    public static func check(_ server: Server) throws {
        guard isIdentifier(server.name) else {
            throw DnsSettingsError.invalidName
        }
        guard isUpstream(server.url) else {
            throw DnsSettingsError.invalidURL
        }
        if let name = server.serverName, (try? domainName(name)) == nil {
            throw DnsSettingsError.invalidURL
        }
    }

    /// `[A-Za-z0-9._-]`, 1–128 characters, as the engine names servers and rule sets.
    public static func isIdentifier(_ name: String) -> Bool {
        (1...128).contains(name.count) && name.unicodeScalars.allSatisfy { scalar in
            scalar.isASCII && (CharacterSet.alphanumerics.contains(scalar) || "._-".unicodeScalars.contains(scalar))
        }
    }

    /// The URL forms the engine reads: `1.1.1.1`, `1.1.1.1:53`, `udp://`, `tcp://`, `tls://`, `https://` (the only
    /// one with a path), `quic://`, or `system`.
    public static func isUpstream(_ url: String) -> Bool {
        if url == systemURL {
            return true
        }
        let parts = url.components(separatedBy: "://")
        guard parts.count <= 2 else {
            return false
        }
        let scheme = parts.count == 2 ? parts[0] : "tcp"
        let rest = parts.last ?? ""
        guard ["udp", "tcp", "tls", "https", "quic"].contains(scheme) else {
            return false
        }
        let slash = rest.firstIndex(of: "/")
        let authority = String(rest[..<(slash ?? rest.endIndex)])
        let path = slash.map { String(rest[$0...]) } ?? ""
        guard scheme == "https" || path.isEmpty, !path.contains(where: { $0.isWhitespace }) else {
            return false
        }
        guard let host = hostPart(of: authority) else {
            return false
        }
        // A bare value must be an address: a name there has no scheme to say how to reach it.
        if parts.count == 1 {
            return isAddress(host)
        }
        return isAddress(host) || (try? domainName(host)) != nil
    }

    /// The host of `host`, `host:port`, `[v6]` or `[v6]:port`, nil when the port is not 1–65535.
    static func hostPart(of authority: String) -> String? {
        if authority.hasPrefix("[") {
            guard let close = authority.firstIndex(of: "]") else {
                return nil
            }
            let host = String(authority[authority.index(after: authority.startIndex)..<close])
            let after = authority[authority.index(after: close)...]
            if after.isEmpty {
                return host
            }
            guard after.hasPrefix(":"), let port = UInt16(after.dropFirst()), port > 0 else {
                return nil
            }
            return host
        }
        let pieces = authority.split(separator: ":", omittingEmptySubsequences: false)
        switch pieces.count {
        case 1:
            return authority.isEmpty ? nil : authority
        case 2:
            guard let port = UInt16(pieces[1]), port > 0, !pieces[0].isEmpty else {
                return nil
            }
            return String(pieces[0])
        default:
            // A bare IPv6 address has no port.
            return isAddress(authority) ? authority : nil
        }
    }

    public static func isAddress(_ text: String) -> Bool {
        var v4 = in_addr()
        var v6 = in6_addr()
        return inet_pton(AF_INET, text, &v4) == 1 || inet_pton(AF_INET6, text, &v6) == 1
    }

    /// A lowercased domain name, or `invalidDomain`.
    static func domainName(_ text: String) throws -> String {
        let name = text.trimmingCharacters(in: .whitespaces).lowercased()
        let labels = name.split(separator: ".", omittingEmptySubsequences: false)
        guard !name.isEmpty, name.count <= 253, labels.allSatisfy({ label in
            (1...63).contains(label.count) && label.unicodeScalars.allSatisfy { scalar in
                scalar.isASCII && (CharacterSet.alphanumerics.contains(scalar) || scalar == "-" || scalar == "_")
            }
        }) else {
            throw DnsSettingsError.invalidDomain
        }
        return name
    }

    // MARK: - Storage

    public static var defaultURL: URL {
        get throws {
            try FileManager.default
                .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("dns-settings.json")
        }
    }

    public static func load(from url: URL) throws -> DnsSettings {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return DnsSettings()
        }
        return try JSONDecoder().decode(DnsSettings.self, from: Data(contentsOf: url))
    }

    public func save(to url: URL) throws {
        try JSONEncoder().encode(self).write(to: url, options: [.atomic])
    }

    /// The tunnel profile value under `TunnelProfileKeys.dns`; nil when there is nothing to set.
    public func profileValue() throws -> Data? {
        if isEmpty {
            return nil
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(self)
    }

    public static func fromProfileValue(_ value: Data?) -> DnsSettings? {
        value.flatMap { try? JSONDecoder().decode(DnsSettings.self, from: $0) }.flatMap { $0.isEmpty ? nil : $0 }
    }

    // MARK: - Presets

    /// A resolver the settings page offers in one tap.
    public struct Preset: Identifiable {
        public let id: String
        public let title: String
        public let server: Server
    }

    public static let presets: [Preset] = [
        Preset(id: "alidns", title: String(localized: "阿里 DNS（DoH，直连）"), server: Server(name: "alidns", url: "https://223.5.5.5/dns-query", serverName: "dns.alidns.com", via: .direct)),
        Preset(id: "alidns-doq", title: String(localized: "阿里 DNS（DoQ，直连）"), server: Server(name: "alidns-doq", url: "quic://223.5.5.5", serverName: "dns.alidns.com", via: .direct)),
        Preset(id: "dnspod", title: String(localized: "腾讯 DNSPod（DoH，直连）"), server: Server(name: "dnspod", url: "https://1.12.12.12/dns-query", serverName: "doh.pub", via: .direct)),
        Preset(id: "cloudflare", title: String(localized: "Cloudflare（DoH，经代理）"), server: Server(name: "cloudflare", url: "https://1.1.1.1/dns-query", serverName: "cloudflare-dns.com")),
        Preset(id: "google", title: String(localized: "Google（DoH，经代理）"), server: Server(name: "google", url: "https://8.8.8.8/dns-query", serverName: "dns.google")),
        Preset(id: "quad9", title: String(localized: "Quad9（DoT，经代理）"), server: Server(name: "quad9", url: "tls://9.9.9.9", serverName: "dns.quad9.net")),
        Preset(id: "system", title: String(localized: "系统 DNS（本机网络，直连）"), server: Server(name: "system", url: systemURL, via: .direct)),
    ]

    // Presets name their servers by address: the extension cannot rely on the system resolver to find a
    // resolver's own host name while the tunnel holds the system's DNS. They are servers only: public builds ship
    // no preset per-domain rules (docs/no-presets.md), so which names go where is always the user's own setting.
}

/// Why a DNS setting was refused. The messages are fixed text without the values typed.
public enum DnsSettingsError: Error, Equatable {
    case invalidName
    case duplicateName
    case invalidURL
    case invalidDomain
    case invalidAddress
    case invalidRule
    case unknownServer
    case tooMany
    case tooManyDefaults
    case fakeIPRange
    /// The rules would list more than `DnsSettings.maxRuleDomains` domains, suffixes and keywords.
    case tooManyDomains

    public var message: String {
        switch self {
        case .invalidName:
            return String(localized: "名称只能用字母、数字和 . _ -")
        case .duplicateName:
            return String(localized: "已有同名的 DNS 服务器")
        case .invalidURL:
            return String(localized: "地址应为 IP、system，或 udp://、tcp://、tls://、https://、quic:// 开头")
        case .invalidDomain:
            return String(localized: "域名格式不正确")
        case .invalidAddress:
            return String(localized: "请填写 1–8 个 IP 地址")
        case .invalidRule:
            return String(localized: "每行写一个域名后缀，或 DOMAIN、DOMAIN-KEYWORD、RULE-SET、GEOSITE 开头的条件")
        case .unknownServer:
            return String(localized: "请先添加 DNS 服务器")
        case .tooMany:
            return String(localized: "已达到数量上限")
        case .tooManyDefaults:
            return String(localized: "最多 4 个默认服务器同时查询")
        case .fakeIPRange:
            return String(localized: "Fake-IP 开启时，Hosts 不能使用 198.18.0.0/15 或 fc00::/18 内的地址")
        case .tooManyDomains:
            return String(localized: "DNS 规则合计最多 \(DnsSettings.maxRuleDomains) 个域名、后缀和关键词。较长的列表请改用 RULE-SET 或 GEOSITE 规则集。")
        }
    }
}
