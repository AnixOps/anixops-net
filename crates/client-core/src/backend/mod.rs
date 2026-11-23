//! Everything behind the window, free of Tauri so the workspace can test it: saved state, the embedded engine,
//! the desktop's proxy settings and the autostart entry. The window layer (`app.rs`) only moves values in and
//! out and does network fetches and delay tests outside the lock.

pub mod engine;
pub mod legacy_windows;
#[cfg(not(any(target_os = "android", windows)))]
pub mod session_key;
#[cfg(windows)]
#[path = "session_key_windows.rs"]
pub mod session_key;
pub mod state;
pub mod trust;
#[cfg(any(target_os = "android", target_os = "linux"))]
pub mod tun;
#[cfg(windows)]
#[path = "tun_windows.rs"]
pub mod tun;
#[cfg(all(target_os = "linux", not(target_os = "android")))]
pub mod tun_helper;

// What belongs to the signed-in user's session lives in `client-session`, so a window that only talks to a
// background core links it without the engine. The paths here stay as they were.
pub use client_session::{autostart, session, system_proxy};

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The file in the data directory `dir` that lists the thumbprints of the roots users trusted in their own
/// stores ([`Backend::note_trusted_root`]).
pub fn trusted_roots_path(dir: &Path) -> PathBuf {
    dir.join("trusted-roots.txt")
}

/// The capture folder in the data directory `dir`: the key file (when there is one) and `sessions/`.
pub fn capture_dir(dir: &Path) -> PathBuf {
    dir.join("capture")
}

use app_log::{Entry as LogEntry, Level as LogLevel};
use app_notifications::{DisconnectWatch, Notice, Settings as NotificationSettings};
use control_domain::NodeDescriptor;
use desktop_dns::redirects::{
    Draft as RedirectDraft, Entry as RedirectEntry, Redirects, View as Redirect,
};
use desktop_mitm::Root;
use engine_embedded::config::RuntimeConfig;
use engine_embedded::profile;
use node_edits::{NodeEdits, NodeForm};
use on_demand::{
    Action as OnDemandAction, Network, Settings as OnDemandSettings, Watch as OnDemandWatch,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use traffic_stats::{Counter as TrafficCounter, TrafficStats};

pub use desktop_dns::DnsPreset;

use self::engine::{Engine, OtherSource};
use self::session::UserSession;
use self::state::{RoutingMode, SavedSource, SavedState, Store};
use crate::refusal;

/// The refusal codes several commands share (`refusal`).
const SUBSCRIPTION_MISSING: &str = "error.subscription_missing";
const NO_SUPPORTED_NODES: &str = "error.no_supported_nodes";

/// The node a command names is in no saved subscription.
pub(crate) fn node_missing() -> String {
    refusal::coded("error.node_missing", "the node is not in any subscription")
}

/// The source label for share links pasted into the import field.
/// Lines the log viewer shows.
const LOG_LINES: usize = 500;
/// Log lines the diagnostics report ends with.
const DIAGNOSTIC_LOG_LINES: usize = 80;

pub const PASTED_LOCATION: &str = "粘贴的链接";

/// The snapshot's `platform`: the interface hides what the platform does not have.
pub const PLATFORM: &str = if cfg!(target_os = "android") {
    "android"
} else {
    "linux"
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusFact {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub tone: &'static str,
}

impl StatusFact {
    fn new(label: &str, detail: Option<String>, tone: &'static str) -> Self {
        Self {
            label: label.to_string(),
            detail,
            tone,
        }
    }
}

/// The interface's `RuntimeSnapshot`; Windows-only facts get fixed values and `platform` lets the interface
/// hide what does not apply.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub platform: &'static str,
    pub connection: String,
    pub connection_label: String,
    pub core: StatusFact,
    pub proxy: StatusFact,
    pub selected_node: Option<String>,
    pub subscription: Option<String>,
    pub subscription_last_updated: Option<String>,
    pub subscription_last_attempt: Option<String>,
    pub subscription_next_attempt: Option<String>,
    pub subscription_refresh_result: String,
    pub subscription_added_node_count: u32,
    pub subscription_removed_node_count: u32,
    pub subscription_changed_node_count: u32,
    pub subscription_refresh_error_code: Option<String>,
    pub subscription_error: Option<String>,
    pub last_error: Option<String>,
    pub configuration_error: Option<String>,
    pub start_after_login: bool,
    pub auto_connect: bool,
    pub auto_recover_core: bool,
    pub auto_subscription_refresh: bool,
    pub auto_select_fastest_node: bool,
    pub script_runtime_configured: bool,
    pub https_decryption_enabled: bool,
    pub dark_theme: bool,
    pub routing_mode: &'static str,
    pub dns_preset: &'static str,
    /// UDP to port 443 is refused so apps fall back from QUIC to TCP.
    pub block_quic: bool,
    /// Debug mode's direct egress is on: the engine runs with no node.
    pub direct_egress: bool,
    /// Desktop TUN mode (Linux, Windows) is on (`set_tun_mode`).
    pub tun_mode_enabled: bool,
    /// Always true: the interface then offers the subscription rules mode and explains the engine's DNS.
    pub embedded_engine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    pub id: String,
    pub label: String,
    pub protocol: String,
    pub selected: bool,
    /// The subscription the node comes from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// A node of the selected subscription, which can be edited and removed (`node_edits`).
    pub editable: bool,
    /// The id of the subscription the node comes from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The node came from share links pasted into the client, not from a subscription with a name of its
    /// own: `group` is then [`PASTED_LOCATION`], which a window replaces by its own wording.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pasted: bool,
}

/// What a window shows about one node: what it is called and where it is, whether that can be changed,
/// and how it is handed to someone else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetails {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub server: String,
    pub port: u16,
    /// A node of the selected subscription: its name, server and port can be changed.
    pub editable: bool,
    /// Why the node cannot be removed: `"other"` for a node of another subscription, `"last"` for the
    /// only node left, `"used"` for one the subscription's groups or rules name; `None` when it can.
    pub keep: Option<&'static str>,
    /// The node as a share link, if its protocol has one.
    pub link: Option<String>,
    /// That link as a QR code, if it fits in one: the code's rows from the top, one character per module
    /// (`#` dark, `.` light), without the quiet zone.
    pub qr: Option<Vec<String>>,
}

/// A policy group in the interface's `NativeGroupSummary` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub tag: String,
    /// `selector`, `urltest` or `fallback`, as the interface labels them.
    pub group_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    pub outbounds: Vec<String>,
    pub outbound_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionSummary {
    pub id: String,
    pub location: String,
    pub selected: bool,
    pub last_successful_update: Option<String>,
    pub last_attempt: Option<String>,
    pub next_attempt: Option<String>,
    pub result: String,
    pub added_node_count: u32,
    pub removed_node_count: u32,
    pub changed_node_count: u32,
    pub error_code: Option<String>,
    pub last_update_error: Option<String>,
    pub name: String,
    pub node_count: Option<usize>,
    pub upload: Option<u64>,
    pub download: Option<u64>,
    pub total: Option<u64>,
    /// Unix seconds.
    pub expire: Option<u64>,
    pub update_interval_hours: u32,
    pub refreshable: bool,
    /// Share links pasted into the client, under no name of their own: `name` is then
    /// [`PASTED_LOCATION`], which a window replaces by its own wording.
    pub pasted: bool,
}

/// What an operation did. A window words `code` itself, in its own language (the desktop UI's operation-text.ts),
/// with `params` filled in; `message` is the English for the log and for a window that does not know the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationResult {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<&'static str, String>,
    /// A second code the window words after `code`, with no parameters: what the change did to the connection
    /// (`plugins.applied_running` after `mapping.saved`, say), or what is still to do (`mitm.root_untrusted`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow: Option<&'static str>,
    /// Already in the window's language (what a restart did with capture); shown after the coded text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl OperationResult {
    /// A result with no code: a window shows `message` as it is.
    pub fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
            params: BTreeMap::new(),
            follow: None,
            note: None,
        }
    }

    /// A result a window words itself, by `code`; `message` is the English.
    pub fn coded(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code: Some(code),
            ..Self::plain(message)
        }
    }

    /// Fills `{name}` of the coded text.
    pub fn with(mut self, name: &'static str, value: impl ToString) -> Self {
        self.params.insert(name, value.to_string());
        self
    }

    /// Has the window word `code` after this result's own code; `message` is its English, added to this one's.
    pub fn followed_by(mut self, code: &'static str, message: &str) -> Self {
        self.message = format!("{}. {message}", self.message.trim_end_matches('.'));
        self.follow = Some(code);
        self
    }

    /// Adds a note already in the window's language; an empty one is left out.
    pub fn noted(mut self, note: &str) -> Self {
        let note = note.trim();
        if !note.is_empty() {
            self.message = format!("{}. {note}", self.message);
            self.note = Some(note.to_string());
        }
        self
    }
}

/// "我的规则" for the Rules page: the text as written and how many rules it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MyRules {
    pub text: String,
    pub count: usize,
}

/// Whether `source` is share links pasted into the client that the user gave no name.
fn is_pasted(source: &SavedSource) -> bool {
    source.location == PASTED_LOCATION
        && source
            .name
            .as_deref()
            .and_then(subscription_meta::clean_name)
            .is_none()
}

/// `before` with the lines of `pasted` it does not have yet, or `None` when it has them all.
fn join_links(before: &str, pasted: &str) -> Option<String> {
    let mut lines: Vec<&str> = before
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let had = lines.len();
    for line in pasted.lines().map(str::trim) {
        if !line.is_empty() && !lines.contains(&line) {
            lines.push(line);
        }
    }
    (lines.len() > had).then(|| lines.join("\n"))
}

fn done(code: &'static str, message: impl Into<String>) -> Result<OperationResult, String> {
    Ok(OperationResult::coded(code, message))
}

/// The preferences the interface saves; fields Linux does not use are ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesInput {
    pub start_after_login: bool,
    pub auto_connect: bool,
}

pub struct Backend {
    store: Store,
    state: SavedState,
    engine: Engine,
    /// Nodes of the selected source.
    nodes: Vec<NodeDescriptor>,
    /// The selected source's envelope with every node, rules and policy groups, when it has them, with the
    /// other sources linked in as policy groups.
    routed_profile: Option<String>,
    /// The other sources' runnable nodes, listed after the selected source's.
    others: Vec<OtherSource>,
    /// The system proxy and the login entry, which belong to the user's session.
    session: Arc<dyn UserSession>,
    last_error: Option<String>,
    configuration_error: Option<String>,
    /// Why the settings file must not be written this run: it could be neither read nor moved aside, and writing
    /// would replace it with the empty state the client started with.
    state_blocked: Option<String>,
    /// A backup of the desktop's proxy settings a previous run left and could not put back yet: no user session
    /// was there to ask when the client opened. The first session that arrives puts them back.
    proxy_restore_pending: bool,
    /// Seven days of traffic by hour, host and policy (`traffic_stats`).
    traffic: TrafficStats,
    traffic_counter: TrafficCounter,
    traffic_saved_at: u64,
    /// Traffic counted since the statistics were last written; kept until a write succeeds.
    traffic_unsaved: bool,
    /// The notification switches (`notification_settings`).
    notifications: NotificationSettings,
    disconnect_watch: DisconnectWatch,
    /// The Wi-Fi rules (`on_demand_settings`); the platform samples the network (Android only).
    on_demand: OnDemandSettings,
    on_demand_watch: OnDemandWatch,
    /// The key that seals saved capture sessions, handed over by the client when capture is turned on
    /// ([`Backend::remember_capture_key`]), so a restart or a disconnect can save what capture holds without
    /// asking the platform's key store while the backend is locked.
    capture_key: Option<desktop_mitm::sessions::SessionKey>,
    /// What the last restart said about capture (saved as a session, or why not), for the change's answer.
    capture_note: Option<String>,
}

impl Backend {
    /// Loads the saved state and first puts back proxy settings a previous run left changed.
    pub fn open(store: Store, session: Box<dyn UserSession>) -> Self {
        let (mut state, state_problem, state_blocked) = match store.load() {
            Ok(state) => (state, None, None),
            // The broken file is kept under another name; the client starts empty beside it.
            Err(error) => match store.set_aside_state(now()) {
                Ok(kept) => (
                    SavedState::default(),
                    Some(format!(
                        "{error}; it was kept as {} and the client started without it",
                        kept.display()
                    )),
                    None,
                ),
                Err(move_error) => {
                    let problem = format!(
                        "{error}, and it could not be moved aside ({move_error}); settings are not saved until it is fixed"
                    );
                    (SavedState::default(), Some(problem.clone()), Some(problem))
                }
            },
        };
        // The cached contents and node edits on disk name the numbers in use, whatever the index says.
        state.reserve_source_numbers(store.highest_source_number());
        desktop_dns::custom::use_file(store.dir().join("dns-settings.json"));
        desktop_dns::redirects::use_file(store.dir().join("redirects.json"));
        desktop_dns::data_sources::use_file(store.dir().join("data-sources.json"));
        desktop_dns::devices::use_file(store.dir().join("devices.json"));
        // ANX device keys and licenses live with the rest of this client's data, wherever the client runs.
        engine::use_license_dir(store.dir());
        // The removed 国内外分流 DNS choice becomes the user's own settings (docs/no-presets.md). Until that
        // works it is applied as "自定义", never as a preset.
        if let Ok(adopted) = desktop_dns::adopt_legacy_split(state.dns_preset) {
            if adopted != state.dns_preset {
                state.dns_preset = adopted;
                let _ = store.save(&state);
            }
        }
        let mut backend = Self {
            store,
            state,
            engine: Engine::default(),
            nodes: Vec::new(),
            routed_profile: None,
            others: Vec::new(),
            session: Arc::from(session),
            last_error: None,
            configuration_error: None,
            state_blocked,
            proxy_restore_pending: false,
            traffic: TrafficStats::default(),
            traffic_counter: TrafficCounter::default(),
            traffic_saved_at: now(),
            traffic_unsaved: false,
            notifications: NotificationSettings::default(),
            disconnect_watch: DisconnectWatch::default(),
            on_demand: OnDemandSettings::default(),
            on_demand_watch: OnDemandWatch::default(),
            capture_key: None,
            capture_note: None,
        };
        backend.traffic = TrafficStats::load(&backend.traffic_path());
        backend.notifications = NotificationSettings::load(&backend.notifications_path());
        backend.on_demand = OnDemandSettings::load(&backend.on_demand_path());
        if let Some(problem) = state_problem {
            backend.log(LogLevel::Error, &problem);
            backend.last_error = Some(problem);
        }
        // Without a user session yet (a service at boot) this cannot work; the first session that arrives does it.
        if backend.restore_proxy().is_err() && backend.state.proxy_backup.is_some() {
            backend.proxy_restore_pending = true;
        }
        backend.reload_nodes();
        backend
    }

    pub fn auto_connect(&self) -> bool {
        self.state.preferences.auto_connect
    }

    /// Writes the state. A failure reaches the caller as well as the last error: no command answers success over a
    /// change that is not on disk.
    fn save(&mut self) -> Result<(), String> {
        let written = match &self.state_blocked {
            Some(problem) => Err(problem.clone()),
            None => self
                .store
                .save(&self.state)
                .map_err(|error| error.to_string()),
        };
        written.map_err(|error| {
            let error = format!("settings could not be saved: {error}");
            self.last_error = Some(error.clone());
            error
        })
    }

    fn reload_nodes(&mut self) {
        self.nodes.clear();
        self.routed_profile = None;
        self.configuration_error = None;
        self.others = self
            .state
            .sources
            .iter()
            .filter(|source| !source.selected)
            .filter_map(|source| {
                let content = self.store.read_source(&source.id).ok()?;
                let mut nodes: Vec<NodeDescriptor> = engine::parse_nodes(&content)
                    .ok()?
                    .into_iter()
                    .filter(engine::is_supported)
                    .collect();
                engine::mark_license_origin(&mut nodes, &source.location);
                Some(OtherSource {
                    id: source.id.clone(),
                    name: subscription_meta::display_name(source.name.as_deref(), &source.location),
                    nodes,
                })
            })
            .filter(|other| !other.nodes.is_empty())
            .collect();
        let Some(source) = self.state.selected_source() else {
            return;
        };
        match self
            .store
            .read_source(&source.id)
            .map_err(|error| error.to_string())
            .and_then(|content| engine::parse_nodes(&content).map(|nodes| (content, nodes)))
        {
            Ok((content, mut nodes)) => {
                engine::mark_license_origin(&mut nodes, &source.location);
                let edited = NodeEdits::load(&self.node_edits_path(&source.id)).applied(&nodes);
                self.routed_profile = engine::subscription_routing(&content, &nodes, &edited)
                    .map(|profile| engine::link_sources(&profile, &self.others));
                self.nodes = edited;
            }
            Err(error) => self.configuration_error = Some(error),
        }
    }

    /// The other source and its node behind a listed id `{source}.{node}`.
    fn other_node(&self, id: &str) -> Option<(&OtherSource, &NodeDescriptor)> {
        self.others.iter().find_map(|other| {
            let node_id = id.strip_prefix(&other.id)?.strip_prefix('.')?;
            let node = other.nodes.iter().find(|node| node.id == node_id)?;
            Some((other, node))
        })
    }

    /// The selected source's routed profile with the saved group choices, if it has rules and groups.
    fn routed(&self) -> Option<Value> {
        let mut profile: Value = serde_json::from_str(self.routed_profile.as_deref()?).ok()?;
        for (group, member) in &self.state.group_choices {
            // A choice the subscription no longer offers is skipped.
            let _ = profile::set_group_default(&mut profile, group, member);
        }
        Some(profile)
    }

    /// The node traffic goes to: FINAL's node in a routed profile, else the chosen node.
    fn current_node_id(&self) -> Option<String> {
        match self.routed() {
            Some(profile) => profile::selected_node(&profile),
            None => engine::choose_node(&self.nodes, self.state.selected_node.as_deref())
                .map(|node| node.id.clone()),
        }
    }

    /// The envelope to run: the profile (routed, or the chosen node) in the routing mode.
    fn envelope(&self) -> Result<String, String> {
        let envelope =
            self.envelope_with_rules(&desktop_dns::rules::load_lines(&self.my_rules_path()))?;
        let envelope = desktop_dns::apply(&envelope, self.state.dns_preset)?;
        let envelope = desktop_dns::apply_block_quic(&envelope, self.state.block_quic)?;
        let envelope = desktop_dns::redirects::apply(&envelope)?;
        let envelope =
            desktop_dns::rules::with_rule_set_cache(&envelope, &self.rule_set_cache_dir())?;
        if !self.state.preferences.https_decryption {
            return Ok(envelope);
        }
        // Linux has no built-in policy: only the hosts of the enabled plugins are decrypted.
        let envelope =
            self.mitm_root()
                .envelope(envelope.as_bytes(), "", Some(&self.plugins_dir()))?;
        String::from_utf8(envelope).map_err(|error| error.to_string())
    }

    /// The profile, or debug mode's node-less one, with the user's own rule lines first.
    fn envelope_with_rules(&self, lines: &[String]) -> Result<String, String> {
        let envelope = if self.state.direct_egress {
            engine::direct_envelope()
        } else {
            self.routed_envelope()?
        };
        desktop_dns::rules::apply(&envelope, lines)
    }

    /// Where the engine keeps the remote rule sets' last good downloads between runs
    /// (`routing.rule_set_cache_dir`), beside the rest of this client's data.
    fn rule_set_cache_dir(&self) -> PathBuf {
        self.store
            .dir()
            .join(desktop_dns::rules::RULE_SET_CACHE_DIR)
    }

    /// Where "我的规则" are kept, as the text the user wrote.
    fn my_rules_path(&self) -> PathBuf {
        self.store.dir().join("my-rules.txt")
    }

    /// The user's own rule lines as written, and how many rules they hold.
    pub fn my_rules(&self) -> MyRules {
        let text = desktop_dns::rules::load(&self.my_rules_path());
        let count = desktop_dns::rules::parse(&text).map_or(0, |lines| lines.len());
        MyRules { text, count }
    }

    /// Checks and saves the user's own rule lines. A running engine takes them in place (`reload_rules`), so open
    /// connections stay; when it cannot (the profile had no rules before), it restarts with them.
    pub fn save_my_rules(&mut self, text: &str) -> Result<OperationResult, String> {
        let lines = desktop_dns::rules::parse(text).map_err(|error| error.message())?;
        // Without a profile there is nothing to check the lines against yet; they are kept for later.
        let envelope = match self.envelope_with_rules(&[]) {
            Ok(_) => {
                let envelope = self.envelope_with_rules(&lines)?;
                let envelope = desktop_dns::apply(&envelope, self.state.dns_preset)?;
                let envelope = desktop_dns::apply_block_quic(&envelope, self.state.block_quic)?;
                let envelope = desktop_dns::redirects::apply(&envelope)?;
                RuntimeConfig::from_json(envelope.as_bytes()).map_err(|error| {
                    refusal::coded(
                        "error.rules_rejected",
                        format!("the engine rejected the rules: {}", error.message),
                    )
                })?;
                Some(envelope)
            }
            Err(_) => None,
        };
        desktop_dns::rules::save(&self.my_rules_path(), text)?;
        if !self.engine.is_running() {
            return Ok(OperationResult::coded(
                "rules.saved_next",
                format!("{} rules saved", lines.len()),
            )
            .with("count", lines.len()));
        }
        let reloaded = envelope
            .as_deref()
            .and_then(desktop_dns::rules::rules_of)
            .and_then(|rules| {
                self.engine
                    .request(&serde_json::json!({ "cmd": "reload_rules", "rules": rules }))
                    .ok()
            })
            .is_some_and(|answer| answer.get("reloaded") == Some(&serde_json::Value::Bool(true)));
        if reloaded {
            return Ok(OperationResult::coded(
                "rules.applied",
                format!(
                    "{} rules saved and applied; open connections stay",
                    lines.len()
                ),
            )
            .with("count", lines.len()));
        }
        self.restart_if_running()?;
        Ok(OperationResult::coded(
            "rules.saved_restarted",
            format!(
                "{} rules saved; the engine restarted with them",
                lines.len()
            ),
        )
        .with("count", lines.len()))
    }

    /// The profile (routed, or the chosen node) in the routing mode.
    fn routed_envelope(&self) -> Result<String, String> {
        let profile = match self.routed() {
            Some(profile) => profile.to_string(),
            None => {
                let node = engine::choose_node(&self.nodes, self.state.selected_node.as_deref())
                    .ok_or(
                        "import a subscription with a node the embedded engine supports first",
                    )?;
                engine::envelope(&self.nodes, &node.id)?
            }
        };
        profile::envelope_for_mode(&profile, self.state.routing_mode.profile_mode())
    }

    fn mitm_dir(&self) -> PathBuf {
        self.store.dir().join("mitm")
    }

    /// Saved capture sessions and, without a Secret Service, their key (`desktop_mitm::sessions`).
    pub fn capture_dir(&self) -> PathBuf {
        capture_dir(self.store.dir())
    }

    /// Where the HTTPS decryption root is kept, whether or not it exists yet.
    pub fn mitm_root(&self) -> Root {
        Root::at(&self.mitm_dir())
    }

    /// The folder of the imported plugin library.
    pub fn plugins_dir(&self) -> PathBuf {
        self.mitm_dir().join("plugins")
    }

    /// The HTTPS decryption root, generated on first use. The user trusts it by hand; the window never
    /// installs it in a trust store when decryption is turned on.
    pub fn ensure_mitm_root(&self) -> Result<Root, String> {
        Root::ensure(&self.mitm_dir())
    }

    /// The user's session, to ask without holding the backend: what it is asked may wait for the user.
    pub fn session(&self) -> Arc<dyn UserSession> {
        Arc::clone(&self.session)
    }

    /// Remembers that a user's own store trusts the root with this thumbprint, so that removing the client
    /// can take it out again (Windows: `anixops-net-service --purge`). One thumbprint per line.
    pub fn note_trusted_root(&self, thumbprint: &str) {
        let path = trusted_roots_path(self.store.dir());
        let known = std::fs::read_to_string(&path).unwrap_or_default();
        if known.lines().any(|line| line.trim() == thumbprint) {
            return;
        }
        if let Err(error) = std::fs::write(&path, format!("{known}{thumbprint}\n")) {
            self.log(
                LogLevel::Error,
                &format!("the trusted root could not be noted: {error}"),
            );
        }
    }

    /// The local port the mixed proxy listens on.
    pub fn listen_port(&self) -> u16 {
        self.state.listen_port
    }

    /// See [`Engine::probe_envelope`].
    pub fn probe_envelope(&self) -> Option<String> {
        self.engine.probe_envelope()
    }

    pub fn https_decryption(&self) -> bool {
        self.state.preferences.https_decryption
    }

    /// Replaces the generated root with the user's own CA and restarts a running engine that decrypts.
    pub fn import_mitm_root(
        &mut self,
        certificate_pem: &str,
        private_key_pem: &str,
    ) -> Result<OperationResult, String> {
        Root::import(&self.mitm_dir(), certificate_pem, private_key_pem)?;
        let restarted = self.state.preferences.https_decryption && self.restart_if_running()?;
        if restarted {
            done(
                "root.in_use",
                "Your root certificate is in use. Install it again on phones and other devices",
            )
        } else {
            done(
                "root.saved_next",
                "Your root certificate is saved; it is used the next time HTTPS decryption runs",
            )
        }
    }

    /// SHA-256 of the HTTPS decryption root, when there is one.
    pub fn mitm_root_fingerprint(&self) -> Option<String> {
        let root = self.mitm_root();
        if root.exists() {
            root.fingerprint().ok()
        } else {
            None
        }
    }

    /// Turns HTTPS decryption on or off and restarts a running engine with it.
    pub fn set_https_decryption(&mut self, enabled: bool) -> Result<OperationResult, String> {
        if enabled {
            self.ensure_mitm_root()?;
        }
        self.state.preferences.https_decryption = enabled;
        self.save()?;
        let restarted = self.restart_if_running()?;
        let (code, message) = match (enabled, restarted) {
            (true, true) => ("mitm.on", "HTTPS decryption is on"),
            (true, false) => (
                "mitm.on_next",
                "HTTPS decryption is on; it starts with the next connection",
            ),
            (false, _) => ("mitm.off", "HTTPS decryption is off"),
        };
        let note = self.take_capture_note();
        Ok(OperationResult::coded(code, message).noted(&note))
    }

    /// Restarts a running engine after the plugin library changed (plugins, mappings, the decryption list), when
    /// it decrypts. The pinned kernel cannot swap the decryption policy of a running engine, so it restarts;
    /// what capture holds is saved as a session first and capture carries on ([`Backend::restart_if_running`]).
    pub fn plugins_changed(&mut self) -> Result<OperationResult, String> {
        if !self.state.preferences.https_decryption {
            return done(
                "plugins.need_decryption",
                "turn on HTTPS decryption for it to take effect",
            );
        }
        let (code, message) = if self.restart_if_running()? {
            (
                "plugins.applied_running",
                "applied to the running connection",
            )
        } else {
            (
                "plugins.applied_next",
                "it applies with the next connection",
            )
        };
        let note = self.take_capture_note();
        Ok(OperationResult::coded(code, message).noted(&note))
    }

    /// What the last restart said about capture (in the window's language), which is then forgotten; empty when
    /// it said nothing.
    fn take_capture_note(&mut self) -> String {
        self.capture_note.take().unwrap_or_default()
    }

    /// Keeps the capture session key for saving capture before the engine stops; the client hands it over when
    /// capture is turned on.
    pub fn remember_capture_key(&mut self, key: desktop_mitm::sessions::SessionKey) {
        self.capture_key = Some(key);
    }

    /// Saves what live capture holds before the engine stops (`desktop_mitm::sessions::preserve`), named in the
    /// window's language, and says what the next engine needs to carry on. Never fails: without the key (capture
    /// was turned on by an older window, or the key store refused it) capture still carries on, unsaved.
    fn preserve_capture(
        &mut self,
        reason: desktop_mitm::sessions::AutoSave,
    ) -> desktop_mitm::sessions::CaptureCarry {
        self.capture_note = None;
        if !self.engine.is_running() {
            return desktop_mitm::sessions::CaptureCarry::default();
        }
        let dir = self.capture_dir().join("sessions");
        let key = self.capture_key.clone();
        let language = self.notifications.language.clone();
        let engine = &self.engine;
        let carry = desktop_mitm::sessions::preserve(
            // The backend's own requests (storage and key, capture status, save): past the window's list.
            |request: &Value| engine.request(request),
            &dir,
            || key.ok_or_else(|| "the capture session key is not available".to_string()),
            reason.name(&language),
        );
        if let Some(session) = carry.saved.as_deref() {
            self.log(
                LogLevel::Info,
                &format!("capture saved as session {session} before the engine stopped"),
            );
        }
        if let Some(error) = carry.save_error.as_deref() {
            self.log(
                LogLevel::Warn,
                &format!("capture could not be saved before the engine stopped: {error}"),
            );
        }
        self.capture_note = carry.note(reason, &language);
        carry
    }

    /// After a stop: the session [`Self::preserve_capture`] just saved opens in the kernel's capture viewer, so the
    /// request and connection lists keep showing what was captured, and it can be exported, while disconnected.
    fn view_saved_capture(&self, carry: &desktop_mitm::sessions::CaptureCarry) {
        let (Some(session), Some(key)) = (carry.saved.as_deref(), self.capture_key.as_ref()) else {
            return;
        };
        let dir = self.capture_dir().join("sessions");
        if let Err(error) = desktop_mitm::sessions::open_in_viewer(&dir, key, session) {
            self.log(
                LogLevel::Warn,
                &format!("the capture saved at the stop could not be opened: {error}"),
            );
        }
    }

    /// Turns capture on again, with its budget, in the engine that started after a restart.
    fn resume_capture(&self, carry: &desktop_mitm::sessions::CaptureCarry) {
        if let Some(request) = carry.resume_request() {
            if let Err(error) = self.engine.control(&request) {
                self.log(
                    LogLevel::Warn,
                    &format!("capture could not be turned on again after a restart: {error}"),
                );
            }
        }
    }

    pub fn store(&self) -> Store {
        self.store.clone()
    }

    pub fn routing_mode(&self) -> RoutingMode {
        self.state.routing_mode
    }

    /// Starts (or restarts) the engine on the current envelope and returns what it routes through.
    fn launch(&mut self) -> Result<(String, std::net::SocketAddr), String> {
        let envelope = self.envelope()?;
        let node_id = self.current_node_id().unwrap_or_default();
        let address = self
            .engine
            .start(&envelope, &node_id, self.state.listen_port)?;
        let label = if self.state.direct_egress {
            "direct connections (debug mode)".to_string()
        } else if self.routed_profile.is_some() && self.state.routing_mode != RoutingMode::Direct {
            "the subscription's policy groups".to_string()
        } else if self.state.routing_mode == RoutingMode::Direct {
            "direct connections".to_string()
        } else {
            self.node_name(&node_id).unwrap_or(node_id)
        };
        Ok((label, address))
    }

    /// Restarts a running engine so a changed choice takes effect. Returns whether it was running.
    /// Capture only lives as long as the engine: what it holds is saved as a session first, and it is turned on
    /// again, with the same budget, in the new engine.
    fn restart_if_running(&mut self) -> Result<bool, String> {
        if !self.engine.is_running() {
            return Ok(false);
        }
        let capture = self.preserve_capture(desktop_mitm::sessions::AutoSave::Reconnect);
        self.launch()?;
        self.resume_capture(&capture);
        Ok(true)
    }

    fn proxy_fact(&self) -> StatusFact {
        let address = self.engine.address().map(|address| address.to_string());
        if !self.state.preferences.system_proxy {
            return StatusFact::new("Not used", address, "neutral");
        }
        match (self.session.proxy_settings(), &self.state.proxy_backup) {
            (None, _) => StatusFact::new(
                "Unavailable",
                Some(address.map_or_else(
                    || {
                        "this desktop is not supported; set applications to the local proxy by hand"
                            .to_string()
                    },
                    |address| format!("set applications to {address} by hand"),
                )),
                "warning",
            ),
            (Some(settings), Some(_)) => {
                StatusFact::new("Enabled", Some(settings.to_string()), "success")
            }
            (Some(settings), None) => {
                StatusFact::new("Disabled", Some(settings.to_string()), "neutral")
            }
        }
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        let running = self.engine.is_running();
        let crashed = !running && self.engine.node_id().is_some();
        let connection = if running {
            "Connected"
        } else if crashed {
            "Core error"
        } else if self.last_error.is_some() {
            "Connection failed"
        } else {
            "Not connected"
        };
        let source = self.state.selected_source();
        RuntimeSnapshot {
            platform: PLATFORM,
            connection: connection.to_string(),
            connection_label: connection.to_string(),
            core: if running {
                StatusFact::new(
                    "Running",
                    self.engine.address().map(|address| address.to_string()),
                    "success",
                )
            } else {
                StatusFact::new("Stopped", None, if crashed { "danger" } else { "neutral" })
            },
            proxy: self.proxy_fact(),
            selected_node: self.current_node_id(),
            subscription: source.map(|source| source.location.clone()),
            subscription_last_updated: source
                .and_then(|source| source.last_success)
                .map(format_time),
            subscription_last_attempt: source
                .and_then(|source| source.last_attempt)
                .map(format_time),
            subscription_next_attempt: None,
            subscription_refresh_result: String::new(),
            subscription_added_node_count: 0,
            subscription_removed_node_count: 0,
            subscription_changed_node_count: 0,
            subscription_refresh_error_code: None,
            subscription_error: source.and_then(|source| source.last_error.clone()),
            last_error: self.last_error.clone(),
            configuration_error: self.configuration_error.clone(),
            start_after_login: self.session.starts_after_login(),
            auto_connect: self.state.preferences.auto_connect,
            auto_recover_core: false,
            auto_subscription_refresh: false,
            auto_select_fastest_node: false,
            script_runtime_configured: false,
            https_decryption_enabled: self.state.preferences.https_decryption,
            dark_theme: false,
            routing_mode: self.state.routing_mode.as_str(),
            dns_preset: self.state.dns_preset.as_str(),
            block_quic: self.state.block_quic,
            direct_egress: self.state.direct_egress,
            tun_mode_enabled: self.state.tun_mode,
            embedded_engine: true,
        }
    }

    /// The selected source's nodes, then every other source's, each with its subscription's name.
    pub fn nodes(&self) -> Vec<NodeSummary> {
        let selected = self.current_node_id();
        let own_source = self.state.selected_source();
        let group = own_source.map(|source| {
            subscription_meta::display_name(source.name.as_deref(), &source.location)
        });
        let own_id = own_source.map(|source| source.id.clone());
        let own_pasted = own_source.is_some_and(is_pasted);
        let summary = |id: String,
                       node: &NodeDescriptor,
                       group: &Option<String>,
                       source: &Option<String>,
                       pasted: bool,
                       editable: bool| NodeSummary {
            selected: selected.as_deref() == Some(id.as_str()),
            id,
            label: node.name.clone(),
            protocol: engine::protocol_name(&node.protocol),
            group: group.clone(),
            editable,
            source: source.clone(),
            pasted,
        };
        let own = self
            .nodes
            .iter()
            .filter(|node| engine::is_supported(node))
            .map(|node| summary(node.id.clone(), node, &group, &own_id, own_pasted, true));
        let others = self.others.iter().flat_map(|other| {
            let group = Some(other.name.clone());
            let source = Some(other.id.clone());
            let pasted = self
                .state
                .sources
                .iter()
                .find(|source| source.id == other.id)
                .is_some_and(is_pasted);
            other
                .nodes
                .iter()
                .map(|node| {
                    summary(
                        other.outbound_id(&node.id),
                        node,
                        &group,
                        &source,
                        pasted,
                        false,
                    )
                })
                .collect::<Vec<_>>()
        });
        own.chain(others).collect()
    }

    /// The routed profile's policy groups, members labelled with node names.
    pub fn groups(&self) -> Vec<GroupSummary> {
        let Some(profile) = self.routed() else {
            return Vec::new();
        };
        profile::groups(&profile)
            .into_iter()
            .map(|group| GroupSummary {
                tag: group.id,
                group_type: match group.kind.as_str() {
                    "select" => "selector".to_string(),
                    "url-test" => "urltest".to_string(),
                    other => other.to_string(),
                },
                selected: group.choice,
                outbound_labels: group
                    .members
                    .iter()
                    .map(|member| self.node_name(member).unwrap_or_else(|| member.clone()))
                    .collect(),
                outbounds: group.members,
            })
            .collect()
    }

    /// Picks `member` for the select group `group`; a running engine restarts on it.
    pub fn select_group_member(
        &mut self,
        group: &str,
        member: &str,
    ) -> Result<OperationResult, String> {
        let mut profile = self.routed().ok_or_else(|| {
            refusal::coded(
                "error.no_policy_groups",
                "the current subscription has no policy groups",
            )
        })?;
        profile::set_group_default(&mut profile, group, member)?;
        self.state
            .group_choices
            .insert(group.to_string(), member.to_string());
        self.save()?;
        let label = self.node_name(member).unwrap_or_else(|| member.to_string());
        if self.restart_if_running()? {
            Ok(OperationResult::coded(
                "group.member_reconnected",
                format!("Policy group {group} now uses {label}; reconnected"),
            )
            .with("group", group)
            .with("member", label))
        } else {
            Ok(OperationResult::coded(
                "group.member_set",
                format!("Policy group {group} now uses {label}"),
            )
            .with("group", group)
            .with("member", label))
        }
    }

    /// Switches the routing mode. A running engine restarts on it.
    pub fn set_routing_mode(&mut self, mode: RoutingMode) -> Result<OperationResult, String> {
        let previous = self.state.routing_mode;
        self.state.routing_mode = mode;
        if let Err(error) = self.restart_if_running() {
            self.state.routing_mode = previous;
            return Err(error);
        }
        self.save()?;
        done(
            match mode {
                RoutingMode::Rule => "routing.set_rule",
                RoutingMode::Direct => "routing.set_direct",
            },
            format!("Routing mode set to {}", mode.as_str()),
        )
    }

    /// Switches the DNS choice. A running engine restarts on it.
    pub fn set_dns_preset(&mut self, preset: DnsPreset) -> Result<OperationResult, String> {
        let previous = self.state.dns_preset;
        self.state.dns_preset = preset;
        if let Err(error) = self.restart_if_running() {
            self.state.dns_preset = previous;
            return Err(error);
        }
        self.save()?;
        done(
            match preset.as_str() {
                "system" => "dns.set_system",
                "encrypted" => "dns.set_encrypted",
                _ => "dns.set_custom",
            },
            format!("DNS set to {}", preset.as_str()),
        )
    }

    /// The "自定义" DNS text and the Fake-IP switch as saved.
    pub fn custom_dns(&self) -> Result<desktop_dns::custom::DnsText, String> {
        desktop_dns::custom::text()
    }

    /// Checks and saves the "自定义" DNS text and the Fake-IP switch. A running engine restarts when the custom
    /// choice is selected or Fake-IP changed; otherwise the text is kept for later.
    pub fn save_custom_dns(
        &mut self,
        text: &desktop_dns::custom::DnsText,
    ) -> Result<OperationResult, String> {
        if text.fake_ip && !cfg!(target_os = "android") {
            return Err(
                "Fake-IP needs the packet tunnel; this Linux client uses the system proxy, which already sees domain names"
                    .to_string(),
            );
        }
        let settings = text.parse()?;
        let fake_ip_changed = !matches!(desktop_dns::custom::current(), Ok(saved) if saved.fake_ip == settings.fake_ip);
        let in_use = self.state.dns_preset == DnsPreset::Custom || fake_ip_changed;
        desktop_dns::custom::replace(text, || {
            // Without a profile there is nothing to check the settings against yet.
            if self.envelope_with_rules(&[]).is_ok() {
                let envelope = self.envelope()?;
                RuntimeConfig::from_json(envelope.as_bytes()).map_err(|error| {
                    format!("the engine rejected the DNS settings: {}", error.message)
                })?;
            }
            Ok(())
        })?;
        if in_use && self.restart_if_running()? {
            return done(
                "dns.settings_saved_restarted",
                "DNS settings saved; the engine restarted with them",
            );
        }
        if in_use {
            done("dns.settings_saved_next", "DNS settings saved")
        } else {
            done(
                "dns.settings_saved_custom",
                "DNS settings saved; choose Custom to use them",
            )
        }
    }

    /// Whether a connect opens the TUN device first (desktop TUN mode).
    pub fn tun_mode(&self) -> bool {
        self.state.tun_mode
    }

    /// Turns desktop TUN mode (Linux, Windows) on or off; only while disconnected, since the device is opened when
    /// connecting.
    pub fn set_tun_mode(&mut self, on: bool) -> Result<OperationResult, String> {
        if self.engine.is_running() {
            return Err(refusal::coded(
                "error.disconnect_first",
                "Disconnect before switching TUN mode",
            ));
        }
        self.state.tun_mode = on;
        self.save()?;
        if !on {
            done(
                "tun.off_next",
                "TUN mode is off; the next connection uses the system proxy",
            )
        } else if cfg!(windows) {
            done(
                "tun.on_next",
                "TUN mode is on; the next connection carries every application",
            )
        } else {
            done(
                "tun.on_next_password",
                "TUN mode is on; the next connection asks for the administrator password",
            )
        }
    }

    /// Turns QUIC blocking on or off. A running engine restarts on it.
    pub fn set_block_quic(&mut self, on: bool) -> Result<OperationResult, String> {
        let previous = self.state.block_quic;
        self.state.block_quic = on;
        if let Err(error) = self.restart_if_running() {
            self.state.block_quic = previous;
            return Err(error);
        }
        self.save()?;
        if on {
            done("quic.blocked", "QUIC is blocked")
        } else {
            done("quic.allowed", "QUIC is allowed")
        }
    }

    /// What importing the selected subscription's rules left out ([`engine::routing_warnings`]), for the rules
    /// page; empty without a subscription or when its text cannot be read.
    pub fn routing_warnings(&self) -> Vec<String> {
        let Some(source) = self.state.selected_source() else {
            return Vec::new();
        };
        let Ok(content) = self.store.read_source(&source.id) else {
            return Vec::new();
        };
        match engine::parse_nodes(&content) {
            Ok(nodes) => engine::routing_warnings(&content, &nodes),
            Err(_) => Vec::new(),
        }
    }

    /// The data lists the user added for `GEOIP` and `IP-ASN` rules, by name.
    pub fn data_sources(&self) -> Vec<desktop_dns::data_sources::DataSource> {
        desktop_dns::data_sources::current()
    }

    /// Adds the data list `name` (`geoip-<cc>` or `asn-<number>`) at `url`, or points it at `url`.
    pub fn save_data_source(&mut self, name: &str, url: &str) -> Result<OperationResult, String> {
        let next =
            desktop_dns::data_sources::setting(&desktop_dns::data_sources::current(), name, url)?;
        self.replace_data_sources(&next)
    }

    /// Removes the data list `name`.
    pub fn remove_data_source(&mut self, name: &str) -> Result<OperationResult, String> {
        let next = desktop_dns::data_sources::removing(&desktop_dns::data_sources::current(), name);
        self.replace_data_sources(&next)
    }

    /// Saves `next` once the engine accepts it, as for the redirects; a running engine restarts with it and
    /// downloads the new lists.
    fn replace_data_sources(
        &mut self,
        next: &[desktop_dns::data_sources::DataSource],
    ) -> Result<OperationResult, String> {
        let restarted = desktop_dns::data_sources::replace(next, || {
            if self.envelope_with_rules(&[]).is_ok() {
                let envelope = self.envelope()?;
                RuntimeConfig::from_json(envelope.as_bytes()).map_err(|error| {
                    format!("the engine rejected the data lists: {}", error.message)
                })?;
            }
            self.restart_if_running()
        })?;
        if restarted {
            done(
                "data_lists.downloading",
                "Data lists saved; the engine is downloading them",
            )
        } else {
            done(
                "data_lists.next",
                "Data lists saved; they download on the next connection",
            )
        }
    }

    /// The names given to local network devices for `DEVICE-NAME` rules, by address.
    pub fn devices(&self) -> Vec<desktop_dns::devices::Device> {
        desktop_dns::devices::current()
    }

    /// Calls the device at `address` `name`, replacing its earlier name.
    pub fn save_device(&mut self, address: &str, name: &str) -> Result<OperationResult, String> {
        let next = desktop_dns::devices::setting(&desktop_dns::devices::current(), address, name)?;
        self.replace_devices(&next)
    }

    /// Forgets the name of the device at `address`.
    pub fn remove_device(&mut self, address: &str) -> Result<OperationResult, String> {
        let next = desktop_dns::devices::removing(&desktop_dns::devices::current(), address);
        self.replace_devices(&next)
    }

    /// Saves `next` once the engine accepts it, as for the data lists; a running engine restarts with it.
    fn replace_devices(
        &mut self,
        next: &[desktop_dns::devices::Device],
    ) -> Result<OperationResult, String> {
        let restarted = desktop_dns::devices::replace(next, || {
            if self.envelope_with_rules(&[]).is_ok() {
                let envelope = self.envelope()?;
                RuntimeConfig::from_json(envelope.as_bytes()).map_err(|error| {
                    format!("the engine rejected the device names: {}", error.message)
                })?;
            }
            self.restart_if_running()
        })?;
        if restarted {
            done(
                "devices.restarted",
                "Device names saved; the engine restarted with them",
            )
        } else {
            done(
                "devices.next",
                "Device names saved; they apply on the next connection",
            )
        }
    }

    /// The destination rewrites, as the window lists them.
    pub fn traffic_redirects(&self) -> Vec<Redirect> {
        desktop_dns::redirects::views(&desktop_dns::redirects::current())
    }

    /// Adds a rewrite from the editor's fields, or changes the one with id `replacing`.
    pub fn add_redirect(
        &mut self,
        draft: &RedirectDraft,
        replacing: Option<&str>,
    ) -> Result<OperationResult, String> {
        let next = desktop_dns::redirects::current().adding(draft, replacing)?;
        self.replace_redirects(&next)
    }

    /// Saves the list as the window changed it: switches, order and deletions.
    pub fn save_redirects(
        &mut self,
        entries: Vec<RedirectEntry>,
    ) -> Result<OperationResult, String> {
        let next = Redirects { entries }.checked()?;
        self.replace_redirects(&next)
    }

    /// Saves `next` once the engine accepts it: checked against the profile when there is one, and a running
    /// engine restarts with it. A refused list leaves the saved one as it was.
    fn replace_redirects(&mut self, next: &Redirects) -> Result<OperationResult, String> {
        let restarted = desktop_dns::redirects::replace(next, || {
            if self.envelope_with_rules(&[]).is_ok() {
                let envelope = self.envelope()?;
                RuntimeConfig::from_json(envelope.as_bytes()).map_err(|error| {
                    format!("the engine rejected the redirects: {}", error.message)
                })?;
            }
            self.restart_if_running()
        })?;
        if restarted {
            done("redirects.applied", "Redirects saved and applied")
        } else {
            done(
                "redirects.next",
                "Redirects saved; they apply on the next connection",
            )
        }
    }

    /// Runs debug mode's node-less profile (`on`) or the nodes and rules again. A running engine restarts on it.
    pub fn set_direct_egress(&mut self, on: bool) -> Result<OperationResult, String> {
        if self.state.direct_egress == on {
            return done("egress.unchanged", "Egress unchanged");
        }
        let previous = self.state.direct_egress;
        self.state.direct_egress = on;
        if let Err(error) = self.restart_if_running() {
            self.state.direct_egress = previous;
            return Err(error);
        }
        self.save()?;
        if on {
            done("egress.direct", "Traffic leaves directly")
        } else {
            done("egress.node", "Traffic leaves through the selected node")
        }
    }

    /// Where a replayed request goes in (the running engine's local proxy) and the HTTPS decryption root it has
    /// to trust while decryption is on.
    /// Where a replayed request goes: the local proxy on the desktop; on Android, whose own traffic stays out of
    /// the VPN, the engine's replay proxy (127.0.0.1, fresh port and credentials each start), started on first use.
    pub fn replay_target(&self) -> Result<desktop_mitm::replay::Target, String> {
        if !self.engine.is_running() {
            return Err(
                "connect first: replayed requests go through the running engine".to_string(),
            );
        }
        let decrypt = self.state.preferences.https_decryption;
        let root = if decrypt {
            std::fs::read_to_string(&self.mitm_root().certificate_path).ok()
        } else {
            None
        };
        if cfg!(target_os = "android") {
            let running = self.engine.replay_proxy(None)?;
            let answer =
                if running.get("running").and_then(serde_json::Value::as_bool) == Some(true) {
                    running
                } else {
                    self.engine.replay_proxy(Some(decrypt))?
                };
            return desktop_mitm::replay::Target::from_replay_proxy(&answer, root);
        }
        let proxy = self.engine.address().ok_or_else(|| {
            refusal::coded(
                "error.connect_first_replay",
                "connect first: replayed requests go through the running engine",
            )
        })?;
        Ok(desktop_mitm::replay::Target {
            proxy,
            credentials: None,
            root_pem: root,
        })
    }

    pub fn subscriptions(&self) -> Vec<SubscriptionSummary> {
        self.state
            .sources
            .iter()
            .map(|source| SubscriptionSummary {
                id: source.id.clone(),
                location: source.location.clone(),
                selected: source.selected,
                last_successful_update: source.last_success.map(format_time),
                last_attempt: source.last_attempt.map(format_time),
                next_attempt: None,
                result: if source.last_error.is_some() {
                    "failed"
                } else {
                    "succeeded"
                }
                .to_string(),
                added_node_count: 0,
                removed_node_count: 0,
                changed_node_count: 0,
                error_code: None,
                last_update_error: source.last_error.clone(),
                name: subscription_meta::display_name(source.name.as_deref(), &source.location),
                node_count: source.node_count,
                upload: source.user_info.and_then(|info| info.upload),
                download: source.user_info.and_then(|info| info.download),
                total: source.user_info.and_then(|info| info.total),
                expire: source.user_info.and_then(|info| info.expire),
                update_interval_hours: source.effective_interval_hours(),
                refreshable: source.refreshable(),
                pasted: is_pasted(source),
            })
            .collect()
    }

    /// Records what the server's headers said about source `id` (the selected one when `None`).
    pub fn record_response(
        &mut self,
        id: Option<&str>,
        meta: subscription_meta::ResponseMeta,
    ) -> Result<(), String> {
        let id = id
            .map(str::to_string)
            .or_else(|| self.state.selected_source().map(|source| source.id.clone()));
        if let Some(source) = self
            .state
            .sources
            .iter_mut()
            .find(|source| Some(&source.id) == id.as_ref())
        {
            source.record_response(meta);
            self.save()?;
        }
        Ok(())
    }

    pub fn rename_subscription(&mut self, id: &str, name: &str) -> Result<OperationResult, String> {
        let name = subscription_meta::clean_name(name)
            .ok_or_else(|| refusal::coded("error.name_required", "enter a name"))?;
        self.source_mut(id)?.name = Some(name);
        self.save()?;
        // Linked policy groups are named after their subscriptions.
        self.reload_nodes();
        done("subscription.renamed", "Subscription renamed")
    }

    pub fn set_subscription_interval(
        &mut self,
        id: &str,
        hours: u32,
    ) -> Result<OperationResult, String> {
        let hours = hours.min(subscription_meta::MAX_UPDATE_INTERVAL_HOURS);
        self.source_mut(id)?.update_interval_hours = Some(hours);
        self.save()?;
        if hours == 0 {
            done(
                "subscription.interval_off",
                "Automatic updates are off for this subscription",
            )
        } else {
            Ok(OperationResult::coded(
                "subscription.interval",
                format!("This subscription updates every {hours} hours"),
            )
            .with("hours", hours))
        }
    }

    fn source_mut(&mut self, id: &str) -> Result<&mut SavedSource, String> {
        self.state
            .sources
            .iter_mut()
            .find(|source| source.id == id)
            .ok_or_else(|| refusal::coded(SUBSCRIPTION_MISSING, "the subscription was not found"))
    }

    /// Source `id`'s URL when it can be fetched again.
    pub fn source_location(&self, id: &str) -> Option<String> {
        self.state
            .sources
            .iter()
            .find(|source| source.id == id && source.refreshable())
            .map(|source| source.location.clone())
    }

    /// Sources whose automatic update is due now, as (id, URL).
    pub fn due_sources(&self) -> Vec<(String, String)> {
        let time = now();
        self.state
            .sources
            .iter()
            .filter(|source| source.update_due(time))
            .map(|source| (source.id.clone(), source.location.clone()))
            .collect()
    }

    /// Records why a connection could not start before the engine was reached.
    pub fn record_error(&mut self, error: &str) {
        self.log(LogLevel::Error, &format!("connect failed: {error}"));
        self.last_error = Some(error.to_string());
    }

    pub fn connect(&mut self) -> Result<OperationResult, String> {
        let result = self.start();
        self.last_error = result.as_ref().err().cloned();
        match &result {
            Ok(done) => {
                self.disconnect_watch.connected();
                self.log(LogLevel::Info, &done.message);
            }
            Err(error) => self.log(LogLevel::Error, &format!("connect failed: {error}")),
        }
        result
    }

    fn notifications_path(&self) -> PathBuf {
        self.store.dir().join("notifications.json")
    }

    /// The notification switches, changed to `update` first when given; `language` (the window's) sets the
    /// language of the texts.
    pub fn notification_settings(
        &mut self,
        update: Option<NotificationSettings>,
        language: &str,
    ) -> Result<NotificationSettings, String> {
        let mut next = update.unwrap_or_else(|| self.notifications.clone());
        if !language.is_empty() {
            next.language = language.to_string();
        }
        if next != self.notifications {
            next.save(&self.notifications_path()).map_err(|error| {
                format!("the notification settings could not be saved: {error}")
            })?;
            self.notifications = next;
        }
        Ok(self.notifications.clone())
    }

    /// What to notify since the last call: an unexpected stop of the engine, and what scripts posted. Called
    /// every few seconds.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        let running = self.engine.is_running();
        if self.disconnect_watch.stopped_unexpectedly(running) {
            self.log(LogLevel::Error, "the connection stopped unexpectedly");
            return self.notifications.disconnected(false).into_iter().collect();
        }
        if !running {
            return Vec::new();
        }
        self.engine
            .script_notifications()
            .map(|answer| self.notifications.script_notices(&answer))
            .unwrap_or_default()
    }

    fn on_demand_path(&self) -> PathBuf {
        self.store.dir().join("on-demand.json")
    }

    /// The Wi-Fi rules, changed to `update` first when given; changed rules apply to the current network at
    /// the next sample.
    pub fn on_demand_settings(
        &mut self,
        update: Option<OnDemandSettings>,
    ) -> Result<OnDemandSettings, String> {
        if let Some(update) = update {
            let next = update
                .checked()
                .map_err(|name| format!("\"{name}\" is not a Wi-Fi name (1 to 32 bytes)"))?;
            if next != self.on_demand {
                next.save(&self.on_demand_path())
                    .map_err(|error| format!("the Wi-Fi rules could not be saved: {error}"))?;
                self.on_demand = next;
                self.on_demand_watch.reset();
            }
        }
        Ok(self.on_demand.clone())
    }

    pub fn on_demand_enabled(&self) -> bool {
        self.on_demand.enabled
    }

    /// Whether the engine runs, for the Wi-Fi rules to tell that a failed connect no longer needs a retry.
    pub fn engine_running(&self) -> bool {
        self.engine.is_running()
    }

    /// Forgets the network the Wi-Fi rules acted on last, so that a connect they started and that failed is tried
    /// again on the same network at the next sample.
    pub fn retry_on_demand(&mut self) {
        self.on_demand_watch.reset();
    }

    /// The notice when a connect the Wi-Fi rules started failed with `error`; logged too.
    pub fn on_demand_failed_notice(&self, error: &str) -> Option<Notice> {
        self.log(
            LogLevel::Error,
            &format!("Wi-Fi rules: connecting failed: {error}"),
        );
        self.notifications.on_demand_failed(error)
    }

    /// What the rules want on `network` when it is new and the connection is not already that way.
    pub fn on_demand_action(&mut self, network: Network) -> Option<OnDemandAction> {
        let running = self.engine.is_running();
        match self.on_demand_watch.observe(&self.on_demand, network)? {
            OnDemandAction::Connect if !running => {
                self.log(LogLevel::Info, "Wi-Fi rules: connecting");
                Some(OnDemandAction::Connect)
            }
            OnDemandAction::Disconnect if running => {
                self.log(LogLevel::Info, "Wi-Fi rules: disconnecting");
                Some(OnDemandAction::Disconnect)
            }
            _ => None,
        }
    }

    /// The notice for a failed automatic update of source `id`.
    /// The notice for a failed automatic update of source `id`: only the first failure since its last success,
    /// so a subscription that keeps failing is reported once rather than at every retry.
    pub fn subscription_failed_notice(&self, id: &str, error: &str) -> Option<Notice> {
        let failures = self
            .state
            .sources
            .iter()
            .find(|source| source.id == id)?
            .failures;
        if !subscription_meta::notify_failure(failures) {
            return None;
        }
        self.notifications
            .subscription_failed(&self.source_name(id), error)
    }

    fn source_name(&self, id: &str) -> String {
        self.state
            .sources
            .iter()
            .find(|source| source.id == id)
            .map(|source| subscription_meta::display_name(source.name.as_deref(), &source.location))
            .unwrap_or_default()
    }

    fn log_path(&self) -> PathBuf {
        self.store.dir().join("logs").join("app.log")
    }

    /// Records an event in the app's log (the log viewer, `logs`).
    pub fn log(&self, level: LogLevel, message: &str) {
        let _ = app_log::append(&self.log_path(), level, message);
    }

    /// The last lines of the app's log, oldest first; emptied first with `clear`.
    pub fn logs(&self, clear: bool) -> Result<Vec<LogEntry>, String> {
        let files = [("app", self.log_path())];
        if clear {
            app_log::clear(&files)
                .map_err(|error| format!("the log could not be cleared: {error}"))?;
        }
        Ok(app_log::read(&files, LOG_LINES))
    }

    fn start(&mut self) -> Result<OperationResult, String> {
        // What an earlier run left changed goes back first, so its backup is not mistaken for this connection's.
        self.settle_pending_restore();
        let (label, address) = self.launch()?;
        // Through the TUN device every application is carried already; the system proxy stays as it is.
        if self.engine.address().is_none() {
            return Ok(OperationResult::coded(
                "connect.connected_via",
                format!("Connected through {label}"),
            )
            .with("node", &label));
        }
        if self.state.preferences.system_proxy
            && self.state.proxy_backup.is_none()
            && self.session.proxy_settings().is_some()
        {
            if let Err(error) = self.take_over_proxy(address.port()) {
                self.engine.stop();
                return Err(error);
            }
        }
        Ok(OperationResult::coded(
            "connect.connected_via_at",
            format!("Connected through {label} on {address}"),
        )
        .with("node", &label)
        .with("address", address))
    }

    /// Points the desktop's proxy at `port` and keeps the backup on disk. A backup that cannot be saved undoes the
    /// change: a proxy setting whose way back is lost when the process ends is never left in place.
    fn take_over_proxy(&mut self, port: u16) -> Result<(), String> {
        let backup = self
            .session
            .apply_proxy(port)
            .map_err(|error| format!("the system proxy could not be set: {error}"))?;
        self.state.proxy_backup = Some(backup.clone());
        if let Err(error) = self.save() {
            self.state.proxy_backup = None;
            return Err(match self.session.restore_proxy(&backup) {
                Ok(()) => format!("the system proxy was not changed: {error}"),
                Err(restore) => {
                    // Kept in memory so "restore network settings" can still try while this run lasts.
                    self.state.proxy_backup = Some(backup);
                    format!(
                        "the system proxy is set but its backup could not be saved ({error}), and undoing it failed: {restore}"
                    )
                }
            });
        }
        Ok(())
    }

    /// Puts back what an earlier run left changed when this run could not reach the user's session at start.
    fn settle_pending_restore(&mut self) {
        if self.proxy_restore_pending
            && self.session.proxy_settings().is_some()
            && self.restore_proxy().is_ok()
        {
            self.proxy_restore_pending = false;
        }
    }

    /// Sets the system proxy now when the engine already runs and the user's session could not be reached
    /// when it started: a service that connects at boot has no window to ask until the user signs in.
    /// Also puts back proxy settings an earlier run left changed, when no session was there to do it at start.
    pub fn session_arrived(&mut self) {
        self.settle_pending_restore();
        let Some(address) = self.engine.address() else {
            return;
        };
        if !self.state.preferences.system_proxy
            || self.state.proxy_backup.is_some()
            || self.session.proxy_settings().is_none()
        {
            return;
        }
        if let Err(error) = self.take_over_proxy(address.port()) {
            self.log(LogLevel::Error, &error);
            self.last_error = Some(error);
        }
    }

    /// Stops the engine without touching the saved state or the last error; on Android this also lets go of
    /// the VPN's TUN descriptor.
    pub fn stop_engine(&mut self) {
        // Android's VPN went away (revoked, or another VPN took over): what capture holds is kept as a session.
        let carry = self.preserve_capture(desktop_mitm::sessions::AutoSave::Disconnect);
        self.engine.stop();
        self.view_saved_capture(&carry);
    }

    pub fn disconnect(&mut self) -> Result<OperationResult, String> {
        // Capture lives only as long as the engine: what it holds is kept as a session, as on iOS and macOS, and
        // stays on screen from the capture viewer.
        let carry = self.preserve_capture(desktop_mitm::sessions::AutoSave::Disconnect);
        self.engine.stop();
        self.view_saved_capture(&carry);
        self.disconnect_watch.disconnected();
        self.last_error = None;
        self.log(LogLevel::Info, "Disconnected");
        // A failure stays the last error, which the window shows; the engine is stopped either way.
        let _ = self.restore_proxy();
        let note = self.take_capture_note();
        Ok(OperationResult::coded("disconnect.done", "Disconnected").noted(&note))
    }

    /// Puts the desktop's proxy settings back from the backup and forgets it once that is on disk. A failure is
    /// logged, kept as the last error and returned.
    fn restore_proxy(&mut self) -> Result<(), String> {
        let Some(backup) = self.state.proxy_backup.clone() else {
            return Ok(());
        };
        let restored = self
            .session
            .restore_proxy(&backup)
            .map_err(|error| format!("the system proxy could not be restored: {error}"))
            .and_then(|()| {
                self.state.proxy_backup = None;
                // The settings are back; a backup still on disk is only applied once more, which changes nothing.
                self.save()
            });
        if let Err(error) = &restored {
            self.log(LogLevel::Error, error);
            self.last_error = Some(error.clone());
        }
        restored
    }

    pub fn restore_network_settings(&mut self) -> Result<OperationResult, String> {
        if self.state.proxy_backup.is_none() {
            return done(
                "proxy.not_managed",
                "The system proxy settings are not changed by this app",
            );
        }
        self.restore_proxy()?;
        self.proxy_restore_pending = false;
        done("proxy.restored", "System proxy settings restored")
    }

    /// Live traffic and connections from the running engine.
    pub fn runtime_control(
        &self,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.engine.control(request)
    }

    /// One request the backend itself makes of the running engine, past the window's command list
    /// ([`Engine::request`]): the capture session storage and key, and the saved session commands
    /// (`Client::capture_session`). Never an invoke: a window's own requests go through [`Self::runtime_control`].
    pub(crate) fn engine_request(
        &self,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.engine.request(request)
    }

    /// The running engine's per-domain decryption results (`mitm_hosts`), cleared first with `clear`;
    /// `{"enabled": false, "hosts": []}` while there is no engine or it answers no such command.
    pub fn mitm_hosts(&self, clear: bool) -> serde_json::Value {
        let empty = serde_json::json!({ "enabled": false, "hosts": [] });
        if !self.engine.is_running() {
            return empty;
        }
        if clear {
            let _ = self
                .engine
                .request(&serde_json::json!({ "cmd": "clear_mitm_hosts" }));
        }
        self.engine
            .request(&serde_json::json!({ "cmd": "mitm_hosts" }))
            .ok()
            .filter(|answer| answer.get("hosts").is_some_and(serde_json::Value::is_array))
            .unwrap_or(empty)
    }

    /// The running engine's answer to `{"cmd":"connections"}`; `null` while there is no engine to ask.
    pub fn connections(&self) -> serde_json::Value {
        if !self.engine.is_running() {
            return serde_json::Value::Null;
        }
        self.engine
            .control(&serde_json::json!({ "cmd": "connections" }))
            .unwrap_or(serde_json::Value::Null)
    }

    fn traffic_path(&self) -> PathBuf {
        self.store.dir().join("traffic-stats.json")
    }

    /// Counts what the running engine's connections moved since the last call; the statistics are saved at most
    /// once a minute and when the app closes.
    pub fn count_traffic(&mut self) {
        if !self.engine.is_running() {
            self.traffic_counter.reset();
            return;
        }
        let Ok(list) = self
            .engine
            .control(&serde_json::json!({ "cmd": "connections" }))
        else {
            return;
        };
        let now = now();
        if self.traffic_counter.ingest(&list, &mut self.traffic, now) {
            self.traffic_unsaved = true;
        }
        // Traffic counted earlier is written even when this round added nothing: a connection that went quiet
        // before the minute was up is not left in memory only.
        if self.traffic_unsaved && now >= self.traffic_saved_at + 60 {
            self.save_traffic();
        }
    }

    /// Writes the statistics. Only a successful write clears the unsaved mark and starts the next minute; a failed
    /// one is tried again at the next count.
    fn save_traffic(&mut self) {
        if self.traffic.save(&self.traffic_path()).is_ok() {
            self.traffic_unsaved = false;
            self.traffic_saved_at = now();
        }
    }

    /// The traffic statistics, emptied first with `clear`.
    pub fn traffic_stats(&mut self, clear: bool) -> TrafficStats {
        if clear {
            self.traffic.clear();
            self.save_traffic();
        }
        self.traffic.prune(now());
        self.traffic.clone()
    }

    /// The engine's metrics text while it runs, and whether the client is connected.
    pub fn metrics(&self) -> (Option<String>, bool) {
        (self.engine.metrics(), self.engine.is_running())
    }

    /// Rebuilds the engine's connections after a network change or a wake; nothing while it is stopped.
    /// Tells the running engine which network it is on, for `subnet` policy groups; nothing while it is stopped.
    pub fn set_network(&self, network: &engine::NetworkFacts) {
        if self.engine.is_running() {
            let _ = self.engine.set_network(network);
        }
    }

    pub fn reset_network(&self, reset: network_watch::Reset) {
        if self.engine.is_running() {
            self.log(LogLevel::Info, "network changed: connections rebuilt");
        }
        let _ = self.engine.reset_network(reset);
    }

    /// Stops the engine and puts the proxy settings back; called when the window closes.
    pub fn shutdown(&mut self) {
        self.save_traffic();
        let _ = self.preserve_capture(desktop_mitm::sessions::AutoSave::Disconnect);
        self.engine.stop();
        let _ = self.restore_proxy();
    }

    pub fn switch_node(&mut self, node_id: &str) -> Result<OperationResult, String> {
        let Some(node) = self
            .nodes
            .iter()
            .find(|node| node.id == node_id && engine::is_supported(node))
            .cloned()
        else {
            return self.switch_to_other_source(node_id);
        };
        // In a routed profile a node switch picks the node in the main select group.
        if let Some(profile) = self.routed() {
            let group = profile::switch_group_for_node(&profile, &node.id).ok_or(
                "the policy groups choose this subscription's nodes; switch them on the Rules page",
            )?;
            return self.select_group_member(&group, &node.id);
        }
        self.state.selected_node = Some(node.id.clone());
        self.save()?;
        self.restart_if_running()?;
        Ok(
            OperationResult::coded("node.switched_to", format!("Switched to {}", node.name))
                .with("node", &node.name),
        )
    }

    /// Switches to a node of another source: through its linked policy group when the selected source's rules
    /// include it, else by selecting that source and its node.
    fn switch_to_other_source(&mut self, id: &str) -> Result<OperationResult, String> {
        let (other, node) = self.other_node(id).ok_or_else(node_missing)?;
        let (source_id, node_id, name, group_id) = (
            other.id.clone(),
            node.id.clone(),
            node.name.clone(),
            other.group_id(),
        );
        if let Some(mut profile) = self.routed() {
            if profile::set_group_default(&mut profile, &group_id, id).is_ok() {
                let mut choices = vec![(group_id.clone(), id.to_string())];
                if let Some(main) = profile::switch_group_for_node(&profile, &group_id) {
                    profile::set_group_default(&mut profile, &main, &group_id)?;
                    choices.push((main, group_id));
                }
                self.state.group_choices.extend(choices);
                self.save()?;
                self.restart_if_running()?;
                return Ok(OperationResult::coded(
                    "node.switched_to",
                    format!("Switched to {name}"),
                )
                .with("node", &name));
            }
        }
        self.state.select_source(&source_id);
        self.state.selected_node = None;
        self.state.group_choices.clear();
        let saved = self.save();
        self.reload_nodes();
        saved?;
        // A routed subscription whose main group cannot pick the node still runs, on its own choices.
        self.switch_node(&node_id).or_else(|_| {
            self.restart_if_running()?;
            Ok(OperationResult::coded(
                "subscription.switched_groups_choose",
                format!(
                    "Switched to the subscription with {name}; its policy groups choose the node"
                ),
            )
            .with("name", &name))
        })
    }

    /// The nodes to build node `id`'s delay test from, its id among them and its name, for tests run
    /// outside the lock.
    pub fn delay_target(&self, id: &str) -> Option<(Vec<NodeDescriptor>, String, String)> {
        if let Some(node) = self.nodes.iter().find(|node| node.id == id) {
            return Some((self.nodes.clone(), node.id.clone(), node.name.clone()));
        }
        let (other, node) = self.other_node(id)?;
        Some((other.nodes.clone(), node.id.clone(), node.name.clone()))
    }

    /// The share link of a node of the selected source or of another source.
    pub fn share_link(&self, id: &str) -> Result<String, String> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.id == id)
            .or_else(|| self.other_node(id).map(|(_, node)| node))
            .ok_or_else(node_missing)?;
        engine::share_link(node)
    }

    /// What a window shows about one node of the selected source or of another source.
    pub fn node_details(&self, id: &str) -> Result<NodeDetails, String> {
        let own = self.nodes.iter().find(|node| node.id == id);
        let node = own
            .or_else(|| self.other_node(id).map(|(_, node)| node))
            .ok_or_else(node_missing)?;
        let keep = match own {
            None => Some("other"),
            Some(_) => match self.node_form(id)?.keep_reason {
                None => None,
                Some(node_edits::KEEP_LAST) => Some("last"),
                Some(_) => Some("used"),
            },
        };
        let link = engine::share_link(node).ok();
        Ok(NodeDetails {
            id: id.to_string(),
            name: node.name.clone(),
            protocol: engine::protocol_name(&node.protocol),
            server: node.endpoint.host.clone(),
            port: node.endpoint.port,
            editable: own.is_some(),
            keep,
            qr: link
                .as_deref()
                .and_then(|link| desktop_mitm::phone::qr_rows(link).ok()),
            link,
        })
    }

    /// The saved node edits of source `id`.
    fn node_edits_path(&self, id: &str) -> PathBuf {
        self.store
            .dir()
            .join("node-edits")
            .join(format!("{id}.json"))
    }

    fn remove_node_edits(&self, id: &str) -> io::Result<()> {
        NodeEdits::default().save(&self.node_edits_path(id))
    }

    /// The edit form of a node of the selected source.
    pub fn node_form(&self, id: &str) -> Result<NodeForm, String> {
        let profile = self
            .routed_profile
            .as_deref()
            .and_then(|profile| serde_json::from_str::<Value>(profile).ok());
        node_edits::form(&self.nodes, profile.as_ref(), id).ok_or_else(|| {
            refusal::coded(
                "error.node_not_editable",
                "only nodes of the selected subscription can be edited",
            )
        })
    }

    /// Gives a node of the selected source a new name, server and port until the subscription is updated.
    /// A running engine restarts on it.
    pub fn edit_node(
        &mut self,
        id: &str,
        name: &str,
        server: &str,
        port: &str,
    ) -> Result<OperationResult, String> {
        let change = node_edits::check(name, server, port)?;
        self.node_form(id)?;
        self.change_node_edits(|edits| edits.change(id, change))?;
        if self.restart_if_running()? {
            return done(
                "node.edit_saved_restarted",
                "Node saved; the engine restarted with it",
            );
        }
        done("node.edit_saved", "Node saved")
    }

    /// Removes a node of the selected source until the subscription is updated. A running engine restarts on it.
    pub fn remove_node(&mut self, id: &str) -> Result<OperationResult, String> {
        if let Some(reason) = self.node_form(id)?.keep_reason {
            return Err(reason.to_string());
        }
        self.change_node_edits(|edits| edits.remove(id))?;
        if self.state.selected_node.as_deref() == Some(id) {
            self.state.selected_node = None;
            self.save()?;
        }
        if self.restart_if_running()? {
            return done(
                "node.removed_restarted",
                "Node removed; the engine restarted without it",
            );
        }
        done("node.removed", "Node removed")
    }

    fn change_node_edits(&mut self, change: impl FnOnce(&mut NodeEdits)) -> Result<(), String> {
        let source = self
            .state
            .selected_source()
            .ok_or_else(|| refusal::coded("error.no_subscription", "no subscription is selected"))?
            .id
            .clone();
        let path = self.node_edits_path(&source);
        let mut edits = NodeEdits::load(&path);
        change(&mut edits);
        edits
            .save(&path)
            .map_err(|error| format!("the node edit could not be saved: {error}"))?;
        self.reload_nodes();
        Ok(())
    }

    /// The listed nodes, for a batch delay test run outside the lock; `None` for an id no source has.
    pub fn delay_nodes(&self, ids: &[String]) -> Vec<(String, Option<NodeDescriptor>)> {
        ids.iter()
            .map(|id| {
                let node = self
                    .nodes
                    .iter()
                    .find(|node| node.id == *id)
                    .or_else(|| self.other_node(id).map(|(_, node)| node))
                    .cloned();
                (id.clone(), node)
            })
            .collect()
    }

    /// The current nodes, for delay tests run outside the lock.
    pub fn node_list(&self) -> Vec<NodeDescriptor> {
        self.nodes.clone()
    }

    pub fn node_name(&self, node_id: &str) -> Option<String> {
        self.nodes
            .iter()
            .find(|node| node.id == node_id)
            .or_else(|| self.other_node(node_id).map(|(_, node)| node))
            .map(|node| node.name.clone())
    }

    /// Saves a newly fetched source. `content` is the URL's body, the file, or the pasted links.
    pub fn import(&mut self, location: &str, content: &str) -> Result<OperationResult, String> {
        let nodes = engine::parse_nodes(content)?;
        let supported = nodes
            .iter()
            .filter(|node| engine::is_supported(node))
            .count();
        if supported == 0 {
            return Err(refusal::coded(
                NO_SUPPORTED_NODES,
                "none of the nodes can run on the embedded engine yet",
            ));
        }
        // Share links pasted again join the ones pasted before: one "pasted links", not one per paste.
        let earlier = (location == PASTED_LOCATION)
            .then(|| self.pasted_links())
            .flatten();
        if let Some((id, before)) = earlier {
            let Some(joined) = join_links(&before, content) else {
                // Every link is there already. The pasted links become the subscription in use, as after
                // any import.
                self.state.select_source(&id);
                let saved = self.save();
                self.reload_nodes();
                saved?;
                return done("links.imported_before", "These links were imported before");
            };
            let count = engine::parse_nodes(&joined)?.len();
            if let Err(error) = self.store.write_source(&id, &joined) {
                return Err(format!("the subscription could not be saved: {error}"));
            }
            let time = now();
            self.state.select_source(&id);
            if let Some(source) = self.state.sources.iter_mut().find(|source| source.id == id) {
                source.node_count = Some(count);
                source.last_success = Some(time);
                source.last_attempt = Some(time);
                source.last_error = None;
                source.failures = 0;
            }
        } else {
            let id = self.state.add_source(location, now());
            if let Some(source) = self.state.sources.iter_mut().find(|source| source.id == id) {
                source.node_count = Some(nodes.len());
            }
            if let Err(error) = self.store.write_source(&id, content) {
                self.state.remove_source(&id);
                return Err(format!("the subscription could not be saved: {error}"));
            }
        }
        self.state.selected_node = None;
        self.state.group_choices.clear();
        let saved = self.save();
        self.reload_nodes();
        saved?;
        let message = format!("Imported {supported} of {} nodes", nodes.len());
        self.log(LogLevel::Info, &message);
        Ok(OperationResult::coded("nodes.imported", message)
            .with("supported", supported)
            .with("total", nodes.len()))
    }

    /// The share links pasted before, if the user did not give them a name of their own: their id and
    /// what was pasted.
    fn pasted_links(&self) -> Option<(String, String)> {
        let source = self.state.sources.iter().find(|source| is_pasted(source))?;
        let content = self.store.read_source(&source.id).ok()?;
        Some((source.id.clone(), content))
    }

    /// The selected source's URL when it can be fetched again.
    pub fn refreshable_location(&self) -> Option<(String, String)> {
        self.state
            .selected_source()
            .filter(|source| source.refreshable())
            .map(|source| (source.id.clone(), source.location.clone()))
    }

    /// Records a refresh of source `id`: the new content, or why it failed. A source removed while it was being
    /// downloaded stays removed: nothing of it is written again and the refresh is not reported as an update.
    /// The new content is on disk before the node edits made to the old one are dropped, so a failed refresh
    /// leaves the old content and its edits together.
    pub fn finish_refresh(
        &mut self,
        id: &str,
        fetched: Result<String, String>,
    ) -> Result<OperationResult, String> {
        if !self.state.sources.iter().any(|source| source.id == id) {
            return Err(refusal::coded(
                "error.subscription_removed",
                "the subscription was removed while it was updating",
            ));
        }
        let time = now();
        let parsed =
            fetched.and_then(|content| engine::parse_nodes(&content).map(|nodes| (content, nodes)));
        let result = match parsed {
            // An update brings back what the provider serves (`node_edits`), once its content is saved.
            Ok((content, nodes)) => self
                .store
                .write_source(id, &content)
                .map_err(|error| format!("the subscription could not be saved: {error}"))
                .and_then(|()| {
                    self.remove_node_edits(id).map_err(|error| {
                        format!("the subscription was saved but its node edits could not be dropped: {error}")
                    })
                })
                .map(|()| nodes.len()),
            Err(error) => Err(error),
        };
        if let Some(source) = self.state.sources.iter_mut().find(|source| source.id == id) {
            source.last_attempt = Some(time);
            match &result {
                Ok(count) => {
                    source.last_success = Some(time);
                    source.last_error = None;
                    source.node_count = Some(*count);
                    source.failures = 0;
                }
                Err(error) => {
                    source.last_error = Some(error.clone());
                    source.failures = source.failures.saturating_add(1);
                }
            }
        }
        let name = self.source_name(id);
        match &result {
            Ok(count) => self.log(
                LogLevel::Info,
                &format!("subscription {name} updated: {count} nodes"),
            ),
            Err(error) => self.log(
                LogLevel::Error,
                &format!("subscription {name} update failed: {error}"),
            ),
        }
        let saved = self.save();
        self.reload_nodes();
        saved?;
        result.map(|count| {
            OperationResult::coded(
                "subscription.updated_count",
                format!("Subscription updated: {count} nodes"),
            )
            .with("count", count)
        })
    }

    pub fn select_subscription(&mut self, id: &str) -> Result<OperationResult, String> {
        if !self.state.select_source(id) {
            return Err(refusal::coded(
                SUBSCRIPTION_MISSING,
                "the subscription was not found",
            ));
        }
        self.state.selected_node = None;
        self.state.group_choices.clear();
        let saved = self.save();
        self.reload_nodes();
        saved?;
        done(
            "subscription.selected_reconnect",
            "Subscription selected; reconnect to use it",
        )
    }

    pub fn remove_subscription(&mut self, id: &str) -> Result<OperationResult, String> {
        let was_selected = self
            .state
            .selected_source()
            .is_some_and(|source| source.id == id);
        if !self.state.remove_source(id) {
            return Err(refusal::coded(
                SUBSCRIPTION_MISSING,
                "the subscription was not found",
            ));
        }
        if was_selected {
            self.state.group_choices.clear();
        }
        self.store.remove_source(id);
        let _ = self.remove_node_edits(id);
        let saved = self.save();
        self.reload_nodes();
        saved?;
        done("subscription.removed", "Subscription removed")
    }

    pub fn save_preferences(
        &mut self,
        input: &PreferencesInput,
    ) -> Result<OperationResult, String> {
        if self.session.starts_after_login() != input.start_after_login {
            self.session
                .set_start_after_login(input.start_after_login)?;
        }
        self.state.preferences.auto_connect = input.auto_connect;
        self.save()?;
        done("preferences.saved", "Settings saved")
    }

    pub fn validate(&self) -> Result<OperationResult, String> {
        if let Some(error) = &self.configuration_error {
            return Err(error.clone());
        }
        let supported = self
            .nodes
            .iter()
            .filter(|node| engine::is_supported(node))
            .count();
        if supported == 0 {
            return Err(refusal::coded(
                NO_SUPPORTED_NODES,
                "no node the embedded engine can run",
            ));
        }
        Ok(OperationResult::coded(
            "validate.supported",
            format!(
                "{supported} of {} nodes can run on the embedded engine",
                self.nodes.len()
            ),
        )
        .with("supported", supported)
        .with("total", self.nodes.len()))
    }

    /// Writes a report without node addresses, names or credentials. `health` is the client health section
    /// (integrity, cold start, redacted crash reports), appended as it is.
    pub fn diagnostics(&self, health: &str) -> Result<OperationResult, String> {
        let report = self.diagnostics_report(health);
        let path = self
            .store
            .write_report(&format!("diagnostics-{}.log", now()), &report)
            .map_err(|error| format!("the report could not be written: {error}"))?;
        Ok(OperationResult::coded(
            "diagnostics.saved",
            format!("Diagnostics report saved to {}", path.display()),
        )
        .with("path", path.display()))
    }

    /// The diagnostics report: the client's state, `health`, and the last lines of the app's log.
    pub fn diagnostics_report(&self, health: &str) -> String {
        let snapshot = self.snapshot();
        let mut protocols: Vec<String> = self
            .nodes
            .iter()
            .map(|node| engine::protocol_name(&node.protocol))
            .collect();
        protocols.sort();
        protocols.dedup();
        let report = format!(
            "AnixOps Net (Linux) {}\nconnection: {}\ncore: {}\nsystem proxy: {} ({})\ndesktop: {}\nsources: {}\nnodes: {} ({} supported)\nprotocols: {}\nlast error: {}\nconfiguration error: {}\n",
            env!("CARGO_PKG_VERSION"),
            snapshot.connection,
            snapshot.core.label,
            snapshot.proxy.label,
            snapshot.proxy.detail.as_deref().unwrap_or("-"),
            self.session.proxy_settings().unwrap_or("unsupported"),
            self.state.sources.len(),
            self.nodes.len(),
            self.nodes.iter().filter(|node| engine::is_supported(node)).count(),
            protocols.join(", "),
            snapshot.last_error.as_deref().unwrap_or("none"),
            snapshot.configuration_error.as_deref().unwrap_or("none"),
        );
        let log = self
            .logs(false)
            .unwrap_or_default()
            .iter()
            .rev()
            .take(DIAGNOSTIC_LOG_LINES)
            .rev()
            .map(|entry| format!("{} {:?} {}", entry.time_ms, entry.level, entry.message))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{report}\n{health}\n\n--- app log ---\n{log}\n")
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `YYYY-MM-DD HH:MM UTC` from Unix seconds.
pub fn format_time(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let minutes = (seconds % 86_400) / 60;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        minutes / 60,
        minutes % 60
    )
}

/// How long a delay result reads in a message.
pub fn format_delay(delay: Duration) -> String {
    format!("{} ms", delay.as_millis())
}

#[cfg(test)]
mod tests {
    use super::session::LocalSession;
    use super::*;

    const LINK: &str = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@82.47.34.99:11111#HK";

    fn backend(name: &str) -> (Backend, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("anixops-net-backend-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let backend = Backend::open(
            Store::new(dir.join("config")),
            Box::new(LocalSession::new(
                None,
                Some(dir.join("autostart")),
                PathBuf::from("/usr/bin/anixops-net"),
            )),
        );
        (backend, dir)
    }

    #[test]
    fn links_pasted_again_join_the_ones_pasted_before() {
        const SECOND: &str = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@82.47.34.98:11111#JP";
        let (mut backend, dir) = backend("pasted-again");
        backend.import(PASTED_LOCATION, LINK).unwrap();
        // The link that is there already and one that is not: one subscription, two nodes.
        backend
            .import(PASTED_LOCATION, &format!("{LINK}\n\n  {SECOND}\n"))
            .unwrap();
        let subscriptions = backend.subscriptions();
        assert_eq!(subscriptions.len(), 1);
        assert!(subscriptions[0].pasted);
        assert_eq!(subscriptions[0].node_count, Some(2));
        let labels: Vec<String> = backend.nodes().into_iter().map(|node| node.label).collect();
        assert_eq!(labels, ["HK", "JP"]);

        // Nothing new: nothing is added, and it is not an error.
        backend.import(PASTED_LOCATION, SECOND).unwrap();
        assert_eq!(backend.subscriptions().len(), 1);
        assert_eq!(backend.nodes().len(), 2);

        // Pasted links the user named are a subscription of their own; the next paste starts a new one.
        let id = backend.subscriptions()[0].id.clone();
        backend.rename_subscription(&id, "Work").unwrap();
        backend.import(PASTED_LOCATION, LINK).unwrap();
        let subscriptions = backend.subscriptions();
        assert_eq!(subscriptions.len(), 2);
        assert_eq!(
            subscriptions
                .iter()
                .map(|subscription| subscription.pasted)
                .collect::<Vec<_>>(),
            [false, true]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn formats_utc_times() {
        assert_eq!(format_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_time(1_761_300_000), "2025-10-24 10:00 UTC");
        assert_eq!(format_delay(Duration::from_millis(123)), "123 ms");
    }

    #[test]
    fn my_rules_go_first_and_the_engine_accepts_them() {
        let (mut backend, dir) = backend("my-rules");
        // Without a profile the lines are only checked and kept.
        backend.save_my_rules("DOMAIN,kept.test,DIRECT").unwrap();
        assert!(backend.save_my_rules("RULE-SET,ads,DIRECT").is_err());
        assert_eq!(backend.my_rules().count, 1);

        backend.import(PASTED_LOCATION, LINK).unwrap();
        backend
            .save_my_rules(
                "DOMAIN-SUFFIX,ads.test,REJECT\nPROCESS-NAME,curl,DIRECT\nDOMAIN,a.test,PROXY",
            )
            .unwrap();
        assert_eq!(backend.my_rules().count, 3);
        let envelope = backend.envelope().unwrap();
        assert!(
            RuntimeConfig::from_json(envelope.as_bytes()).is_ok(),
            "{envelope}"
        );
        let rules = desktop_dns::rules::rules_of(&envelope).unwrap();
        assert_eq!(rules[0], "DOMAIN-SUFFIX,ads.test,REJECT");
        assert_eq!(rules[2], "DOMAIN,a.test,proxy");

        // Imported lists and the built-in cn categories become rule sets the engine downloads itself.
        backend
            .save_my_rules(
                "DOMAIN-SUFFIX,ads.test,REJECT\nPROCESS-NAME,curl,DIRECT\nDOMAIN,a.test,PROXY\n\
                 RULE-SET,https://lists.test/ads.list,REJECT\nGEOIP,CN,DIRECT,no-resolve",
            )
            .unwrap();
        let routed = backend.envelope().unwrap();
        assert!(
            RuntimeConfig::from_json(routed.as_bytes()).is_ok(),
            "{routed}"
        );
        let value: serde_json::Value = serde_json::from_str(&routed).unwrap();
        assert_eq!(
            value["routing"]["rule_sets"]["local-1"]["url"],
            "https://lists.test/ads.list"
        );
        assert!(value["routing"]["rule_sets"]["geoip-cn"]["url"].is_string());
        backend
            .save_my_rules(
                "DOMAIN-SUFFIX,ads.test,REJECT\nPROCESS-NAME,curl,DIRECT\nDOMAIN,a.test,PROXY",
            )
            .unwrap();

        // Debug mode's direct egress has no node: PROXY lines go direct, the others still apply.
        backend.set_direct_egress(true).unwrap();
        let envelope = backend.envelope().unwrap();
        assert!(
            RuntimeConfig::from_json(envelope.as_bytes()).is_ok(),
            "{envelope}"
        );
        let rules = desktop_dns::rules::rules_of(&envelope).unwrap();
        assert_eq!(rules[0], "DOMAIN-SUFFIX,ads.test,REJECT");
        assert_eq!(rules[2], "DOMAIN,a.test,DIRECT");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The backend's own engine commands are not held to the window's command list: rules reload in place,
    /// saved capture sessions work and the decryption results are read (client-core-ipc-01).
    #[cfg(not(target_os = "android"))]
    #[test]
    fn backend_commands_reach_a_running_engine() {
        let (mut backend, dir) = backend("engine-commands");
        backend.set_direct_egress(true).unwrap();
        backend.save_my_rules("DOMAIN,kept.test,DIRECT").unwrap();
        let envelope = backend.envelope().unwrap();
        let address = backend.engine.start(&envelope, "direct", 0).unwrap();

        // `reload_rules`: the rules apply while the engine keeps running, with no restart.
        let saved = backend
            .save_my_rules("DOMAIN,kept.test,DIRECT\nDOMAIN-SUFFIX,ads.test,REJECT")
            .unwrap();
        assert!(
            saved.message.contains("open connections stay"),
            "{}",
            saved.message
        );
        assert_eq!(backend.engine.address(), Some(address));

        // Saved sessions: the storage and key first, then the session command.
        let sessions = dir.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let key = desktop_mitm::sessions::SessionKey::new(
            &[7u8; desktop_mitm::sessions::KEY_LEN],
            desktop_mitm::sessions::Protection::File,
        )
        .unwrap();
        let storage = desktop_mitm::sessions::storage_request(&sessions, &key);
        backend.engine_request(&storage).unwrap();
        let list = desktop_mitm::sessions::SessionAction::List
            .request()
            .unwrap();
        let answer = backend.engine_request(&list).unwrap();
        assert!(answer["sessions"].is_array(), "{answer}");
        // A window still cannot name the storage or the key, nor reload rules itself.
        assert!(backend.runtime_control(&storage).is_err());
        assert!(backend
            .runtime_control(&serde_json::json!({ "cmd": "reload_rules", "rules": [] }))
            .is_err());

        // The decryption results come from the engine, through the backend and through a window's request.
        let hosts = backend.mitm_hosts(true);
        assert!(hosts["hosts"].is_array(), "{hosts}");
        let asked = backend
            .runtime_control(&serde_json::json!({ "cmd": "mitm_hosts" }))
            .unwrap();
        assert!(asked["hosts"].is_array(), "{asked}");

        backend.engine.stop();
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The ANX licenses follow the backend's data directory, not `$HOME` (client-core-ipc-07): a system
    /// service, the Linux daemon and the Android app have no usable home.
    #[test]
    fn anx_licenses_live_in_the_data_directory() {
        let (_backend, dir) = backend("anx-license");
        let store = engine::license_store().expect("the backend names the license directory");
        // Whichever test's backend opened first, its directory is a temporary one, never the user's config.
        assert!(
            store.dir().starts_with(std::env::temp_dir()),
            "{}",
            store.dir().display()
        );
        assert!(store.dir().ends_with("anx-license"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn direct_egress_runs_without_any_node() {
        let (mut backend, dir) = backend("direct");
        assert!(backend.envelope().is_err());
        backend.set_direct_egress(true).unwrap();
        assert!(backend.snapshot().direct_egress);
        let envelope = backend.envelope().unwrap();
        assert!(envelope.contains("\"direct\""), "{envelope}");
        assert!(RuntimeConfig::from_json(envelope.as_bytes()).is_ok());

        backend.import(PASTED_LOCATION, LINK).unwrap();
        assert!(backend.envelope().unwrap().contains("\"direct\""));
        backend.set_direct_egress(false).unwrap();
        assert!(backend.envelope().unwrap().contains("82.47.34.99"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn https_decryption_adds_the_root_and_plugins_to_the_envelope() {
        const PLUGIN: &str = concat!(
            "#!name = ipleak test\n",
            "[Script]\n",
            "http-response ^https?:\\/\\/ipleak\\.net\\/ script-path=https://plugins.example/a.js, requires-body=true, tag=ipleak\n",
            "[MITM]\n",
            "hostname = ipleak.net\n",
        );
        let (mut backend, dir) = backend("mitm");
        backend.import(PASTED_LOCATION, LINK).unwrap();
        assert!(!backend.snapshot().https_decryption_enabled);
        assert!(!backend.envelope().unwrap().contains("\"mitm\""));
        let changed = backend.plugins_changed().unwrap();
        assert_eq!(changed.code, Some("plugins.need_decryption"));
        assert_eq!(
            changed.message,
            "turn on HTTPS decryption for it to take effect"
        );
        assert_eq!(changed.note, None);

        backend.set_https_decryption(true).unwrap();
        assert!(backend.mitm_root().exists());
        assert!(backend.snapshot().https_decryption_enabled);
        desktop_mitm::plugins::import(
            &backend.plugins_dir(),
            "https://plugins.example/a.plugin",
            |url, _| {
                Ok(if url.ends_with(".plugin") {
                    PLUGIN.as_bytes().to_vec()
                } else {
                    b"$done({});".to_vec()
                })
            },
        )
        .unwrap();
        let changed = backend.plugins_changed().unwrap();
        assert_eq!(changed.code, Some("plugins.applied_next"));
        assert_eq!(changed.message, "it applies with the next connection");
        assert_eq!(changed.note, None);
        let envelope = backend.envelope().unwrap();
        let parsed: Value = serde_json::from_str(&envelope).unwrap();
        assert_eq!(parsed["mitm"]["plugins"], serde_json::json!([PLUGIN]));
        assert_eq!(
            parsed["mitm"]["script_assets"].as_object().unwrap().len(),
            1
        );
        assert!(
            RuntimeConfig::from_json(envelope.as_bytes()).is_ok(),
            "the engine accepts the envelope"
        );

        // The choice is saved, and turning it off leaves the root for the next time.
        let reopened = Backend::open(
            Store::new(dir.join("config")),
            Box::new(LocalSession::new(
                None,
                None,
                PathBuf::from("/usr/bin/anixops-net"),
            )),
        );
        assert!(reopened.https_decryption());
        backend.set_https_decryption(false).unwrap();
        assert!(!backend.envelope().unwrap().contains("\"mitm\""));
        assert!(backend.mitm_root().exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A changed setting restarts the engine, and a new engine starts with capture off and the small default
    /// budget. Capture carries over: it is on again in the new engine with the budget it had.
    #[test]
    fn a_restart_keeps_capture_on_with_its_budget() {
        use desktop_mitm::sessions::{
            storage_request, Protection, SessionKey, DESKTOP_CAPTURE_BUDGET,
        };
        use serde_json::json;

        let (mut backend, dir) = backend("capture-restart");
        backend.set_direct_egress(true).unwrap();
        // Any free port: tests run side by side.
        backend.state.listen_port = 0;
        backend
            .connect()
            .expect("the engine starts on the node-less profile");
        let status = backend
            .runtime_control(&json!({
                "cmd": "set_capture", "enabled": true, "max_total_bytes": DESKTOP_CAPTURE_BUDGET
            }))
            .unwrap();
        assert_eq!(status["enabled"], true);
        assert_eq!(status["max_total_bytes"], DESKTOP_CAPTURE_BUDGET);
        backend.remember_capture_key(SessionKey::new(&[9u8; 32], Protection::File).unwrap());

        backend.set_https_decryption(true).unwrap();
        let status = backend
            .runtime_control(&json!({ "cmd": "capture" }))
            .unwrap();
        assert_eq!(
            status["enabled"], true,
            "capture is on again after the restart"
        );
        assert_eq!(status["max_total_bytes"], DESKTOP_CAPTURE_BUDGET);
        assert_eq!(status["connections"], 0, "a new engine holds nothing yet");

        // Session commands go through the backend's own path; the window's list refuses them.
        let key = SessionKey::new(&[9u8; 32], Protection::File).unwrap();
        let storage = storage_request(&backend.capture_dir().join("sessions"), &key);
        assert!(backend.runtime_control(&storage).is_err());
        backend.engine_request(&storage).unwrap();
        let sessions = backend
            .engine_request(&json!({ "cmd": "capture_sessions" }))
            .unwrap();
        assert!(sessions["sessions"].is_array(), "{sessions}");

        backend.disconnect().unwrap();
        assert!(!backend.engine.is_running());
        // Disconnected, the capture commands reach the kernel's capture viewer, so saved sessions still open; the
        // other commands still say there is no engine.
        let status = backend
            .runtime_control(&json!({ "cmd": "capture" }))
            .unwrap();
        assert_eq!(status["enabled"], false, "{status}");
        assert!(backend
            .runtime_control(&json!({ "cmd": "traffic" }))
            .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_fresh_backend_is_disconnected_and_linux() {
        let (backend, dir) = backend("fresh");
        let snapshot = backend.snapshot();
        assert_eq!(snapshot.platform, "linux");
        assert_eq!(snapshot.connection, "Not connected");
        assert_eq!(snapshot.proxy.label, "Unavailable");
        assert!(backend.nodes().is_empty());
        let json = serde_json::to_value(&snapshot).unwrap();
        assert!(
            json.get("connectionLabel").is_some(),
            "fields use the interface's camelCase names"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    const CLASH: &str = r#"
proxies:
  - name: HK
    type: ss
    server: 82.47.34.99
    port: 11111
    cipher: aes-256-gcm
    password: f43c0eee-13b9-4f07-bec9-d4b744141503
  - name: JP
    type: ss
    server: 82.47.34.100
    port: 11112
    cipher: aes-256-gcm
    password: f43c0eee-13b9-4f07-bec9-d4b744141504
proxy-groups:
  - name: Proxy
    type: select
    proxies:
      - Auto
      - HK
      - DIRECT
  - name: Auto
    type: url-test
    proxies:
      - JP
      - HK
    url: http://www.gstatic.com/generate_204
    interval: 300
rules:
  - DOMAIN-SUFFIX,example.org,DIRECT
  - MATCH,Proxy
"#;

    /// Each line of what the import left out carries the kernel's code, so a window names its kind in its own
    /// language (finding feature-gap-07).
    #[test]
    fn routing_warnings_carry_the_kernel_code() {
        let content = CLASH.replace(
            "rules:\n",
            "  - name: Chain\n    type: relay\n    proxies:\n      - HK\n      - JP\nrules:\n",
        );
        let nodes = engine::parse_nodes(&content).unwrap();
        let warnings = engine::routing_warnings(&content, &nodes);
        assert!(!warnings.is_empty());
        for line in &warnings {
            let (code, english) = refusal::split(line).expect("a coded line");
            assert!(code.starts_with("config.core.routing_import."), "{line}");
            assert!(!english.is_empty());
        }
        assert!(
            warnings.iter().any(|line| line
                .starts_with("config.core.routing_import.group_skipped: policy group Chain")),
            "{warnings:?}"
        );
    }

    #[test]
    fn subscription_rules_route_and_policy_groups_switch() {
        let (mut backend, dir) = backend("routed");
        backend.import("https://example.com/sub", CLASH).unwrap();
        let groups = backend.groups();
        assert_eq!(
            groups
                .iter()
                .map(|group| group.tag.as_str())
                .collect::<Vec<_>>(),
            ["Proxy", "Auto"]
        );
        assert_eq!(groups[0].group_type, "selector");
        assert_eq!(groups[0].selected.as_deref(), Some("Auto"));
        assert_eq!(groups[1].group_type, "urltest");
        assert!(groups[1].outbound_labels.contains(&"JP".to_string()));
        let hk = backend
            .node_list()
            .into_iter()
            .find(|node| node.name == "HK")
            .unwrap()
            .id;
        let jp = groups[1].outbounds[0].clone();
        assert_eq!(
            backend.snapshot().selected_node.as_deref(),
            Some(jp.as_str())
        );

        // A node switch picks the node in the main select group, and the choice is saved.
        backend.switch_node(&hk).unwrap();
        assert_eq!(backend.groups()[0].selected.as_deref(), Some(hk.as_str()));
        assert_eq!(
            backend.snapshot().selected_node.as_deref(),
            Some(hk.as_str())
        );
        assert!(backend.select_group_member("Auto", &hk).is_err());
        backend.select_group_member("Proxy", "DIRECT").unwrap();
        assert_eq!(backend.snapshot().selected_node, None);
        let routed: serde_json::Value = serde_json::from_str(&backend.envelope().unwrap()).unwrap();
        assert_eq!(routed["routing"]["groups"][0]["default"], "DIRECT");

        // Rule and direct modes only: public builds ship no preset list modes (docs/no-presets.md).
        assert_eq!(backend.snapshot().routing_mode, "rule");
        assert_eq!(RoutingMode::parse("bypass_china"), None); // neutral-terms: allow (value saved by an earlier version)
        assert_eq!(RoutingMode::parse("gfw_list"), None); // neutral-terms: allow (value saved by an earlier version)
        assert_eq!(RoutingMode::parse("return_china"), None); // neutral-terms: allow (value saved by an earlier version)
        backend.set_routing_mode(RoutingMode::Direct).unwrap();
        assert_eq!(backend.snapshot().routing_mode, "direct");
        let direct: serde_json::Value = serde_json::from_str(&backend.envelope().unwrap()).unwrap();
        assert_eq!(
            direct["routing"]["rules"],
            serde_json::json!(["FINAL,DIRECT"])
        );
        // The engine starts in its direct mode, so DNS through a node (the default 1.1.1.1:53 among it) goes
        // direct too, and the remote rule sets keep their downloads in the client's folder.
        assert_eq!(direct["mode"], "direct");
        assert_eq!(
            direct["routing"]["rule_set_cache_dir"],
            serde_json::json!(backend.rule_set_cache_dir().to_str().unwrap())
        );
        backend.set_routing_mode(RoutingMode::Rule).unwrap();
        let rule: serde_json::Value = serde_json::from_str(&backend.envelope().unwrap()).unwrap();
        assert!(rule.get("mode").is_none());
        assert_eq!(
            rule["routing"]["rule_set_cache_dir"],
            serde_json::json!(backend.rule_set_cache_dir().to_str().unwrap())
        );

        // Choices belong to the subscription they were made in.
        let _ = backend.import(PASTED_LOCATION, LINK);
        assert!(backend.state.group_choices.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn other_subscriptions_are_listed_and_linked_as_policy_groups() {
        let (mut backend, dir) = backend("linked");
        backend.import("https://two.example/sub", LINK).unwrap();
        backend.import("https://example.com/sub", CLASH).unwrap();
        let nodes = backend.nodes();
        assert_eq!(
            nodes
                .iter()
                .map(|node| (node.label.as_str(), node.group.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("HK", Some("example.com")),
                ("JP", Some("example.com")),
                ("HK", Some("two.example"))
            ]
        );
        let other = nodes[2].id.clone();
        assert!(other.starts_with("s1."), "{other}");

        // The other subscription joins the rules as a select group and every select group can pick it.
        let groups = backend.groups();
        let linked = groups.last().unwrap();
        assert_eq!(linked.tag, "订阅·two.example");
        assert_eq!(linked.outbounds, [other.as_str()]);
        assert_eq!(linked.outbound_labels, ["HK"]);
        assert!(groups[0].outbounds.contains(&linked.tag));
        assert!(!groups[1].outbounds.contains(&linked.tag));

        // Picking its node routes the main group through it.
        backend.switch_node(&other).unwrap();
        assert_eq!(
            backend.snapshot().selected_node.as_deref(),
            Some(other.as_str())
        );
        assert_eq!(
            backend.groups()[0].selected.as_deref(),
            Some("订阅·two.example")
        );
        let (delay_nodes, delay_id, name) = backend.delay_target(&other).unwrap();
        assert_eq!(name, "HK");
        assert!(delay_nodes.iter().any(|node| node.id == delay_id));
        assert!(backend.delay_target("s9.missing").is_none());
        let routed: serde_json::Value = serde_json::from_str(&backend.envelope().unwrap()).unwrap();
        assert!(routed["routing"]["outbounds"].get(&other).is_some());

        // From the plain subscription, a node of the routed one selects that subscription.
        backend.select_subscription("s1").unwrap();
        let nodes = backend.nodes();
        assert_eq!(nodes[0].group.as_deref(), Some("two.example"));
        assert!(nodes[0].selected);
        let hk = nodes
            .iter()
            .find(|node| node.label == "HK" && node.group.as_deref() == Some("example.com"))
            .unwrap()
            .id
            .clone();
        assert!(hk.starts_with("s2."), "{hk}");
        backend.switch_node(&hk).unwrap();
        assert_eq!(backend.state.selected_source().unwrap().id, "s2");
        let selected = backend.snapshot().selected_node.unwrap();
        assert_eq!(backend.node_name(&selected).as_deref(), Some("HK"));
        assert!(backend.switch_node("s9.missing").is_err());

        // Linked groups follow a rename.
        backend.rename_subscription("s1", "Two, B").unwrap();
        assert_eq!(backend.groups().last().unwrap().tag, "订阅·Two  B");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn node_edits_change_and_remove_nodes_until_the_subscription_is_updated() {
        let (mut backend, dir) = backend("node-edits");
        let two = format!("{LINK}\n{}", LINK.replace("11111#HK", "22222#JP"));
        backend.import("https://edit.example/sub", &two).unwrap();
        let nodes = backend.nodes();
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|node| node.editable));
        let (hk, jp) = (nodes[0].id.clone(), nodes[1].id.clone());

        let form = backend.node_form(&hk).unwrap();
        assert_eq!((form.server.as_str(), form.port), ("82.47.34.99", 11111));
        assert!(form.keep_reason.is_none());
        assert!(backend.edit_node(&hk, "香港", "hk.example", "0").is_err());
        let details = backend.node_details(&hk).unwrap();
        assert_eq!(
            (
                details.name.as_str(),
                details.server.as_str(),
                details.port,
                details.editable,
                details.keep
            ),
            ("HK", "82.47.34.99", 11111, true, None)
        );
        assert!(details
            .link
            .as_deref()
            .is_some_and(|link| link.starts_with("ss://")));
        assert!(details
            .qr
            .as_deref()
            .is_some_and(|qr| qr.len() >= 21 && qr[0].starts_with("#######.")));
        assert!(backend.node_details("no such node").is_err());
        backend
            .edit_node(&hk, "香港", "hk.example", "8443")
            .unwrap();
        assert_eq!(backend.nodes()[0].label, "香港");
        backend.switch_node(&hk).unwrap();
        let envelope = backend.envelope().unwrap();
        assert!(
            envelope.contains("hk.example") && envelope.contains("8443"),
            "{envelope}"
        );

        backend.remove_node(&jp).unwrap();
        assert_eq!(backend.nodes().len(), 1);
        assert!(backend.node_form(&hk).unwrap().keep_reason.is_some());
        assert_eq!(backend.node_details(&hk).unwrap().keep, Some("last"));
        assert!(backend.remove_node(&hk).is_err());

        // Edits survive a restart and go away with an update.
        let reopened = Backend::open(
            Store::new(dir.join("config")),
            Box::new(LocalSession::new(
                None,
                None,
                PathBuf::from("/usr/bin/anixops-net"),
            )),
        );
        assert_eq!(reopened.nodes().len(), 1);
        let id = backend.subscriptions()[0].id.clone();
        backend.finish_refresh(&id, Ok(two)).unwrap();
        assert_eq!(backend.nodes().len(), 2);
        assert_eq!(backend.nodes()[0].label, "HK");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn routed_nodes_its_groups_name_cannot_be_removed() {
        let (mut backend, dir) = backend("node-edits-routed");
        backend.import("https://example.com/sub", CLASH).unwrap();
        let hk = backend.nodes()[0].id.clone();
        assert!(backend.node_form(&hk).unwrap().keep_reason.is_some());
        assert!(backend.remove_node(&hk).is_err());
        backend.edit_node(&hk, "", "hk.example", "443").unwrap();
        let envelope = backend.envelope().unwrap();
        assert!(envelope.contains("hk.example"), "{envelope}");
        assert!(RuntimeConfig::from_json(envelope.as_bytes()).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn importing_selects_the_source_and_preferences_write_autostart() {
        let (mut backend, dir) = backend("import");
        assert!(backend.import(PASTED_LOCATION, "nothing useful").is_err());
        match backend.import(PASTED_LOCATION, LINK) {
            Ok(result) => {
                assert!(result.message.starts_with("Imported 1 of 1"));
                assert_eq!(backend.subscriptions().len(), 1);
                assert_eq!(backend.nodes().len(), 1);
                assert!(backend.nodes()[0].selected);
                assert!(backend.validate().is_ok());
                assert!(
                    backend.refreshable_location().is_none(),
                    "pasted links cannot be refetched"
                );
                let id = backend.subscriptions()[0].id.clone();
                assert!(backend.remove_subscription(&id).is_ok());
                assert!(backend.nodes().is_empty());
            }
            // Only when this build's engine lacks the node's cipher.
            Err(error) => assert!(error.contains("embedded engine")),
        }

        backend
            .save_preferences(&PreferencesInput {
                start_after_login: true,
                auto_connect: true,
            })
            .unwrap();
        assert!(backend.snapshot().start_after_login);
        assert!(backend.auto_connect());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn logs_imports_and_failed_updates_without_url_queries() {
        let (mut backend, dir) = backend("logs");
        backend
            .import("https://a.example/sub?token=x", LINK)
            .unwrap();
        let id = backend.state.sources[0].id.clone();
        let _ = backend.finish_refresh(
            &id,
            Err("error sending request for url (https://a.example/sub?token=x)".to_string()),
        );
        let logs = backend.logs(false).unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].message, "Imported 1 of 1 nodes");
        assert_eq!(logs[1].level, LogLevel::Error);
        assert!(!logs[1].message.contains("token"));
        assert!(backend.logs(true).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keeps_notification_settings_and_notices_nothing_while_disconnected() {
        let (mut backend, dir) = backend("notifications");
        let settings = backend.notification_settings(None, "en").unwrap();
        assert!(settings.scripts && settings.connection && settings.subscriptions);
        assert_eq!(settings.language, "en");
        let off = NotificationSettings {
            scripts: false,
            ..settings
        };
        backend
            .notification_settings(Some(off.clone()), "")
            .unwrap();
        assert_eq!(
            NotificationSettings::load(&backend.notifications_path()),
            off
        );
        assert!(backend.take_notices().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keeps_wifi_rules_and_connects_only_when_disconnected() {
        let (mut backend, dir) = backend("on-demand");
        assert!(!backend.on_demand_settings(None).unwrap().enabled);
        assert_eq!(backend.on_demand_action(Network::Cellular), None);
        let rules = OnDemandSettings {
            enabled: true,
            wifi: vec![on_demand::Rule {
                ssid: " Home ".to_string(),
                action: OnDemandAction::Disconnect,
            }],
            ..OnDemandSettings::default()
        };
        let saved = backend.on_demand_settings(Some(rules)).unwrap();
        assert_eq!(saved.wifi[0].ssid, "Home");
        assert_eq!(OnDemandSettings::load(&backend.on_demand_path()), saved);
        // Not connected: "Home" wants a disconnect, which there is nothing to do for; cellular connects once.
        assert_eq!(
            backend.on_demand_action(Network::Wifi(Some("Home".into()))),
            None
        );
        assert_eq!(
            backend.on_demand_action(Network::Cellular),
            Some(OnDemandAction::Connect)
        );
        assert_eq!(backend.on_demand_action(Network::Cellular), None);
        let refused = OnDemandSettings {
            wifi: vec![on_demand::Rule {
                ssid: String::new(),
                action: OnDemandAction::Connect,
            }],
            ..saved.clone()
        };
        assert!(backend.on_demand_settings(Some(refused)).is_err());
        assert_eq!(backend.on_demand_settings(None).unwrap(), saved);
        let _ = std::fs::remove_dir_all(dir);
    }

    const SUBSCRIPTION: &str = "https://sub.example/link";

    #[test]
    fn a_refresh_of_a_removed_subscription_writes_nothing_back() {
        let (mut backend, dir) = backend("refresh-removed");
        backend.import(SUBSCRIPTION, LINK).unwrap();
        let id = backend.subscriptions()[0].id.clone();
        // The download was under way when the subscription was removed.
        backend.remove_subscription(&id).unwrap();
        assert!(backend.finish_refresh(&id, Ok(LINK.to_string())).is_err());
        assert!(backend.subscriptions().is_empty());
        assert!(
            backend.store.read_source(&id).is_err(),
            "no cache is left behind"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_refresh_keeps_the_old_content_and_its_node_edits() {
        let (mut backend, dir) = backend("refresh-keeps-edits");
        backend.import(SUBSCRIPTION, LINK).unwrap();
        let id = backend.subscriptions()[0].id.clone();
        let node = backend.nodes()[0].id.clone();
        backend
            .edit_node(&node, "Mine", "82.47.34.99", "11111")
            .unwrap();
        let edits = backend.node_edits_path(&id);
        let edited = std::fs::read(&edits).unwrap();

        // Content that does not parse.
        assert!(backend
            .finish_refresh(&id, Ok("not a subscription".to_string()))
            .is_err());
        assert_eq!(std::fs::read(&edits).unwrap(), edited);

        // Content that parses but cannot be written: a folder stands where the cache goes.
        let cache = backend
            .store
            .dir()
            .join("sources")
            .join(format!("{id}.txt"));
        std::fs::remove_file(&cache).unwrap();
        std::fs::create_dir_all(cache.join("blocked")).unwrap();
        assert!(backend.finish_refresh(&id, Ok(LINK.to_string())).is_err());
        assert_eq!(std::fs::read(&edits).unwrap(), edited);

        // Once the new content is saved, the edits of the old one go.
        std::fs::remove_dir_all(&cache).unwrap();
        backend.finish_refresh(&id, Ok(LINK.to_string())).unwrap();
        assert_eq!(NodeEdits::load(&edits), NodeEdits::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_automatic_updates_wait_and_are_notified_once() {
        let (mut backend, dir) = backend("update-backoff");
        backend.import(SUBSCRIPTION, LINK).unwrap();
        let id = backend.subscriptions()[0].id.clone();
        backend.set_subscription_interval(&id, 1).unwrap();
        // Long overdue.
        backend.state.sources[0].last_success = Some(1000);
        assert_eq!(backend.due_sources().len(), 1);

        let _ = backend.finish_refresh(&id, Err("timed out".to_string()));
        assert_eq!(backend.state.sources[0].failures, 1);
        assert!(
            backend.due_sources().is_empty(),
            "the next one-minute check does not download again"
        );
        assert!(backend
            .subscription_failed_notice(&id, "timed out")
            .is_some());

        let _ = backend.finish_refresh(&id, Err("timed out".to_string()));
        assert_eq!(backend.state.sources[0].failures, 2);
        assert!(backend
            .subscription_failed_notice(&id, "timed out")
            .is_none());

        backend.finish_refresh(&id, Ok(LINK.to_string())).unwrap();
        assert_eq!(backend.state.sources[0].failures, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_save_that_fails_reaches_the_command() {
        let (mut backend, dir) = backend("save-fails");
        backend.import(SUBSCRIPTION, LINK).unwrap();
        let id = backend.subscriptions()[0].id.clone();
        backend.state_blocked = Some("blocked for the test".to_string());
        assert!(backend.rename_subscription(&id, "Other").is_err());
        assert!(backend.set_subscription_interval(&id, 6).is_err());
        assert!(backend.set_tun_mode(true).is_err());
        assert!(backend.last_error.is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unreadable_settings_file_is_kept_and_its_numbers_skipped() {
        let dir = std::env::temp_dir().join(format!(
            "anixops-net-backend-unreadable-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config");
        let store = Store::new(config.clone());
        store.write_source("s1", "ss://old").unwrap();
        std::fs::write(config.join("state.json"), b"{ broken").unwrap();
        let mut backend = Backend::open(
            store,
            Box::new(LocalSession::new(
                None,
                None,
                PathBuf::from("/usr/bin/anixops-net"),
            )),
        );
        assert!(backend.last_error.as_deref().unwrap().contains("not valid"));
        let kept: Vec<_> = std::fs::read_dir(&config)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("state.unreadable-")
            })
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read(kept[0].path()).unwrap(), b"{ broken");

        backend.import(SUBSCRIPTION, LINK).unwrap();
        assert_eq!(backend.subscriptions()[0].id, "s2");
        assert_eq!(backend.store.read_source("s1").unwrap(), "ss://old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn traffic_counted_before_a_quiet_minute_is_still_saved() {
        let (mut backend, dir) = backend("traffic-dirty");
        backend.traffic_unsaved = true;
        backend.traffic_saved_at = 0;
        // Nothing runs, so this round adds nothing; the earlier traffic is written all the same.
        backend.save_traffic();
        assert!(!backend.traffic_unsaved);
        assert!(backend.traffic_path().exists());

        // A write that fails keeps the mark and the old time, so the next round tries again.
        let path = backend.traffic_path();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir_all(path.join("blocked")).unwrap();
        backend.traffic_unsaved = true;
        let before = backend.traffic_saved_at;
        backend.save_traffic();
        assert!(backend.traffic_unsaved);
        assert_eq!(backend.traffic_saved_at, before);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A user session whose window can come and go, recording what it was asked to put back.
    #[cfg(target_os = "linux")]
    struct WindowSession {
        window: std::sync::Arc<std::sync::atomic::AtomicBool>,
        restored: std::sync::Arc<std::sync::Mutex<Vec<system_proxy::ProxyBackup>>>,
    }

    #[cfg(target_os = "linux")]
    impl UserSession for WindowSession {
        fn proxy_settings(&self) -> Option<&'static str> {
            self.window
                .load(std::sync::atomic::Ordering::SeqCst)
                .then_some("GNOME")
        }
        fn apply_proxy(&self, _port: u16) -> Result<system_proxy::ProxyBackup, String> {
            Ok(old_backup())
        }
        fn restore_proxy(&self, backup: &system_proxy::ProxyBackup) -> Result<(), String> {
            if !self.window.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("no window".to_string());
            }
            self.restored.lock().unwrap().push(backup.clone());
            Ok(())
        }
        fn starts_after_login(&self) -> bool {
            false
        }
        fn set_start_after_login(&self, _enabled: bool) -> Result<(), String> {
            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    fn old_backup() -> system_proxy::ProxyBackup {
        system_proxy::ProxyBackup {
            desktop: system_proxy::Desktop::Gnome,
            previous_mode: "manual".to_string(),
            previous_keys: Vec::new(),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_backup_left_by_an_earlier_run_is_put_back_when_the_first_window_arrives() {
        let dir = std::env::temp_dir().join(format!(
            "anixops-net-backend-pending-restore-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::new(dir.join("config"));
        let mut state = SavedState::default();
        state.proxy_backup = Some(old_backup());
        store.save(&state).unwrap();
        let window = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let restored = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        // A service at boot: no window yet, so the backup cannot go back when the client opens.
        let mut backend = Backend::open(
            store.clone(),
            Box::new(WindowSession {
                window: window.clone(),
                restored: restored.clone(),
            }),
        );
        assert!(backend.proxy_restore_pending);
        assert!(restored.lock().unwrap().is_empty());

        // The engine is not running (auto-connect off); the window's arrival alone puts the settings back.
        window.store(true, std::sync::atomic::Ordering::SeqCst);
        backend.session_arrived();
        assert_eq!(*restored.lock().unwrap(), vec![old_backup()]);
        assert!(!backend.proxy_restore_pending);
        assert_eq!(store.load().unwrap().proxy_backup, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_proxy_whose_backup_cannot_be_saved_is_undone() {
        let (mut backend, dir) = backend("proxy-unsaved");
        let window = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let restored = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        backend.session = Arc::new(WindowSession {
            window,
            restored: restored.clone(),
        });
        backend.state_blocked = Some("blocked for the test".to_string());
        let error = backend.take_over_proxy(7890).unwrap_err();
        assert!(error.contains("was not changed"), "{error}");
        assert_eq!(*restored.lock().unwrap(), vec![old_backup()]);
        assert_eq!(backend.state.proxy_backup, None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
