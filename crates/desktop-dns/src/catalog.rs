//! The public lists behind the mainland China `GEOSITE` and `GEOIP` categories, so a subscription's or the user's
//! `GEOSITE,cn` and `GEOIP,CN` rules match without a rule set of their own, as on iOS and macOS (`RuleSetCatalog`).
//! Only these two: no other list is built in (docs/no-presets.md). The engine downloads them itself, directly,
//! once a day, and only when a rule names them; a set the profile defines under the same name is used instead.

use serde_json::{json, Map, Value};

/// The rule-set name and list URL of each built-in category.
pub const KNOWN: [(&str, &str); 2] = [
    (
        "geosite-cn",
        "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/china-list.txt",
    ),
    (
        "geoip-cn",
        "https://raw.githubusercontent.com/gaoyifan/china-operator-ip/ip-lists/china.txt",
    ),
];

/// The rule-set names `rule` refers to: `RULE-SET,<name>`, `GEOSITE,<category>` as `geosite-<category>` and
/// `GEOIP,<cc>` as `geoip-<cc>`, also inside `AND`, `OR` and `NOT` rules.
pub fn referenced_names(rule: &str) -> Vec<String> {
    let mut names = Vec::new();
    for condition in rule.split(['(', ')']) {
        let mut parts = condition.trim_matches(',').split(',').map(str::trim);
        let (Some(kind), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let name = match kind.to_ascii_uppercase().as_str() {
            "RULE-SET" => value.to_string(),
            "GEOSITE" => format!("geosite-{}", value.to_ascii_lowercase()),
            "GEOIP" => format!("geoip-{}", value.to_ascii_lowercase()),
            _ => continue,
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Adds the built-in list for every category `rules` name that `sets` lacks.
pub fn fill(rules: &[Value], sets: &mut Map<String, Value>) {
    for rule in rules.iter().filter_map(Value::as_str) {
        for name in referenced_names(rule) {
            if sets.contains_key(&name) {
                continue;
            }
            if let Some((_, url)) = KNOWN.iter().find(|(known, _)| *known == name) {
                sets.insert(name, json!({ "url": url }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_names_also_in_logical_rules() {
        assert_eq!(
            referenced_names("AND,((GEOSITE,CN),(DST-PORT,443)),Proxy"),
            vec!["geosite-cn"]
        );
        assert_eq!(
            referenced_names("GEOIP,CN,DIRECT,no-resolve"),
            vec!["geoip-cn"]
        );
        assert_eq!(referenced_names("RULE-SET,local-1,REJECT"), vec!["local-1"]);
        assert!(referenced_names("DOMAIN,a.test,DIRECT").is_empty());
    }

    #[test]
    fn fills_only_known_missing_sets() {
        let own = json!({ "url": "https://lists.example/cn.txt" });
        let mut sets = Map::new();
        sets.insert("geosite-cn".to_string(), own.clone());
        let rules = json!([
            "GEOSITE,cn,DIRECT",
            "GEOIP,CN,DIRECT",
            "GEOIP,US,proxy",
            "FINAL,proxy"
        ]);
        fill(rules.as_array().unwrap(), &mut sets);
        assert_eq!(sets["geosite-cn"], own);
        assert_eq!(sets["geoip-cn"]["url"], KNOWN[1].1);
        assert!(!sets.contains_key("geoip-us"));
        assert!(!sets.contains_key("geosite-google"));
    }
}
