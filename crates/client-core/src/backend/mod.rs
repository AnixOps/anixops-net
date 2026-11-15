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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationResult {
    pub message: String,
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

fn done(message: impl Into<String>) -> Result<OperationResult, String> {
    Ok(OperationResult {
        message: message.into(),
    })
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
    /// Seven days of traffic by hour, host and policy (`traffic_stats`).
    traffic: TrafficStats,
    traffic_counter: TrafficCounter,
    traffic_saved_at: u64,
    /// The notification switches (`notification_settings`).
    notifications: NotificationSettings,
    disconnect_watch: DisconnectWatch,
    /// The Wi-Fi rules (`on_demand_settings`); the platform samples the network (Android only).
    on_demand: OnDemandSettings,
    on_demand_watch: OnDemandWatch,
}

impl Backend {
    /// Loads the saved state and first puts back proxy settings a previous run left changed.
    pub fn open(store: Store, session: Box<dyn UserSession>) -> Self {
        let mut state = store.load();
        desktop_dns::custom::use_file(store.dir().join("dns-settings.json"));
        desktop_dns::redirects::use_file(store.dir().join("redirects.json"));
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
            traffic: TrafficStats::default(),
            traffic_counter: TrafficCounter::default(),
            traffic_saved_at: now(),
            notifications: NotificationSettings::default(),
            disconnect_watch: DisconnectWatch::default(),
            on_demand: OnDemandSettings::default(),
            on_demand_watch: OnDemandWatch::default(),
        };
        backend.traffic = TrafficStats::load(&backend.traffic_path());
        backend.notifications = NotificationSettings::load(&backend.notifications_path());
        backend.on_demand = OnDemandSettings::load(&backend.on_demand_path());
        backend.restore_proxy();
        backend.reload_nodes();
        backend
    }

    pub fn auto_connect(&self) -> bool {
        self.state.preferences.auto_connect
    }

    fn save(&mut self) {
        if let Err(error) = self.store.save(&self.state) {
            self.last_error = Some(format!("settings could not be saved: {error}"));
        }
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
                let nodes: Vec<NodeDescriptor> = engine::parse_nodes(&content)
                    .ok()?
                    .into_iter()
                    .filter(engine::is_supported)
                    .collect();
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
            Ok((content, nodes)) => {
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
                RuntimeConfig::from_json(envelope.as_bytes())
                    .map_err(|error| format!("the engine rejected the rules: {}", error.message))?;
                Some(envelope)
            }
            Err(_) => None,
        };
        desktop_dns::rules::save(&self.my_rules_path(), text)?;
        if !self.engine.is_running() {
            return done(format!("{} rules saved", lines.len()));
        }
        let reloaded = envelope
            .as_deref()
            .and_then(desktop_dns::rules::rules_of)
            .and_then(|rules| {
                self.engine
                    .control(&serde_json::json!({ "cmd": "reload_rules", "rules": rules }))
                    .ok()
            })
            .is_some_and(|answer| answer.get("reloaded") == Some(&serde_json::Value::Bool(true)));
        if reloaded {
            return done(format!(
                "{} rules saved and applied; open connections stay",
                lines.len()
            ));
        }
        self.restart_if_running()?;
        done(format!(
            "{} rules saved; the engine restarted with them",
            lines.len()
        ))
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
        done(if restarted {
            "Your root certificate is in use. Install it again on phones and other devices"
        } else {
            "Your root certificate is saved; it is used the next time HTTPS decryption runs"
        })
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
        self.save();
        let restarted = self.restart_if_running()?;
        done(match (enabled, restarted) {
            (true, true) => "HTTPS decryption is on",
            (true, false) => "HTTPS decryption is on; it starts with the next connection",
            (false, _) => "HTTPS decryption is off",
        })
    }

    /// Restarts a running engine after the plugin library changed, when it decrypts.
    pub fn plugins_changed(&mut self) -> Result<&'static str, String> {
        if !self.state.preferences.https_decryption {
            return Ok("turn on HTTPS decryption for it to take effect");
        }
        Ok(if self.restart_if_running()? {
            "applied to the running connection"
        } else {
            "it applies with the next connection"
        })
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
    fn restart_if_running(&mut self) -> Result<bool, String> {
        if !self.engine.is_running() {
            return Ok(false);
        }
        self.launch()?;
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
        let mut profile = self
            .routed()
            .ok_or("the current subscription has no policy groups")?;
        profile::set_group_default(&mut profile, group, member)?;
        self.state
            .group_choices
            .insert(group.to_string(), member.to_string());
        self.save();
        let label = self.node_name(member).unwrap_or_else(|| member.to_string());
        if self.restart_if_running()? {
            done(format!(
                "Policy group {group} now uses {label}; reconnected"
            ))
        } else {
            done(format!("Policy group {group} now uses {label}"))
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
        self.save();
        done(format!("Routing mode set to {}", mode.as_str()))
    }

    /// Switches the DNS choice. A running engine restarts on it.
    pub fn set_dns_preset(&mut self, preset: DnsPreset) -> Result<OperationResult, String> {
        let previous = self.state.dns_preset;
        self.state.dns_preset = preset;
        if let Err(error) = self.restart_if_running() {
            self.state.dns_preset = previous;
            return Err(error);
        }
        self.save();
        done(format!("DNS set to {}", preset.as_str()))
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
            return done("DNS settings saved; the engine restarted with them");
        }
        done(if in_use {
            "DNS settings saved"
        } else {
            "DNS settings saved; choose Custom to use them"
        })
    }

    /// Whether a connect opens the TUN device first (desktop TUN mode).
    pub fn tun_mode(&self) -> bool {
        self.state.tun_mode
    }

    /// Turns desktop TUN mode (Linux, Windows) on or off; only while disconnected, since the device is opened when
    /// connecting.
    pub fn set_tun_mode(&mut self, on: bool) -> Result<OperationResult, String> {
        if self.engine.is_running() {
            return Err("Disconnect before switching TUN mode".to_string());
        }
        self.state.tun_mode = on;
        self.save();
        done(if !on {
            "TUN mode is off; the next connection uses the system proxy"
        } else if cfg!(windows) {
            "TUN mode is on; the next connection carries every application"
        } else {
            "TUN mode is on; the next connection asks for the administrator password"
        })
    }

    /// Turns QUIC blocking on or off. A running engine restarts on it.
    pub fn set_block_quic(&mut self, on: bool) -> Result<OperationResult, String> {
        let previous = self.state.block_quic;
        self.state.block_quic = on;
        if let Err(error) = self.restart_if_running() {
            self.state.block_quic = previous;
            return Err(error);
        }
        self.save();
        done(if on {
            "QUIC is blocked"
        } else {
            "QUIC is allowed"
        })
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
        done(if restarted {
            "Redirects saved and applied"
        } else {
            "Redirects saved; they apply on the next connection"
        })
    }

    /// Runs debug mode's node-less profile (`on`) or the nodes and rules again. A running engine restarts on it.
    pub fn set_direct_egress(&mut self, on: bool) -> Result<OperationResult, String> {
        if self.state.direct_egress == on {
            return done("Egress unchanged");
        }
        let previous = self.state.direct_egress;
        self.state.direct_egress = on;
        if let Err(error) = self.restart_if_running() {
            self.state.direct_egress = previous;
            return Err(error);
        }
        self.save();
        done(if on {
            "Traffic leaves directly"
        } else {
            "Traffic leaves through the selected node"
        })
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
        let proxy = self
            .engine
            .address()
            .ok_or("connect first: replayed requests go through the running engine")?;
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
    pub fn record_response(&mut self, id: Option<&str>, meta: subscription_meta::ResponseMeta) {
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
            self.save();
        }
    }

    pub fn rename_subscription(&mut self, id: &str, name: &str) -> Result<OperationResult, String> {
        let name = subscription_meta::clean_name(name).ok_or("enter a name")?;
        self.source_mut(id)?.name = Some(name);
        self.save();
        // Linked policy groups are named after their subscriptions.
        self.reload_nodes();
        done("Subscription renamed")
    }

    pub fn set_subscription_interval(
        &mut self,
        id: &str,
        hours: u32,
    ) -> Result<OperationResult, String> {
        let hours = hours.min(subscription_meta::MAX_UPDATE_INTERVAL_HOURS);
        self.source_mut(id)?.update_interval_hours = Some(hours);
        self.save();
        if hours == 0 {
            done("Automatic updates are off for this subscription")
        } else {
            done(format!("This subscription updates every {hours} hours"))
        }
    }

    fn source_mut(&mut self, id: &str) -> Result<&mut SavedSource, String> {
        self.state
            .sources
            .iter_mut()
            .find(|source| source.id == id)
            .ok_or_else(|| "the subscription was not found".to_string())
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
    pub fn subscription_failed_notice(&self, id: &str, error: &str) -> Option<Notice> {
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
        let (label, address) = self.launch()?;
        // Through the TUN device every application is carried already; the system proxy stays as it is.
        if self.engine.address().is_none() {
            return done(format!("Connected through {label}"));
        }
        if self.state.preferences.system_proxy
            && self.state.proxy_backup.is_none()
            && self.session.proxy_settings().is_some()
        {
            match self.session.apply_proxy(address.port()) {
                Ok(backup) => {
                    self.state.proxy_backup = Some(backup);
                    self.save();
                }
                Err(error) => {
                    self.engine.stop();
                    return Err(format!("the system proxy could not be set: {error}"));
                }
            }
        }
        done(format!("Connected through {label} on {address}"))
    }

    /// Sets the system proxy now when the engine already runs and the user's session could not be reached
    /// when it started: a service that connects at boot has no window to ask until the user signs in.
    pub fn session_arrived(&mut self) {
        let Some(address) = self.engine.address() else {
            return;
        };
        if !self.state.preferences.system_proxy
            || self.state.proxy_backup.is_some()
            || self.session.proxy_settings().is_none()
        {
            return;
        }
        match self.session.apply_proxy(address.port()) {
            Ok(backup) => {
                self.state.proxy_backup = Some(backup);
                self.save();
            }
            Err(error) => {
                let error = format!("the system proxy could not be set: {error}");
                self.log(LogLevel::Error, &error);
                self.last_error = Some(error);
            }
        }
    }

    /// Stops the engine without touching the saved state or the last error; on Android this also lets go of
    /// the VPN's TUN descriptor.
    pub fn stop_engine(&mut self) {
        self.engine.stop();
    }

    pub fn disconnect(&mut self) -> Result<OperationResult, String> {
        self.engine.stop();
        self.disconnect_watch.disconnected();
        self.last_error = None;
        self.log(LogLevel::Info, "Disconnected");
        self.restore_proxy();
        done("Disconnected")
    }

    fn restore_proxy(&mut self) {
        if let Some(backup) = self.state.proxy_backup.clone() {
            match self.session.restore_proxy(&backup) {
                Ok(()) => {
                    self.state.proxy_backup = None;
                    self.save();
                }
                Err(error) => {
                    let error = format!("the system proxy could not be restored: {error}");
                    self.log(LogLevel::Error, &error);
                    self.last_error = Some(error);
                }
            }
        }
    }

    pub fn restore_network_settings(&mut self) -> Result<OperationResult, String> {
        if self.state.proxy_backup.is_none() {
            return done("The system proxy settings are not changed by this app");
        }
        self.restore_proxy();
        match &self.last_error {
            Some(error) if self.state.proxy_backup.is_some() => Err(error.clone()),
            _ => done("System proxy settings restored"),
        }
    }

    /// Live traffic and connections from the running engine.
    pub fn runtime_control(
        &self,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.engine.control(request)
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
        if self.traffic_counter.ingest(&list, &mut self.traffic, now)
            && now >= self.traffic_saved_at + 60
        {
            self.save_traffic();
        }
    }

    fn save_traffic(&mut self) {
        self.traffic_saved_at = now();
        let _ = self.traffic.save(&self.traffic_path());
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
    pub fn reset_network(&self, reset: network_watch::Reset) {
        if self.engine.is_running() {
            self.log(LogLevel::Info, "network changed: connections rebuilt");
        }
        let _ = self.engine.reset_network(reset);
    }

    /// Stops the engine and puts the proxy settings back; called when the window closes.
    pub fn shutdown(&mut self) {
        self.save_traffic();
        self.engine.stop();
        self.restore_proxy();
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
        self.save();
        self.restart_if_running()?;
        done(format!("Switched to {}", node.name))
    }

    /// Switches to a node of another source: through its linked policy group when the selected source's rules
    /// include it, else by selecting that source and its node.
    fn switch_to_other_source(&mut self, id: &str) -> Result<OperationResult, String> {
        let (other, node) = self
            .other_node(id)
            .ok_or("the node is not in any subscription")?;
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
                self.save();
                self.restart_if_running()?;
                return done(format!("Switched to {name}"));
            }
        }
        self.state.select_source(&source_id);
        self.state.selected_node = None;
        self.state.group_choices.clear();
        self.save();
        self.reload_nodes();
        // A routed subscription whose main group cannot pick the node still runs, on its own choices.
        self.switch_node(&node_id).or_else(|_| {
            self.restart_if_running()?;
            done(format!(
                "Switched to the subscription with {name}; its policy groups choose the node"
            ))
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
            .ok_or("the node is not in any subscription")?;
        engine::share_link(node)
    }

    /// What a window shows about one node of the selected source or of another source.
    pub fn node_details(&self, id: &str) -> Result<NodeDetails, String> {
        let own = self.nodes.iter().find(|node| node.id == id);
        let node = own
            .or_else(|| self.other_node(id).map(|(_, node)| node))
            .ok_or("the node is not in any subscription")?;
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
        node_edits::form(&self.nodes, profile.as_ref(), id)
            .ok_or_else(|| "only nodes of the selected subscription can be edited".to_string())
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
            return done("Node saved; the engine restarted with it");
        }
        done("Node saved")
    }

    /// Removes a node of the selected source until the subscription is updated. A running engine restarts on it.
    pub fn remove_node(&mut self, id: &str) -> Result<OperationResult, String> {
        if let Some(reason) = self.node_form(id)?.keep_reason {
            return Err(reason.to_string());
        }
        self.change_node_edits(|edits| edits.remove(id))?;
        if self.state.selected_node.as_deref() == Some(id) {
            self.state.selected_node = None;
            self.save();
        }
        if self.restart_if_running()? {
            return done("Node removed; the engine restarted without it");
        }
        done("Node removed")
    }

    fn change_node_edits(&mut self, change: impl FnOnce(&mut NodeEdits)) -> Result<(), String> {
        let source = self
            .state
            .selected_source()
            .ok_or("no subscription is selected")?
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
            return Err("none of the nodes can run on the embedded engine yet".to_string());
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
                self.save();
                self.reload_nodes();
                return done("These links were imported before");
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
        self.save();
        self.reload_nodes();
        let message = format!("Imported {supported} of {} nodes", nodes.len());
        self.log(LogLevel::Info, &message);
        done(message)
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

    /// Records a refresh of source `id`: the new content, or why it failed.
    pub fn finish_refresh(
        &mut self,
        id: &str,
        fetched: Result<String, String>,
    ) -> Result<OperationResult, String> {
        let time = now();
        let parsed =
            fetched.and_then(|content| engine::parse_nodes(&content).map(|nodes| (content, nodes)));
        let result = match parsed {
            // An update brings back what the provider serves (`node_edits`).
            Ok((content, nodes)) => self
                .remove_node_edits(id)
                .and_then(|()| self.store.write_source(id, &content))
                .map(|()| nodes.len())
                .map_err(|error| format!("the subscription could not be saved: {error}")),
            Err(error) => Err(error),
        };
        if let Some(source) = self.state.sources.iter_mut().find(|source| source.id == id) {
            source.last_attempt = Some(time);
            match &result {
                Ok(count) => {
                    source.last_success = Some(time);
                    source.last_error = None;
                    source.node_count = Some(*count);
                }
                Err(error) => source.last_error = Some(error.clone()),
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
        self.save();
        self.reload_nodes();
        result.map(|count| OperationResult {
            message: format!("Subscription updated: {count} nodes"),
        })
    }

    pub fn select_subscription(&mut self, id: &str) -> Result<OperationResult, String> {
        if !self.state.select_source(id) {
            return Err("the subscription was not found".to_string());
        }
        self.state.selected_node = None;
        self.state.group_choices.clear();
        self.save();
        self.reload_nodes();
        done("Subscription selected; reconnect to use it")
    }

    pub fn remove_subscription(&mut self, id: &str) -> Result<OperationResult, String> {
        let was_selected = self
            .state
            .selected_source()
            .is_some_and(|source| source.id == id);
        if !self.state.remove_source(id) {
            return Err("the subscription was not found".to_string());
        }
        if was_selected {
            self.state.group_choices.clear();
        }
        self.store.remove_source(id);
        let _ = self.remove_node_edits(id);
        self.save();
        self.reload_nodes();
        done("Subscription removed")
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
        self.save();
        done("Settings saved")
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
            return Err("no node the embedded engine can run".to_string());
        }
        done(format!(
            "{supported} of {} nodes can run on the embedded engine",
            self.nodes.len()
        ))
    }

    /// Writes a report without node addresses, names or credentials. `health` is the client health section
    /// (integrity, cold start, redacted crash reports), appended as it is.
    pub fn diagnostics(&self, health: &str) -> Result<OperationResult, String> {
        let report = self.diagnostics_report(health);
        let path = self
            .store
            .write_report(&format!("diagnostics-{}.log", now()), &report)
            .map_err(|error| format!("the report could not be written: {error}"))?;
        done(format!("Diagnostics report saved to {}", path.display()))
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
        assert!(backend.save_my_rules("GEOSITE,cn,DIRECT").is_err());
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
        assert_eq!(
            backend.plugins_changed().unwrap(),
            "turn on HTTPS decryption for it to take effect"
        );

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
        assert_eq!(
            backend.plugins_changed().unwrap(),
            "it applies with the next connection"
        );
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
        assert_eq!(RoutingMode::parse("bypass_china"), None);
        assert_eq!(RoutingMode::parse("gfw_list"), None);
        assert_eq!(RoutingMode::parse("return_china"), None);
        backend.set_routing_mode(RoutingMode::Direct).unwrap();
        assert_eq!(backend.snapshot().routing_mode, "direct");
        let direct: serde_json::Value = serde_json::from_str(&backend.envelope().unwrap()).unwrap();
        assert_eq!(
            direct["routing"]["rules"],
            serde_json::json!(["FINAL,DIRECT"])
        );
        backend.set_routing_mode(RoutingMode::Rule).unwrap();

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
}
