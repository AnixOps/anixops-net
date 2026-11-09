import Foundation

/// The rule mode of a node list without subscription rules (a plain link or base64 subscription): a built-in preset
/// (the same lists the desktop modes use) after the user's own rules. The user's rules also go in front of a Clash,
/// Surge or Loon subscription's rules. Not secret; the app keeps them in `local-rules.json` and copies them into the
/// tunnel profile (`TunnelProfileKeys.localRules`), and the extension builds the engine `routing` with
/// `NodeLibrary.routingForModes(localRules:)`.
///
/// The engine refuses the whole routing when one line is wrong, so every line is checked here (`rule(from:)`) before it
/// is kept.
public struct LocalRules: Codable, Equatable {
    /// The engine accepts more, but a hand-kept list this long is already hard to read on a phone.
    public static let maxRules = 1000

    /// What the traffic no rule of the user's matched does.
    public enum Preset: String, Codable, CaseIterable, Identifiable {
        /// Mainland China sites and addresses direct, everything else through the proxy.
        case bypassChina
        /// Only the GFW list through the proxy, everything else direct.
        case gfwList
        /// Mainland China sites and addresses through the proxy, everything else direct (back to China from abroad).
        case returnChina
        /// No list: only the user's rules, everything else through the proxy.
        case proxyAll

        public var id: String {
            rawValue
        }
    }

    /// Where a rule sends matching traffic. `proxy` is the selected node, or with subscription rules the
    /// subscription's first policy group.
    public enum Policy: String, Codable, CaseIterable, Identifiable {
        case proxy = "PROXY"
        case direct = "DIRECT"
        case reject = "REJECT"

        public var id: String {
            rawValue
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

    public var preset: Preset
    /// Private and local networks (`10.0.0.0/8`, `192.168.0.0/16`, `*.local` and the like) go direct.
    public var bypassLAN: Bool
    public var rules: [Rule]

    public init(preset: Preset = .bypassChina, bypassLAN: Bool = true, rules: [Rule] = []) {
        self.preset = preset
        self.bypassLAN = bypassLAN
        self.rules = rules
    }

    enum CodingKeys: String, CodingKey {
        case preset
        case bypassLAN = "bypass_lan"
        case rules
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let preset = try container.decodeIfPresent(String.self, forKey: .preset).flatMap(Preset.init(rawValue:))
        self.init(
            preset: preset ?? .bypassChina,
            bypassLAN: try container.decodeIfPresent(Bool.self, forKey: .bypassLAN) ?? true,
            rules: try container.decodeIfPresent([Rule].self, forKey: .rules) ?? []
        )
    }

    // MARK: - Engine lines

    /// The rule types a local rule can use. `GEOSITE` is limited to the catalog's lists (`RuleSetCatalog.known`)
    /// because the engine refuses a category it has no list for; `RULE-SET` takes an `https` list URL.
    public static let kinds = [
        "DOMAIN-SUFFIX", "DOMAIN", "DOMAIN-KEYWORD", "DOMAIN-WILDCARD", "IP-CIDR", "IP-CIDR6", "GEOIP", "GEOSITE",
        "DST-PORT", "PROTOCOL", "USER-AGENT", "RULE-SET",
    ]

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

    /// The user's enabled rules as engine lines, `PROXY` replaced by `proxy`, and the rule sets their `RULE-SET` URLs
    /// become, downloaded through `proxy`.
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
        return (lines, sets)
    }

    /// Every engine line for a node list without subscription rules: the user's rules, the private networks, then
    /// the preset ending in `FINAL`.
    public func engineLines(proxy: String) -> (lines: [String], ruleSets: [String: EmbeddedRuntimeConfig.RuleSet]) {
        var (lines, sets) = userLines(proxy: proxy)
        if bypassLAN {
            lines += Self.lanLines
        }
        switch preset {
        case .bypassChina:
            lines += ["GEOSITE,cn,DIRECT", "GEOIP,cn,DIRECT", "FINAL,\(proxy)"]
        case .gfwList:
            lines += ["GEOSITE,gfw,\(proxy)", "FINAL,DIRECT"]
        case .returnChina:
            lines += ["GEOSITE,cn,\(proxy)", "GEOIP,cn,\(proxy)", "FINAL,DIRECT"]
        case .proxyAll:
            lines += ["FINAL,\(proxy)"]
        }
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
        let isIP = ["IP-CIDR", "IP-CIDR6", "GEOIP"].contains(kind)
        return Rule(kind: kind, value: value, policy: policy, noResolve: noResolve && isIP)
    }

    static func policy(named name: String) -> Policy {
        let upper = name.uppercased()
        if upper == "DIRECT" {
            return .direct
        }
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
        case "GEOSITE":
            let category = value.lowercased()
            guard RuleSetCatalog.known["geosite-" + category] != nil else {
                throw invalid
            }
            return category
        case "DST-PORT":
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
            guard ["TCP", "UDP", "QUIC", "HTTP", "HTTPS"].contains(name) else {
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

    /// The saved rules; profiles saved before local rules existed get the default preset.
    public static func fromProfileValue(_ value: Data?) -> LocalRules {
        value.flatMap { try? JSONDecoder().decode(LocalRules.self, from: $0) } ?? LocalRules()
    }
}
