import Foundation

/// The rule mode of a node list without subscription rules (a plain link or base64 subscription): the user's own
/// rules and rule sets, then everything else through the proxy. The app ships no preset rules or rule lists of its
/// own (`docs/no-presets.md`); what is here is only what the user wrote, pasted or added by URL. The user's rules also
/// go in front of a Clash, Surge or Loon subscription's rules. Not secret; the app keeps them in `local-rules.json` and copies them into the
/// tunnel profile (`TunnelProfileKeys.localRules`), and the extension builds the engine `routing` with
/// `NodeLibrary.routingForModes(localRules:)`.
///
/// The engine refuses the whole routing when one line is wrong, so every line is checked here (`rule(from:)`) before it
/// is kept.
public struct LocalRules: Codable, Equatable {
    /// The engine accepts more, but a hand-kept list this long is already hard to read on a phone.
    public static let maxRules = 1000

    /// Where a rule sends matching traffic. `proxy` is the selected node, or with subscription rules the
    /// subscription's first policy group. The engine answers `rejectDrop` with nothing, `rejectTinyGif` with a
    /// transparent 1x1 GIF to a plain HTTP request, and never turns `rejectNoDrop` into a drop when an app retries
    /// it too often (a plain `reject` it does).
    public enum Policy: String, Codable, CaseIterable, Identifiable {
        case proxy = "PROXY"
        case direct = "DIRECT"
        case reject = "REJECT"
        case rejectNoDrop = "REJECT-NO-DROP"
        case rejectDrop = "REJECT-DROP"
        case rejectTinyGif = "REJECT-TINYGIF"

        public var id: String {
            rawValue
        }

        /// Whether the policy refuses the connection.
        public var isReject: Bool {
            switch self {
            case .proxy, .direct:
                return false
            case .reject, .rejectNoDrop, .rejectDrop, .rejectTinyGif:
                return true
            }
        }
    }

    public struct Rule: Codable, Equatable, Identifiable {
        public var id: UUID
        /// `DOMAIN-SUFFIX`, `IP-CIDR`, `RULE-SET` and the other types `kinds` lists, upper case.
        public var kind: String
        /// The value, such as `example.com`, `10.0.0.0/8` or a rule-set URL.
        public var value: String
        public var policy: Policy
        /// `no-resolve` on an IP rule: a destination given by name is not resolved for it.
        public var noResolve: Bool
        public var enabled: Bool

        public init(
            id: UUID = UUID(),
            kind: String,
            value: String,
            policy: Policy,
            noResolve: Bool = false,
            enabled: Bool = true
        ) {
            self.id = id
            self.kind = kind
            self.value = value
            self.policy = policy
            self.noResolve = noResolve
            self.enabled = enabled
        }

        /// The rule as Surge and Loon write it, with `PROXY` for the proxy.
        public var text: String {
            var parts = [kind, value, policy.rawValue]
            if noResolve {
                parts.append("no-resolve")
            }
            return parts.joined(separator: ",")
        }
    }

    /// A rule list the user added by URL (any Surge, Loon or Clash list), which the engine downloads and refreshes
    /// itself. Matching traffic goes to `policy`.
    public struct Subscription: Codable, Equatable, Identifiable {
        public var id: UUID
        public var name: String
        /// An `https` list URL.
        public var url: String
        public var policy: Policy
        public var enabled: Bool

        public init(
            id: UUID = UUID(),
            name: String,
            url: String,
            policy: Policy,
            enabled: Bool = true
        ) {
            self.id = id
            self.name = name
            self.url = url
            self.policy = policy
            self.enabled = enabled
        }

        /// The name the engine knows the list by, stable across edits and reordering.
        public var engineName: String {
            "sub-" + id.uuidString.prefix(8).lowercased()
        }
    }

    /// The engine accepts at most 32 rule sets in all, and the built-in GEOSITE/GEOIP lists and a subscription's own
    /// sets need some of them.
    public static let maxSubscriptions = 20

    /// Private and local networks (`10.0.0.0/8`, `192.168.0.0/16`, `*.local` and the like) go direct.
    public var bypassLAN: Bool
    public var rules: [Rule]
    /// Downloaded rule lists, matched after `rules` and in this order.
    public var subscriptions: [Subscription]
    /// The `GEOIP` and `IP-ASN` data lists the user added by URL, by name. They replace a built-in list and the
    /// subscription's own set of the same name, as on Windows, Linux and Android (`data-sources.json`).
    public var dataSources: [DataSource]
    /// Names for the devices that use the Mac as their gateway, by the address their connections come from, for
    /// `DEVICE-NAME` rules (the engine's `routing.devices`), as on Windows and Linux (`devices.json`).
    public var devices: [String: String]

    public init(
        bypassLAN: Bool = true,
        rules: [Rule] = [],
        subscriptions: [Subscription] = [],
        dataSources: [DataSource] = [],
        devices: [String: String] = [:]
    ) {
        self.bypassLAN = bypassLAN
        self.rules = rules
        self.subscriptions = subscriptions
        self.dataSources = dataSources
        self.devices = devices
    }

    /// Values saved by earlier versions may still carry a `preset` (one of the removed preset list modes); it is ignored, since the
    /// app no longer ships preset lists, and unmatched traffic goes through the proxy.
    enum CodingKeys: String, CodingKey {
        case bypassLAN = "bypass_lan"
        case rules
        case subscriptions
        case dataSources = "data_sources"
        case devices
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            bypassLAN: try container.decodeIfPresent(Bool.self, forKey: .bypassLAN) ?? true,
            rules: try container.decodeIfPresent([Rule].self, forKey: .rules) ?? [],
            subscriptions: try container.decodeIfPresent([Subscription].self, forKey: .subscriptions) ?? [],
            dataSources: (try? container.decodeIfPresent([DataSource].self, forKey: .dataSources)) ?? [],
            devices: (try? container.decodeIfPresent([String: String].self, forKey: .devices)) ?? [:]
        )
    }

    // MARK: - Engine lines

    /// The rule types a local rule can use. `GEOSITE` is limited to the catalog's lists (`RuleSetCatalog.known`)
    /// because the engine refuses a category it has no list for; `RULE-SET` takes an `https` list URL.
    /// `DEVICE-NAME` only on the Mac, where the tunnel can be the gateway of other devices; `CELLULAR-RADIO` only on
    /// the iPhone and iPad, which report the radio technology.
    public static let kinds: [String] = {
        var kinds = [
            "DOMAIN-SUFFIX", "DOMAIN", "DOMAIN-KEYWORD", "DOMAIN-WILDCARD", "IP-CIDR", "IP-CIDR6", "GEOIP", "IP-ASN",
            "GEOSITE", "DST-PORT", "SRC-PORT", "PROTOCOL", "HOSTNAME-TYPE", "SUBNET",
        ]
        #if os(macOS)
        kinds.append("DEVICE-NAME")
        #else
        kinds.append("CELLULAR-RADIO")
        #endif
        return kinds + ["USER-AGENT", "RULE-SET"]
    }()

    static let lanLines = [
        "DOMAIN-SUFFIX,local,DIRECT",
        "IP-CIDR,10.0.0.0/8,DIRECT,no-resolve",
        "IP-CIDR,100.64.0.0/10,DIRECT,no-resolve",
        "IP-CIDR,127.0.0.0/8,DIRECT,no-resolve",
        "IP-CIDR,169.254.0.0/16,DIRECT,no-resolve",
        "IP-CIDR,172.16.0.0/12,DIRECT,no-resolve",
        "IP-CIDR,192.168.0.0/16,DIRECT,no-resolve",
        "IP-CIDR6,fc00::/7,DIRECT,no-resolve",
        "IP-CIDR6,fe80::/10,DIRECT,no-resolve",
    ]

    /// The name the engine knows a `RULE-SET` URL by.
    static func ruleSetName(_ index: Int) -> String {
        "local-\(index + 1)"
    }

    /// The user's enabled rules and then the enabled rule-list subscriptions as engine lines, `PROXY` replaced by
    /// `proxy`, and the rule sets their URLs become, downloaded through `proxy`.
    public func userLines(proxy: String) -> (lines: [String], ruleSets: [String: EmbeddedRuntimeConfig.RuleSet]) {
        var lines: [String] = []
        var sets: [String: EmbeddedRuntimeConfig.RuleSet] = [:]
        var urls: [String: String] = [:]
        for rule in rules where rule.enabled {
            let target = rule.policy == .proxy ? proxy : rule.policy.rawValue
            var value = rule.value
            if rule.kind == "RULE-SET" {
                if let name = urls[rule.value] {
                    value = name
                } else {
                    let name = Self.ruleSetName(urls.count)
                    urls[rule.value] = name
                    sets[name] = .init(url: rule.value, via: proxy)
                    value = name
                }
            }
            var line = "\(rule.kind),\(value),\(target)"
            if rule.noResolve {
                line += ",no-resolve"
            }
            lines.append(line)
        }
        for subscription in subscriptions where subscription.enabled {
            let name = subscription.engineName
            let target = subscription.policy == .proxy ? proxy : subscription.policy.rawValue
            sets[name] = .init(url: subscription.url, via: proxy)
            lines.append("RULE-SET,\(name),\(target)")
        }
        for source in dataSources {
            sets[source.name] = .init(url: source.url, via: proxy)
        }
        return (lines, sets)
    }

    /// Every engine line for a node list without subscription rules: the user's rules and rule sets, the private
    /// networks, then `FINAL` through the proxy.
    public func engineLines(proxy: String) -> (lines: [String], ruleSets: [String: EmbeddedRuntimeConfig.RuleSet]) {
        var (lines, sets) = userLines(proxy: proxy)
        if bypassLAN {
            lines += Self.lanLines
        }
        lines += ["FINAL,\(proxy)"]
        for (name, set) in RuleSetCatalog.ruleSets(for: lines, defined: nil) ?? [:] where sets[name] == nil {
            sets[name] = set
        }
        return (lines, sets)
    }

    // MARK: - Editing

    public enum Failure: Error, Equatable {
        case empty
        case unsupportedKind(String)
        case invalidValue(String)
        case tooMany
    }

    /// One rule from a line such as `DOMAIN-SUFFIX,example.com,PROXY`. A policy that is not `DIRECT` or a `REJECT`
    /// variant (a Surge or Loon group name such as `Proxy` or `🚀 节点选择`) becomes `PROXY`; a missing policy is
    /// `defaultPolicy`.
    public static func rule(from line: String, defaultPolicy: Policy = .proxy) throws -> Rule {
        let parts = line.split(separator: ",", omittingEmptySubsequences: false).map {
            $0.trimmingCharacters(in: .whitespaces)
        }
        guard let first = parts.first, !first.isEmpty, parts.count >= 2, !parts[1].isEmpty else {
            throw Failure.empty
        }
        let kind = first.uppercased() == "DEST-PORT" ? "DST-PORT" : first.uppercased()
        guard kinds.contains(kind) else {
            throw Failure.unsupportedKind(first)
        }
        let options = ["no-resolve", "pre-matching", "extended-matching", ""]
        var rest = Array(parts.dropFirst(2))
        var policy = defaultPolicy
        if let candidate = rest.first, !options.contains(candidate.lowercased()) {
            policy = Self.policy(named: candidate)
            rest.removeFirst()
        }
        let noResolve = rest.contains { $0.lowercased() == "no-resolve" }
        let value = try Self.value(parts[1], kind: kind)
        let isIP = ["IP-CIDR", "IP-CIDR6", "GEOIP", "IP-ASN"].contains(kind)
        return Rule(kind: kind, value: value, policy: policy, noResolve: noResolve && isIP)
    }

    static func policy(named name: String) -> Policy {
        let upper = name.uppercased()
        if upper == "DIRECT" {
            return .direct
        }
        if upper == "REJECT-IMG" {
            return .rejectTinyGif
        }
        if let policy = Policy(rawValue: upper), policy.isReject {
            return policy
        }
        // Loon's REJECT-200, REJECT-DICT and the like only mean something after decryption.
        if upper.hasPrefix("REJECT") {
            return .reject
        }
        return .proxy
    }

    /// The value as the engine wants it, or `invalidValue`.
    static func value(_ raw: String, kind: String) throws -> String {
        let value = raw.trimmingCharacters(in: .whitespaces)
        let invalid = Failure.invalidValue(raw)
        switch kind {
        case "DOMAIN", "DOMAIN-SUFFIX":
            let domain = value.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "."))
            guard isDomain(domain) else {
                throw invalid
            }
            return domain
        case "DOMAIN-KEYWORD":
            let keyword = value.lowercased()
            guard !keyword.isEmpty, keyword.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || "-._".contains($0)) }) else {
                throw invalid
            }
            return keyword
        case "DOMAIN-WILDCARD":
            let pattern = value.lowercased()
            guard !pattern.isEmpty,
                  pattern.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || "-._*?".contains($0)) })
            else {
                throw invalid
            }
            return pattern
        case "IP-CIDR", "IP-CIDR6":
            guard isNetwork(value) else {
                throw invalid
            }
            return value.lowercased()
        case "GEOIP":
            let code = value.uppercased()
            guard code.count == 2, code.allSatisfy({ $0.isASCII && $0.isLetter }) else {
                throw invalid
            }
            return code
        case "IP-ASN":
            // `AS13335` and `13335` name the same network; the engine's data list is `asn-13335`.
            let number = value.uppercased().hasPrefix("AS") ? String(value.dropFirst(2)) : value
            guard DataSource.normalizedName("asn-" + number) != nil else {
                throw invalid
            }
            return number
        case "GEOSITE":
            let category = value.lowercased()
            guard RuleSetCatalog.known["geosite-" + category] != nil else {
                throw invalid
            }
            return category
        case "DST-PORT", "SRC-PORT":
            let pieces = value.split(separator: "-", omittingEmptySubsequences: false)
            let bounds = pieces.compactMap { UInt16($0) }
            guard let low = bounds.first, let high = bounds.last, bounds.count == pieces.count,
                  (1...2).contains(bounds.count), low <= high
            else {
                throw invalid
            }
            return value
        case "PROTOCOL":
            let name = value.uppercased()
            guard ["TCP", "UDP", "QUIC", "HTTP", "HTTPS", "STUN", "DOH", "DOQ"].contains(name) else {
                throw invalid
            }
            return name
        case "HOSTNAME-TYPE":
            let name = value.uppercased()
            guard ["IPV4", "IPV6", "DOMAIN", "SIMPLE"].contains(name) else {
                throw invalid
            }
            return name
        case "SUBNET":
            return try subnet(value, invalid: invalid)
        case "CELLULAR-RADIO":
            let radio = value.uppercased()
            guard !radio.isEmpty, radio.count <= 24, radio.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber) }) else {
                throw invalid
            }
            return radio
        case "DEVICE-NAME":
            let name = value.trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            guard !name.isEmpty, name.count <= 64, !name.contains(","),
                  !name.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
            else {
                throw invalid
            }
            return name
        case "USER-AGENT":
            let pattern = value.trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            guard !pattern.isEmpty, !pattern.contains(",") else {
                throw invalid
            }
            return pattern
        case "RULE-SET":
            guard let url = URL(string: value), url.scheme?.lowercased() == "https", url.host?.isEmpty == false else {
                throw invalid
            }
            return value
        default:
            throw Failure.unsupportedKind(kind)
        }
    }

    /// A `SUBNET` value: `SSID:<name>`, `BSSID:<mac>`, `ROUTER:<ip>`, `TYPE:WIFI|WIRED|CELLULAR` or a network the
    /// device's address is in. The prefix is upper-cased; a Wi-Fi name keeps its case.
    static func subnet(_ value: String, invalid: Failure) throws -> String {
        if isNetwork(value) {
            return value.lowercased()
        }
        guard let colon = value.firstIndex(of: ":") else {
            throw invalid
        }
        let prefix = value[..<colon].trimmingCharacters(in: .whitespaces).uppercased()
        let rest = String(value[value.index(after: colon)...])
        switch prefix {
        case "SSID":
            guard !rest.isEmpty, rest.utf8.count <= 32 else {
                throw invalid
            }
            return "SSID:" + rest
        case "BSSID":
            let mac = rest.trimmingCharacters(in: .whitespaces).lowercased().replacingOccurrences(of: "-", with: ":")
            let parts = mac.split(separator: ":", omittingEmptySubsequences: false)
            guard parts.count == 6, parts.allSatisfy({ $0.count == 2 && $0.allSatisfy(\.isHexDigit) }) else {
                throw invalid
            }
            return "BSSID:" + mac
        case "ROUTER":
            let address = rest.trimmingCharacters(in: .whitespaces)
            guard !address.contains("/"), isNetwork(address + (address.contains(":") ? "/128" : "/32")) else {
                throw invalid
            }
            return "ROUTER:" + address.lowercased()
        case "TYPE":
            let type = rest.trimmingCharacters(in: .whitespaces).uppercased()
            guard ["WIFI", "WIRED", "CELLULAR"].contains(type) else {
                throw invalid
            }
            return "TYPE:" + type
        case "SUBNET":
            let network = rest.trimmingCharacters(in: .whitespaces)
            guard isNetwork(network) else {
                throw invalid
            }
            return "SUBNET:" + network.lowercased()
        default:
            throw invalid
        }
    }

    static func isDomain(_ text: String) -> Bool {
        guard !text.isEmpty, text.count <= 253 else {
            return false
        }
        return text.split(separator: ".", omittingEmptySubsequences: false).allSatisfy { label in
            !label.isEmpty && label.count <= 63
                && label.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") }
        }
    }

    static func isNetwork(_ text: String) -> Bool {
        let parts = text.split(separator: "/", omittingEmptySubsequences: false)
        guard parts.count == 2, let prefix = Int(parts[1]) else {
            return false
        }
        let address = String(parts[0])
        var v4 = in_addr()
        var v6 = in6_addr()
        if inet_pton(AF_INET, address, &v4) == 1 {
            return (0...32).contains(prefix)
        }
        if inet_pton(AF_INET6, address, &v6) == 1 {
            return (0...128).contains(prefix)
        }
        return false
    }

    /// Adds `rule`, or replaces the rule with its id.
    public func saving(_ rule: Rule) throws -> LocalRules {
        var copy = self
        if let index = copy.rules.firstIndex(where: { $0.id == rule.id }) {
            copy.rules[index] = rule
        } else {
            guard copy.rules.count < Self.maxRules else {
                throw Failure.tooMany
            }
            copy.rules.append(rule)
        }
        return copy
    }

    /// The rules of pasted text: a Surge or Loon `[Rule]` section, a whole profile (only its `[Rule]` section is
    /// read) or one rule per line. `FINAL`, comments, unsupported types and lines already in the list are skipped
    /// and counted.
    public static func parse(_ text: String) -> (rules: [Rule], skipped: Int) {
        var lines = text.components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespaces) }
        let hasSections = lines.contains { $0.hasPrefix("[") && $0.hasSuffix("]") }
        if hasSections {
            var inRules = false
            lines = lines.filter { line in
                if line.hasPrefix("[") && line.hasSuffix("]") {
                    inRules = line.lowercased() == "[rule]"
                    return false
                }
                return inRules
            }
        }
        var rules: [Rule] = []
        var seen = Set<String>()
        var skipped = 0
        for line in lines {
            if line.isEmpty || line.hasPrefix("#") || line.hasPrefix(";") || line.hasPrefix("//") {
                continue
            }
            let upper = line.uppercased()
            if upper.hasPrefix("FINAL") || upper.hasPrefix("MATCH") {
                continue
            }
            guard let rule = try? Self.rule(from: line), seen.insert(rule.text).inserted else {
                skipped += 1
                continue
            }
            rules.append(rule)
        }
        return (rules, skipped)
    }

    /// Adds `subscription`, or replaces the one with its id. The URL must be `https`.
    public func saving(_ subscription: Subscription) throws -> LocalRules {
        var subscription = subscription
        subscription.name = subscription.name.trimmingCharacters(in: .whitespacesAndNewlines)
        subscription.url = try Self.value(subscription.url, kind: "RULE-SET")
        if subscription.name.isEmpty {
            subscription.name = URL(string: subscription.url)?.deletingPathExtension().lastPathComponent ?? subscription.url
        }
        var copy = self
        if let index = copy.subscriptions.firstIndex(where: { $0.id == subscription.id }) {
            copy.subscriptions[index] = subscription
        } else {
            guard copy.subscriptions.count < Self.maxSubscriptions else {
                throw Failure.tooMany
            }
            copy.subscriptions.append(subscription)
        }
        return copy
    }

    /// The most data lists kept; they share the engine's 32 rule sets with the user's lists and the subscription's.
    public static let maxDataSources = 16

    /// `name` (`geoip-us`, `asn-13335`, or what the user typed: `US`, `GEOIP,US`, `AS13335`) pointing at the
    /// `https` list `url`, replacing the list of that name.
    public func saving(dataSource name: String, url: String) throws -> LocalRules {
        guard let name = DataSource.name(from: name) else {
            throw Failure.invalidValue(name)
        }
        let url = try Self.value(url, kind: "RULE-SET")
        var copy = self
        copy.dataSources.removeAll { $0.name == name }
        guard copy.dataSources.count < Self.maxDataSources else {
            throw Failure.tooMany
        }
        copy.dataSources.append(DataSource(name: name, url: url))
        copy.dataSources.sort { $0.name < $1.name }
        return copy
    }

    public func removing(dataSource name: String) -> LocalRules {
        var copy = self
        copy.dataSources.removeAll { $0.name == name }
        return copy
    }

    /// The most device names kept; the engine takes 256.
    public static let maxDevices = 256

    /// The device at `address` called `name`, replacing its earlier name. The address is written as the engine
    /// compares it (an IPv4-mapped address as IPv4); the name is 1 to 64 characters without a comma.
    public func saving(device address: String, name: String) throws -> LocalRules {
        guard let address = Self.deviceAddress(address) else {
            throw Failure.invalidValue(address)
        }
        let name = try Self.value(name, kind: "DEVICE-NAME")
        var copy = self
        copy.devices[address] = nil
        guard copy.devices.count < Self.maxDevices else {
            throw Failure.tooMany
        }
        copy.devices[address] = name
        return copy
    }

    public func removing(device address: String) -> LocalRules {
        var copy = self
        copy.devices[Self.deviceAddress(address) ?? address] = nil
        return copy
    }

    /// `text` as an IP address the engine compares sources with, or nil.
    static func deviceAddress(_ text: String) -> String? {
        let address = text.trimmingCharacters(in: CharacterSet(charactersIn: " []"))
        var v4 = in_addr()
        if inet_pton(AF_INET, address, &v4) == 1 {
            return address
        }
        var v6 = in6_addr()
        guard inet_pton(AF_INET6, address, &v6) == 1 else {
            return nil
        }
        let lower = address.lowercased()
        // `::ffff:192.168.1.30` is the IPv4 device seen through a dual-stack socket.
        if lower.hasPrefix("::ffff:"), inet_pton(AF_INET, String(lower.dropFirst(7)), &v4) == 1 {
            return String(lower.dropFirst(7))
        }
        var buffer = [CChar](repeating: 0, count: Int(INET6_ADDRSTRLEN))
        guard inet_ntop(AF_INET6, &v6, &buffer, socklen_t(buffer.count)) != nil else {
            return nil
        }
        return String(cString: buffer)
    }

    /// The list with `incoming` appended, leaving out rules already in it and anything past `maxRules`.
    public func merging(_ incoming: [Rule]) -> (rules: LocalRules, added: Int) {
        var copy = self
        var known = Set(rules.map(\.text))
        var added = 0
        for rule in incoming where copy.rules.count < Self.maxRules && known.insert(rule.text).inserted {
            copy.rules.append(rule)
            added += 1
        }
        return (copy, added)
    }

    // MARK: - Storage

    public static var defaultURL: URL {
        get throws {
            try FileManager.default
                .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("local-rules.json")
        }
    }

    public static func load(from url: URL) throws -> LocalRules {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return LocalRules()
        }
        return try JSONDecoder().decode(LocalRules.self, from: Data(contentsOf: url))
    }

    public func save(to url: URL) throws {
        try JSONEncoder().encode(self).write(to: url, options: [.atomic])
    }

    /// The tunnel profile value under `TunnelProfileKeys.localRules`.
    public func profileValue() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(self)
    }

    /// The saved rules; profiles saved before local rules existed get none.
    public static func fromProfileValue(_ value: Data?) -> LocalRules {
        value.flatMap { try? JSONDecoder().decode(LocalRules.self, from: $0) } ?? LocalRules()
    }
}

extension LocalRules {
    /// One `GEOIP` or `IP-ASN` data list: the rule-set name its rules use (`geoip-us`, `asn-13335`) and the `https`
    /// list the engine downloads through the proxy once a day (one network per line, or `IP-CIDR` lines).
    public struct DataSource: Codable, Equatable, Identifiable {
        public var name: String
        public var url: String

        public init(name: String, url: String) {
            self.name = name
            self.url = url
        }

        public var id: String {
            name
        }

        /// The rule this list serves, as the user writes it: `GEOIP,US` or `IP-ASN,13335`.
        public var rule: String {
            Self.rule(for: name)
        }

        public static func rule(for name: String) -> String {
            if name.hasPrefix("geoip-") {
                return "GEOIP," + name.dropFirst(6).uppercased()
            }
            if name.hasPrefix("asn-") {
                return "IP-ASN," + name.dropFirst(4)
            }
            return name
        }

        /// `name` as stored when it names data a rule can use: `geoip-<two letters>` or `asn-<1…4294967295>`.
        public static func normalizedName(_ name: String) -> String? {
            let name = name.trimmingCharacters(in: .whitespaces).lowercased()
            if name.hasPrefix("geoip-") {
                let code = name.dropFirst(6)
                return code.count == 2 && code.allSatisfy({ $0.isASCII && $0.isLetter }) ? name : nil
            }
            guard name.hasPrefix("asn-") else {
                return nil
            }
            let number = name.dropFirst(4)
            guard !number.isEmpty, !number.hasPrefix("0"), number.allSatisfy({ $0.isASCII && $0.isNumber }),
                  let value = UInt32(number), value > 0
            else {
                return nil
            }
            return name
        }

        /// The list name for what the user typed: `US`, `GEOIP,US` or `geoip-us`; `13335`, `AS13335`,
        /// `IP-ASN,13335` or `asn-13335`.
        public static func name(from text: String) -> String? {
            var value = text.trimmingCharacters(in: .whitespaces).lowercased()
            for prefix in ["geoip,", "geoip-"] where value.hasPrefix(prefix) {
                value = "geoip-" + value.dropFirst(prefix.count).trimmingCharacters(in: .whitespaces)
            }
            if value.hasPrefix("ip-asn,") {
                value = "asn-" + value.dropFirst(7).trimmingCharacters(in: .whitespaces)
            }
            if value.hasPrefix("asn-as") {
                value = "asn-" + value.dropFirst(6)
            } else if value.hasPrefix("as"), value.dropFirst(2).first?.isNumber == true {
                value = "asn-" + value.dropFirst(2)
            }
            if value.count == 2, value.allSatisfy({ $0.isASCII && $0.isLetter }) {
                value = "geoip-" + value
            } else if let first = value.first, first.isNumber {
                value = "asn-" + value
            }
            return normalizedName(value)
        }
    }
}
