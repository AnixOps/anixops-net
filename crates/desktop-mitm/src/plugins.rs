//! HTTPS decryption plugins the user imported from a URL (Loon, Surge and
//! Quantumult X texts). Each plugin is kept with the scripts and data files
//! it names, downloaded once at import; enabled plugins join the `mitm`
//! section of the envelope the desktop engine runs on, next to the built-in
//! policy. Importing a plugin is the user's consent to run its scripts.

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
/// and saves it enabled. Importing the same URL again replaces the plugin.
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
    let text = String::from_utf8(text).map_err(|_| "Plugin text is not UTF-8.".to_string())?;
    let id = format!("{:016x}", hash(url));

    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut scripts = BTreeMap::new();
    let mut resources = BTreeMap::new();
    let listed = description["resources"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for resource in &listed {
        let Some(resource_url) = resource["url"].as_str() else {
            continue;
        };
        if scripts.contains_key(resource_url) || resources.contains_key(resource_url) {
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
        let extension = if script { "js" } else { "dat" };
        let file = format!("{id}-{:016x}.{extension}", hash(resource_url));
        fs::write(directory.join(&file), body).map_err(|error| error.to_string())?;
        if script {
            scripts.insert(resource_url.to_string(), file);
        } else {
            resources.insert(resource_url.to_string(), file);
        }
    }

    let plugin = StoredPlugin {
        id: id.clone(),
        name: description["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(url)
            .to_string(),
        url: url.to_string(),
        enabled: true,
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
    let summary = plugin.summary();
    let mut library = read_library(directory)?;
    match library
        .plugins
        .iter_mut()
        .find(|existing| existing.id == id)
    {
        Some(existing) => *existing = plugin,
        None => library.plugins.push(plugin),
    }
    write_library(directory, &library)?;
    Ok(summary)
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
pub fn apply(directory: &Path, mitm: &mut Map<String, Value>) {
    let Ok(library) = read_library(directory) else {
        return;
    };
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

fn read_library(directory: &Path) -> Result<Library, String> {
    match fs::read(directory.join(LIBRARY_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "The saved plugin list is damaged.".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Library::default()),
        Err(error) => Err(error.to_string()),
    }
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
    fn imported_plugins_join_the_mitm_section_with_their_scripts() {
        let directory = scratch("import");
        let summary = import(&directory, PLUGIN_URL, fetch).unwrap();
        assert_eq!(summary.name, "ipleak test");
        assert_eq!(summary.hostnames, ["ipleak.net"]);
        assert_eq!(summary.script_count, 1);
        assert!(summary.enabled);
        // The same address replaces the plugin instead of adding a second one.
        import(&directory, PLUGIN_URL, fetch).unwrap();
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
