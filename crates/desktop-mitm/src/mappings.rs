//! Map Local and Map Remote (debug client plan E3), made from the debugging views instead of hand-written plugin
//! lines. The engine already runs both as plugin lines, so the enabled entries become one generated Loon plugin,
//! "AnixOps 调试映射", that joins the imported plugins: `[Map Local]` for local responses, `[Rewrite] … 307 …` for
//! remote ones (307 keeps the method and body) and `[MITM] hostname` for the hosts they need decrypted. The entries
//! are kept in `debug-mappings.json` next to the plugin library, in the same shape as the Apple clients'
//! `DebugMappings` (docs/debug-mode.md).
//!
//! The same file keeps the decryption list ("解密域名", `domains`): HTTPS decryption is per domain, and the list's
//! lines join the plugin's `[MITM] hostname` line even when no mapping is on: domains and wildcard patterns to
//! decrypt, exclusions (`-pattern`, which win over every include) and paused lines. The list starts empty. Files
//! from before the list had exclusions keep only `hosts`, which load as lines that are on; `hosts` is still
//! written for older builds.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::capture::{decode_base64, encode_base64};

/// Most entries kept.
pub const MAX_ENTRIES: usize = 200;
/// Most decrypted domains kept.
pub const MAX_HOSTS: usize = 500;
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

/// Everything in `debug-mappings.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// The decrypted patterns (`decrypted()`), written for older builds; read only from files without `domains`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    /// The decryption list, in the order added. A pattern is listed once, as an include or an exclusion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<Domain>,
}

/// One line of the decryption list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Domain {
    /// Lower-cased, as `normalize_pattern` gives it: `api.example.com`, `*.example.com`, `img-?.example.com`.
    pub pattern: String,
    /// Never decrypted, even when another line, a plugin or a mapping names it (`-pattern` in the plugin).
    #[serde(default)]
    pub exclude: bool,
    /// A paused line stays in the list but does not reach the engine.
    #[serde(default = "enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

impl Domain {
    fn new(pattern: String, exclude: bool) -> Self {
        Self {
            pattern,
            exclude,
            enabled: true,
        }
    }
}

impl Stored {
    /// The patterns decrypted: the list's lines that are on and not exclusions.
    pub fn decrypted(&self) -> Vec<String> {
        self.domains
            .iter()
            .filter(|domain| domain.enabled && !domain.exclude)
            .map(|domain| domain.pattern.clone())
            .collect()
    }

    /// The patterns never decrypted: the list's exclusions that are on.
    pub fn excluded(&self) -> Vec<String> {
        self.domains
            .iter()
            .filter(|domain| domain.enabled && domain.exclude)
            .map(|domain| domain.pattern.clone())
            .collect()
    }

    /// The generated plugin of this file, as `plugin_text` makes it.
    pub fn plugin_text(&self) -> Option<String> {
        plugin_text(&self.entries, &self.decrypted(), &self.excluded())
    }

    /// An older file's `hosts` as list lines that are on.
    fn migrate(mut self) -> Self {
        if self.domains.is_empty() && !self.hosts.is_empty() {
            self.domains = self
                .hosts
                .iter()
                .map(|host| Domain::new(host.clone(), false))
                .collect();
        }
        self
    }
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

/// The generated plugin of the enabled, usable entries, the decrypted patterns and the exclusions (written
/// `-pattern`); none when there is none of them.
pub fn plugin_text(entries: &[Entry], decrypted: &[String], excluded: &[String]) -> Option<String> {
    let active: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.enabled && entry.validate().is_ok())
        .collect();
    if active.is_empty() && decrypted.is_empty() && excluded.is_empty() {
        return None;
    }
    let mut lines = vec![
        format!("#!name={PLUGIN_NAME}"),
        "#!desc=由调试视图生成的 Map Local / Map Remote 规则和解密域名".to_string(),
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
    for host in active
        .iter()
        .filter_map(|entry| entry.host())
        .chain(decrypted.iter().cloned())
    {
        if !hosts.contains(&host) && !excluded.contains(&host) {
            hosts.push(host);
        }
    }
    hosts.extend(excluded.iter().map(|pattern| format!("-{pattern}")));
    if !hosts.is_empty() {
        lines.push(String::new());
        lines.push("[MITM]".to_string());
        lines.push(format!("hostname = %APPEND% {}", hosts.join(", ")));
    }
    Some(lines.join("\n") + "\n")
}

/// A domain as typed or taken from a request ("API.example.com:443", "https://a.test/x", "*.example.com"),
/// lower-cased and without scheme, path or port; none when it is not a domain name.
pub fn normalize_host(text: &str) -> Option<String> {
    let text = text.trim();
    if text.contains("://") {
        return host_of(text).filter(|host| !is_ipv4(host));
    }
    let (wildcard, name) = match text.strip_prefix("*.") {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let name = name.split(['/', '?', '#']).next().unwrap_or_default();
    let name = name
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.');
    let allowed = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if name.is_empty()
        || !allowed
        || !name.contains('.')
        || name.starts_with(['.', '-'])
        || is_ipv4(name)
    {
        return None;
    }
    let name = name.to_ascii_lowercase();
    Some(if wildcard { format!("*.{name}") } else { name })
}

/// A list line as typed: a domain (`normalize_host`), or a wildcard pattern where `*` is any run of characters
/// (dots too) and `?` one character, such as `*.example.com`, `*cdn.example.com` or `img-?.example.*`. Same rules
/// as the engine's `[MITM] hostname` items; a pattern must keep a dot and a literal letter or digit, so a bare `*`
/// is refused. Same rules as the Apple clients' `DebugMappings.normalizePattern`.
pub fn normalize_pattern(text: &str) -> Option<String> {
    let text = text.trim();
    if !text.contains(['*', '?']) {
        return normalize_host(text);
    }
    let mut name = text.split_once("://").map_or(text, |(_, rest)| rest);
    name = name.split(['/', '#']).next().unwrap_or_default();
    if let Some((before, _)) = name
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit()))
    {
        name = before;
    }
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let allowed = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'*' | b'?'));
    if name.len() > 253
        || !allowed
        || !name.contains('.')
        || name.starts_with(['.', '-'])
        || name.contains("..")
        || !name.bytes().any(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(name)
}

/// `text` as a list line: the pattern and whether it is an exclusion (`exclude`, or a leading `-` / `!`).
fn parse_line(text: &str, exclude: bool) -> Result<(String, bool), String> {
    let text = text.trim();
    let (text, exclude) = match text.strip_prefix(['-', '!']) {
        Some(rest) => (rest, true),
        None => (text, exclude),
    };
    normalize_pattern(text)
        .map(|pattern| (pattern, exclude))
        .ok_or_else(|| "Not a domain name or pattern.".to_string())
}

/// Whether `pattern` matches the domain `host`: `*` any run of characters, `?` one character.
pub fn pattern_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = host.trim_end_matches('.').to_ascii_lowercase();
    let text = text.as_bytes();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<usize> = None;
    let mut mark = 0;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if let Some(at) = star {
            p = at + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// Digits and dots only: an IPv4 address, never a domain (top-level domains are never all digits). Decryption is
/// chosen per domain, so an address is refused.
fn is_ipv4(name: &str) -> bool {
    name.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Everything saved; empty when nothing was saved.
pub fn load(directory: &Path) -> Result<Stored, String> {
    let path = directory.join(FILE);
    if !path.exists() {
        return Ok(Stored::default());
    }
    let text = fs::read(&path).map_err(|error| error.to_string())?;
    serde_json::from_slice::<Stored>(&text)
        .map(Stored::migrate)
        .map_err(|error| format!("debug mappings could not be read: {error}"))
}

/// Adds a line to the decryption list (a domain, a wildcard pattern, or an exclusion with `exclude` or a leading
/// `-`) and answers the list. A pattern already listed is turned on and takes the new kind.
pub fn add_host(directory: &Path, text: &str, exclude: bool) -> Result<Vec<Domain>, String> {
    let (pattern, exclude) = parse_line(text, exclude)?;
    let mut stored = load(directory)?;
    if let Some(domain) = stored
        .domains
        .iter_mut()
        .find(|domain| domain.pattern == pattern)
    {
        domain.exclude = exclude;
        domain.enabled = true;
    } else {
        if stored.domains.len() >= MAX_HOSTS {
            return Err(format!("At most {MAX_HOSTS} domains can be listed."));
        }
        stored.domains.push(Domain::new(pattern, exclude));
    }
    write(directory, &stored)?;
    Ok(stored.domains)
}

/// Rewrites the line for `pattern` from `text` and `exclude`, in the same place and on, and answers the list.
pub fn edit_host(
    directory: &Path,
    pattern: &str,
    text: &str,
    exclude: bool,
) -> Result<Vec<Domain>, String> {
    let (next, exclude) = parse_line(text, exclude)?;
    let mut stored = load(directory)?;
    let Some(index) = stored
        .domains
        .iter()
        .position(|domain| domain.pattern == pattern)
    else {
        return add_host(directory, text, exclude);
    };
    if next != pattern && stored.domains.iter().any(|domain| domain.pattern == next) {
        return Err("This domain is already in the list.".to_string());
    }
    stored.domains[index] = Domain::new(next, exclude);
    write(directory, &stored)?;
    Ok(stored.domains)
}

/// Turns the line for `pattern` on or pauses it, and answers the list.
pub fn set_host_enabled(
    directory: &Path,
    pattern: &str,
    enabled: bool,
) -> Result<Vec<Domain>, String> {
    let mut stored = load(directory)?;
    for domain in stored
        .domains
        .iter_mut()
        .filter(|domain| domain.pattern == pattern)
    {
        domain.enabled = enabled;
    }
    write(directory, &stored)?;
    Ok(stored.domains)
}

/// Removes the line for `pattern` and answers the list.
pub fn remove_host(directory: &Path, pattern: &str) -> Result<Vec<Domain>, String> {
    let mut stored = load(directory)?;
    stored.domains.retain(|domain| domain.pattern != pattern);
    write(directory, &stored)?;
    Ok(stored.domains)
}

/// The saved entries, oldest first; none when nothing was saved.
pub fn list(directory: &Path) -> Result<Vec<Entry>, String> {
    load(directory).map(|stored| stored.entries)
}

fn write(directory: &Path, stored: &Stored) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let stored = Stored {
        hosts: stored.decrypted(),
        ..stored.clone()
    };
    let text = serde_json::to_vec_pretty(&stored).map_err(|error| error.to_string())?;
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
    let mut stored = load(directory)?;
    if entry.id.is_empty() {
        entry.id = new_id();
    }
    match stored
        .entries
        .iter()
        .position(|existing| existing.id == entry.id)
    {
        Some(index) => stored.entries[index] = entry.clone(),
        None if stored.entries.len() >= MAX_ENTRIES => {
            return Err(format!("At most {MAX_ENTRIES} mappings can be kept."));
        }
        None => stored.entries.push(entry.clone()),
    }
    write(directory, &stored)?;
    Ok(entry)
}

pub fn remove(directory: &Path, id: &str) -> Result<(), String> {
    let mut stored = load(directory)?;
    stored.entries.retain(|entry| entry.id != id);
    write(directory, &stored)
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
        let text = plugin_text(&[entry, remote], &[], &[]).unwrap();
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
            plugin_text(
                &[Entry {
                    enabled: false,
                    ..local("https://a.example.com/")
                }],
                &[],
                &[]
            ),
            None
        );
    }

    #[test]
    fn decrypts_added_domains_without_any_mapping() {
        let text = plugin_text(
            &[],
            &[
                "api.example.com".to_string(),
                "*.cdn.example.com".to_string(),
            ],
            &[],
        )
        .unwrap();
        assert!(text.starts_with("#!name=AnixOps 调试映射\n"), "{text}");
        assert!(
            !text.contains("[Rewrite]") && !text.contains("[Map Local]"),
            "{text}"
        );
        assert!(
            text.ends_with("[MITM]\nhostname = %APPEND% api.example.com, *.cdn.example.com\n"),
            "{text}"
        );
        // A domain a mapping already needs is listed once.
        let text = plugin_text(
            &[local("https://api.example.com/v1")],
            &["api.example.com".to_string()],
            &[],
        )
        .unwrap();
        assert!(
            text.ends_with("hostname = %APPEND% api.example.com\n"),
            "{text}"
        );
    }

    #[test]
    fn normalizes_typed_domains() {
        assert_eq!(
            normalize_host("API.Example.com:443").as_deref(),
            Some("api.example.com")
        );
        assert_eq!(
            normalize_host(" https://a.test/x?y ").as_deref(),
            Some("a.test")
        );
        assert_eq!(
            normalize_host("*.Example.com").as_deref(),
            Some("*.example.com")
        );
        assert_eq!(
            normalize_host("example.com.").as_deref(),
            Some("example.com")
        );
        assert_eq!(normalize_host("localhost"), None);
        assert_eq!(normalize_host("10.0.0.1:443"), None);
        assert_eq!(normalize_host("https://1.2.3.4/x"), None);
        assert_eq!(normalize_host("bad host.com"), None);
        assert_eq!(normalize_host("-a.com"), None);
        assert_eq!(normalize_host(""), None);
    }

    #[test]
    fn keeps_domains_and_mappings_side_by_side() {
        let dir = std::env::temp_dir().join(format!("desktop-mitm-hosts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let api = Domain::new("api.example.com".to_string(), false);
        assert_eq!(
            add_host(&dir, "API.example.com", false).unwrap(),
            vec![api.clone()]
        );
        assert_eq!(
            add_host(&dir, "api.example.com", false).unwrap(),
            vec![api.clone()]
        );
        assert!(add_host(&dir, "not a domain", false).is_err());
        save(&dir, local("https://b.example.com/")).unwrap();
        let stored = load(&dir).unwrap();
        assert_eq!(stored.domains, vec![api]);
        assert_eq!(stored.hosts, vec!["api.example.com"]);
        assert_eq!(stored.entries.len(), 1);
        assert!(remove_host(&dir, "api.example.com").unwrap().is_empty());
        assert_eq!(list(&dir).unwrap().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_takes_wildcards_exclusions_and_paused_lines() {
        let dir = std::env::temp_dir().join(format!("desktop-mitm-list-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        add_host(&dir, "*.Example.com", false).unwrap();
        add_host(&dir, "img-?.cdn.*", false).unwrap();
        add_host(&dir, "-pay.example.com", false).unwrap();
        add_host(&dir, "ads.example.com", true).unwrap();
        add_host(&dir, "old.example.org", false).unwrap();
        set_host_enabled(&dir, "old.example.org", false).unwrap();
        let stored = load(&dir).unwrap();
        assert_eq!(stored.decrypted(), vec!["*.example.com", "img-?.cdn.*"]);
        assert_eq!(
            stored.excluded(),
            vec!["pay.example.com", "ads.example.com"]
        );
        let text = stored.plugin_text().unwrap();
        assert!(
            text.ends_with(
                "\n[MITM]\nhostname = %APPEND% *.example.com, img-?.cdn.*, -pay.example.com, -ads.example.com\n"
            ),
            "{text}"
        );
        assert!(!text.contains("old.example.org"), "{text}");

        // One line per pattern: adding it again changes its kind.
        let domains = add_host(&dir, "pay.example.com", false).unwrap();
        assert_eq!(
            domains
                .iter()
                .filter(|d| d.pattern == "pay.example.com")
                .count(),
            1
        );
        assert!(!domains[2].exclude);

        let domains = edit_host(&dir, "img-?.cdn.*", "video.cdn.test", false).unwrap();
        assert_eq!(domains[1], Domain::new("video.cdn.test".to_string(), false));
        assert!(edit_host(&dir, "video.cdn.test", "*.example.com", false).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_exclusion_alone_still_makes_a_plugin() {
        let text = plugin_text(&[], &[], &["bank.example.com".to_string()]).unwrap();
        assert!(
            text.ends_with("hostname = %APPEND% -bank.example.com\n"),
            "{text}"
        );
    }

    #[test]
    fn patterns_keep_to_host_names() {
        assert_eq!(
            normalize_pattern("*Spclient.spotify.com").as_deref(),
            Some("*spclient.spotify.com")
        );
        assert_eq!(
            normalize_pattern("https://*.example.com:443/x").as_deref(),
            Some("*.example.com")
        );
        assert_eq!(
            normalize_pattern("api.example.com").as_deref(),
            Some("api.example.com")
        );
        assert_eq!(normalize_pattern("*"), None);
        assert_eq!(normalize_pattern("*.*"), None);
        assert_eq!(normalize_pattern("a b*.com"), None);
        assert_eq!(normalize_pattern("*..example.com"), None);
        assert!(pattern_matches("*.example.com", "a.b.example.com"));
        assert!(!pattern_matches("*.example.com", "example.com"));
        assert!(pattern_matches("img-?.cdn.*", "IMG-1.cdn.test"));
        assert!(!pattern_matches("img-?.cdn.*", "img-12.cdn.test"));
    }

    #[test]
    fn older_files_load_as_lines_that_are_on() {
        let stored: Stored =
            serde_json::from_str(r#"{"entries":[],"hosts":["api.example.com","*.cdn.test"]}"#)
                .unwrap();
        let stored = stored.migrate();
        assert_eq!(
            stored.domains,
            vec![
                Domain::new("api.example.com".to_string(), false),
                Domain::new("*.cdn.test".to_string(), false)
            ]
        );
        // The Apple clients write the same shape.
        let apple: Stored = serde_json::from_str(
            r#"{"entries":[],"domains":[{"pattern":"a.test","exclude":true,"enabled":false}],"hosts":[]}"#,
        )
        .unwrap();
        assert_eq!(
            apple.domains,
            vec![Domain {
                pattern: "a.test".to_string(),
                exclude: true,
                enabled: false
            }]
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
