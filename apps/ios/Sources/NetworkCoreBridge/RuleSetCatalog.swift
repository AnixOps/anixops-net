import Foundation

/// The public lists behind the mainland China `GEOSITE` and `GEOIP` categories, so a subscription's or the user's
/// `GEOSITE,cn` and `GEOIP,CN` rules match without a rule set of their own. Only these two: no GFW or other list is
/// built in (docs/no-presets.md). The engine downloads them itself, directly, once a day, and only when a rule names
/// them; a set the subscription defines under the same name is used instead.
public enum RuleSetCatalog {
    public static let known: [String: EmbeddedRuntimeConfig.RuleSet] = [
        "geosite-cn": .init(url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/china-list.txt"),
        "geoip-cn": .init(url: "https://raw.githubusercontent.com/gaoyifan/china-operator-ip/ip-lists/china.txt"),
    ]

    /// Sets the engine has built in, so a rule naming one runs without the subscription defining it: Surge's `LAN`
    /// (local names and the private, shared, loopback and link-local networks). A set the subscription defines
    /// under the same name is used instead.
    public static let builtIn: Set<String> = ["LAN"]

    /// The rule-set names `rules` refer to, in order of first use: `RULE-SET,<name>`, `GEOSITE,<category>` as
    /// `geosite-<category>`, `GEOIP,<cc>` as `geoip-<cc>` and `IP-ASN,<n>` as `asn-<n>`, also inside `AND`, `OR` and
    /// `NOT` rules.
    public static func referencedNames(in rules: [String]) -> [String] {
        var names: [String] = []
        for rule in rules {
            for (kind, value) in conditions(in: rule) {
                let name: String
                switch kind {
                case "RULE-SET":
                    name = value
                case "GEOSITE":
                    name = "geosite-" + value.lowercased()
                case "GEOIP":
                    name = "geoip-" + value.lowercased()
                case "IP-ASN":
                    let number = value.uppercased().hasPrefix("AS") ? String(value.dropFirst(2)) : value
                    name = "asn-" + number
                default:
                    continue
                }
                if !names.contains(name) {
                    names.append(name)
                }
            }
        }
        return names
    }

    /// The `rule_sets` the engine runs `rules` with: the subscription's own sets, then the catalog's lists for any
    /// other name the rules use. Nil when there are none.
    public static func ruleSets(
        for rules: [String],
        defined: [String: EmbeddedRuntimeConfig.RuleSet]?
    ) -> [String: EmbeddedRuntimeConfig.RuleSet]? {
        var sets = defined ?? [:]
        for name in referencedNames(in: rules) where sets[name] == nil {
            if let list = known[name] {
                sets[name] = list
            }
        }
        return sets.isEmpty ? nil : sets
    }

    /// The rules the engine accepts with `sets`: it refuses the whole config when a `RULE-SET` or `GEOSITE` rule
    /// names a set it does not have, so those rules are left out (a missing `GEOIP` set only never matches).
    public static func runnableRules(_ rules: [String], sets: [String: EmbeddedRuntimeConfig.RuleSet]?) -> [String] {
        rules.filter { missingSets(in: $0, sets: sets).isEmpty }
    }

    /// The `RULE-SET` and `GEOSITE` names of `rule` that `sets` lacks.
    public static func missingSets(in rule: String, sets: [String: EmbeddedRuntimeConfig.RuleSet]?) -> [String] {
        conditions(in: rule).compactMap { kind, value in
            let name: String
            switch kind {
            case "RULE-SET":
                name = value
            case "GEOSITE":
                name = "geosite-" + value.lowercased()
            default:
                return nil
            }
            return sets?[name] == nil && !builtIn.contains(name) ? name : nil
        }
    }

    /// Whether `rules` hold a `PROCESS-NAME` rule, which matches nothing on iOS and macOS: the tunnel cannot tell
    /// which app opened a connection there (only Windows and Linux can).
    public static func hasProcessRules(_ rules: [String]) -> Bool {
        rules.contains { rule in conditions(in: rule).contains { $0.0 == "PROCESS-NAME" } }
    }

    /// Every `KIND,value` pair of a rule line, including the conditions of a logical rule such as
    /// `AND,((GEOIP,CN),(DST-PORT,443)),DIRECT`.
    static func conditions(in rule: String) -> [(String, String)] {
        let pattern = #"(?:^|[(,])\s*([A-Za-z-]+)\s*,\s*([^,()\s]+)"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else {
            return []
        }
        let range = NSRange(rule.startIndex..., in: rule)
        return regex.matches(in: rule, range: range).compactMap { match in
            guard let kind = Range(match.range(at: 1), in: rule), let value = Range(match.range(at: 2), in: rule) else {
                return nil
            }
            return (rule[kind].uppercased(), String(rule[value]))
        }
    }
}
