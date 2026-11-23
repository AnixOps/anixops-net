//! HTTPS decryption plugins the user imported from a URL (Loon, Surge and
//! Quantumult X texts). Each plugin is kept with the scripts and data files
//! it names, downloaded once at import; enabled plugins join the `mitm`
//! section of the envelope the desktop engine runs on, next to the built-in
//! policy.
//!
//! Importing is not consent to run anything: a plugin is saved turned off,
//! and the window shows what it decrypts, rewrites and which scripts it runs
//! ([`details`]) before the user turns it on, as iOS asks before a script
//! runs. Importing it again keeps it on only when its text and every script
//! are byte for byte what the user turned on; anything else turns it off for
//! another look. Plugins saved by earlier versions keep their switch.

use engine_embedded::mitm::inspect::{inspect_plugin_json, MAX_PLUGIN_LEN};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Duration;

/// Largest script or data file a plugin may name.
const MAX_RESOURCE_LEN: usize = 4 * 1024 * 1024;
const LIBRARY_FILE: &str = "plugins.json";
const STORE_FILE: &str = "script-store.json";
const KIND_SCRIPT: &str = "script";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Library {
    #[serde(default)]
    plugins: Vec<StoredPlugin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPlugin {
    id: String,
    name: String,
    url: String,
    enabled: bool,
    /// `full`, `partial` or `unsupported`, as the engine graded the text.
    level: String,
    #[serde(default)]
    hostnames: Vec<String>,
    text: String,
    /// Script URL as written in the plugin to its file in the plugin folder.
    #[serde(default)]
    scripts: BTreeMap<String, String>,
    /// Remote `Map Local` bodies, URL to file.
    #[serde(default)]
    resources: BTreeMap<String, String>,
}

/// What the plugin list shows for one plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub level: String,
    pub hostnames: Vec<String>,
    pub script_count: usize,
    /// Set by an import that replaced a plugin which was on: what the engine runs changed even when the new one
    /// is off. Not part of the list the window shows.
    #[serde(skip)]
    pub replaced_one_on: bool,
}

impl StoredPlugin {
    fn summary(&self) -> PluginSummary {
        PluginSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            url: self.url.clone(),
            enabled: self.enabled,
            level: self.level.clone(),
            hostnames: self.hostnames.clone(),
            script_count: self.scripts.len(),
            replaced_one_on: false,
        }
    }
}

pub fn list(directory: &Path) -> Result<Vec<PluginSummary>, String> {
    Ok(read_library(directory)?
        .plugins
        .iter()
        .map(StoredPlugin::summary)
        .collect())
}

/// Downloads the plugin at `url` and every script and data file it names,
/// and saves it turned off for the user to review. Importing the same URL
/// again replaces the plugin (see [`save`] for when it stays on).
pub fn import(
    directory: &Path,
    url: &str,
    fetch: impl Fn(&str, usize) -> Result<Vec<u8>, String>,
) -> Result<PluginSummary, String> {
    let url = url.trim();
    if !is_https(url) {
        return Err("Plugin address must start with https://".to_string());
    }
    let text = fetch(url, MAX_PLUGIN_LEN)?;
    let description: Value = inspect_plugin_json(&text)
        .and_then(|json| serde_json::from_slice(&json).ok())
        .ok_or("This address does not hold a Loon, Surge or Quantumult X plugin.")?;
    let id = format!("{:016x}", hash(url));
    save(directory, id, url, text, &description, fetch)
}

/// Saves a plugin the user opened from a file or pasted, named `source` (the file name, or empty when
/// pasted), with every script and data file it names downloaded as for [`import`], turned off for the user to
/// review. Importing a plugin with the same name again replaces it.
pub fn import_text(
    directory: &Path,
    source: &str,
    text: &str,
    fetch: impl Fn(&str, usize) -> Result<Vec<u8>, String>,
) -> Result<PluginSummary, String> {
    let text = text.trim().as_bytes().to_vec();
    if text.len() > MAX_PLUGIN_LEN {
        return Err(format!(
            "The plugin is larger than {} KB.",
            MAX_PLUGIN_LEN / 1024
        ));
    }
    let description: Value = inspect_plugin_json(&text)
        .and_then(|json| serde_json::from_slice(&json).ok())
        .ok_or("This text is not a Loon, Surge or Quantumult X plugin.")?;
    // Any text parses; a plugin has at least one line the engine runs or a domain to decrypt.
    let lines = |kind: &str| description["lines"][kind].as_u64().unwrap_or_default();
    let decrypts = description["hostnames"]
        .as_array()
        .is_some_and(|hosts| !hosts.is_empty());
    if lines("supported") + lines("partial") == 0 && !decrypts {
        return Err("This text is not a Loon, Surge or Quantumult X plugin.".to_string());
    }
    let key = description["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .map_or_else(
            || format!("text:{}", hash_bytes(&text)),
            |name| format!("name:{name}"),
        );
    let id = format!("{:016x}", hash(&key));
    save(directory, id, source.trim(), text, &description, fetch)
}

/// Downloads the files the plugin names and saves it in place of a plugin with the same id: turned off, unless
/// that plugin was on and its text and scripts are unchanged. A failed import never leaves a plugin on with files the
/// user did not look at: nothing is written until every file has downloaded, and a plugin that was on is saved off
/// before its files are replaced.
fn save(
    directory: &Path,
    id: String,
    url: &str,
    text: Vec<u8>,
    description: &Value,
    fetch: impl Fn(&str, usize) -> Result<Vec<u8>, String>,
) -> Result<PluginSummary, String> {
    let text = String::from_utf8(text).map_err(|_| "Plugin text is not UTF-8.".to_string())?;

    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut library = read_library(directory)?;
    let existing = library
        .plugins
        .iter()
        .find(|existing| existing.id == id)
        .cloned();
    // What the user turned on, read before anything on disk changes.
    let previous = existing
        .as_ref()
        .filter(|existing| existing.enabled)
        .map(|existing| {
            let bodies: BTreeMap<String, Option<Vec<u8>>> = existing
                .scripts
                .iter()
                .map(|(url, file)| (url.clone(), fs::read(directory.join(file)).ok()))
                .collect();
            (existing.text.clone(), bodies)
        });

    // Every file is downloaded before the folder is touched, so a file that fails to download changes nothing.
    let mut downloads: Vec<(String, bool, Vec<u8>)> = Vec::new();
    let listed = description["resources"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for resource in &listed {
        let Some(resource_url) = resource["url"].as_str() else {
            continue;
        };
        if downloads.iter().any(|(known, _, _)| known == resource_url) {
            continue;
        }
        if !is_https(resource_url) {
            return Err(format!(
                "Plugin names a file that is not served over https: {resource_url}"
            ));
        }
        let script = resource["kind"].as_str() == Some(KIND_SCRIPT);
        let body = fetch(resource_url, MAX_RESOURCE_LEN)
            .map_err(|error| format!("{resource_url} could not be downloaded: {error}"))?;
        downloads.push((resource_url.to_string(), script, body));
    }

    let replaced_one_on = previous.is_some();
    let script_count = downloads.iter().filter(|(_, script, _)| *script).count();
    let unchanged = previous.is_some_and(|(previous_text, previous_scripts)| {
        previous_text == text
            && previous_scripts.len() == script_count
            && downloads
                .iter()
                .filter(|(_, script, _)| *script)
                .all(|(url, _, body)| {
                    previous_scripts
                        .get(url)
                        .is_some_and(|previous| previous.as_deref() == Some(body.as_slice()))
                })
    });
    // The new files take the names the plugin that is on points to. Turn it off on disk first, so an error while
    // they are written cannot leave it on with scripts nobody looked at.
    if replaced_one_on && !unchanged {
        if let Some(stored) = library
            .plugins
            .iter_mut()
            .find(|existing| existing.id == id)
        {
            stored.enabled = false;
        }
        write_library(directory, &library)?;
    }

    let mut scripts = BTreeMap::new();
    let mut resources = BTreeMap::new();
    for (resource_url, script, body) in downloads {
        let extension = if script { "js" } else { "dat" };
        let file = format!("{id}-{:016x}.{extension}", hash(&resource_url));
        write_file(directory, &file, &body)?;
        if script {
            scripts.insert(resource_url, file);
        } else {
            resources.insert(resource_url, file);
        }
    }

    let plugin = StoredPlugin {
        id: id.clone(),
        name: description["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(if url.is_empty() { "Plugin" } else { url })
            .to_string(),
        url: url.to_string(),
        enabled: unchanged,
        level: description["level"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        hostnames: description["hostnames"]
            .as_array()
            .map(|hosts| {
                hosts
                    .iter()
                    .filter_map(|host| host.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        text,
        scripts,
        resources,
    };
    let summary = PluginSummary {
        replaced_one_on,
        ..plugin.summary()
    };
    let kept: Vec<String> = plugin
        .scripts
        .values()
        .chain(plugin.resources.values())
        .cloned()
        .collect();
    match library
        .plugins
        .iter_mut()
        .find(|existing| existing.id == id)
    {
        Some(existing) => *existing = plugin,
        None => library.plugins.push(plugin),
    }
    write_library(directory, &library)?;
    // Files the plugin no longer names.
    if let Some(existing) = existing {
        for file in existing.scripts.values().chain(existing.resources.values()) {
            if !kept.contains(file) {
                let _ = fs::remove_file(directory.join(file));
            }
        }
    }
    Ok(summary)
}

/// What a saved plugin holds, for the plugin inspector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetails {
    pub excluded_hostnames: Vec<String>,
    /// Lines the engine runs fully, in part, not at all, and leaves for later.
    pub supported_lines: u64,
    pub partial_lines: u64,
    pub unsupported_lines: u64,
    pub deferred_lines: u64,
    /// Script addresses, in the order the plugin names them.
    pub scripts: Vec<String>,
    /// Other files the plugin names (Map Local bodies).
    pub resources: Vec<String>,
    /// The engine's line-by-line report.
    pub report: String,
}

/// The saved plugin `id`, described again from its text.
pub fn details(directory: &Path, id: &str) -> Result<PluginDetails, String> {
    let library = read_library(directory)?;
    let plugin = library
        .plugins
        .iter()
        .find(|plugin| plugin.id == id)
        .ok_or("This plugin is no longer in the list.")?;
    let description: Value = inspect_plugin_json(plugin.text.as_bytes())
        .and_then(|json| serde_json::from_slice(&json).ok())
        .ok_or("The saved plugin text can no longer be read.")?;
    let strings = |value: &Value| -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let resources = |script: bool| -> Vec<String> {
        let mut urls: Vec<String> = Vec::new();
        for resource in description["resources"].as_array().into_iter().flatten() {
            if (resource["kind"].as_str() == Some(KIND_SCRIPT)) != script {
                continue;
            }
            if let Some(url) = resource["url"].as_str() {
                if !urls.iter().any(|known| known == url) {
                    urls.push(url.to_string());
                }
            }
        }
        urls
    };
    let lines = |kind: &str| description["lines"][kind].as_u64().unwrap_or_default();
    Ok(PluginDetails {
        excluded_hostnames: strings(&description["excluded_hostnames"]),
        supported_lines: lines("supported"),
        partial_lines: lines("partial"),
        unsupported_lines: lines("unsupported"),
        deferred_lines: lines("deferred"),
        scripts: resources(true),
        resources: resources(false),
        report: description["report"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    })
}

pub fn set_enabled(directory: &Path, id: &str, enabled: bool) -> Result<(), String> {
    let mut library = read_library(directory)?;
    let plugin = library
        .plugins
        .iter_mut()
        .find(|plugin| plugin.id == id)
        .ok_or("This plugin is no longer in the list.")?;
    plugin.enabled = enabled;
    write_library(directory, &library)
}

pub fn remove(directory: &Path, id: &str) -> Result<(), String> {
    let mut library = read_library(directory)?;
    let Some(index) = library.plugins.iter().position(|plugin| plugin.id == id) else {
        return Ok(());
    };
    let plugin = library.plugins.remove(index);
    write_library(directory, &library)?;
    for file in plugin.scripts.values().chain(plugin.resources.values()) {
        let _ = fs::remove_file(directory.join(file));
    }
    Ok(())
}

/// Adds the enabled plugins, their scripts and data files to a `mitm`
/// section. A file that is gone from disk is left out, so one missing
/// script cannot keep the engine from starting.
/// The debug mappings (Map Local / Map Remote) made in the capture view
/// join as one more plugin, after the imported ones.
pub fn apply(directory: &Path, mitm: &mut Map<String, Value>) {
    let library = read_library(directory).unwrap_or_default();
    let mut texts = Vec::new();
    let mut scripts = Map::new();
    let mut resources = Map::new();
    for plugin in library.plugins.iter().filter(|plugin| plugin.enabled) {
        texts.push(Value::String(plugin.text.clone()));
        for (assets, files) in [
            (&mut scripts, &plugin.scripts),
            (&mut resources, &plugin.resources),
        ] {
            for (url, file) in files {
                let path = directory.join(file);
                if path.is_file() {
                    assets.insert(url.clone(), json!(path.to_string_lossy()));
                }
            }
        }
    }
    if let Some(text) = crate::mappings::load(directory)
        .ok()
        .and_then(|stored| stored.plugin_text())
    {
        texts.push(Value::String(text));
    }
    if texts.is_empty() {
        return;
    }
    mitm.insert("plugins".to_string(), Value::Array(texts));
    if !scripts.is_empty() {
        mitm.insert("script_assets".to_string(), Value::Object(scripts));
        mitm.insert(
            "script_store_path".to_string(),
            json!(directory.join(STORE_FILE).to_string_lossy()),
        );
    }
    if !resources.is_empty() {
        mitm.insert("resource_assets".to_string(), Value::Object(resources));
    }
}

/// Downloads `url`, refusing a body over `limit` bytes.
pub fn download(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("AnixOpsNet/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())?;
    let body = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::bytes)
        .map_err(|error| error.to_string())?;
    if body.len() > limit {
        return Err(format!(
            "the file is larger than {} MB",
            limit / (1024 * 1024)
        ));
    }
    Ok(body.to_vec())
}

fn is_https(url: &str) -> bool {
    const SCHEME: &str = "https://";
    url.len() > SCHEME.len()
        && url
            .get(..SCHEME.len())
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case(SCHEME))
}

fn hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn hash_bytes(value: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn read_library(directory: &Path) -> Result<Library, String> {
    match fs::read(directory.join(LIBRARY_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "The saved plugin list is damaged.".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Library::default()),
        Err(error) => Err(error.to_string()),
    }
}

/// Writes `name` in `directory` whole or not at all: a failed write never leaves part of a script behind.
fn write_file(directory: &Path, name: &str, contents: &[u8]) -> Result<(), String> {
    let temporary = directory.join(format!("{name}.tmp"));
    fs::write(&temporary, contents).map_err(|error| error.to_string())?;
    fs::rename(&temporary, directory.join(name)).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })
}

fn write_library(directory: &Path, library: &Library) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(library).map_err(|error| error.to_string())?;
    let temporary = directory.join(format!("{LIBRARY_FILE}.tmp"));
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, directory.join(LIBRARY_FILE)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const PLUGIN_URL: &str = "https://plugins.example/ipleak.plugin";
    const SCRIPT_URL: &str = "https://plugins.example/ipleak.js";
    const PLUGIN: &str = concat!(
        "#!name = ipleak test\n",
        "[Script]\n",
        "http-response ^https?:\\/\\/ipleak\\.net\\/ script-path=https://plugins.example/ipleak.js, requires-body=true, tag=ipleak\n",
        "[MITM]\n",
        "hostname = ipleak.net\n",
    );

    fn fetch(url: &str, _limit: usize) -> Result<Vec<u8>, String> {
        match url {
            PLUGIN_URL => Ok(PLUGIN.as_bytes().to_vec()),
            SCRIPT_URL => Ok(b"$done({});".to_vec()),
            _ => Err("not found".to_string()),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "anixops-mitm-plugins-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn pasted_plugins_are_saved_by_name_and_described() {
        let directory = scratch("pasted");
        assert!(import_text(&directory, "", "not a plugin", fetch).is_err());
        let first = import_text(&directory, "ipleak.plugin", PLUGIN, fetch).unwrap();
        assert_eq!(first.name, "ipleak test");
        assert_eq!(first.url, "ipleak.plugin");
        assert_eq!(first.script_count, 1);
        // The same name again replaces it.
        let again = import_text(&directory, "", &format!("{PLUGIN}\n"), fetch).unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(list(&directory).unwrap().len(), 1);

        let details = details(&directory, &first.id).unwrap();
        assert_eq!(details.scripts, [SCRIPT_URL]);
        assert!(details.resources.is_empty());
        assert!(details.supported_lines + details.partial_lines > 0);
        assert!(!details.report.is_empty());
        assert!(super::details(&directory, "missing").is_err());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn imported_plugins_join_the_mitm_section_with_their_scripts() {
        let directory = scratch("import");
        let imported = import(&directory, PLUGIN_URL, fetch).unwrap();
        assert_eq!(imported.name, "ipleak test");
        assert_eq!(imported.hostnames, ["ipleak.net"]);
        assert_eq!(imported.script_count, 1);
        // Saved off: nothing of it runs before the user has looked at it and turned it on.
        assert!(!imported.enabled);
        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        assert!(mitm.is_empty());
        set_enabled(&directory, &imported.id, true).unwrap();
        let summary = list(&directory).unwrap().remove(0);
        assert!(summary.enabled);
        // The same address replaces the plugin instead of adding a second one, and the same content stays on.
        let again = import(&directory, PLUGIN_URL, fetch).unwrap();
        assert!(again.enabled && again.replaced_one_on);
        assert_eq!(again.id, summary.id);
        assert_eq!(list(&directory).unwrap(), std::slice::from_ref(&summary));

        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        assert_eq!(mitm["plugins"], json!([PLUGIN]));
        let script = mitm["script_assets"][SCRIPT_URL].as_str().unwrap();
        assert_eq!(fs::read(script).unwrap(), b"$done({});");
        assert!(mitm.contains_key("script_store_path"));

        // A script that is gone from disk is left out.
        fs::remove_file(script).unwrap();
        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        assert!(mitm.contains_key("plugins"));
        assert!(!mitm.contains_key("script_assets"));

        set_enabled(&directory, &summary.id, false).unwrap();
        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        assert!(mitm.is_empty());

        remove(&directory, &summary.id).unwrap();
        assert!(list(&directory).unwrap().is_empty());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_plugin_whose_script_or_text_changed_is_turned_off_for_another_look() {
        let directory = scratch("changed");
        let id = import(&directory, PLUGIN_URL, fetch).unwrap().id;
        set_enabled(&directory, &id, true).unwrap();
        // The script at the same address now does something else.
        let changed_script = |url: &str, limit: usize| match url {
            SCRIPT_URL => Ok(b"$done({ body: 'other' });".to_vec()),
            _ => fetch(url, limit),
        };
        assert!(
            !import(&directory, PLUGIN_URL, changed_script)
                .unwrap()
                .enabled
        );
        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        assert!(mitm.is_empty());

        // Turned on again, then the plugin decrypts one more domain.
        set_enabled(&directory, &id, true).unwrap();
        let more_hosts = |url: &str, limit: usize| match url {
            PLUGIN_URL => Ok(PLUGIN
                .replace("hostname = ipleak.net", "hostname = ipleak.net, *")
                .into_bytes()),
            _ => changed_script(url, limit),
        };
        assert!(!import(&directory, PLUGIN_URL, more_hosts).unwrap().enabled);

        // Pasted text follows the same rule.
        let pasted = import_text(&directory, "", PLUGIN, fetch).unwrap();
        assert!(!pasted.enabled);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_failed_import_never_leaves_a_plugin_on_with_scripts_nobody_looked_at() {
        const SECOND_URL: &str = "https://plugins.example/second.js";
        let directory = scratch("failed-again");
        let id = import(&directory, PLUGIN_URL, fetch).unwrap().id;
        set_enabled(&directory, &id, true).unwrap();
        let script_file = |mitm: &Map<String, Value>| {
            mitm.get("script_assets")
                .and_then(|assets| assets[SCRIPT_URL].as_str())
                .map(PathBuf::from)
        };
        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        let original = script_file(&mitm).unwrap();

        // The plugin now names a second script; the first one changed and the second does not download.
        let with_second = PLUGIN.replace(
            "[MITM]\n",
            "http-response ^https?:\\/\\/ipleak\\.net\\/second script-path=https://plugins.example/second.js, requires-body=true, tag=second\n[MITM]\n",
        );
        let second_missing = |url: &str, limit: usize| match url {
            PLUGIN_URL => Ok(with_second.clone().into_bytes()),
            SCRIPT_URL => Ok(b"$done({ body: 'other' });".to_vec()),
            _ => fetch(url, limit),
        };
        let error = import(&directory, PLUGIN_URL, second_missing).unwrap_err();
        assert!(error.contains(SECOND_URL), "{error}");
        // Nothing on disk changed: what runs is still what the user turned on.
        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        assert_eq!(mitm["plugins"], json!([PLUGIN]));
        assert_eq!(script_file(&mitm).as_ref(), Some(&original));
        assert_eq!(fs::read(&original).unwrap(), b"$done({});");

        // The second script downloads but cannot be written: the plugin is off before any new file lands.
        let second_changed = |url: &str, limit: usize| match url {
            SECOND_URL => Ok(b"$done({});".to_vec()),
            _ => second_missing(url, limit),
        };
        let blocked = directory.join(format!("{id}-{:016x}.js.tmp", hash(SECOND_URL)));
        fs::create_dir_all(&blocked).unwrap();
        assert!(import(&directory, PLUGIN_URL, second_changed).is_err());
        assert!(!list(&directory).unwrap()[0].enabled);
        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        assert!(mitm.is_empty());

        // Once it can be written, the new content is saved off for review.
        fs::remove_dir_all(&blocked).unwrap();
        let again = import(&directory, PLUGIN_URL, second_changed).unwrap();
        assert!(!again.enabled);
        assert_eq!(again.script_count, 2);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_plugin_that_names_fewer_files_leaves_none_behind() {
        let directory = scratch("fewer");
        let id = import(&directory, PLUGIN_URL, fetch).unwrap().id;
        let script = directory.join(format!("{id}-{:016x}.js", hash(SCRIPT_URL)));
        assert!(script.is_file());
        let without_script = |url: &str, limit: usize| match url {
            PLUGIN_URL => Ok(PLUGIN
                .lines()
                .filter(|line| !line.contains("script-path"))
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes()),
            _ => fetch(url, limit),
        };
        assert_eq!(
            import(&directory, PLUGIN_URL, without_script)
                .unwrap()
                .script_count,
            0
        );
        assert!(!script.exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn plugins_saved_before_the_review_keep_their_switch() {
        let directory = scratch("earlier");
        fs::create_dir_all(&directory).unwrap();
        // A library an earlier version wrote: imported on, with its script.
        fs::write(directory.join("old-script.js"), b"$done({});").unwrap();
        let library = json!({ "plugins": [{
            "id": "0123456789abcdef",
            "name": "ipleak test",
            "url": PLUGIN_URL,
            "enabled": true,
            "level": "full",
            "hostnames": ["ipleak.net"],
            "text": PLUGIN,
            "scripts": { SCRIPT_URL: "old-script.js" },
        }] });
        fs::write(directory.join(LIBRARY_FILE), library.to_string()).unwrap();
        assert!(list(&directory).unwrap()[0].enabled);
        let mut mitm = Map::new();
        apply(&directory, &mut mitm);
        assert_eq!(mitm["plugins"], json!([PLUGIN]));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn imports_need_https_and_a_plugin_whose_files_download() {
        let directory = scratch("refused");
        assert!(import(&directory, "http://plugins.example/a.plugin", fetch).is_err());
        assert!(import(&directory, "https://plugins.example/missing.plugin", fetch).is_err());
        let broken = |url: &str, limit: usize| match url {
            SCRIPT_URL => Err("offline".to_string()),
            _ => fetch(url, limit),
        };
        let error = import(&directory, PLUGIN_URL, broken).unwrap_err();
        assert!(error.contains(SCRIPT_URL), "{error}");
        assert!(list(&directory).unwrap().is_empty());
        let _ = fs::remove_dir_all(&directory);
    }
}
