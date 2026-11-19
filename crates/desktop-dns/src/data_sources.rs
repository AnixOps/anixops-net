//! Data lists the user imports by URL for `GEOIP` and `IP-ASN` rules, on Windows, Linux and Android, as on iOS and
//! macOS (`LocalRules.dataSources`). The client builds in only the mainland China lists ([`crate::catalog`],
//! docs/no-presets.md); `GEOIP,US` looks in a rule set `geoip-us` and `IP-ASN,13335` in `asn-13335` that the user
//! names here with a list URL. The engine downloads each list itself, through `PROXY`, once a day, and reports a
//! rule's missing data (`rule_sets` → `missing`) so the rules page can offer to add it.
//!
//! A source the user adds replaces the built-in list and a subscription's own set of the same name.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The most sources kept. The engine runs 32 rule sets in all, which the user's lists and the subscription's share.
pub const MAX_SOURCES: usize = 16;

/// The longest list URL accepted, as for the user's rule lists.
const MAX_URL_LEN: usize = 2048;

/// One data list: the rule-set name its rules use and where the engine downloads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSource {
    /// `geoip-<two letters>` or `asn-<number>`, lower case.
    pub name: String,
    /// An `https://` list: one network per line, or Surge/Clash `IP-CIDR` lines.
    pub url: String,
}

/// `name` as stored when it names data a rule can use: `geoip-<cc>` (two letters) or `asn-<number>` (1 to
/// 4294967295), in any case. `None` for anything else.
pub fn normalized_name(name: &str) -> Option<String> {
    let name = name.trim().to_ascii_lowercase();
    if let Some(country) = name.strip_prefix("geoip-") {
        return (country.len() == 2 && country.bytes().all(|byte| byte.is_ascii_lowercase()))
            .then_some(name);
    }
    let number = name.strip_prefix("asn-")?;
    let valid = !number.is_empty()
        && !number.starts_with('0')
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && number.parse::<u32>().is_ok();
    valid.then_some(name)
}

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Where the app keeps the sources; set once at start. Until then there are none.
pub fn use_file(path: PathBuf) {
    let _ = FILE.set(path);
}

/// The saved sources, by name; none when nothing was saved or the file cannot be read.
pub fn current() -> Vec<DataSource> {
    FILE.get().map(|path| load(path)).unwrap_or_default()
}

fn load(path: &Path) -> Vec<DataSource> {
    let mut sources: Vec<DataSource> = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    sources.retain(|source| normalized_name(&source.name).as_deref() == Some(source.name.as_str()));
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    sources.dedup_by(|a, b| a.name == b.name);
    sources.truncate(MAX_SOURCES);
    sources
}

/// `sources` with `name` pointing at `url`, replacing the source of that name; checked.
pub fn setting(sources: &[DataSource], name: &str, url: &str) -> Result<Vec<DataSource>, String> {
    let name = normalized_name(name)
        .ok_or("the name must be geoip- and a two-letter country code, or asn- and an AS number")?;
    let url = url.trim();
    if !url.starts_with("https://") || url.len() > MAX_URL_LEN || url.contains(char::is_whitespace)
    {
        return Err("the list must be an https:// URL".to_string());
    }
    let mut next: Vec<DataSource> = sources
        .iter()
        .filter(|source| source.name != name)
        .cloned()
        .collect();
    if next.len() >= MAX_SOURCES {
        return Err(format!("at most {MAX_SOURCES} data lists"));
    }
    next.push(DataSource {
        name,
        url: url.to_string(),
    });
    next.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(next)
}

/// `sources` without the one named `name`.
pub fn removing(sources: &[DataSource], name: &str) -> Vec<DataSource> {
    let name = name.trim().to_ascii_lowercase();
    sources
        .iter()
        .filter(|source| source.name != name)
        .cloned()
        .collect()
}

/// Puts every source into `sets` as a remote rule set downloaded through `via` (directly without one), over any set
/// of the same name.
pub fn insert(sources: &[DataSource], sets: &mut Map<String, Value>, via: Option<&str>) {
    for source in sources {
        let mut set = json!({ "url": source.url });
        if let Some(via) = via {
            set["via"] = Value::String(via.to_string());
        }
        sets.insert(source.name.clone(), set);
    }
}

/// Saves `sources` once `check` (which builds envelopes with them, or restarts the engine) succeeds; when it fails,
/// the previous file is put back.
pub fn replace<T>(
    sources: &[DataSource],
    check: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let path = FILE.get().ok_or("the data lists have no file yet")?;
    let previous = fs::read(path).ok();
    write(
        path,
        &serde_json::to_vec_pretty(sources).map_err(|error| error.to_string())?,
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
    let dir = path.parent().ok_or("the data list file has no folder")?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let temporary = dir.join(".data-sources.tmp");
    fs::write(&temporary, bytes).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_geoip_countries_and_as_numbers() {
        assert_eq!(normalized_name("GeoIP-US").as_deref(), Some("geoip-us"));
        assert_eq!(normalized_name("asn-13335").as_deref(), Some("asn-13335"));
        for bad in [
            "geoip-usa",
            "geoip-",
            "geoip-1a",
            "asn-0",
            "asn-013",
            "asn-4294967296",
            "asn-",
            "geosite-cn",
            "local-1",
        ] {
            assert_eq!(normalized_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn setting_replaces_by_name_and_checks_the_url() {
        let one = setting(&[], "GEOIP-US", "https://lists.test/us.txt").unwrap();
        assert_eq!(
            one,
            [DataSource {
                name: "geoip-us".to_string(),
                url: "https://lists.test/us.txt".to_string(),
            }]
        );
        let two = setting(&one, "asn-13335", "https://lists.test/13335.txt").unwrap();
        assert_eq!(two[0].name, "asn-13335");
        let replaced = setting(&two, "geoip-us", "https://lists.test/us-2.txt").unwrap();
        assert_eq!(replaced.len(), 2);
        assert_eq!(replaced[1].url, "https://lists.test/us-2.txt");
        assert!(setting(&two, "geoip-de", "http://lists.test/de.txt").is_err());
        assert!(setting(&two, "geoip-de", "https://lists.test/a b").is_err());
        assert_eq!(removing(&replaced, "GEOIP-US").len(), 1);

        let full: Vec<DataSource> = (1..=MAX_SOURCES as u32)
            .map(|number| DataSource {
                name: format!("asn-{number}"),
                url: "https://lists.test/a".to_string(),
            })
            .collect();
        assert!(setting(&full, "geoip-us", "https://lists.test/us").is_err());
        assert!(setting(&full, "asn-1", "https://lists.test/b").is_ok());
    }

    #[test]
    fn sources_replace_sets_of_the_same_name() {
        let sources = setting(&[], "geoip-cn", "https://lists.test/cn.txt").unwrap();
        let mut sets = Map::new();
        sets.insert(
            "geoip-cn".to_string(),
            json!({ "url": "https://built-in.test/cn" }),
        );
        insert(&sources, &mut sets, Some("Proxy"));
        assert_eq!(
            sets["geoip-cn"],
            json!({ "url": "https://lists.test/cn.txt", "via": "Proxy" })
        );
    }

    #[test]
    fn a_damaged_or_hand_edited_file_keeps_only_valid_sources() {
        let dir = std::env::temp_dir().join(format!("data-sources-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data-sources.json");
        fs::write(
            &path,
            r#"[{"name":"geoip-us","url":"https://a.test/us"},{"name":"GEOIP-DE","url":"https://a.test/de"},{"name":"geoip-us","url":"https://a.test/us-2"}]"#,
        )
        .unwrap();
        let sources = load(&path);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].url, "https://a.test/us");
        fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
