//! Map Local and Map Remote (debug client plan E3), made from the debugging views instead of hand-written plugin
//! lines. The engine already runs both as plugin lines, so the enabled entries become one generated Loon plugin,
//! "AnixOps 调试映射", that joins the imported plugins: `[Map Local]` for local responses, `[Rewrite] … 307 …` for
//! remote ones (307 keeps the method and body) and `[MITM] hostname` for the hosts they need decrypted. The entries
//! are kept in `debug-mappings.json` next to the plugin library, in the same shape as the Apple clients'
//! `DebugMappings` (docs/debug-mode.md).

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::capture::{decode_base64, encode_base64};

/// Most entries kept.
pub const MAX_ENTRIES: usize = 200;
/// Largest local response body: the engine inlines `[Map Local]` data up to this size.
pub const MAX_BODY_BYTES: usize = 256 * 1024;
/// The generated plugin's name, as the engine's plugin report shows it.
pub const PLUGIN_NAME: &str = "AnixOps 调试映射";
const FILE: &str = "debug-mappings.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Answer from the device (`[Map Local]`).
    Local,
    /// Send the request to another URL (`[Rewrite]` 307).
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Match {
    /// This URL with any query string.
    Url,
    /// Every URL that starts with this one; Map Remote keeps the rest of the path.
    Prefix,
    /// A regular expression over the whole URL, as plugin lines write it.
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Empty for a new entry: [`save`] gives it one.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub kind: Kind,
    #[serde(rename = "match")]
    pub matching: Match,
    /// A URL, a URL prefix or a regular expression, by `matching`.
    pub pattern: String,
    // Map Local.
    #[serde(default = "ok_status")]
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<Header>,
    /// The body as base64, as Swift's `Data` encodes it.
    #[serde(default)]
    pub body: String,
    // Map Remote.
    #[serde(default)]
    pub target: String,
}

fn enabled_by_default() -> bool {
    true
}

fn ok_status() -> u16 {
    200
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    entries: Vec<Entry>,
}

impl Entry {
    /// The regular expression the engine matches the request URL with.
    pub fn regex(&self) -> String {
        match self.matching {
            // The query, if any, is captured so Map Remote can pass it on.
            Match::Url => format!("^{}(\\?.*)?$", escape(self.pattern.trim())),
            Match::Prefix => format!("^{}(.*)$", escape(self.pattern.trim())),
            Match::Regex => self.pattern.trim().to_string(),
        }
    }

    /// The host the entry needs decrypted, when the URL names one.
    pub fn host(&self) -> Option<String> {
        match self.matching {
            Match::Regex => regex_host(self.pattern.trim()),
            _ => host_of(self.pattern.trim()),
        }
    }

    /// The plugin line for the entry.
    pub fn line(&self) -> String {
        match self.kind {
            Kind::Local => {
                let mut fields = vec![
                    quote(&self.regex(), false),
                    "data-type=base64".to_string(),
                    format!("data=\"{}\"", self.body.trim()),
                    format!("status-code={}", self.status),
                ];
                let header = self
                    .headers
                    .iter()
                    .filter(|header| !header.name.trim().is_empty())
                    .map(|header| format!("{}:{}", header.name.trim(), header.value))
                    .collect::<Vec<_>>()
                    .join("|");
                if !header.is_empty() {
                    fields.push(format!("header={}", quote(&header, true)));
                }
                fields.join(" ")
            }
            Kind::Remote => {
                // The captured query (`url`) or rest of the URL (`prefix`) follows the new address.
                let next = match self.matching {
                    Match::Regex => self.target.trim().to_string(),
                    _ => format!("{}$1", self.target.trim()),
                };
                format!(
                    "{} 307 {}",
                    quote(&self.regex(), false),
                    quote(&next, false)
                )
            }
        }
    }

    /// Why the engine could not use the entry, naming the field; fixed text without the values typed.
    pub fn validate(&self) -> Result<(), String> {
        let pattern = self.pattern.trim();
        if pattern.is_empty() || pattern.contains(['\n', '\r']) {
            return Err("Enter the URL to match.".to_string());
        }
        if self.matching != Match::Regex && host_of(pattern).is_none() {
            return Err(
                "The URL to match must be an http or https address with a host.".to_string(),
            );
        }
        match self.kind {
            Kind::Local => {
                if !(100..=599).contains(&self.status) {
                    return Err("The status code must be between 100 and 599.".to_string());
                }
                let body = decode_base64(&self.body)
                    .map_err(|_| "The response body is not valid.".to_string())?;
                if body.len() > MAX_BODY_BYTES {
                    return Err("The response body is larger than 256 KiB.".to_string());
                }
                for header in self
                    .headers
                    .iter()
                    .filter(|header| !header.name.trim().is_empty())
                {
                    let name_ok = header
                        .name
                        .trim()
                        .bytes()
                        .all(|byte| (0x21..=0x7e).contains(&byte) && byte != b':');
                    if !name_ok || header.value.contains(['|', '\n', '\r']) {
                        return Err("A response header has an invalid name or value.".to_string());
                    }
                }
            }
            Kind::Remote => {
                let target = self.target.trim();
                if host_of(target).is_none() || target.contains(char::is_whitespace) {
                    return Err(
                        "The new URL must be an http or https address with a host.".to_string()
                    );
                }
            }
        }
        Ok(())
    }
}

/// `text` with every regular-expression metacharacter escaped.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if "\\^$.|?*+()[]{}".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// The host a regular expression names literally, as in `^https://api\.example\.com/v1/.*`; none when the host
/// part is itself a pattern.
pub fn regex_host(regex: &str) -> Option<String> {
    let text = regex.strip_prefix('^').unwrap_or(regex).replace("\\/", "/");
    for scheme in ["https?://", "http://", "https://"] {
        let Some(rest) = text.strip_prefix(scheme) else {
            continue;
        };
        let mut name = String::new();
        let mut escaping = false;
        for character in rest.chars() {
            if escaping {
                name.push(character);
                escaping = false;
            } else if character == '\\' {
                escaping = true;
            } else if character == '/' || character == ':' || "^$|?*+()[]{}".contains(character) {
                break;
            } else if character == '.' {
                // An unescaped dot matches any character: not one host.
                return None;
            } else {
                name.push(character);
            }
        }
        return host_of(&format!("https://{name}"));
    }
    None
}

/// The host of an `http(s)://host/...` text, lower-cased.
pub fn host_of(text: &str) -> Option<String> {
    let (scheme, rest) = text.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    if host.starts_with('[') {
        return None;
    }
    let name = host.split(':').next().unwrap_or_default();
    let allowed = name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '-');
    (!name.is_empty() && allowed && name.contains('.')).then(|| name.to_lowercase())
}

/// A plugin token: bare when it has no space or quote, else in double quotes with `"` and `\` escaped.
fn quote(token: &str, always: bool) -> String {
    if !always
        && !token.is_empty()
        && !token.starts_with(['"', '\''])
        && !token.contains(char::is_whitespace)
    {
        return token.to_string();
    }
    let mut quoted = String::from("\"");
    for character in token.chars() {
        if character == '"' || character == '\\' {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

/// The generated plugin of the enabled, usable entries; none when no entry is on.
pub fn plugin_text(entries: &[Entry]) -> Option<String> {
    let active: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.enabled && entry.validate().is_ok())
        .collect();
    if active.is_empty() {
        return None;
    }
    let mut lines = vec![
        format!("#!name={PLUGIN_NAME}"),
        "#!desc=由调试视图生成的 Map Local / Map Remote 规则".to_string(),
    ];
    for (kind, section) in [(Kind::Remote, "[Rewrite]"), (Kind::Local, "[Map Local]")] {
        let section_lines: Vec<String> = active
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.line())
            .collect();
        if !section_lines.is_empty() {
            lines.push(String::new());
            lines.push(section.to_string());
            lines.extend(section_lines);
        }
    }
    let mut hosts: Vec<String> = Vec::new();
    for host in active.iter().filter_map(|entry| entry.host()) {
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    if !hosts.is_empty() {
        lines.push(String::new());
        lines.push("[MITM]".to_string());
        lines.push(format!("hostname = %APPEND% {}", hosts.join(", ")));
    }
    Some(lines.join("\n") + "\n")
}

/// The saved entries, oldest first; none when nothing was saved.
pub fn list(directory: &Path) -> Result<Vec<Entry>, String> {
    let path = directory.join(FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read(&path).map_err(|error| error.to_string())?;
    serde_json::from_slice::<Stored>(&text)
        .map(|stored| stored.entries)
        .map_err(|error| format!("debug mappings could not be read: {error}"))
}

fn write(directory: &Path, entries: Vec<Entry>) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let text = serde_json::to_vec_pretty(&Stored { entries }).map_err(|error| error.to_string())?;
    let path = directory.join(FILE);
    let temporary = directory.join(format!("{FILE}.tmp"));
    fs::write(&temporary, text).map_err(|error| error.to_string())?;
    fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

/// A new entry id: time and a counter in hex, unique on this device.
fn new_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("{nanos:x}-{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Adds `entry`, or replaces the entry with its id, and answers the saved entry.
pub fn save(directory: &Path, mut entry: Entry) -> Result<Entry, String> {
    entry.validate()?;
    let mut entries = list(directory)?;
    if entry.id.is_empty() {
        entry.id = new_id();
    }
    match entries.iter_mut().find(|existing| existing.id == entry.id) {
        Some(existing) => *existing = entry.clone(),
        None if entries.len() >= MAX_ENTRIES => {
            return Err(format!("At most {MAX_ENTRIES} mappings can be kept."));
        }
        None => entries.push(entry.clone()),
    }
    write(directory, entries)?;
    Ok(entry)
}

pub fn remove(directory: &Path, id: &str) -> Result<(), String> {
    let mut entries = list(directory)?;
    entries.retain(|entry| entry.id != id);
    write(directory, entries)
}

/// A body as stored in an entry.
pub fn body_from_bytes(bytes: &[u8]) -> String {
    encode_base64(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(pattern: &str) -> Entry {
        Entry {
            id: String::new(),
            name: String::new(),
            enabled: true,
            kind: Kind::Local,
            matching: Match::Url,
            pattern: pattern.to_string(),
            status: 200,
            headers: vec![Header {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            }],
            body: body_from_bytes(b"{\"ok\":true}"),
            target: String::new(),
        }
    }

    #[test]
    fn writes_the_same_lines_as_the_apple_clients() {
        let entry = local("https://api.example.com/v1/user");
        assert_eq!(
            entry.line(),
            "^https://api\\.example\\.com/v1/user(\\?.*)?$ data-type=base64 data=\"eyJvayI6dHJ1ZX0=\" status-code=200 header=\"Content-Type:application/json\""
        );
        let remote = Entry {
            kind: Kind::Remote,
            matching: Match::Prefix,
            target: "https://staging.example.com/v1/".to_string(),
            ..local("https://api.example.com/v1/")
        };
        assert_eq!(
            remote.line(),
            "^https://api\\.example\\.com/v1/(.*)$ 307 https://staging.example.com/v1/$1"
        );
        let text = plugin_text(&[entry, remote]).unwrap();
        assert!(text.starts_with("#!name=AnixOps 调试映射\n"), "{text}");
        assert!(text.contains("\n[Rewrite]\n^https://api"), "{text}");
        assert!(text.contains("\n[Map Local]\n^https://api"), "{text}");
        assert!(
            text.ends_with("[MITM]\nhostname = %APPEND% api.example.com\n"),
            "{text}"
        );
    }

    #[test]
    fn finds_hosts_in_urls_and_literal_regexes() {
        assert_eq!(
            host_of("https://User@API.example.com:8443/x?y"),
            Some("api.example.com".to_string())
        );
        assert_eq!(host_of("ftp://example.com/"), None);
        assert_eq!(host_of("https://localhost/"), None);
        assert_eq!(
            regex_host("^https?:\\/\\/api\\.example\\.com\\/v1"),
            Some("api.example.com".to_string())
        );
        assert_eq!(regex_host("^https://api.example.com/"), None);
        assert_eq!(regex_host("^https://(a|b)\\.example\\.com/"), None);
    }

    #[test]
    fn refuses_entries_the_engine_could_not_use() {
        assert!(local("https://api.example.com/").validate().is_ok());
        assert!(local("api.example.com/x").validate().is_err());
        assert!(Entry {
            status: 99,
            ..local("https://a.example.com/")
        }
        .validate()
        .is_err());
        let bad_header = Entry {
            headers: vec![Header {
                name: "X".to_string(),
                value: "a|b".to_string(),
            }],
            ..local("https://a.example.com/")
        };
        assert!(bad_header.validate().is_err());
        let remote = Entry {
            kind: Kind::Remote,
            target: "not a url".to_string(),
            ..local("https://a.example.com/")
        };
        assert!(remote.validate().is_err());
        assert_eq!(
            plugin_text(&[Entry {
                enabled: false,
                ..local("https://a.example.com/")
            }]),
            None
        );
    }

    #[test]
    fn saves_replaces_and_removes_entries() {
        let dir =
            std::env::temp_dir().join(format!("desktop-mitm-mappings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(list(&dir).unwrap().is_empty());
        let saved = save(&dir, local("https://a.example.com/")).unwrap();
        assert!(!saved.id.is_empty());
        save(
            &dir,
            Entry {
                status: 404,
                ..saved.clone()
            },
        )
        .unwrap();
        let entries = list(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, 404);
        remove(&dir, &saved.id).unwrap();
        assert!(list(&dir).unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_the_apple_json_shape() {
        let json = r#"{"entries":[{"id":"6F9619FF-8B86-D011-B42D-00C04FC964FF","name":"","enabled":true,"kind":"remote","match":"url","pattern":"https://a.example.com/x","status":200,"headers":[],"body":"","target":"https://b.example.com/x"}]}"#;
        let stored: Stored = serde_json::from_str(json).unwrap();
        assert_eq!(stored.entries[0].kind, Kind::Remote);
        assert_eq!(stored.entries[0].matching, Match::Url);
    }
}
