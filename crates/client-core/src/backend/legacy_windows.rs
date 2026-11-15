//! Carries over what the Windows Tauri client saved, so a user who moves to a client built on this crate
//! keeps their subscriptions, node choice, rules, DNS settings and decryption root.
//!
//! The old client keeps one `desktop-state.json` and a handful of files beside it under
//! `%ProgramData%\AnixOps\NetworkCore`. [`migrate`] reads them and writes this crate's store. It only ever
//! reads the old folder: nothing there is changed or deleted, so the old client still starts with what it
//! had. It writes the new `state.json` last and does nothing once that file exists, so a run that was cut
//! short is simply done again and a store the user has since changed is never overwritten.
//!
//! The decryption root's private key is among the files copied. The caller makes sure the new folder is one
//! only the client can read before calling this (the installer and the service do; see
//! `docs/architecture/desktop-native.md`).
//!
//! Not carried over, with the reason:
//! - Update times of subscriptions. The old client wrote them as local time without a zone; they are left
//!   empty, so each subscription is updated once soon after the move.
//! - The cached content of subscriptions other than the one in use. The old client kept only that one; the
//!   others are downloaded again when they are selected or updated.
//! - System proxy settings to restore and the certificate and driver the old client installed. Those belong
//!   to the old program, which puts them back or removes them itself.
//! - Saved capture sessions. Their key is tied to the Windows account of the old program and they expire
//!   after a week.
//! - Whether HTTPS decryption was on. The root is carried over, so turning it on again needs no new trust.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use desktop_dns::DnsPreset;
use engine_embedded::profile::{RoutingMode, RuleList};
use serde::Deserialize;
use serde_json::Value;

use super::state::{SavedState, Store};

const STATE_FILE: &str = "desktop-state.json";

/// Files both clients keep in the same format, by their path under either folder.
const SAME_FILES: [&str; 6] = [
    "my-rules.txt",
    "dns-settings.json",
    "redirects.json",
    "on-demand.json",
    "notifications.json",
    "traffic-stats.json",
];

/// Folders both clients keep in the same format: the decryption root with the plugin library, and the ANX
/// device licence.
const SAME_DIRS: [&str; 2] = ["mitm", "anx-license"];

/// The old client's public rule lists and the list each one becomes.
const RULE_FILES: [(&str, RuleList); 3] = [
    ("china-domains.json", RuleList::ChinaDomains),
    ("china-ips.json", RuleList::ChinaIps),
    ("gfw-domains.json", RuleList::GfwDomains),
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyRoutingMode {
    // What the old client used when the file did not say.
    #[default]
    BypassChina,
    GfwList,
    Direct,
    ReturnChina,
    Rule,
}

impl From<LegacyRoutingMode> for RoutingMode {
    fn from(mode: LegacyRoutingMode) -> Self {
        match mode {
            LegacyRoutingMode::BypassChina => Self::BypassChina,
            LegacyRoutingMode::GfwList => Self::GfwList,
            LegacyRoutingMode::Direct => Self::Direct,
            LegacyRoutingMode::ReturnChina => Self::ReturnChina,
            LegacyRoutingMode::Rule => Self::Rule,
        }
    }
}

/// The parts of the old `desktop-state.json` that have a place in this crate's state. Fields the file
/// lacks take the old client's defaults; fields it has beyond these are ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyState {
    routing_mode: LegacyRoutingMode,
    dns_preset: DnsPreset,
    block_quic: bool,
    direct_egress: bool,
    tun_mode_enabled: bool,
    profile_source_path: Option<PathBuf>,
    profile_source_url: Option<String>,
    profile_node_id: Option<String>,
    start_after_login: bool,
    auto_connect: bool,
    subscription_sources: Vec<LegacySource>,
}

impl LegacyState {
    /// Where the subscription in use came from: its URL, else the file it was imported from.
    fn current_location(&self) -> Option<String> {
        self.profile_source_url.clone().or_else(|| {
            self.profile_source_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacySource {
    location: String,
    last_update_error: Option<String>,
    name: Option<String>,
    user_info: Option<subscription_meta::UserInfo>,
    node_count: Option<usize>,
    update_interval_hours: Option<u32>,
    suggested_interval_hours: Option<u32>,
}

/// What a migration carried over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Migrated {
    /// Subscriptions now in the store.
    pub sources: usize,
    /// Whether the content of the subscription in use came along, so its nodes show without a download.
    pub nodes_cached: bool,
    /// Files and folders copied or converted, by their path under the new folder.
    pub carried: Vec<String>,
}

/// Fills an empty `store` from the old client's folder `old`. `None` when there is nothing to do: the store
/// already has its own state, or `old` holds no saved state. An old state file that cannot be read is an
/// error, and then nothing has been written.
pub fn migrate(old: &Path, store: &Store) -> Result<Option<Migrated>, String> {
    if store.dir().join("state.json").exists() {
        return Ok(None);
    }
    let state_path = old.join(STATE_FILE);
    if !state_path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&state_path)
        .map_err(|error| format!("the old client's saved state could not be read: {error}"))?;
    let legacy: LegacyState = serde_json::from_slice(&bytes)
        .map_err(|error| format!("the old client's saved state is not valid: {error}"))?;

    let mut migrated = Migrated::default();
    let (state, selected) = convert(&legacy, SavedState::default());
    migrated.sources = state.sources.len();

    if let Some(id) = &selected {
        let embedded = old.join("embedded");
        if let Ok(content) = fs::read_to_string(embedded.join("source.txt")) {
            store
                .write_source(id, &content)
                .map_err(|error| failed("the subscription in use", &error))?;
            migrated.nodes_cached = true;
            migrated.carried.push(format!("sources/{id}.txt"));
        }
        let edits = format!("node-edits/{id}.json");
        if copy_file(&embedded.join("node-edits.json"), store, &edits)? {
            migrated.carried.push(edits);
        }
    }
    for name in SAME_FILES {
        if copy_file(&old.join(name), store, name)? {
            migrated.carried.push(name.to_string());
        }
    }
    for name in SAME_DIRS {
        if copy_dir(&old.join(name), store, name)? {
            migrated.carried.push(name.to_string());
        }
    }
    for (name, list) in RULE_FILES {
        let Some(entries) = rule_entries(&old.join("routing-rules").join(name), list) else {
            continue;
        };
        store
            .write_rule_list(list, &entries)
            .map_err(|error| failed(name, &error))?;
        migrated.carried.push(format!("rules/{}", list.file_name()));
    }

    // Last, so a run cut short before this point starts over.
    store
        .save(&state)
        .map_err(|error| failed("the saved state", &error))?;
    Ok(Some(migrated))
}

/// `state`, a new store's defaults, with what the old state says, and the id of the subscription in use.
fn convert(legacy: &LegacyState, mut state: SavedState) -> (SavedState, Option<String>) {
    state.routing_mode = legacy.routing_mode.into();
    state.dns_preset = legacy.dns_preset;
    state.block_quic = legacy.block_quic;
    state.direct_egress = legacy.direct_egress;
    state.tun_mode = legacy.tun_mode_enabled;
    state.preferences.start_after_login = legacy.start_after_login;
    state.preferences.auto_connect = legacy.auto_connect;
    state.selected_node = legacy.profile_node_id.clone();

    let current = legacy.current_location();
    let mut selected = None;
    for source in &legacy.subscription_sources {
        if source.location.is_empty() {
            continue;
        }
        let saved = state.adopt_source(&source.location);
        saved.last_error = source.last_update_error.clone();
        saved.name = source.name.clone();
        saved.user_info = source.user_info;
        saved.node_count = source.node_count;
        saved.update_interval_hours = source.update_interval_hours;
        saved.suggested_interval_hours = source.suggested_interval_hours;
        if selected.is_none() && current.as_deref() == Some(source.location.as_str()) {
            selected = Some(saved.id.clone());
        }
    }
    // Versions before the subscription list kept only the one in use.
    if selected.is_none() {
        if let Some(location) = current.filter(|location| !location.is_empty()) {
            selected = Some(state.adopt_source(&location).id.clone());
        }
    }
    if let Some(id) = &selected {
        state.select_source(id);
    } else {
        // The node choice belonged to a subscription that is gone.
        state.selected_node = None;
    }
    (state, selected)
}

/// The entries of one of the old client's rule lists: `{"rules": [{"domain_suffix": [..]}]}`, or `ip_cidr`
/// for networks. `None` when the list was never downloaded or holds nothing usable.
fn rule_entries(path: &Path, list: RuleList) -> Option<Vec<String>> {
    let source: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let key = if list.is_networks() {
        "ip_cidr"
    } else {
        "domain_suffix"
    };
    let entries: Vec<String> = source
        .get("rules")?
        .as_array()?
        .iter()
        .filter_map(|rule| rule.get(key)?.as_array())
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    (!entries.is_empty()).then_some(entries)
}

fn failed(what: &str, error: &io::Error) -> String {
    format!("{what} could not be carried over: {error}")
}

/// Copies one file into the store; `false` when the old client has no such file.
fn copy_file(from: &Path, store: &Store, relative: &str) -> Result<bool, String> {
    let bytes = match fs::read(from) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(failed(relative, &error)),
    };
    store
        .write_file(relative, &bytes)
        .map_err(|error| failed(relative, &error))?;
    Ok(true)
}

/// Copies a folder and what is under it into the store; `false` when the old client has no such folder or it
/// is empty.
fn copy_dir(from: &Path, store: &Store, relative: &str) -> Result<bool, String> {
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(failed(relative, &error)),
    };
    let mut copied = false;
    for entry in entries {
        let entry = entry.map_err(|error| failed(relative, &error))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let target = format!("{relative}/{name}");
        let kind = entry.file_type().map_err(|error| failed(&target, &error))?;
        copied |= if kind.is_dir() {
            copy_dir(&entry.path(), store, &target)?
        } else if kind.is_file() {
            copy_file(&entry.path(), store, &target)?
        } else {
            // Links are not followed: the old client never made any.
            false
        };
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `desktop-state.json` as the last Windows Tauri release writes it, with two subscriptions, the
    /// first in use.
    const STATE: &str = r#"{
        "routing_mode": "gfw_list",
        "dns_preset": "split",
        "block_quic": true,
        "direct_egress": false,
        "bilibili_web_ad_block_enabled": false,
        "tun_mode_enabled": true,
        "proxy_snapshot": {
            "enabled": false, "server": "", "bypass": "",
            "winhttp_access_type": 1, "winhttp_server": "", "winhttp_bypass": ""
        },
        "applied_proxy": { "enabled": true, "server": "127.0.0.1:7890", "bypass": "<local>" },
        "certificate_sha1": null,
        "driver_inf_path": null,
        "profile_source_path": null,
        "profile_source_url": "https://example.com/sub/a",
        "profile_node_id": "node-hk",
        "profile_node_catalog": [
            { "id": "node-hk", "label": "HK", "protocol": "shadowsocks", "outbound_tag": "node-hk" }
        ],
        "delay_test_url": null,
        "debug_enabled": false,
        "dark_theme": true,
        "profile_last_successful_update": "2026-11-14 21:03:11",
        "profile_last_attempt": "2026-11-14 21:03:10",
        "profile_next_attempt": null,
        "profile_refresh_result": "success",
        "profile_added_node_count": 1,
        "profile_removed_node_count": 0,
        "profile_changed_node_count": 0,
        "profile_refresh_error_code": null,
        "profile_last_update_error": null,
        "start_after_login": true,
        "auto_connect": true,
        "auto_recover_core": true,
        "auto_subscription_refresh": true,
        "auto_select_fastest_node": false,
        "subscription_sources": [
            {
                "id": "source-1a2b",
                "location": "https://example.com/sub/a",
                "last_attempt": "2026-11-14 21:03:10",
                "last_successful_update": "2026-11-14 21:03:11",
                "next_attempt": null,
                "result": "success",
                "added_node_count": 1,
                "removed_node_count": 0,
                "changed_node_count": 0,
                "error_code": null,
                "last_update_error": null,
                "name": "A",
                "user_info": null,
                "node_count": 1,
                "update_interval_hours": 12,
                "suggested_interval_hours": 24
            },
            {
                "id": "source-3c4d",
                "location": "https://example.com/sub/b",
                "result": "failed",
                "last_update_error": "timed out"
            }
        ]
    }"#;

    const LINK: &str = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@82.47.34.99:11111#HK";

    fn dirs(name: &str) -> (PathBuf, Store) {
        let root =
            std::env::temp_dir().join(format!("anixops-net-legacy-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let old = root.join("old");
        fs::create_dir_all(&old).unwrap();
        (old, Store::new(root.join("new")))
    }

    fn write(old: &Path, relative: &str, content: &str) {
        let path = old.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn old_client(old: &Path) {
        write(old, STATE_FILE, STATE);
        write(old, "embedded/source.txt", LINK);
        write(
            old,
            "embedded/node-edits.json",
            r#"{"changed":{},"removed":["node-x"]}"#,
        );
        write(old, "my-rules.txt", "DOMAIN-SUFFIX,example.org,DIRECT\n");
        write(old, "dns-settings.json", r#"{"fake_ip":true}"#);
        write(old, "mitm/root-ca.pem", "certificate");
        write(old, "mitm/root-ca-key.pem", "key");
        write(old, "mitm/plugins/library.json", "[]");
        write(
            old,
            "routing-rules/gfw-domains.json",
            r#"{"version":1,"rules":[{"domain_suffix":["example.net","example.org"]}]}"#,
        );
        write(
            old,
            "routing-rules/china-ips.json",
            r#"{"version":1,"rules":[{"ip_cidr":["1.0.1.0/24"]}]}"#,
        );
    }

    #[test]
    fn settings_and_subscriptions_are_carried_over() {
        let (old, store) = dirs("state");
        old_client(&old);
        let migrated = migrate(&old, &store).unwrap().unwrap();
        assert_eq!(migrated.sources, 2);
        assert!(migrated.nodes_cached);

        let state = store.load();
        assert_eq!(state.routing_mode, RoutingMode::GfwList);
        assert_eq!(state.dns_preset, DnsPreset::Split);
        assert!(state.block_quic);
        assert!(!state.direct_egress);
        assert!(state.tun_mode);
        assert!(state.preferences.start_after_login);
        assert!(state.preferences.auto_connect);
        assert_eq!(state.selected_node.as_deref(), Some("node-hk"));
        // The old client's proxy backup stays with the old client.
        assert_eq!(state.proxy_backup, None);

        assert_eq!(state.sources.len(), 2);
        let first = &state.sources[0];
        assert_eq!(first.location, "https://example.com/sub/a");
        assert!(first.selected);
        assert_eq!(first.name.as_deref(), Some("A"));
        assert_eq!(first.node_count, Some(1));
        assert_eq!(first.update_interval_hours, Some(12));
        assert_eq!(first.suggested_interval_hours, Some(24));
        // Local times without a zone are not guessed at: the subscription is due for an update.
        assert_eq!(first.last_success, None);
        assert!(first.update_due(0));
        let second = &state.sources[1];
        assert_eq!(second.location, "https://example.com/sub/b");
        assert!(!second.selected);
        assert_eq!(second.last_error.as_deref(), Some("timed out"));
        assert_ne!(first.id, second.id);

        // A source added afterwards gets an id of its own.
        let mut state = state;
        let third = state.add_source("https://example.com/sub/c", 1);
        assert!(state.sources[..2].iter().all(|source| source.id != third));
    }

    #[test]
    fn files_are_carried_over_and_the_old_folder_is_left_alone() {
        let (old, store) = dirs("files");
        old_client(&old);
        migrate(&old, &store).unwrap().unwrap();
        let state = store.load();
        let id = state.selected_source().unwrap().id.clone();
        let new = store.dir();

        assert_eq!(store.read_source(&id).unwrap(), LINK);
        assert_eq!(
            fs::read_to_string(new.join("node-edits").join(format!("{id}.json"))).unwrap(),
            r#"{"changed":{},"removed":["node-x"]}"#
        );
        assert_eq!(
            fs::read_to_string(new.join("my-rules.txt")).unwrap(),
            "DOMAIN-SUFFIX,example.org,DIRECT\n"
        );
        assert_eq!(
            fs::read_to_string(new.join("dns-settings.json")).unwrap(),
            r#"{"fake_ip":true}"#
        );
        assert_eq!(
            fs::read_to_string(new.join("mitm").join("root-ca.pem")).unwrap(),
            "certificate"
        );
        assert_eq!(
            fs::read_to_string(new.join("mitm").join("root-ca-key.pem")).unwrap(),
            "key"
        );
        assert_eq!(
            fs::read_to_string(new.join("mitm").join("plugins").join("library.json")).unwrap(),
            "[]"
        );
        assert_eq!(
            store.read_rule_list(RuleList::GfwDomains).unwrap(),
            ["example.net", "example.org"]
        );
        assert_eq!(
            store.read_rule_list(RuleList::ChinaIps).unwrap(),
            ["1.0.1.0/24"]
        );
        // Never downloaded by the old client, so not made up here.
        assert!(!store.has_rule_list(RuleList::ChinaDomains));
        // Files the old client did not have are not created.
        assert!(!new.join("redirects.json").exists());

        // Everything the old client had is still where it was.
        assert_eq!(fs::read_to_string(old.join(STATE_FILE)).unwrap(), STATE);
        assert_eq!(
            fs::read_to_string(old.join("embedded").join("source.txt")).unwrap(),
            LINK
        );
        assert!(old.join("mitm").join("root-ca-key.pem").exists());
    }

    #[test]
    fn a_store_with_its_own_state_is_not_overwritten() {
        let (old, store) = dirs("twice");
        old_client(&old);
        assert!(migrate(&old, &store).unwrap().is_some());

        // The user changes something in the new client; the old folder is still there at the next start.
        let mut state = store.load();
        state.block_quic = false;
        store.save(&state).unwrap();
        assert_eq!(migrate(&old, &store).unwrap(), None);
        assert!(!store.load().block_quic);
    }

    #[test]
    fn a_run_cut_short_is_done_again() {
        let (old, store) = dirs("again");
        old_client(&old);
        // Files were copied but the state was not written yet.
        store.write_file("my-rules.txt", b"half").unwrap();
        let migrated = migrate(&old, &store).unwrap().unwrap();
        assert!(migrated.carried.contains(&"my-rules.txt".to_string()));
        assert_eq!(
            fs::read_to_string(store.dir().join("my-rules.txt")).unwrap(),
            "DOMAIN-SUFFIX,example.org,DIRECT\n"
        );
    }

    #[test]
    fn nothing_saved_means_nothing_to_do() {
        let (old, store) = dirs("empty");
        assert_eq!(migrate(&old, &store).unwrap(), None);
        assert!(!store.dir().exists());
    }

    #[test]
    fn a_broken_state_file_writes_nothing() {
        let (old, store) = dirs("broken");
        old_client(&old);
        write(&old, STATE_FILE, "{ not json");
        assert!(migrate(&old, &store).is_err());
        assert!(!store.dir().exists());
    }

    #[test]
    fn a_state_from_before_the_subscription_list_keeps_its_one_subscription() {
        let (old, store) = dirs("single");
        write(
            &old,
            STATE_FILE,
            r#"{
                "proxy_snapshot": null,
                "certificate_sha1": null,
                "driver_inf_path": null,
                "profile_source_url": "https://example.com/only",
                "profile_node_id": "node-hk"
            }"#,
        );
        write(&old, "embedded/source.txt", LINK);
        let migrated = migrate(&old, &store).unwrap().unwrap();
        assert_eq!(migrated.sources, 1);
        let state = store.load();
        // The old client's default when the file did not say.
        assert_eq!(state.routing_mode, RoutingMode::BypassChina);
        let source = state.selected_source().unwrap();
        assert_eq!(source.location, "https://example.com/only");
        assert_eq!(store.read_source(&source.id).unwrap(), LINK);
        assert_eq!(state.selected_node.as_deref(), Some("node-hk"));
    }

    #[test]
    fn a_state_without_subscriptions_keeps_the_settings() {
        let (old, store) = dirs("no-sources");
        write(
            &old,
            STATE_FILE,
            r#"{
                "routing_mode": "direct",
                "direct_egress": true,
                "proxy_snapshot": null,
                "certificate_sha1": null,
                "driver_inf_path": null,
                "profile_node_id": "node-gone"
            }"#,
        );
        let migrated = migrate(&old, &store).unwrap().unwrap();
        assert_eq!(migrated.sources, 0);
        assert!(!migrated.nodes_cached);
        let state = store.load();
        assert_eq!(state.routing_mode, RoutingMode::Direct);
        assert!(state.direct_egress);
        assert_eq!(state.selected_node, None);
        assert!(state.sources.is_empty());
    }

    /// The moved store opens as a backend with the nodes of the subscription in use.
    #[test]
    fn the_backend_opens_what_was_carried_over() {
        use crate::backend::session::LocalSession;
        use crate::backend::Backend;

        let (old, store) = dirs("backend");
        old_client(&old);
        migrate(&old, &store).unwrap().unwrap();
        let backend = Backend::open(
            store,
            Box::new(LocalSession::new(None, None, PathBuf::from("anixops-net"))),
        );
        assert_eq!(backend.nodes().len(), 1);
        assert_eq!(backend.subscriptions().len(), 2);
    }
}
