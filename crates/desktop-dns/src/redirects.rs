//! Destination rewrites (the engine's `redirects`), as in the iOS 重定向 page (`TrafficRedirects.swift`): matching
//! TCP connections and UDP flows go to another address, for example a game's server to a development server. The
//! engine applies them before routing, so the rules then see the new address; UDP replies still come from the
//! address the app asked for. They act on what reaches the engine: on Windows the TUN mode and on Android the VPN
//! carry every app; through the system proxy only proxied connections are rewritten.
//!
//! The app keeps them in one file ([`use_file`]); [`apply`] writes the enabled lines into an envelope.

use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The engine accepts at most 256 lines.
pub const MAX_ENTRIES: usize = 256;

/// Which traffic a rewrite takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    #[default]
    Any,
    Tcp,
    Udp,
}

/// One rewrite: the engine line is `match,target`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// The rule part, such as `IP-CIDR,203.0.113.10/32` or `AND,((DOMAIN-SUFFIX,game.example.com),(PROTOCOL,UDP))`.
    #[serde(rename = "match")]
    pub matching: String,
    /// `host`, `host:port`, `[v6]:port` or `:port`.
    pub target: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

impl Entry {
    pub fn line(&self) -> String {
        format!("{},{}", self.matching, self.target)
    }
}

/// The editor's fields: `source` is an address, a network, a domain (which also matches its subdomains) or a whole
/// rule with its type; `ports` is empty, a port or a `start-end` range.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub source: String,
    pub ports: String,
    pub network: Network,
    pub target: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redirects {
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Redirects {
    /// The lines the engine gets: the enabled entries, in order.
    pub fn lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.enabled)
            .map(Entry::line)
            .collect()
    }

    /// Adds an entry from `draft`, or replaces the entry with id `replacing`.
    pub fn adding(&self, draft: &Draft, replacing: Option<&str>) -> Result<Self, String> {
        let matching = matching(&draft.source, &draft.ports, draft.network)?;
        let target = target(&draft.target)?;
        let mut copy = self.clone();
        match replacing.and_then(|id| copy.entries.iter_mut().find(|entry| entry.id == id)) {
            Some(entry) => {
                entry.matching = matching;
                entry.target = target;
            }
            None => {
                if copy.entries.len() >= MAX_ENTRIES {
                    return Err(format!("at most {MAX_ENTRIES} redirects"));
                }
                copy.entries.push(Entry {
                    id: new_id(),
                    matching,
                    target,
                    enabled: true,
                });
            }
        }
        Ok(copy)
    }

    /// The entries as given by the window (switches, order, deletions), checked: each kept entry must be one
    /// already saved or a valid line.
    pub fn checked(self) -> Result<Self, String> {
        if self.entries.len() > MAX_ENTRIES {
            return Err(format!("at most {MAX_ENTRIES} redirects"));
        }
        for entry in &self.entries {
            if entry.matching.trim().is_empty() || entry.matching.contains('\n') {
                return Err(format!("\"{}\" is not a rule", entry.matching));
            }
            target(&entry.target)?;
        }
        Ok(self)
    }
}

/// An entry as the window lists it, with its editor fields when the editor can show them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct View {
    #[serde(flatten)]
    pub entry: Entry,
    pub draft: Option<Draft>,
}

/// The entries as the window lists them.
pub fn views(redirects: &Redirects) -> Vec<View> {
    redirects
        .entries
        .iter()
        .map(|entry| View {
            entry: entry.clone(),
            draft: fields(entry),
        })
        .collect()
}

/// The editor's fields for an entry the editor made; `None` for a rule it cannot show field by field.
pub fn fields(entry: &Entry) -> Option<Draft> {
    let mut conditions = vec![entry.matching.as_str()];
    if let Some(inner) = entry
        .matching
        .strip_prefix("AND,((")
        .and_then(|rest| rest.strip_suffix("))"))
    {
        conditions = inner.split("),(").collect();
    }
    let mut draft = Draft {
        target: entry.target.clone(),
        ..Draft::default()
    };
    for condition in conditions {
        let (kind, value) = condition.split_once(',')?;
        match kind {
            "IP-CIDR" if value.ends_with("/32") => {
                draft.source = value.trim_end_matches("/32").to_string()
            }
            "IP-CIDR6" if value.ends_with("/128") => {
                draft.source = value.trim_end_matches("/128").to_string()
            }
            "IP-CIDR" | "IP-CIDR6" | "DOMAIN-SUFFIX" => draft.source = value.to_string(),
            "DST-PORT" => draft.ports = value.to_string(),
            "PROTOCOL" if value == "TCP" => draft.network = Network::Tcp,
            "PROTOCOL" if value == "UDP" => draft.network = Network::Udp,
            _ => return None,
        }
    }
    Some(draft)
}

/// The rule part for the editor's fields.
pub fn matching(source: &str, ports: &str, network: Network) -> Result<String, String> {
    let mut conditions = Vec::new();
    let source = source.trim();
    if !source.is_empty() {
        conditions.push(condition(source)?);
    }
    let ports: String = ports.chars().filter(|c| *c != ' ').collect();
    if !ports.is_empty() {
        if !is_port_range(&ports) {
            return Err(format!("\"{ports}\" is not a port or port range"));
        }
        conditions.push(format!("DST-PORT,{ports}"));
    }
    match network {
        Network::Any => {}
        Network::Tcp => conditions.push("PROTOCOL,TCP".to_string()),
        Network::Udp => conditions.push("PROTOCOL,UDP".to_string()),
    }
    match conditions.len() {
        0 => Err("give an address, a domain or ports to match".to_string()),
        1 => Ok(conditions.remove(0)),
        _ => Ok(format!(
            "AND,({})",
            conditions
                .iter()
                .map(|condition| format!("({condition})"))
                .collect::<Vec<_>>()
                .join(",")
        )),
    }
}

/// Rule types a typed rule may start with: those the engine matches without data only routing loads.
const RULE_TYPES: [&str; 12] = [
    "DOMAIN",
    "DOMAIN-SUFFIX",
    "DOMAIN-KEYWORD",
    "DOMAIN-WILDCARD",
    "IP-CIDR",
    "IP-CIDR6",
    "DST-PORT",
    "DEST-PORT",
    "PROTOCOL",
    "AND",
    "OR",
    "NOT",
];

fn condition(source: &str) -> Result<String, String> {
    let refused = || format!("\"{source}\" is not an address, a network, a domain or a rule");
    if source.contains(',') {
        // A whole rule typed by hand; the engine checks the rest when it starts.
        let (kind, rest) = source.split_once(',').ok_or_else(refused)?;
        let kind = kind.trim().to_ascii_uppercase();
        if !RULE_TYPES.contains(&kind.as_str()) || source.contains('\n') {
            return Err(refused());
        }
        if matches!(kind.as_str(), "AND" | "OR" | "NOT") {
            let opened = source.matches('(').count();
            if opened != source.matches(')').count() || !source.ends_with(')') {
                return Err(refused());
            }
        } else if rest.contains(',') || rest.trim().is_empty() {
            // `TYPE,value` only: a third field would be read as the target.
            return Err(refused());
        }
        return Ok(source.to_string());
    }
    let (address, prefix) = match source.split_once('/') {
        Some((address, prefix)) => (address, Some(prefix)),
        None => (source, None),
    };
    if let Ok(ip) = address.parse::<IpAddr>() {
        let max = if ip.is_ipv6() { 128 } else { 32 };
        let prefix = match prefix {
            None => max,
            Some(text) => text
                .parse::<u8>()
                .ok()
                .filter(|value| *value <= max)
                .ok_or_else(refused)?,
        };
        let kind = if ip.is_ipv6() { "IP-CIDR6" } else { "IP-CIDR" };
        return Ok(format!("{kind},{ip}/{prefix}"));
    }
    if prefix.is_some() {
        return Err(refused());
    }
    let domain = domain_name(source.strip_prefix('.').unwrap_or(source)).ok_or_else(refused)?;
    Ok(format!("DOMAIN-SUFFIX,{domain}"))
}

/// A normalized target: `host`, `host:port`, `[v6]`, `[v6]:port` or `:port`.
pub fn target(text: &str) -> Result<String, String> {
    let value = text.trim();
    let refused = || format!("\"{value}\" is not an address, a host or a port to send to");
    if let Some(port) = value.strip_prefix(':') {
        return if !port.contains(':') && is_port(port) {
            Ok(value.to_string())
        } else {
            Err(refused())
        };
    }
    if value.parse::<IpAddr>().is_ok() {
        return Ok(value.to_string());
    }
    if let Some(rest) = value.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or_else(refused)?;
        let host_ok = host.contains(':') && host.parse::<IpAddr>().is_ok();
        let after_ok = after.is_empty() || after.strip_prefix(':').is_some_and(is_port);
        return if host_ok && after_ok {
            Ok(value.to_string())
        } else {
            Err(refused())
        };
    }
    let pieces: Vec<&str> = value.split(':').collect();
    if pieces.len() > 2 || (pieces.len() == 2 && !is_port(pieces[1])) {
        return Err(refused());
    }
    if pieces[0].parse::<IpAddr>().is_ok() {
        return Ok(value.to_string());
    }
    let domain = domain_name(pieces[0]).ok_or_else(refused)?;
    Ok(match pieces.get(1) {
        Some(port) => format!("{domain}:{port}"),
        None => domain,
    })
}

/// A domain in lowercase: labels of letters, digits and `-` (`_` allowed, as in service names), not starting or
/// ending with `-`, at most 63 bytes each and 253 in all.
fn domain_name(text: &str) -> Option<String> {
    let name = text.trim().trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name.len() > 253 {
        return None;
    }
    let valid = name.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    });
    valid.then_some(name)
}

fn is_port(text: &str) -> bool {
    text.parse::<u16>().is_ok_and(|port| port > 0)
}

fn is_port_range(text: &str) -> bool {
    match text.split_once('-') {
        None => is_port(text),
        Some((start, end)) => {
            is_port(start) && is_port(end) && start.parse::<u16>().ok() <= end.parse::<u16>().ok()
        }
    }
}

fn new_id() -> String {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos:x}-{count:x}")
}

/// Puts the enabled lines into `envelope` as `redirects`; none removes the key.
pub fn apply_lines(envelope: &str, lines: &[String]) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(envelope).map_err(|error| error.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or("the envelope must be a JSON object")?;
    if lines.is_empty() {
        if object.remove("redirects").is_none() {
            return Ok(envelope.to_string());
        }
    } else {
        object.insert("redirects".to_string(), Value::from(lines.to_vec()));
    }
    Ok(value.to_string())
}

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Where the app keeps the rewrites; set once at start. Until then there are none.
pub fn use_file(path: PathBuf) {
    let _ = FILE.set(path);
}

/// The saved rewrites; none when nothing was saved or the file cannot be read.
pub fn current() -> Redirects {
    FILE.get().map(|path| load(path)).unwrap_or_default()
}

/// [`apply_lines`] with the saved rewrites.
pub fn apply(envelope: &str) -> Result<String, String> {
    apply_lines(envelope, &current().lines())
}

fn load(path: &Path) -> Redirects {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Saves `redirects` once `check` (which builds envelopes with them, or restarts the engine) succeeds; when it
/// fails, the previous file is put back.
pub fn replace<T>(
    redirects: &Redirects,
    check: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let path = FILE.get().ok_or("the redirects have no file yet")?;
    let previous = fs::read(path).ok();
    write(
        path,
        &serde_json::to_vec_pretty(redirects).map_err(|error| error.to_string())?,
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
    let dir = path.parent().ok_or("the redirects file has no folder")?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let temporary = dir.join(".redirects.tmp");
    fs::write(&temporary, bytes).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(source: &str, ports: &str, network: Network, target: &str) -> Draft {
        Draft {
            source: source.to_string(),
            ports: ports.to_string(),
            network,
            target: target.to_string(),
        }
    }

    #[test]
    fn builds_engine_lines_as_on_ios() {
        let redirects = Redirects::default()
            .adding(&draft("203.0.113.10", "", Network::Any, "10.0.0.5"), None)
            .unwrap()
            .adding(
                &draft(
                    ".Game.Example.com",
                    "7000-7100",
                    Network::Udp,
                    "dev.example.com:7001",
                ),
                None,
            )
            .unwrap()
            .adding(
                &draft("2001:db8::/32", "", Network::Tcp, "[::1]:8080"),
                None,
            )
            .unwrap()
            .adding(&draft("", "443", Network::Any, ":8443"), None)
            .unwrap();
        assert_eq!(
            redirects.lines(),
            vec![
                "IP-CIDR,203.0.113.10/32,10.0.0.5",
                "AND,((DOMAIN-SUFFIX,game.example.com),(DST-PORT,7000-7100),(PROTOCOL,UDP)),dev.example.com:7001",
                "AND,((IP-CIDR6,2001:db8::/32),(PROTOCOL,TCP)),[::1]:8080",
                "DST-PORT,443,:8443",
            ]
        );
        assert_eq!(
            fields(&redirects.entries[1]),
            Some(draft(
                "game.example.com",
                "7000-7100",
                Network::Udp,
                "dev.example.com:7001"
            ))
        );
        assert_eq!(
            fields(&redirects.entries[0]).unwrap().source,
            "203.0.113.10"
        );
    }

    #[test]
    fn refuses_bad_fields_and_replaces_in_place() {
        let none = Redirects::default();
        for (source, ports, target) in [
            ("", "", "1.2.3.4"),
            ("bad host!", "", "1.2.3.4"),
            ("1.2.3.4/40", "", "1.2.3.4"),
            ("1.2.3.4", "0", "1.2.3.4"),
            ("1.2.3.4", "90-80", "1.2.3.4"),
            ("1.2.3.4", "", "host:99999"),
            ("1.2.3.4", "", "a:b:c"),
            ("DOMAIN,a.com,extra", "", "1.2.3.4"),
            ("GEOIP,CN", "", "1.2.3.4"),
        ] {
            assert!(
                none.adding(&draft(source, ports, Network::Any, target), None)
                    .is_err(),
                "{source} {ports} {target}"
            );
        }
        let one = none
            .adding(
                &draft("DOMAIN,login.example.com", "", Network::Any, "127.0.0.1"),
                None,
            )
            .unwrap();
        let id = one.entries[0].id.clone();
        let changed = one
            .adding(&draft("10.0.0.0/8", "", Network::Any, ":80"), Some(&id))
            .unwrap();
        assert_eq!(changed.entries.len(), 1);
        assert_eq!(changed.lines(), vec!["IP-CIDR,10.0.0.0/8,:80"]);
        assert_eq!(fields(&one.entries[0]), None);
    }

    #[test]
    fn writes_enabled_lines_into_the_envelope() {
        let mut redirects = Redirects::default()
            .adding(&draft("1.2.3.4", "", Network::Any, "5.6.7.8"), None)
            .unwrap();
        let envelope = r#"{"schema_version":1}"#;
        let with = apply_lines(envelope, &redirects.lines()).unwrap();
        let value: Value = serde_json::from_str(&with).unwrap();
        assert_eq!(value["redirects"][0], "IP-CIDR,1.2.3.4/32,5.6.7.8");
        redirects.entries[0].enabled = false;
        assert_eq!(apply_lines(&with, &redirects.lines()).unwrap(), envelope);
        assert_eq!(apply_lines(envelope, &[]).unwrap(), envelope);
    }

    #[test]
    fn keeps_the_file_only_when_the_check_passes() {
        let dir = std::env::temp_dir().join(format!("redirects-{}", std::process::id()));
        use_file(dir.join("redirects.json"));
        let first = Redirects::default()
            .adding(&draft("1.2.3.4", "", Network::Any, "5.6.7.8"), None)
            .unwrap();
        replace(&first, || Ok(())).unwrap();
        assert_eq!(current(), first);
        let second = Redirects::default();
        assert!(replace(&second, || Err::<(), _>("no".to_string())).is_err());
        assert_eq!(current(), first);
        let _ = fs::remove_dir_all(dir);
    }
}
