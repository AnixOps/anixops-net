//! "我的规则" on Windows, Linux and Android: rule lines the user writes, matched before the routing mode's or the
//! subscription's own rules, as on iOS and macOS (`LocalRules`). They are kept as text, one Surge / Loon style line
//! each (`DOMAIN-SUFFIX,example.com,PROXY`), checked here line by line and then by the engine with the rest of the
//! envelope.
//!
//! `PROXY` is the selected node, or with subscription rules the subscription's first policy group. Without any node
//! (debug mode's direct egress) `PROXY` lines go direct.
//!
//! Rule lists the user imports are `RULE-SET,<https URL>,<policy>` lines: the engine downloads each list itself,
//! through `PROXY`, once a day. `GEOSITE,cn` and `GEOIP,CN` use the built-in public lists ([`crate::catalog`]) unless
//! the profile has its own set of that name; other categories need a set of that name in the subscription.

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::catalog;

/// The most lines kept, as on iOS and macOS.
pub const MAX_RULES: usize = 1000;

/// Longest text accepted, so a pasted file cannot grow the envelope without bound.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;

/// The most rule lists the user can import. The engine takes 32 rule sets in all, and the built-in categories and a
/// subscription's own sets need some of them (as on iOS and macOS).
pub const MAX_RULE_SETS: usize = 20;

/// The rule types a line can use.
pub const KINDS: [&str; 17] = [
    "DOMAIN-SUFFIX",
    "DOMAIN",
    "DOMAIN-KEYWORD",
    "DOMAIN-WILDCARD",
    "IP-CIDR",
    "IP-CIDR6",
    "DST-PORT",
    "PROCESS-NAME",
    "SRC-IP",
    "SRC-IP-CIDR",
    "IN-PORT",
    "USER-AGENT",
    "URL-REGEX",
    "PROTOCOL",
    "RULE-SET",
    "GEOSITE",
    "GEOIP",
];

/// Where a line sends traffic. `PROXY` is replaced when the envelope is made. `REJECT-DROP` answers nothing,
/// `REJECT-TINYGIF` answers a plain HTTP request with a transparent 1x1 GIF, and `REJECT-NO-DROP` is a `REJECT` the
/// engine never turns into a drop when an app retries it too often.
pub const POLICIES: [&str; 6] = [
    "PROXY",
    "DIRECT",
    "REJECT",
    "REJECT-NO-DROP",
    "REJECT-DROP",
    "REJECT-TINYGIF",
];

/// Why a line was refused: its 0-based number among all lines of the text, and a fixed reason.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LineError {
    pub line: usize,
    pub code: &'static str,
}

impl LineError {
    pub fn message(&self) -> String {
        let reason = match self.code {
            "empty" => "has no type or value",
            "kind" => "uses a rule type this client does not support",
            "policy" => {
                "must end with PROXY, DIRECT, REJECT, REJECT-NO-DROP, REJECT-DROP or REJECT-TINYGIF"
            }
            "option" => "has an option other than no-resolve",
            "too_many" => "is past the limit of 1000 rules",
            "url" => "must name an https:// rule list",
            "too_many_sets" => "is past the limit of 20 rule lists",
            _ => "is not a rule",
        };
        format!("line {} {reason}", self.line + 1)
    }
}

/// The engine lines of `text`: blank lines and comments (`#`, `;`, `//`) skipped, types upper-cased, `DEST-PORT`
/// spelled `DST-PORT`, a group name as policy refused rather than guessed.
pub fn parse(text: &str) -> Result<Vec<String>, LineError> {
    let mut lines = Vec::new();
    let mut urls: Vec<String> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with(';')
            || line.starts_with("//")
        {
            continue;
        }
        let error = |code| LineError { line: index, code };
        if lines.len() == MAX_RULES {
            return Err(error("too_many"));
        }
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 3 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(error("empty"));
        }
        let mut kind = parts[0].to_ascii_uppercase();
        if kind == "DEST-PORT" {
            kind = "DST-PORT".to_string();
        }
        if !KINDS.contains(&kind.as_str()) {
            return Err(error("kind"));
        }
        // URL-REGEX and USER-AGENT values may hold commas; the policy is the last part that is not an option.
        let mut rest: Vec<&str> = parts[1..].to_vec();
        let mut no_resolve = false;
        while rest
            .last()
            .is_some_and(|part| part.eq_ignore_ascii_case("no-resolve"))
        {
            rest.pop();
            no_resolve = true;
        }
        let Some(mut policy) = rest.pop().map(str::to_ascii_uppercase) else {
            return Err(error("empty"));
        };
        // Loon's name for the GIF answer.
        if policy == "REJECT-IMG" {
            policy = "REJECT-TINYGIF".to_string();
        }
        if !POLICIES.contains(&policy.as_str()) {
            return Err(error("policy"));
        }
        let mut value = rest.join(",");
        if value.is_empty() {
            return Err(error("empty"));
        }
        match kind.as_str() {
            "RULE-SET" => {
                if !value.starts_with("https://") || value.contains(char::is_whitespace) {
                    return Err(error("url"));
                }
                if !urls.contains(&value) {
                    if urls.len() == MAX_RULE_SETS {
                        return Err(error("too_many_sets"));
                    }
                    urls.push(value.clone());
                }
            }
            "GEOSITE" => value = value.to_ascii_lowercase(),
            "GEOIP" => value = value.to_ascii_uppercase(),
            "SRC-IP-CIDR" => kind = "SRC-IP".to_string(),
            _ => {}
        }
        if value.contains(',') && !matches!(kind.as_str(), "URL-REGEX" | "USER-AGENT") {
            return Err(error("option"));
        }
        let mut engine = format!("{kind},{value},{policy}");
        // A source address or listener port never needs a lookup; the engine refuses the option on IN-PORT.
        if no_resolve && !matches!(kind.as_str(), "SRC-IP" | "IN-PORT") {
            engine.push_str(",no-resolve");
        }
        lines.push(engine);
    }
    Ok(lines)
}

/// `envelope` with `lines` (from [`parse`]) in front of its rules. A single-node envelope becomes a routing one so
/// the lines have somewhere to go. Each imported list becomes a rule set `local-<n>` the engine downloads through
/// `PROXY`, and the built-in category lists are added for the rules that name them. A `GEOSITE` line whose category
/// has no list is left out, since the engine refuses the whole profile for it (a missing `GEOIP` list only never
/// matches). No lines leave a single-node envelope as it is.
pub fn apply(envelope: &str, lines: &[String]) -> Result<String, String> {
    if lines.is_empty() {
        return with_category_lists(envelope);
    }
    let mut value: Value = serde_json::from_str(envelope).map_err(|error| error.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or("the envelope must be a JSON object")?;
    if !object.contains_key("routing") {
        let outbound = object
            .remove("outbound")
            .ok_or("the envelope has neither an outbound nor routing")?;
        let direct = outbound.get("type").and_then(Value::as_str) == Some("direct");
        let routing = if direct {
            json!({ "outbounds": { "direct-egress": outbound }, "rules": ["FINAL,DIRECT"] })
        } else {
            json!({ "outbounds": { "proxy": outbound }, "rules": ["FINAL,proxy"] })
        };
        object.insert("routing".to_string(), routing);
    }
    let routing = object
        .get_mut("routing")
        .and_then(Value::as_object_mut)
        .ok_or("the envelope's routing must be a JSON object")?;
    let proxy = proxy_target(routing);
    let had_sets = routing.contains_key("rule_sets");
    let mut sets = match routing.remove("rule_sets") {
        Some(Value::Object(sets)) => sets,
        None | Some(Value::Null) => Map::new(),
        Some(_) => return Err("the envelope's rule sets must be a JSON object".to_string()),
    };
    let mut urls: Vec<String> = Vec::new();
    let mut rules: Vec<Value> = Vec::new();
    for line in lines {
        let mut line = with_target(line, proxy.as_deref());
        // A list URL holds no comma ([`parse`] refuses one), so the rest of the line follows the first comma.
        if let Some((url, tail)) = line
            .strip_prefix("RULE-SET,")
            .filter(|rest| rest.starts_with("https://"))
            .and_then(|rest| rest.split_once(','))
            .map(|(url, tail)| (url.to_string(), tail.to_string()))
        {
            let index = match urls.iter().position(|known| *known == url) {
                Some(index) => index,
                None => {
                    urls.push(url.clone());
                    urls.len() - 1
                }
            };
            let name = format!("local-{}", index + 1);
            let mut set = json!({ "url": url });
            if let Some(proxy) = proxy.as_deref() {
                set["via"] = Value::String(proxy.to_string());
            }
            sets.insert(name.clone(), set);
            line = format!("RULE-SET,{name},{tail}");
        }
        rules.push(Value::String(line));
    }
    let existing = routing
        .get("rules")
        .and_then(Value::as_array)
        .ok_or("the envelope's routing has no rules")?;
    rules.extend(existing.iter().cloned());
    catalog::fill(&rules, &mut sets);
    let user = lines.len();
    let mut index = 0;
    rules.retain(|rule| {
        index += 1;
        index > user
            || !rule.as_str().is_some_and(|rule| {
                rule.starts_with("GEOSITE,")
                    && catalog::referenced_names(rule)
                        .iter()
                        .any(|name| !sets.contains_key(name))
            })
    });
    routing.insert("rules".to_string(), Value::Array(rules));
    if had_sets || !sets.is_empty() {
        routing.insert("rule_sets".to_string(), Value::Object(sets));
    }
    Ok(value.to_string())
}

/// A routing `envelope` with the built-in category lists added for the rules that name them; any other envelope as
/// it is.
fn with_category_lists(envelope: &str) -> Result<String, String> {
    let Ok(mut value) = serde_json::from_str::<Value>(envelope) else {
        return Ok(envelope.to_string());
    };
    let Some(routing) = value.get_mut("routing").and_then(Value::as_object_mut) else {
        return Ok(envelope.to_string());
    };
    let Some(rules) = routing.get("rules").and_then(Value::as_array).cloned() else {
        return Ok(envelope.to_string());
    };
    let mut sets = match routing.get("rule_sets") {
        Some(Value::Object(sets)) => sets.clone(),
        None | Some(Value::Null) => Map::new(),
        Some(_) => return Ok(envelope.to_string()),
    };
    let before = sets.len();
    catalog::fill(&rules, &mut sets);
    if sets.len() == before {
        return Ok(envelope.to_string());
    }
    routing.insert("rule_sets".to_string(), Value::Object(sets));
    Ok(value.to_string())
}

/// What `PROXY` means in `routing`: the first policy group, the single node, or FINAL's target when it is a node.
fn proxy_target(routing: &serde_json::Map<String, Value>) -> Option<String> {
    if let Some(group) = routing
        .get("groups")
        .and_then(Value::as_array)
        .and_then(|groups| groups.first())
        .and_then(|group| group.get("id"))
        .and_then(Value::as_str)
    {
        return Some(group.to_string());
    }
    let outbounds = routing.get("outbounds").and_then(Value::as_object)?;
    if outbounds.contains_key("proxy") {
        return Some("proxy".to_string());
    }
    routing
        .get("rules")
        .and_then(Value::as_array)
        .and_then(|rules| rules.last())
        .and_then(Value::as_str)
        .and_then(|rule| rule.strip_prefix("FINAL,"))
        .map(str::trim)
        .filter(|target| outbounds.contains_key(*target))
        .filter(|target| {
            outbounds
                .get(*target)
                .and_then(|outbound| outbound.get("type"))
                .and_then(Value::as_str)
                != Some("direct")
        })
        .map(str::to_string)
}

/// `line` with its `PROXY` policy replaced by `proxy`, or `DIRECT` when there is none.
fn with_target(line: &str, proxy: Option<&str>) -> String {
    let (head, no_resolve) = match line.strip_suffix(",no-resolve") {
        Some(head) => (head, true),
        None => (line, false),
    };
    let Some((rule, policy)) = head.rsplit_once(',') else {
        return line.to_string();
    };
    let target = if policy == "PROXY" {
        proxy.unwrap_or("DIRECT")
    } else {
        policy
    };
    let mut out = format!("{rule},{target}");
    if no_resolve {
        out.push_str(",no-resolve");
    }
    out
}

/// The saved text at `path`, or none.
pub fn load(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// The engine lines saved at `path`; a damaged file counts as none, so a bad edit outside the app never stops a
/// connection.
pub fn load_lines(path: &Path) -> Vec<String> {
    parse(&load(path)).unwrap_or_default()
}

/// Checks `text` and saves it at `path` in one step; answers the engine lines.
pub fn save(path: &Path, text: &str) -> Result<Vec<String>, String> {
    if text.len() > MAX_TEXT_BYTES {
        return Err("the rules are longer than 256 KiB".to_string());
    }
    let lines = parse(text).map_err(|error| error.message())?;
    let dir = path.parent().ok_or("the rules file has no folder")?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let temporary = dir.join(".my-rules.tmp");
    fs::write(&temporary, text).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(lines)
}

/// The routing rules of `envelope`, for the engine's `reload_rules` while it runs.
pub fn rules_of(envelope: &str) -> Option<Vec<String>> {
    let value: Value = serde_json::from_str(envelope).ok()?;
    value
        .pointer("/routing/rules")?
        .as_array()?
        .iter()
        .map(|rule| rule.as_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_skips_comments() {
        let text = "# mine\n\ndomain-suffix, example.com ,proxy\nIP-CIDR,10.0.0.0/8,DIRECT,no-resolve\n// x\nDEST-PORT,22,REJECT\nURL-REGEX,^https://a\\.test/(x|y),z,REJECT\n";
        assert_eq!(
            parse(text).unwrap(),
            vec![
                "DOMAIN-SUFFIX,example.com,PROXY",
                "IP-CIDR,10.0.0.0/8,DIRECT,no-resolve",
                "DST-PORT,22,REJECT",
                "URL-REGEX,^https://a\\.test/(x|y),z,REJECT",
            ]
        );
    }

    #[test]
    fn reject_variants_and_source_rules_are_kept() {
        let text = "DOMAIN,ads.test,reject-drop\nDOMAIN,img.test,REJECT-IMG\nDOMAIN,keep.test,REJECT-NO-DROP\nDOMAIN,gif.test,REJECT-TINYGIF\nSRC-IP-CIDR,192.168.1.0/24,DIRECT,no-resolve\nSRC-IP,10.0.0.7,PROXY\nIN-PORT,7891,REJECT\n";
        assert_eq!(
            parse(text).unwrap(),
            vec![
                "DOMAIN,ads.test,REJECT-DROP",
                "DOMAIN,img.test,REJECT-TINYGIF",
                "DOMAIN,keep.test,REJECT-NO-DROP",
                "DOMAIN,gif.test,REJECT-TINYGIF",
                "SRC-IP,192.168.1.0/24,DIRECT",
                "SRC-IP,10.0.0.7,PROXY",
                "IN-PORT,7891,REJECT",
            ]
        );
        assert_eq!(
            parse("DOMAIN,a.test,REJECT-200").unwrap_err().code,
            "policy"
        );
    }

    #[test]
    fn refuses_bad_lines_with_their_number() {
        assert_eq!(
            parse("DOMAIN,a.test,PROXY\nSCRIPT,corp,DIRECT").unwrap_err(),
            LineError {
                line: 1,
                code: "kind"
            }
        );
        assert_eq!(
            parse("DOMAIN,a.test,🚀 节点选择").unwrap_err().code,
            "policy"
        );
        assert_eq!(parse("DOMAIN,a.test").unwrap_err().code, "empty");
        assert_eq!(parse("DOMAIN,a,b,PROXY").unwrap_err().code, "option");
        assert_eq!(parse("RULE-SET,ads,PROXY").unwrap_err().code, "url");
        assert_eq!(
            parse("RULE-SET,http://x.test/l.list,PROXY")
                .unwrap_err()
                .code,
            "url"
        );
        let lists: String = (0..=MAX_RULE_SETS)
            .map(|n| format!("RULE-SET,https://x.test/{n}.list,REJECT\n"))
            .collect();
        assert_eq!(parse(&lists).unwrap_err().code, "too_many_sets");
        let many = "DOMAIN,a.test,DIRECT\n".repeat(MAX_RULES + 1);
        assert_eq!(
            parse(&many).unwrap_err(),
            LineError {
                line: MAX_RULES,
                code: "too_many"
            }
        );
        assert_eq!(
            parse("x").unwrap_err().message(),
            "line 1 has no type or value"
        );
    }

    #[test]
    fn a_single_node_becomes_routing_with_the_lines_first() {
        let envelope = r#"{"schema_version":1,"outbound":{"type":"shadowsocks"}}"#;
        let lines = parse("DOMAIN,a.test,PROXY\nDOMAIN,b.test,DIRECT").unwrap();
        let value: Value = serde_json::from_str(&apply(envelope, &lines).unwrap()).unwrap();
        assert_eq!(
            value["routing"]["outbounds"]["proxy"]["type"],
            "shadowsocks"
        );
        assert_eq!(
            value["routing"]["rules"],
            json!(["DOMAIN,a.test,proxy", "DOMAIN,b.test,DIRECT", "FINAL,proxy"])
        );
        assert!(value.get("outbound").is_none());
    }

    #[test]
    fn subscription_rules_follow_and_proxy_is_the_first_group() {
        let envelope = r#"{"routing":{"outbounds":{"n1":{"type":"vless"}},"groups":[{"id":"Proxy","type":"select","members":["n1"]}],"rules":["DOMAIN,x.test,DIRECT","FINAL,Proxy"]}}"#;
        let lines = parse("DOMAIN-SUFFIX,a.test,PROXY,no-resolve").unwrap();
        let value: Value = serde_json::from_str(&apply(envelope, &lines).unwrap()).unwrap();
        assert_eq!(
            value["routing"]["rules"],
            json!([
                "DOMAIN-SUFFIX,a.test,Proxy,no-resolve",
                "DOMAIN,x.test,DIRECT",
                "FINAL,Proxy"
            ])
        );
        assert_eq!(rules_of(&value.to_string()).unwrap().len(), 3);
    }

    #[test]
    fn a_mode_envelope_keeps_its_proxy_and_direct_egress_has_none() {
        let routed = r#"{"routing":{"outbounds":{"proxy":{"type":"trojan"}},"rules":["RULE-SET,own,proxy","FINAL,DIRECT"],"rule_sets":{}}}"#;
        let lines = parse("DOMAIN,a.test,PROXY").unwrap();
        let value: Value = serde_json::from_str(&apply(routed, &lines).unwrap()).unwrap();
        assert_eq!(value["routing"]["rules"][0], "DOMAIN,a.test,proxy");

        let direct = r#"{"outbound":{"type":"direct"}}"#;
        let lines = parse("DOMAIN,a.test,PROXY\nDOMAIN,ads.test,REJECT").unwrap();
        let value: Value = serde_json::from_str(&apply(direct, &lines).unwrap()).unwrap();
        assert_eq!(
            value["routing"]["rules"],
            json!([
                "DOMAIN,a.test,DIRECT",
                "DOMAIN,ads.test,REJECT",
                "FINAL,DIRECT"
            ])
        );
    }

    #[test]
    fn imported_lists_become_rule_sets_downloaded_through_proxy() {
        let envelope = r#"{"routing":{"outbounds":{"n1":{"type":"vless"}},"groups":[{"id":"Proxy","type":"select","members":["n1"]}],"rules":["FINAL,Proxy"]}}"#;
        let text = "RULE-SET,https://lists.test/ads.list,REJECT\nrule-set,https://lists.test/media.list,PROXY\nRULE-SET,https://lists.test/ads.list,REJECT";
        let lines = parse(text).unwrap();
        assert_eq!(lines[0], "RULE-SET,https://lists.test/ads.list,REJECT");
        let value: Value = serde_json::from_str(&apply(envelope, &lines).unwrap()).unwrap();
        assert_eq!(
            value["routing"]["rules"],
            json!([
                "RULE-SET,local-1,REJECT",
                "RULE-SET,local-2,Proxy",
                "RULE-SET,local-1,REJECT",
                "FINAL,Proxy"
            ])
        );
        assert_eq!(
            value["routing"]["rule_sets"],
            json!({
                "local-1": { "url": "https://lists.test/ads.list", "via": "Proxy" },
                "local-2": { "url": "https://lists.test/media.list", "via": "Proxy" }
            })
        );

        // Without a node the lists download directly.
        let direct = r#"{"outbound":{"type":"direct"}}"#;
        let value: Value = serde_json::from_str(&apply(direct, &lines).unwrap()).unwrap();
        assert!(value["routing"]["rule_sets"]["local-1"]
            .get("via")
            .is_none());
    }

    #[test]
    fn only_the_cn_categories_are_built_in() {
        let envelope = r#"{"outbound":{"type":"trojan"}}"#;
        let lines =
            parse("geosite,CN,DIRECT\nGEOIP,cn,DIRECT,no-resolve\nGEOSITE,google,PROXY").unwrap();
        let value: Value = serde_json::from_str(&apply(envelope, &lines).unwrap()).unwrap();
        assert_eq!(
            value["routing"]["rules"],
            json!([
                "GEOSITE,cn,DIRECT",
                "GEOIP,CN,DIRECT,no-resolve",
                "FINAL,proxy"
            ])
        );
        let sets = value["routing"]["rule_sets"].as_object().unwrap();
        assert_eq!(sets.len(), 2);
        assert!(sets.contains_key("geosite-cn") && sets.contains_key("geoip-cn"));

        // A subscription's own set of that name wins, and its rules get the built-in lists too.
        let own = r#"{"routing":{"outbounds":{"proxy":{"type":"trojan"}},"rules":["GEOIP,CN,DIRECT","GEOSITE,google,proxy","FINAL,proxy"],"rule_sets":{"geosite-google":{"url":"https://lists.test/google.txt"}}}}"#;
        let value: Value = serde_json::from_str(&apply(own, &[]).unwrap()).unwrap();
        assert_eq!(
            value["routing"]["rule_sets"]["geosite-google"]["url"],
            "https://lists.test/google.txt"
        );
        assert!(value["routing"]["rule_sets"]["geoip-cn"]["url"].is_string());
        let lines = parse("GEOSITE,google,PROXY").unwrap();
        let value: Value = serde_json::from_str(&apply(own, &lines).unwrap()).unwrap();
        assert_eq!(value["routing"]["rules"][0], "GEOSITE,google,proxy");
    }

    #[test]
    fn no_lines_change_nothing_and_saves_round_trip() {
        let envelope = r#"{"outbound":{"type":"direct"}}"#;
        assert_eq!(apply(envelope, &[]).unwrap(), envelope);
        let dir = std::env::temp_dir().join(format!("anixops-my-rules-{}", std::process::id()));
        let path = dir.join("my-rules.txt");
        assert!(save(&path, "DOMAIN,a.test,FOO").is_err());
        assert!(!path.exists());
        assert_eq!(
            save(&path, "# x\nDOMAIN,a.test,REJECT\n").unwrap(),
            vec!["DOMAIN,a.test,REJECT"]
        );
        assert_eq!(load_lines(&path), vec!["DOMAIN,a.test,REJECT"]);
        assert_eq!(load(&path), "# x\nDOMAIN,a.test,REJECT\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
