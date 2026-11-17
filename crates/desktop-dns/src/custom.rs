//! The "自定义" DNS choice and the Fake-IP switch on Windows, Linux and Android, as on iOS and macOS
//! (`DnsSettings`): the user's own servers, per-domain servers and hosts, written as text, and Fake-IP for the
//! packet tunnel (Windows TUN mode, Android).
//!
//! The text is kept as written, comments included, in one JSON file next to the app's state, and turned into the
//! engine's `dns` section each time an envelope is made. The engine checks the section once more with the rest of
//! the envelope.
//!
//! - Servers, one per line: `name = url[, server-name=host][, direct][, default][, fallback]`, the URL being
//!   `1.1.1.1`, `1.1.1.1:53`, `tcp://host[:53]`, `udp://host[:53]`, `tls://host[:853]`,
//!   `https://host[:443][/dns-query]`, `quic://host[:853]` (DNS over QUIC) or `system` (the device's own
//!   resolvers, always direct). Without `direct` the server is reached through the proxy. The servers marked
//!   `default` (up to 4, asked at once and the first answer wins) answer names no rule picks; without any, the
//!   first server does. The servers marked `fallback` (up to 4) are asked when those give no answer.
//! - Per-domain servers, first match wins: `DOMAIN-SUFFIX,cn,alidns`, `DOMAIN,…`, `DOMAIN-KEYWORD,…` or
//!   `RULE-SET,<name>,alidns` (only while the envelope defines that rule set, such as one of the subscription's).
//! - Hosts: `name = address[, address…]`, `*.name` for its subdomains.

use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The engine's limits (`engine_embedded::dns_policy`).
pub const MAX_SERVERS: usize = 16;
pub const MAX_RULES: usize = 64;
pub const MAX_RULE_DOMAINS: usize = 4096;
pub const MAX_HOSTS: usize = 1024;
pub const MAX_HOST_ADDRESSES: usize = 8;
/// Servers raced together (`default`, `fallback`).
pub const MAX_RACE: usize = 4;
/// The IPv6 network Fake-IP answers `AAAA` from; every client's tunnel routes all of IPv6.
pub const FAKE_IP_RANGE6: &str = "fc00::/18";
/// Longest text of each field.
pub const MAX_TEXT_BYTES: usize = 128 * 1024;

/// The settings as the user wrote them. Not secret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DnsText {
    pub servers: String,
    pub rules: String,
    pub hosts: String,
    pub fake_ip: bool,
}

/// One upstream, as the engine takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub name: String,
    pub url: String,
    pub server_name: Option<String>,
    pub direct: bool,
    /// Among the servers for names no rule picks (raced when several).
    pub default: bool,
    /// Among the servers asked when the chosen ones fail.
    pub fallback: bool,
}

/// Names one server answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rule {
    pub domain: Vec<String>,
    pub domain_suffix: Vec<String>,
    pub domain_keyword: Vec<String>,
    pub rule_set: Option<String>,
    pub server: String,
}

/// The text, checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DnsSettings {
    pub servers: Vec<Server>,
    pub rules: Vec<Rule>,
    pub hosts: BTreeMap<String, Vec<IpAddr>>,
    pub fake_ip: bool,
}

impl DnsText {
    /// Checks every line; the error names the field and the line.
    pub fn parse(&self) -> Result<DnsSettings, String> {
        for (field, text) in [
            ("servers", &self.servers),
            ("per-domain servers", &self.rules),
            ("hosts", &self.hosts),
        ] {
            if text.len() > MAX_TEXT_BYTES {
                return Err(format!("the {field} are longer than 128 KiB"));
            }
        }
        let servers = parse_servers(&self.servers)?;
        let rules = parse_rules(&self.rules, &servers)?;
        let hosts = parse_hosts(&self.hosts)?;
        if self.fake_ip {
            if let Some(name) = hosts
                .iter()
                .find(|(_, addresses)| addresses.iter().any(is_fake_ip_address))
                .map(|(name, _)| name)
            {
                return Err(format!(
                    "hosts line for {name} answers with an address in the Fake-IP range 198.18.0.0/15 or fc00::/18; change it or turn Fake-IP off"
                ));
            }
        }
        Ok(DnsSettings {
            servers,
            rules,
            hosts,
            fake_ip: self.fake_ip,
        })
    }
}

impl DnsSettings {
    /// The engine's `dns` section for the "自定义" choice, or none when nothing is set (the engine's default
    /// resolver). Rules naming a rule set the envelope does not define are left out.
    pub fn section(&self, envelope: &Value) -> Option<Value> {
        let mut section = Map::new();
        if !self.servers.is_empty() {
            section.insert(
                "servers".to_string(),
                Value::Array(self.servers.iter().map(Server::to_json).collect()),
            );
            let marked = |pick: fn(&Server) -> bool| {
                self.servers
                    .iter()
                    .filter(|server| pick(server))
                    .map(|server| server.name.as_str())
                    .collect::<Vec<&str>>()
            };
            let default = marked(|server| server.default);
            section.insert(
                "default".to_string(),
                match default.len() {
                    0 => json!(self.servers[0].name),
                    1 => json!(default[0]),
                    _ => json!(default),
                },
            );
            let fallback = marked(|server| server.fallback);
            match fallback.len() {
                0 => {}
                1 => {
                    section.insert("fallback".to_string(), json!(fallback[0]));
                }
                _ => {
                    section.insert("fallback".to_string(), json!(fallback));
                }
            }
            if self.servers.iter().any(Server::is_system) {
                let system = crate::system::servers();
                if !system.is_empty() {
                    section.insert("system_servers".to_string(), json!(system));
                }
            }
            let rules: Vec<Value> = self
                .rules
                .iter()
                .filter(|rule| {
                    rule.rule_set.as_ref().is_none_or(|name| {
                        envelope
                            .pointer("/routing/rule_sets")
                            .and_then(|sets| sets.get(name))
                            .is_some()
                    })
                })
                .map(Rule::to_json)
                .collect();
            if !rules.is_empty() {
                section.insert("rules".to_string(), Value::Array(rules));
            }
        }
        if !self.hosts.is_empty() {
            let hosts: Map<String, Value> = self
                .hosts
                .iter()
                .map(|(name, addresses)| {
                    (
                        name.clone(),
                        Value::Array(
                            addresses
                                .iter()
                                .map(|address| json!(address.to_string()))
                                .collect(),
                        ),
                    )
                })
                .collect();
            section.insert("hosts".to_string(), Value::Object(hosts));
        }
        if self.fake_ip {
            section.insert("fake_ip".to_string(), fake_ip_section());
        }
        (!section.is_empty()).then_some(Value::Object(section))
    }
}

/// The engine's `fake_ip` section when the switch is on: IPv4 and IPv6 fake answers.
pub fn fake_ip_section() -> Value {
    json!({ "enabled": true, "range6": FAKE_IP_RANGE6 })
}

impl Server {
    /// The device's own resolvers (`system`).
    pub fn is_system(&self) -> bool {
        self.url.eq_ignore_ascii_case("system")
    }

    fn to_json(&self) -> Value {
        let mut server = json!({ "name": self.name, "url": self.url });
        if let Some(name) = &self.server_name {
            server["server_name"] = json!(name);
        }
        if self.direct && !self.is_system() {
            server["via"] = json!("direct");
        }
        server
    }
}

impl Rule {
    fn to_json(&self) -> Value {
        let mut rule = json!({ "server": self.server });
        for (key, list) in [
            ("domain", &self.domain),
            ("domain_suffix", &self.domain_suffix),
            ("domain_keyword", &self.domain_keyword),
        ] {
            if !list.is_empty() {
                rule[key] = json!(list);
            }
        }
        if let Some(name) = &self.rule_set {
            rule["rule_set"] = json!(name);
        }
        rule
    }

    fn entry_count(&self) -> usize {
        self.domain.len() + self.domain_suffix.len() + self.domain_keyword.len()
    }
}

/// Lines that are not blank or comments, with their 1-based numbers.
fn content_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line.trim()))
        .filter(|(_, line)| {
            !(line.is_empty()
                || line.starts_with('#')
                || line.starts_with(';')
                || line.starts_with("//"))
        })
}

fn parse_servers(text: &str) -> Result<Vec<Server>, String> {
    let mut servers: Vec<Server> = Vec::new();
    for (number, line) in content_lines(text) {
        let error = |reason: &str| format!("servers line {number} {reason}");
        let (name, rest) = line
            .split_once('=')
            .ok_or_else(|| error("must look like name = address"))?;
        let name = name.trim();
        if !is_server_name(name) {
            return Err(error(
                "needs a name of up to 32 letters, digits, - or _ before =",
            ));
        }
        if servers.iter().any(|server| server.name == name) {
            return Err(error("repeats a server name"));
        }
        let mut parts = rest.split(',').map(str::trim);
        let url = parts.next().unwrap_or_default();
        if !is_server_url(url) {
            return Err(error(
                "needs an address: 1.1.1.1, tcp://, udp://, tls://, https:// or quic:// followed by the host, or system",
            ));
        }
        let mut server = Server {
            name: name.to_string(),
            url: url.to_string(),
            server_name: None,
            direct: false,
            default: false,
            fallback: false,
        };
        for option in parts {
            let lower = option.to_ascii_lowercase();
            if lower == "default" {
                server.default = true;
            } else if lower == "fallback" {
                server.fallback = true;
            } else if lower == "direct" || lower == "via=direct" {
                server.direct = true;
            } else if lower == "proxy" || lower == "via=proxy" {
                server.direct = false;
            } else if let Some(value) = lower
                .strip_prefix("server-name=")
                .or_else(|| lower.strip_prefix("sni="))
            {
                if !is_domain(value) {
                    return Err(error("has a server-name that is not a domain"));
                }
                server.server_name = Some(value.to_string());
            } else {
                return Err(error(
                    "has an option other than server-name=, direct, proxy, default or fallback",
                ));
            }
        }
        servers.push(server);
        if servers.len() > MAX_SERVERS {
            return Err(error("is past the limit of 16 servers"));
        }
        if servers.iter().filter(|server| server.default).count() > MAX_RACE {
            return Err(error("is past the limit of 4 default servers"));
        }
        if servers.iter().filter(|server| server.fallback).count() > MAX_RACE {
            return Err(error("is past the limit of 4 fallback servers"));
        }
    }
    Ok(servers)
}

fn parse_rules(text: &str, servers: &[Server]) -> Result<Vec<Rule>, String> {
    let mut rules: Vec<Rule> = Vec::new();
    for (number, line) in content_lines(text) {
        let error = |reason: &str| format!("per-domain servers line {number} {reason}");
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let [kind, value, server] = fields[..] else {
            return Err(error("must look like DOMAIN-SUFFIX,example.com,server"));
        };
        if servers.is_empty() {
            return Err(error("needs at least one server above"));
        }
        if !servers.iter().any(|known| known.name == server) {
            return Err(error("names a server that is not in the server list"));
        }
        let value = value.to_ascii_lowercase();
        let kind = kind.to_ascii_uppercase();
        if kind == "RULE-SET" {
            if !is_server_name(&value) {
                return Err(error("needs a rule set name"));
            }
            rules.push(Rule {
                rule_set: Some(value),
                server: server.to_string(),
                ..Rule::default()
            });
        } else {
            let valid =
                match kind.as_str() {
                    "DOMAIN" | "DOMAIN-SUFFIX" => is_domain(&value),
                    "DOMAIN-KEYWORD" => !value.is_empty() && !value.contains(char::is_whitespace),
                    _ => return Err(error(
                        "uses a type other than DOMAIN, DOMAIN-SUFFIX, DOMAIN-KEYWORD or RULE-SET",
                    )),
                };
            if !valid {
                return Err(error("has a value that is not a domain"));
            }
            // Lines in a row for the same server become one rule: the same answers, fewer rules.
            let joins = rules
                .last()
                .is_some_and(|last| last.rule_set.is_none() && last.server == server);
            if !joins {
                rules.push(Rule {
                    server: server.to_string(),
                    ..Rule::default()
                });
            }
            let rule = rules.last_mut().expect("a rule was just pushed");
            match kind.as_str() {
                "DOMAIN" => rule.domain.push(value),
                "DOMAIN-SUFFIX" => rule.domain_suffix.push(value),
                _ => rule.domain_keyword.push(value),
            }
        }
        if rules.len() > MAX_RULES {
            return Err(error(
                "is past the limit of 64 rules (lines for the same server in a row count once)",
            ));
        }
        if rules.iter().map(Rule::entry_count).sum::<usize>() > MAX_RULE_DOMAINS {
            return Err(error("is past the limit of 4096 domains"));
        }
    }
    Ok(rules)
}

fn parse_hosts(text: &str) -> Result<BTreeMap<String, Vec<IpAddr>>, String> {
    let mut hosts = BTreeMap::new();
    for (number, line) in content_lines(text) {
        let error = |reason: &str| format!("hosts line {number} {reason}");
        let (name, addresses) = line
            .split_once('=')
            .ok_or_else(|| error("must look like name = address"))?;
        let name = name.trim().to_ascii_lowercase();
        if !is_domain(name.strip_prefix("*.").unwrap_or(&name)) {
            return Err(error(
                "needs a domain, or *.domain for its subdomains, before =",
            ));
        }
        let addresses = addresses
            .split(',')
            .map(|address| address.trim().parse::<IpAddr>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| error("has an address that is not an IP address"))?;
        if addresses.is_empty() || addresses.len() > MAX_HOST_ADDRESSES {
            return Err(error("needs 1 to 8 addresses"));
        }
        if hosts.insert(name, addresses).is_some() {
            return Err(error("repeats a name"));
        }
        if hosts.len() > MAX_HOSTS {
            return Err(error("is past the limit of 1024 names"));
        }
    }
    Ok(hosts)
}

fn is_server_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_server_url(url: &str) -> bool {
    if url.parse::<IpAddr>().is_ok()
        || url.parse::<SocketAddr>().is_ok()
        || url.eq_ignore_ascii_case("system")
    {
        return true;
    }
    let lower = url.to_ascii_lowercase();
    ["tcp://", "udp://", "tls://", "https://", "quic://"]
        .iter()
        .any(|scheme| {
            lower.strip_prefix(scheme).is_some_and(|rest| {
                // Only DNS over HTTPS has a path.
                let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
                !host.is_empty()
                    && !host.contains(char::is_whitespace)
                    && (path.is_empty() || *scheme == "https://")
            })
        })
}

fn is_domain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// Whether `address` is in the Fake-IP ranges the clients use, 198.18.0.0/15 and fc00::/18.
pub fn is_fake_ip_address(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => v4.octets()[0] == 198 && v4.octets()[1] & 0xFE == 18,
        IpAddr::V6(v6) => v6.segments()[0] == 0xfc00 && v6.segments()[1] & 0xC000 == 0,
    }
}

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Where the app keeps the settings; set once at start. Until then the settings are empty.
pub fn use_file(path: PathBuf) {
    let _ = FILE.set(path);
}

/// The saved text; empty when nothing was saved.
pub fn text() -> Result<DnsText, String> {
    match FILE.get() {
        Some(path) => load(path),
        None => Ok(DnsText::default()),
    }
}

/// The saved settings, checked.
pub fn current() -> Result<DnsSettings, String> {
    text()?.parse()
}

fn load(path: &Path) -> Result<DnsText, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("the DNS settings file is damaged: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DnsText::default()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Saves `text` once it parses and `check` (which builds envelopes with it) succeeds; when `check` fails, the
/// previous settings are put back.
pub fn replace<T>(text: &DnsText, check: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    text.parse()?;
    let path = FILE.get().ok_or("the DNS settings have no file yet")?;
    let previous = fs::read(path).ok();
    write(
        path,
        &serde_json::to_vec_pretty(text).map_err(|error| error.to_string())?,
    )?;
    match check() {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = match previous {
                Some(bytes) => write(path, &bytes),
                None => fs::remove_file(path).map_err(|error| error.to_string()),
            };
            Err(error)
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("the DNS settings file has no folder")?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let temporary = dir.join(".dns-settings.tmp");
    fs::write(&temporary, bytes).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(servers: &str, rules: &str, hosts: &str) -> DnsText {
        DnsText {
            servers: servers.to_string(),
            rules: rules.to_string(),
            hosts: hosts.to_string(),
            fake_ip: false,
        }
    }

    #[test]
    fn servers_rules_and_hosts_become_the_engine_section() {
        let settings = text(
            "# 境外\ncloudflare = https://1.1.1.1/dns-query, server-name=cloudflare-dns.com\nalidns = tls://223.5.5.5, sni=dns.alidns.com, direct",
            "DOMAIN-SUFFIX,cn,alidns\nDOMAIN,example.com,alidns\nDOMAIN-KEYWORD,google,cloudflare\nRULE-SET,china,alidns\nRULE-SET,missing,alidns",
            "router.lan = 192.168.1.1\n*.home.lan = 192.168.1.2, fd00::2",
        )
        .parse()
        .unwrap();
        let envelope = json!({ "routing": { "rule_sets": { "china": {} } } });
        let section = settings.section(&envelope).unwrap();
        assert_eq!(section["default"], "cloudflare");
        assert_eq!(section["servers"][0]["server_name"], "cloudflare-dns.com");
        assert!(section["servers"][0].get("via").is_none());
        assert_eq!(section["servers"][1]["via"], "direct");
        let rules = section["rules"].as_array().unwrap();
        // The two alidns lines in a row are one rule; the rule set the envelope lacks is left out.
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0]["domain_suffix"], json!(["cn"]));
        assert_eq!(rules[0]["domain"], json!(["example.com"]));
        assert_eq!(rules[1]["server"], "cloudflare");
        assert_eq!(rules[2]["rule_set"], "china");
        assert_eq!(
            section["hosts"]["*.home.lan"],
            json!(["192.168.1.2", "fd00::2"])
        );
        assert!(section.get("fake_ip").is_none());
    }

    #[test]
    fn nothing_set_leaves_the_engine_default() {
        assert_eq!(
            DnsText::default().parse().unwrap().section(&json!({})),
            None
        );
        let fake = DnsText {
            fake_ip: true,
            ..DnsText::default()
        };
        assert_eq!(
            fake.parse().unwrap().section(&json!({})),
            Some(json!({ "fake_ip": { "enabled": true, "range6": "fc00::/18" } }))
        );
    }

    #[test]
    fn bad_lines_are_named() {
        let error = |servers: &str, rules: &str, hosts: &str| {
            text(servers, rules, hosts).parse().unwrap_err()
        };
        assert!(error("a", "", "").starts_with("servers line 1"));
        assert!(error("a = ftp://x", "", "").contains("address"));
        assert!(error("a = 1.1.1.1\na = 8.8.8.8", "", "").contains("repeats"));
        assert!(error("a = 1.1.1.1, fast", "", "").contains("option"));
        assert!(error("", "DOMAIN,x.com,a", "").contains("at least one server"));
        assert!(error("a = 1.1.1.1", "DOMAIN,x.com,b", "").contains("not in the server list"));
        assert!(error("a = 1.1.1.1", "GEOIP,cn,a", "").contains("type"));
        assert!(error("", "", "x.lan = 999.1.1.1").starts_with("hosts line 1"));
        assert!(error("", "", "x.lan = 1.1.1.1\nx.lan = 1.1.1.2").contains("repeats"));
    }

    #[test]
    fn fake_ip_refuses_hosts_in_its_range() {
        let mut settings = text("", "", "x.lan = 198.19.0.1");
        assert!(settings.parse().is_ok());
        settings.fake_ip = true;
        assert!(settings.parse().unwrap_err().contains("Fake-IP"));
        assert!(!is_fake_ip_address(&"198.20.0.1".parse().unwrap()));
        assert!(is_fake_ip_address(&"fc00:3fff::1".parse().unwrap()));
        assert!(!is_fake_ip_address(&"fc00:4000::1".parse().unwrap()));
        assert!(!is_fake_ip_address(&"fd00::1".parse().unwrap()));
    }

    #[test]
    fn new_upstreams_race_and_fall_back() {
        let settings = text(
            "cf = quic://1.1.1.1, server-name=cloudflare-dns.com, default
ali = udp://223.5.5.5, direct, default
sys = system, direct, fallback",
            "DOMAIN-SUFFIX,corp.example,sys",
            "",
        )
        .parse()
        .unwrap();
        let section = settings.section(&json!({})).unwrap();
        assert_eq!(section["default"], json!(["cf", "ali"]));
        assert_eq!(section["fallback"], "sys");
        assert_eq!(section["servers"][0]["url"], "quic://1.1.1.1");
        assert_eq!(section["servers"][1]["via"], "direct");
        // The device's resolvers are always direct; no via is written.
        assert!(section["servers"][2].get("via").is_none());
        assert_eq!(section["rules"][0]["server"], "sys");

        let one = text(
            "a = udp://1.1.1.1
b = 8.8.8.8, default",
            "",
            "",
        )
        .parse()
        .unwrap()
        .section(&json!({}))
        .unwrap();
        assert_eq!(one["default"], "b");
        assert!(one.get("fallback").is_none());
        assert!(one.get("system_servers").is_none());
    }

    #[test]
    fn race_limits_and_urls_are_checked() {
        let five: String = (0..5)
            .map(|i| format!("s{i} = 1.1.1.{i}, default\n"))
            .collect();
        assert!(text(&five, "", "")
            .parse()
            .unwrap_err()
            .contains("4 default"));
        let five: String = (0..5)
            .map(|i| format!("s{i} = 1.1.1.{i}, fallback\n"))
            .collect();
        assert!(text(&five, "", "")
            .parse()
            .unwrap_err()
            .contains("4 fallback"));
        for url in [
            "udp://1.1.1.1/x",
            "quic://",
            "systems",
            "quic://dns.example/dns-query",
        ] {
            assert!(
                text(&format!("a = {url}"), "", "").parse().is_err(),
                "{url}"
            );
        }
        for url in [
            "udp://[2606:4700::1111]:53",
            "quic://dns.adguard-dns.com",
            "SYSTEM",
            "https://dns.google/dns-query",
        ] {
            assert!(text(&format!("a = {url}"), "", "").parse().is_ok(), "{url}");
        }
    }

    #[test]
    fn limits_hold() {
        let servers: String = (0..=MAX_SERVERS)
            .map(|i| format!("s{i} = 1.1.1.1\n"))
            .collect();
        assert!(text(&servers, "", "").parse().unwrap_err().contains("16"));
        // Alternating servers keep every line its own rule.
        let rules: String = (0..=MAX_RULES)
            .map(|i| format!("DOMAIN,d{i}.com,{}\n", if i % 2 == 0 { "a" } else { "b" }))
            .collect();
        let error = text("a = 1.1.1.1\nb = 8.8.8.8", &rules, "")
            .parse()
            .unwrap_err();
        assert!(error.contains("64"));
        // The same server in a row stays one rule.
        let joined: String = (0..200).map(|i| format!("DOMAIN,d{i}.com,a\n")).collect();
        let settings = text("a = 1.1.1.1", &joined, "").parse().unwrap();
        assert_eq!(settings.rules.len(), 1);
        assert_eq!(settings.rules[0].domain.len(), 200);
    }

    #[test]
    fn a_failed_check_puts_the_previous_settings_back() {
        let dir = std::env::temp_dir().join(format!("anixops-dns-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        use_file(dir.join("dns-settings.json"));
        let first = text("a = 1.1.1.1", "", "");
        replace(&first, || Ok(())).unwrap();
        assert_eq!(text_now(), first);
        let second = text("b = 8.8.8.8", "", "");
        assert_eq!(
            replace(&second, || Err::<(), _>("no".to_string())).unwrap_err(),
            "no"
        );
        assert_eq!(text_now(), first);
        assert!(replace(&text("bad", "", ""), || Ok(())).is_err());
        assert_eq!(current().unwrap().servers[0].name, "a");
        let _ = fs::remove_dir_all(&dir);
    }

    fn text_now() -> DnsText {
        super::text().unwrap()
    }
}
