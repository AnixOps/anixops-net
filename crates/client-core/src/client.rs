//! The client service behind every window that drives this backend, free of Tauri: one [`Client`] answers the
//! interface's commands by name ([`Client::invoke`], names and JSON shapes as in the shared interface's
//! `bridge.ts`) and runs the background work (subscription updates, Wi-Fi rules, traffic counting, network
//! changes). The Tauri window (`app.rs`) forwards its IPC here; the native Android app calls the same commands
//! through JNI, so both read one contract.
//!
//! What the platform does itself (the system VPN, the shared Downloads folder, the key store, the current
//! network, system notifications) sits behind [`Platform`].

mod agent;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use app_notifications::{Notice, Settings as NotificationSettings};
use client_health::Health;
use desktop_mitm::mappings;
use desktop_mitm::plugins;
use engine_embedded::checkup::{CheckupOptions, CheckupReport};
use on_demand::{Action as OnDemandAction, Settings as OnDemandSettings};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use subscription_meta::ResponseMeta;

use crate::backend::engine;
use crate::backend::engine::NetworkFacts;
use crate::backend::session::{LocalSession, UserSession};
use crate::backend::state::{RoutingMode, Store};
use crate::backend::system_proxy::Desktop;
#[cfg(not(target_os = "android"))]
use crate::backend::trust;
use crate::backend::{
    autostart, format_delay, Backend, DnsPreset, OperationResult, PreferencesInput, PASTED_LOCATION,
};
use crate::refusal;

/// The network a [`Platform`] reports.
pub use on_demand::Network;

/// The URL the delay tests fetch, as on Windows.
const DELAY_URL: &str = "https://www.gstatic.com/generate_204";
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// The network watch reads the Wi-Fi name every this many samples (`network_watch::POLL` apart) besides on a
/// path change.
const NETWORK_READ_EVERY: u32 = 10;
const UNAVAILABLE: &str = if cfg!(target_os = "android") {
    "Not available on Android yet"
} else {
    "Not available on Linux yet"
};
/// A subscription from a file or pasted links has no link to update from.
const NOT_UPDATABLE: &str = "error.not_updatable";

const NO_DOWNLOADS: &str = "this platform has no shared Downloads folder";

/// What the LAN share commands answer where there is no share (Android).
#[cfg(target_os = "android")]
const LAN_SHARE_UNAVAILABLE: &str = "LAN share is not available on this platform";

/// Commands the Windows client has and this backend does not; the interface hides their controls.
const WINDOWS_ONLY: &[&str] = &[
    "configure_script_runtime",
    "clear_script_runtime",
    "install_certificate",
    "remove_certificate",
    "install_driver",
    "remove_driver",
];

/// Addresses, routes, DNS and MTU of the VPN interface, as the iOS client's `TunnelInterface.proxy`: the
/// userspace stack answers on any address, and DNS to 198.18.0.2 is answered by the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInterface {
    pub addresses: Vec<String>,
    pub routes: Vec<String>,
    pub dns_servers: Vec<String>,
    pub mtu: u32,
}

impl TunnelInterface {
    pub fn proxy() -> Self {
        let owned = |values: &[&str]| values.iter().map(|value| value.to_string()).collect();
        Self {
            addresses: owned(&["198.18.0.1/24", "fd00:4e43::1/64"]),
            routes: owned(&["0.0.0.0/0", "::/0"]),
            dns_servers: owned(&["198.18.0.2"]),
            mtu: 1500,
        }
    }
}

/// What the platform does for the client. The defaults are a desktop without a system VPN, a key store or a
/// network reader; every method may be called from any thread.
pub trait Platform: Send + Sync + 'static {
    /// Whether traffic goes through a system VPN ([`Platform::open_tunnel`]) instead of a local proxy.
    fn has_system_vpn(&self) -> bool {
        false
    }

    /// Opens the system VPN and hands back its TUN descriptor (Android's VpnService). May wait for the user to
    /// allow the VPN.
    fn open_tunnel(&self, interface: &TunnelInterface) -> Result<i32, String> {
        let _ = interface;
        Err("this platform has no system VPN".to_string())
    }

    /// Closes the system VPN again.
    fn close_tunnel(&self) {}

    /// Closes the system VPN while the Wi-Fi rules are on, which may have to connect again by themselves on the
    /// next network: the platform keeps what lets them (Android: the VPN service stays in the foreground,
    /// waiting for the network, so the process is neither frozen nor stopped). As [`Platform::close_tunnel`]
    /// by default.
    fn close_tunnel_for_rules(&self) {
        self.close_tunnel();
    }

    /// Puts a text file where the user can open it from the system settings (Android: Downloads) and returns
    /// where it went.
    fn save_to_downloads(&self, name: &str, contents: &str) -> Result<String, String> {
        let _ = (name, contents);
        Err(NO_DOWNLOADS.to_string())
    }

    /// As [`Platform::save_to_downloads`], but a file of the same name the app saved earlier is replaced instead
    /// of kept next to the new one (the decryption root, saved again each time decryption is turned on).
    fn replace_in_downloads(&self, name: &str, contents: &str) -> Result<String, String> {
        self.save_to_downloads(name, contents)
    }

    /// Puts a binary file, given as base64, in the same place as [`Platform::save_to_downloads`] and returns
    /// where it went.
    fn save_base64_to_downloads(
        &self,
        name: &str,
        mime_type: &str,
        data_base64: &str,
    ) -> Result<String, String> {
        let _ = (name, mime_type, data_base64);
        Err(NO_DOWNLOADS.to_string())
    }

    /// The key that seals saved capture sessions, as base64, kept by the platform's key store (Android: the
    /// Keystore), made on first use.
    fn capture_session_key(&self) -> Result<String, String> {
        Err("this platform keeps the capture session key itself".to_string())
    }

    /// The network the device is on, for the Wi-Fi rules; `ask` asks for the permission that reveals the
    /// Wi-Fi name when it is withheld. `None` where the platform does not read it (Linux desktop).
    fn current_network(&self, ask: bool) -> Option<Network> {
        let _ = ask;
        None
    }

    /// Shows one system notification; one the system refuses is dropped.
    fn post_notice(&self, notice: &Notice) {
        let _ = notice;
    }
}

/// Where the client keeps its data and which program autostart starts.
pub struct Config {
    pub dir: PathBuf,
    pub executable: PathBuf,
    /// The client runs in a system service or daemon (as SYSTEM or root) that windows of other users drive,
    /// not in the user's own process. It then opens no path a window names (`import_subscription`): the
    /// window reads the file as the user and sends its content (`import_subscription_text`).
    pub shared_service: bool,
}

/// The backend, its health record and what the commands keep between calls.
pub struct Client {
    backend: Arc<Mutex<Backend>>,
    health: Arc<Health>,
    platform: Arc<dyn Platform>,
    /// The capture session key once read, so the key store is asked once per run.
    session_key: Mutex<Option<desktop_mitm::sessions::SessionKey>>,
    /// `anixops://` links the interface has not read yet (`take_app_links`).
    links: Mutex<Vec<String>>,
    /// What each node's last delay test said (`None`: no answer), by node id. Kept for as long as the core
    /// runs, so that every window shows the same delays and a window that is opened again still has them.
    delays: Mutex<HashMap<String, Option<u64>>>,
    /// See [`Config::shared_service`].
    shared_service: bool,
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Client {
    /// Opens the saved state, starts the health record and the metrics endpoint, connects when the user asked
    /// for that at start (only without a system VPN, which needs the user's consent in the window) and watches
    /// the network. Call [`Client::start_background`] once the platform can post notifications.
    pub fn open(config: Config, platform: Arc<dyn Platform>) -> Arc<Self> {
        let session = LocalSession::new(
            Desktop::from_env(),
            // Android has no login entries; the Windows one is not an XDG autostart file.
            if cfg!(any(target_os = "android", windows)) {
                None
            } else {
                autostart::autostart_dir()
            },
            config.executable.clone(),
        );
        Self::open_with_session(config, platform, Box::new(session))
    }

    /// As [`Client::open`], for a client that does not run in the user's own session (a system service):
    /// `session` reaches the user's proxy settings and login entry for it.
    pub fn open_with_session(
        config: Config,
        platform: Arc<dyn Platform>,
        session: Box<dyn UserSession>,
    ) -> Arc<Self> {
        let Config {
            dir,
            executable,
            shared_service,
        } = config;
        let health = Health::start(
            &dir,
            crate::backend::PLATFORM,
            env!("CARGO_PKG_VERSION"),
            std::env::current_exe().unwrap_or_else(|_| executable.clone()),
        );
        // Sessions expire after a week even when the engine does not run to delete them.
        desktop_mitm::sessions::purge(
            &crate::backend::capture_dir(&dir).join("sessions"),
            desktop_mitm::sessions::now_ms(),
        );
        let backend = Backend::open(Store::new(dir), session);
        let auto_connect = backend.auto_connect();
        let shared = Arc::new(Mutex::new(backend));
        // The endpoint skips the engine while another command holds the backend, rather than waiting on it.
        let probe = Arc::downgrade(&shared);
        health.serve_metrics(Arc::new(move || {
            let Some(backend) = probe.upgrade() else {
                return (None, false);
            };
            let guard = backend.try_lock();
            match guard {
                Ok(backend) => backend.metrics(),
                Err(_) => (None, false),
            }
        }));
        let client = Arc::new(Self {
            backend: shared,
            health,
            platform,
            session_key: Mutex::new(None),
            links: Mutex::new(Vec::new()),
            delays: Mutex::new(HashMap::new()),
            shared_service,
        });
        if auto_connect && !client.platform.has_system_vpn() {
            let starting = client.clone();
            thread::spawn(move || {
                // TUN mode asks for the password at login; refused, the client stays disconnected.
                let _ = starting.connect_through_tunnel();
            });
        }
        client.start_network_watch();
        client
    }

    /// Starts the subscription updates, the Wi-Fi rules (where the platform reads the network) and the monitor
    /// that counts traffic and posts notifications.
    pub fn start_background(self: &Arc<Self>) {
        self.start_subscription_updates();
        self.start_on_demand();
        self.start_monitor();
    }

    /// Call when the user's session becomes reachable after the client was opened (a window connected to
    /// the service): what was waiting for it, the system proxy of a connection made earlier, is done now.
    pub fn session_arrived(&self) {
        self.backend().session_arrived();
    }

    /// Puts the desktop's proxy settings back; call when the app exits.
    pub fn shutdown(&self) {
        self.backend().shutdown();
    }

    /// Keeps the `anixops://` links among `urls` for the interface; answers whether there were any, so the
    /// caller can bring the window forward and tell the interface to read them.
    pub fn receive_app_links(&self, urls: Vec<String>) -> bool {
        let urls: Vec<String> = urls
            .into_iter()
            .filter(|url| url.to_ascii_lowercase().starts_with("anixops:"))
            .collect();
        if urls.is_empty() {
            return false;
        }
        lock(&self.links).extend(urls);
        true
    }

    fn backend(&self) -> MutexGuard<'_, Backend> {
        lock(&self.backend)
    }

    /// Answers one interface command: `args` is the command's argument object (`{}` or `null` when it takes
    /// none), the answer what the interface expects back. Blocks, sometimes for long (downloads, delay tests,
    /// the VPN consent dialog): call it off the UI thread.
    pub fn invoke(&self, command: &str, args: Value) -> Result<Value, String> {
        let args = if args.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            args
        };
        // What every window decides the same way (`view.` commands) needs nothing of this client.
        if let Some(answer) = client_view::invoke(command, &args) {
            return answer;
        }
        match handler(command) {
            Some(handler) => handler(self, args),
            None if WINDOWS_ONLY.contains(&command) => Err(UNAVAILABLE.to_string()),
            None => Err(format!("unknown command {command}")),
        }
    }

    /// Where the platform carries traffic through a system VPN, or the desktop is in TUN mode, opens it
    /// first so the engine has its TUN device.
    fn open_tunnel(&self) -> Result<(), String> {
        if self.platform.has_system_vpn() {
            return self.open_system_tunnel();
        }
        if !self.backend().tun_mode() {
            return Ok(());
        }
        self.open_helper_tunnel()
    }

    /// Linux desktop TUN mode: the polkit helper makes the device (the desktop asks for the administrator
    /// password) and the engine runs on it. A device already open is kept.
    #[cfg(target_os = "linux")]
    fn open_helper_tunnel(&self) -> Result<(), String> {
        if engine::tunnel_attached() {
            return Ok(());
        }
        self.backend().stop_engine();
        let fd = crate::backend::tun_helper::open(TunnelInterface::proxy().mtu)?;
        engine::attach_tunnel(fd);
        Ok(())
    }

    /// Windows TUN mode: nothing to open ahead, the engine makes its own Wintun adapter when it starts
    /// (`backend::tun`). The core runs in the system service, which may create one.
    #[cfg(windows)]
    fn open_helper_tunnel(&self) -> Result<(), String> {
        if engine::tunnel_attached() {
            return Ok(());
        }
        self.backend().stop_engine();
        engine::attach_tunnel();
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn open_helper_tunnel(&self) -> Result<(), String> {
        Err(UNAVAILABLE.to_string())
    }

    /// Keeps a live descriptor; one whose VPN was taken away (another VPN, or the system settings) is dropped
    /// and the VPN opened again.
    #[cfg(target_os = "android")]
    fn open_system_tunnel(&self) -> Result<(), String> {
        if engine::tunnel_attached() {
            return Ok(());
        }
        self.backend().stop_engine();
        let fd = self.platform.open_tunnel(&TunnelInterface::proxy())?;
        engine::attach_tunnel(fd);
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    fn open_system_tunnel(&self) -> Result<(), String> {
        Err("this platform has no system VPN".to_string())
    }

    /// Stops the engine and closes the system VPN, if the platform has one, or the desktop's TUN mode device.
    fn close_tunnel(&self) {
        if self.platform.has_system_vpn() {
            let rules_on = {
                let mut backend = self.backend();
                backend.stop_engine();
                backend.on_demand_enabled()
            };
            if rules_on {
                self.platform.close_tunnel_for_rules();
            } else {
                self.platform.close_tunnel();
            }
        } else if self.backend().tun_mode() {
            self.backend().stop_engine();
        }
    }

    /// Connects, through the system VPN where the platform has one; a failed start closes that VPN again.
    fn connect_through_tunnel(&self) -> Result<OperationResult, String> {
        if let Err(error) = self.open_tunnel() {
            self.backend().record_error(&error);
            return Err(error);
        }
        let result = self.backend().connect();
        if result.is_err() {
            self.close_tunnel();
        }
        result
    }

    /// Saved capture sessions: list, save what capture holds, load one for viewing, delete. The engine is told
    /// where sessions go and the key before every action, since it forgets both when it restarts.
    fn capture_session(
        &self,
        action: desktop_mitm::sessions::SessionAction,
    ) -> Result<Value, String> {
        let request = action.request()?;
        let capture_dir = self.backend().capture_dir();
        let key = self.session_key(&capture_dir)?;
        let storage = desktop_mitm::sessions::storage_request(&capture_dir.join("sessions"), &key);
        let protection = key.protection;
        // Not `runtime_control`: these are not window commands, and its list refuses them.
        let answer = {
            let backend = self.backend();
            // Both are the backend's own requests: the window's command list leaves them out on purpose.
            backend.engine_request(&storage)?;
            backend.engine_request(&request)?
        };
        Ok(desktop_mitm::sessions::with_protection(answer, protection))
    }

    /// The capture session key, read from the platform's key store once per run. Never called with the backend
    /// locked: the key store can be slow or ask the user.
    fn session_key(
        &self,
        capture_dir: &Path,
    ) -> Result<desktop_mitm::sessions::SessionKey, String> {
        if let Some(key) = lock(&self.session_key).clone() {
            return Ok(key);
        }
        let key = self.load_session_key(capture_dir)?;
        *lock(&self.session_key) = Some(key.clone());
        Ok(key)
    }

    /// A window command for the engine. Turning capture on first hands the backend the session key, read here
    /// outside the backend's lock, so a restart or a disconnect can save what capture holds as a session.
    fn runtime_control(&self, request: &Value) -> Result<Value, String> {
        let turns_capture_on = request.get("cmd").and_then(Value::as_str) == Some("set_capture")
            && request.get("enabled").and_then(Value::as_bool) == Some(true);
        if turns_capture_on {
            let capture_dir = self.backend().capture_dir();
            match self.session_key(&capture_dir) {
                Ok(key) => self.backend().remember_capture_key(key),
                // Capture still turns on; it is only not saved when the engine stops.
                Err(error) => self.backend().log(
                    app_log::Level::Warn,
                    &format!("the capture session key could not be read: {error}"),
                ),
            }
        }
        // The share starts with the kept credentials, never with what the window put in (`desktop_mitm::phone`).
        if desktop_mitm::phone::is_start(request) {
            let auth = self.lan_share_auth()?;
            let request = desktop_mitm::phone::authorized_start(request, &auth)?;
            return self.backend().runtime_control(&request);
        }
        self.backend().runtime_control(request)
    }

    /// The LAN share's proxy credentials, made and kept on first use. Read outside the backend's lock: the
    /// key store can be slow or ask the user.
    #[cfg(not(target_os = "android"))]
    fn lan_share_auth(&self) -> Result<desktop_mitm::phone::LanShareAuth, String> {
        let dir = self.backend().lan_share_dir();
        crate::backend::lan_share_auth::load(&dir)
    }

    /// Android offers no LAN share, so it keeps no credentials and starts none.
    #[cfg(target_os = "android")]
    fn lan_share_auth(&self) -> Result<desktop_mitm::phone::LanShareAuth, String> {
        Err(LAN_SHARE_UNAVAILABLE.to_string())
    }

    /// New LAN share credentials, or the switch that requires them changed.
    #[cfg(not(target_os = "android"))]
    fn change_lan_share_auth(
        &self,
        required: Option<bool>,
    ) -> Result<desktop_mitm::phone::LanShareAuth, String> {
        let dir = self.backend().lan_share_dir();
        match required {
            Some(required) => crate::backend::lan_share_auth::set_required(&dir, required),
            None => crate::backend::lan_share_auth::regenerate(&dir),
        }
    }

    #[cfg(target_os = "android")]
    fn change_lan_share_auth(
        &self,
        _required: Option<bool>,
    ) -> Result<desktop_mitm::phone::LanShareAuth, String> {
        Err(LAN_SHARE_UNAVAILABLE.to_string())
    }

    /// Android: the Keystore keeps the key, through the platform.
    #[cfg(target_os = "android")]
    fn load_session_key(&self, _dir: &Path) -> Result<desktop_mitm::sessions::SessionKey, String> {
        let key = desktop_mitm::capture::decode_base64(&self.platform.capture_session_key()?)?;
        desktop_mitm::sessions::SessionKey::new(&key, desktop_mitm::sessions::Protection::Keystore)
    }

    /// Linux: the Secret Service, or a file only this user can read.
    #[cfg(not(target_os = "android"))]
    fn load_session_key(&self, dir: &Path) -> Result<desktop_mitm::sessions::SessionKey, String> {
        crate::backend::session_key::load(dir)
    }

    /// Exports the packet capture (`format` zip, pcapng or har; one connection with `id`, else all) to Downloads
    /// without replacing an older file, and answers where it went.
    fn export_capture(
        &self,
        format: &str,
        id: Option<u64>,
        options: desktop_mitm::capture::ExportOptions,
    ) -> Result<SavedFile, String> {
        let request = desktop_mitm::capture::export_request(format, id, options);
        let answer = self.backend().runtime_control(&request)?;
        let export = desktop_mitm::capture::Export::from_answer(&answer)?;
        let location = self.save_export(&export)?;
        Ok(SavedFile { location })
    }

    /// Android keeps files the user opens elsewhere in the shared Downloads folder, which only the platform can
    /// write.
    #[cfg(target_os = "android")]
    fn save_export(&self, export: &desktop_mitm::capture::Export) -> Result<String, String> {
        self.platform.save_base64_to_downloads(
            &export.file_name,
            &export.content_type,
            &export.data_base64,
        )
    }

    /// Saves a body as the inspector shows it (decoded) where exports go, and answers where it went.
    fn save_body(&self, file: &desktop_mitm::capture::BodyFile) -> Result<SavedFile, String> {
        let name = desktop_mitm::capture::body_file_name(&file.name, &file.content_type);
        let export = desktop_mitm::capture::Export {
            file_name: name,
            content_type: file.content_type.clone(),
            data_base64: file.data_base64.clone(),
        };
        let location = self.save_export(&export)?;
        Ok(SavedFile { location })
    }

    #[cfg(not(target_os = "android"))]
    fn save_export(&self, export: &desktop_mitm::capture::Export) -> Result<String, String> {
        let dir = desktop_mitm::capture::downloads_dir()
            .ok_or("the Downloads folder could not be found")?;
        let path = desktop_mitm::capture::save_new_file(&dir, &export.file_name, &export.bytes()?)?;
        Ok(path.display().to_string())
    }

    /// The exit address and the direct delay for the overview; `connected` says whether the engine is up.
    fn network_probe(&self, connected: bool) -> NetworkProbe {
        let (port, envelope) = {
            let backend = self.backend();
            (backend.listen_port(), backend.probe_envelope())
        };
        match envelope.filter(|_| connected) {
            // Android: the exit through a short-lived local proxy on the running configuration.
            Some(envelope) => {
                engine::through_temporary_proxy(&envelope, |port| probe_network(Some(port)))
                    .unwrap_or_else(|_| probe_network(None))
            }
            None => probe_network(connected.then_some(port)),
        }
    }

    /// The node list as the interface gets it from `list_nodes`.
    fn node_rows(&self) -> Result<Vec<Value>, String> {
        self.backend().nodes().into_iter().map(json).collect()
    }

    /// Keeps what a round of delay tests said, over what was known about those nodes.
    fn remember_delays(&self, results: &[NodeDelay]) {
        let mut delays = lock(&self.delays);
        for result in results {
            delays.insert(result.node_id.clone(), result.delay_ms);
        }
    }

    /// The delays known so far, in the shape `client-view` reads: `{ id: { "delayMs": … } }`, without
    /// `delayMs` for a test that got no answer.
    fn known_delays(&self) -> Value {
        lock(&self.delays)
            .iter()
            .map(|(id, delay)| {
                let entry = match delay {
                    Some(delay) => serde_json::json!({ "delayMs": delay }),
                    None => serde_json::json!({}),
                };
                (id.clone(), entry)
            })
            .collect::<serde_json::Map<String, Value>>()
            .into()
    }

    fn select_fastest_node(&self) -> Result<OperationResult, String> {
        let nodes = self.backend().node_list();
        let (id, delay) = engine::fastest_node(&nodes, DELAY_URL).ok_or_else(|| {
            refusal::coded("error.no_delay_answer", "no node answered the delay test")
        })?;
        let switched = self.backend().switch_node(&id)?;
        // Switching to a node of another subscription whose policy groups choose names that subscription.
        let node = switched
            .params
            .get("node")
            .or_else(|| switched.params.get("name"))
            .cloned()
            .unwrap_or_default();
        Ok(OperationResult::coded(
            "fastest.selected_delay",
            format!("{} ({})", switched.message, format_delay(delay)),
        )
        .with("node", node)
        .with("delay", format_delay(delay)))
    }

    fn create_diagnostics(&self) -> Result<SavedFileResult, String> {
        let section = self.health.diagnostics();
        // Android: the app's own directory is out of the user's reach, so the report goes to Downloads.
        if self.platform.has_system_vpn() {
            let report = self.backend().diagnostics_report(&section);
            let name = format!("anixops-net-diagnostics-{}.log", unix_seconds());
            let saved = self.platform.save_to_downloads(&name, &report)?;
            return Ok(SavedFileResult {
                message: format!("Diagnostics report saved to {saved}"),
                code: Some("diagnostics.saved"),
                params: [("path", saved.clone())].into_iter().collect(),
                location: Some(saved),
                ..SavedFileResult::default()
            });
        }
        self.backend()
            .diagnostics(&section)
            .map(|result| SavedFileResult {
                message: result.message,
                code: result.code,
                params: result.params,
                ..SavedFileResult::default()
            })
    }

    fn set_crash_reports(&self, enabled: bool) -> Result<OperationResult, String> {
        self.health
            .set_crash_reports(enabled)
            .map_err(|error| format!("the setting could not be saved: {error}"))?;
        Ok(if enabled {
            OperationResult::coded("crash_reports.on", "Crash reports on")
        } else {
            OperationResult::coded(
                "crash_reports.off",
                "Crash reports off; kept reports deleted",
            )
        })
    }

    /// Turns the local Prometheus endpoint on or off (off by default).
    fn set_metrics(&self, enabled: bool) -> Result<OperationResult, String> {
        self.health
            .set_metrics(enabled)
            .map_err(|error| format!("the setting could not be saved: {error}"))?;
        Ok(if enabled {
            OperationResult::coded("metrics.on", "Prometheus metrics on")
        } else {
            OperationResult::coded("metrics.off", "Prometheus metrics off")
        })
    }

    fn import_subscription(&self, location: &str) -> Result<OperationResult, String> {
        let location = location.trim();
        if location.is_empty() {
            return Err(refusal::coded(
                "error.import_input_required",
                "enter a subscription link, share links or a file path",
            ));
        }
        let label = if location.contains("://") && !location.starts_with("http") {
            PASTED_LOCATION.to_string()
        } else {
            location.to_string()
        };
        let (content, meta) = fetch(location, !self.shared_service)?;
        let mut backend = self.backend();
        let result = backend.import(&label, &content)?;
        backend.record_response(None, meta)?;
        Ok(result)
    }

    /// Imports a file the window read as the user (`import_subscription_text`): a core in a system service
    /// never opens a path a window names. `name` is the file's name, kept as the source's label; the source
    /// cannot be updated, as one imported from a path could not.
    fn import_subscription_text(
        &self,
        name: &str,
        content: &str,
    ) -> Result<OperationResult, String> {
        if content.len() as u64 > MAX_SUBSCRIPTION_FILE_BYTES {
            return Err(too_large());
        }
        self.backend().import(&file_label(name), content)
    }

    /// Downloads source `id` again outside the lock and records the result.
    fn refresh_source(&self, id: &str, location: &str) -> Result<OperationResult, String> {
        // Only links are updated: a source from a file or pasted links has no location to read again.
        let (content, meta) = match fetch(location, false) {
            Ok((content, meta)) => (Ok(content), Some(meta)),
            Err(error) => (Err(error), None),
        };
        let mut backend = self.backend();
        let result = backend.finish_refresh(id, content)?;
        if let Some(meta) = meta {
            backend.record_response(Some(id), meta)?;
        }
        Ok(result)
    }

    /// Changes the plugin library outside the lock, then applies it to a running engine. `change` answers what
    /// was changed, coded; the window words that, then what it did to the connection (`follow`).
    fn change_plugins<F>(&self, change: F) -> Result<OperationResult, String>
    where
        F: FnOnce(&Path) -> Result<OperationResult, String>,
    {
        let directory = self.backend().plugins_dir();
        let changed = change(&directory)?;
        let applied = self.backend().plugins_changed()?;
        Ok(OperationResult {
            message: format!("{}; {}", changed.message, applied.message),
            follow: applied.code,
            note: applied.note,
            ..changed
        })
    }

    /// What importing `plugin` did. A new plugin is saved off and changes nothing the engine runs; one that
    /// replaced a plugin which was on is applied like any other change.
    fn plugin_imported(&self, plugin: &plugins::PluginSummary) -> Result<OperationResult, String> {
        let result = plugin_imported(plugin);
        if plugin.enabled || plugin.replaced_one_on {
            return self.change_plugins(move |_| Ok(result));
        }
        Ok(result)
    }

    /// Generates the root on first use and restarts a running engine with decryption on. The root is not
    /// trusted here: the user does that with `open_mitm_root` (TERMS.md section 4).
    #[cfg(not(target_os = "android"))]
    fn enable_https_mitm(&self) -> Result<OperationResult, String> {
        let root = self.backend().ensure_mitm_root()?;
        let trusted = self.root_trusted(&root);
        let result = self.backend().set_https_decryption(true)?;
        if trusted {
            return Ok(result);
        }
        // The window words what turning it on did (now, or with the next connection), then what is still to do.
        Ok(result.followed_by(
            "mitm.root_untrusted",
            "The root certificate is not trusted yet: choose Trust root certificate under Extensions",
        ))
    }

    /// Android has no trust store an app may change: the root is saved to Downloads, the user installs it in
    /// the system settings, and only apps that trust user certificates (most browsers) accept it.
    ///
    /// The file is named after the root's fingerprint, so a new root (after the app's data was cleared) never
    /// takes the name of an old one left in Downloads, and saving the same root again replaces its file. The
    /// answer carries where it went (`location`), the fingerprint the user can compare in the system's list of
    /// certificates, and why it was not saved (`saveError`), for the app to say in its own language.
    #[cfg(target_os = "android")]
    fn enable_https_mitm(&self) -> Result<SavedFileResult, String> {
        let root = self.backend().ensure_mitm_root()?;
        let fingerprint = root.fingerprint().ok();
        let saved = fs::read_to_string(&root.certificate_path)
            .map_err(|error| format!("the root certificate could not be read: {error}"))
            .and_then(|pem| {
                let name = root_download_name(fingerprint.as_deref().unwrap_or_default());
                self.platform.replace_in_downloads(&name, &pem)
            });
        let result = self.backend().set_https_decryption(true)?;
        let install = match &saved {
            Ok(location) => format!(
                "The root certificate is saved as {location}; install it under Settings > Security > \
                 Encryption & credentials > Install a certificate > CA certificate"
            ),
            Err(error) => {
                format!("The root certificate could not be saved to Downloads: {error}")
            }
        };
        let (location, save_error) = match saved {
            Ok(location) => (Some(location), None),
            Err(error) => (None, Some(error)),
        };
        Ok(SavedFileResult {
            message: format!(
                "{}. {install}. Apps that ignore user certificates are not decrypted",
                result.message
            ),
            location,
            fingerprint,
            save_error,
            ..SavedFileResult::default()
        })
    }

    /// Whether the root exists and a browser database or the system store holds it. Read only.
    #[cfg(not(target_os = "android"))]
    fn mitm_root_status(&self) -> Result<MitmRootStatus, String> {
        let root = self.backend().mitm_root();
        let generated = root.exists();
        let trusted = generated && self.root_trusted(&root);
        Ok(MitmRootStatus { generated, trusted })
    }

    /// Android: the user installs the root in the system settings; there is nothing to read or open here.
    #[cfg(target_os = "android")]
    fn mitm_root_status(&self) -> Result<MitmRootStatus, String> {
        Err(UNAVAILABLE.to_string())
    }

    /// Whether the root is trusted: in the user's own store where the session keeps one (the core runs in
    /// a service and the window answers), otherwise in a browser database or the system store.
    #[cfg(not(target_os = "android"))]
    fn root_trusted(&self, root: &desktop_mitm::Root) -> bool {
        // The session is asked without the backend held: its window may be busy with the user.
        let session = self.backend().session();
        let Ok(pem) = fs::read_to_string(&root.certificate_path) else {
            return false;
        };
        session
            .trusts_root(&pem)
            .unwrap_or_else(|| trust::is_trusted(&pem))
    }

    /// The user's own request to trust the root. Where the session keeps the user's store, the window adds
    /// it there and the system asks the user to confirm; otherwise the browsers' databases and, after
    /// polkit asks, the system store.
    #[cfg(not(target_os = "android"))]
    fn open_mitm_root(&self) -> Result<OperationResult, String> {
        let root = self.backend().ensure_mitm_root()?;
        let session = self.backend().session();
        let pem = fs::read_to_string(&root.certificate_path)
            .map_err(|error| format!("the root certificate could not be read: {error}"))?;
        if session.trusts_root(&pem).is_none() {
            let trusted = trust::install(&root.certificate_path);
            return Ok(trust_result(&trusted, false));
        }
        let thumbprint = desktop_mitm::thumbprint(&pem)?;
        // Noted before the user answers: a root that was noted and then refused costs nothing, one that
        // was trusted and not noted would stay behind when the client is removed.
        self.backend().note_trusted_root(&thumbprint);
        session.trust_root(&pem)?;
        Ok(OperationResult::coded(
            "root.trusted_user",
            "Root certificate trusted for this user",
        ))
    }

    /// Replaces the root with the user's own CA. Stores that trusted the earlier root do not trust this one:
    /// where nothing here trusts it yet the answer says to trust it again.
    fn import_mitm_root(
        &self,
        certificate_pem: &str,
        private_key_pem: &str,
    ) -> Result<OperationResult, String> {
        let result = self
            .backend()
            .import_mitm_root(certificate_pem, private_key_pem)?;
        // The window words what was saved, then what is still to do (`mitm.root_untrusted`).
        Ok(match self.imported_root_note() {
            Some(note) => result.followed_by("mitm.root_untrusted", note),
            None => result,
        })
    }

    #[cfg(not(target_os = "android"))]
    fn imported_root_note(&self) -> Option<&'static str> {
        let root = self.backend().mitm_root();
        if self.root_trusted(&root) {
            None
        } else {
            Some("This computer does not trust it yet: choose Trust root certificate under Extensions")
        }
    }

    /// Android: the user installs the root in the system settings, as the answer already says.
    #[cfg(target_os = "android")]
    fn imported_root_note(&self) -> Option<&'static str> {
        None
    }

    /// The user's own request to stop trusting the root: the user's own store where the session keeps
    /// one, otherwise the browsers' databases and, after polkit asks, the system store.
    #[cfg(not(target_os = "android"))]
    fn remove_mitm_root_trust(&self) -> Result<OperationResult, String> {
        let root = self.backend().mitm_root();
        let session = self.backend().session();
        let pem = fs::read_to_string(&root.certificate_path).ok();
        match pem.filter(|pem| session.trusts_root(pem).is_some()) {
            Some(pem) => {
                session.untrust_root(&pem)?;
                Ok(OperationResult::coded(
                    "root.removed_user",
                    "Root certificate removed for this user",
                ))
            }
            None => Ok(trust_result(&trust::remove(), true)),
        }
    }

    #[cfg(target_os = "android")]
    fn remove_mitm_root_trust(&self) -> Result<OperationResult, String> {
        Err(UNAVAILABLE.to_string())
    }

    #[cfg(target_os = "android")]
    fn open_mitm_root(&self) -> Result<OperationResult, String> {
        Err(UNAVAILABLE.to_string())
    }

    /// Rebuilds the engine's connections when the network changes or the computer wakes (`network_watch`), and
    /// tells it the network for its `subnet` groups.
    fn start_network_watch(&self) {
        let backend = self.backend.clone();
        let platform = self.platform.clone();
        thread::spawn(move || {
            let mut watch = network_watch::NetworkWatch::new();
            let mut last_path = None;
            let mut network = None;
            let mut samples: u32 = 0;
            loop {
                let path = network_watch::current_path();
                // The Wi-Fi name can change without the addresses (Windows asks `netsh`, a process): read it
                // when the path changes and every thirty seconds.
                if path != last_path || samples.is_multiple_of(NETWORK_READ_EVERY) {
                    network = platform.current_network(false);
                }
                samples = samples.wrapping_add(1);
                last_path = path.clone();
                let reset = watch.observe(std::time::SystemTime::now(), path);
                let facts = NetworkFacts::new(network.as_ref(), network_watch::current_addresses());
                // Every sample, so an engine started since the last one learns the network too (the engine only
                // acts when it changed), and before the reset, so the rebuilt connections pick by the new one.
                let guard = lock(&backend);
                guard.set_network(&facts);
                if let Some(reset) = reset {
                    guard.reset_network(reset);
                }
                drop(guard);
                thread::sleep(network_watch::POLL);
            }
        });
    }

    /// Every five seconds counts the engine's traffic for the statistics (`traffic_stats`) and posts what
    /// there is to notify: an unexpected stop and what scripts posted.
    fn start_monitor(self: &Arc<Self>) {
        let client = self.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            let notices = {
                let mut backend = client.backend();
                backend.count_traffic();
                backend.take_notices()
            };
            for notice in notices {
                client.platform.post_notice(&notice);
            }
        });
    }

    /// Applies the Wi-Fi rules: samples the network every ten seconds while they are on and connects or
    /// disconnects once when it changes. Only where the platform has a system VPN and reads the network.
    fn start_on_demand(self: &Arc<Self>) {
        if !self.platform.has_system_vpn() {
            return;
        }
        let client = self.clone();
        thread::spawn(move || {
            // A connect the rules started that failed: when to try again on the same network (`None` once they
            // stopped trying there, `on_demand::retry`), and how many failures in a row. The first failure is
            // notified, and so is the stop; the retries in between are not. A new network starts over.
            let mut retry: Option<(Option<Instant>, u32)> = None;
            loop {
                thread::sleep(Duration::from_secs(10));
                if !client.backend().on_demand_enabled() {
                    retry = None;
                    continue;
                }
                let Some(network) = client.platform.current_network(false) else {
                    continue;
                };
                let due = retry.is_some_and(|(at, _)| at.is_some_and(|at| Instant::now() >= at));
                if due {
                    client.backend().retry_on_demand();
                }
                let action = client.backend().on_demand_action(network);
                match action {
                    Some(OnDemandAction::Connect) => match client.connect_through_tunnel() {
                        Ok(_) => retry = None,
                        Err(error) => {
                            // A try the wait made counts on; a connect for a new network counts from one.
                            let earlier = if due {
                                retry.map_or(0, |(_, failures)| failures)
                            } else {
                                0
                            };
                            let failures = earlier + 1;
                            let next = on_demand::retry::after(failures);
                            retry = Some((next.map(|wait| Instant::now() + wait), failures));
                            let notice = if next.is_none() {
                                client.backend().on_demand_stopped_notice(&error, failures)
                            } else if failures == 1 {
                                client.backend().on_demand_failed_notice(&error)
                            } else {
                                None
                            };
                            if let Some(notice) = notice {
                                client.platform.post_notice(&notice);
                            }
                        }
                    },
                    Some(OnDemandAction::Disconnect) => {
                        retry = None;
                        let _ = client.backend().disconnect();
                        client.close_tunnel();
                    }
                    // Connected meanwhile (by hand, or the network now wants no connect): nothing to retry.
                    _ if due || client.backend().engine_running() => retry = None,
                    _ => {}
                }
            }
        });
    }

    /// Updates every subscription whose interval has passed, checking once a minute; a failed update is tried
    /// again after a wait that grows with each failure, and notified once until it works again.
    fn start_subscription_updates(self: &Arc<Self>) {
        let client = self.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(60));
            let due = client.backend().due_sources();
            for (id, location) in due {
                let fetched = fetch(&location, false);
                let mut backend = client.backend();
                let (content, meta) = match fetched {
                    Ok((content, meta)) => (Ok(content), Some(meta)),
                    Err(error) => (Err(error), None),
                };
                match (backend.finish_refresh(&id, content), meta) {
                    // A failure to save is the last error already, which the window shows.
                    (Ok(_), Some(meta)) => {
                        let _ = backend.record_response(Some(&id), meta);
                    }
                    (Err(error), _) => {
                        if let Some(notice) = backend.subscription_failed_notice(&id, &error) {
                            drop(backend);
                            client.platform.post_notice(&notice);
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}

/// One interface command: the client and the command's argument object in, the answer out.
type Handler = fn(&Client, Value) -> Result<Value, String>;

/// The handler of the interface command `name`, `None` when this backend has no such command.
fn handler(name: &str) -> Option<Handler> {
    let handler: Handler = match name {
        "runtime_snapshot" => |client, _| json(client.backend().snapshot()),
        "list_nodes" => |client, _| json(client.backend().nodes()),
        // The overview page in one answer, read under one lock so that its parts agree: the state, the node
        // in use, and the engine's counters while it runs. What they mean is `client-view`'s to say.
        "overview" => |client, _| {
            let backend = client.backend();
            let snapshot = json(backend.snapshot())?;
            let chosen = snapshot.get("selectedNode").and_then(Value::as_str);
            // The list can be thousands long; the page needs the node in use, which is one of these.
            let nodes = backend
                .nodes()
                .into_iter()
                .filter(|node| node.selected || chosen == Some(node.id.as_str()))
                .map(json)
                .collect::<Result<Vec<Value>, String>>()?;
            let traffic = if client_view::overview::state(&snapshot) == "connected" {
                backend
                    .runtime_control(&serde_json::json!({ "cmd": "traffic" }))
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            };
            Ok(client_view::overview::overview(&snapshot, &nodes, &traffic))
        },
        // The proxies page in one answer: the nodes `query` leaves, split by subscription, each with what
        // its last delay test said.
        "proxies" => |client, args| {
            let query = query(args)?;
            let nodes = client.node_rows()?;
            Ok(client_view::proxies::proxies(
                &nodes,
                &client.known_delays(),
                &query,
            ))
        },
        // Tests the delay of the nodes the proxies page shows for `query` and remembers what each said.
        "test_proxies" => |client, args| {
            let query = query(args)?;
            let ids = client_view::proxies::shown_ids(&client.node_rows()?, &query);
            let nodes = client.backend().delay_nodes(&ids);
            let results = node_delays(engine::node_delays(&nodes, DELAY_URL));
            client.remember_delays(&results);
            let answered = results
                .iter()
                .filter(|result| result.delay_ms.is_some())
                .count();
            Ok(serde_json::json!({ "tested": results.len(), "answered": answered }))
        },
        // The subscriptions as the proxies page lists them: what each one's row says.
        "subscriptions" => |client, _| {
            let subscriptions: Vec<Value> = client
                .backend()
                .subscriptions()
                .into_iter()
                .map(json)
                .collect::<Result<_, _>>()?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            Ok(client_view::subscriptions::subscriptions(
                &subscriptions,
                now,
            ))
        },
        // The policy groups as the proxies page lists them: what each one's row says and offers.
        "groups" => |client, _| {
            let groups: Vec<Value> = client
                .backend()
                .groups()
                .into_iter()
                .map(json)
                .collect::<Result<_, _>>()?;
            Ok(client_view::groups::groups(&groups))
        },
        // The open connections as the overview page lists them, the newest first, searched or not; nothing
        // while the engine is not running.
        "connections" => |client, args| {
            let query = query(args)?;
            let nodes = client.node_rows()?;
            let list = client.backend().connections();
            Ok(client_view::connections::connections(&list, &nodes, &query))
        },
        "close_connection" => |client, args| {
            let ConnectionId { id } = parse(args)?;
            client
                .backend()
                .runtime_control(&serde_json::json!({ "cmd": "close_connection", "id": id }))
        },
        "close_connections" => |client, _| {
            client
                .backend()
                .runtime_control(&serde_json::json!({ "cmd": "close_all" }))
        },
        "list_native_groups" => |client, _| json(client.backend().groups()),
        "list_subscriptions" => |client, _| json(client.backend().subscriptions()),
        "connect" | "restart_service" => |client, _| json(client.connect_through_tunnel()?),
        "disconnect" => |client, _| {
            let result = client.backend().disconnect();
            client.close_tunnel();
            json(result?)
        },
        "validate_configuration" | "check_profile_runtime" => {
            |client, _| json(client.backend().validate()?)
        }
        "switch_node" => |client, args| {
            let NodeId { node_id } = parse(args)?;
            json(client.backend().switch_node(&node_id)?)
        },
        "select_native_group_outbound" => |client, args| {
            let GroupOutbound {
                group_tag,
                outbound_tag,
            } = parse(args)?;
            json(
                client
                    .backend()
                    .select_group_member(&group_tag, &outbound_tag)?,
            )
        },
        "runtime_control" => |client, args| {
            let Request { request } = parse(args)?;
            // Only the backend names where sessions go and the key that seals them (`capture_session`).
            if !desktop_mitm::sessions::window_may_send(&request) {
                return Err(
                    "runtime control command \"set_capture_storage\" is not available".to_string(),
                );
            }
            client.runtime_control(&request)
        },
        "lan_share_auth" => |client, _| json(client.lan_share_auth()?),
        "regenerate_lan_share_auth" => |client, _| json(client.change_lan_share_auth(None)?),
        "set_lan_share_auth_required" => |client, args| {
            let Required { required } = parse(args)?;
            json(client.change_lan_share_auth(Some(required))?)
        },
        "export_capture" => |client, args| {
            let ExportCapture {
                format,
                id,
                options,
            } = parse(args)?;
            json(client.export_capture(&format, id, options.unwrap_or_default())?)
        },
        "decode_body" => |_, args| {
            let DecodeBody { request } = parse(args)?;
            desktop_mitm::capture::decode_body(&request)
        },
        "save_body" => |client, args| {
            let Body { file } = parse(args)?;
            json(client.save_body(&file)?)
        },
        "open_body" => |_, args| {
            let Body { file } = parse(args)?;
            let path = desktop_mitm::capture::open_body(&file)?;
            json(SavedFile {
                location: path.display().to_string(),
            })
        },
        "capture_session" => |client, args| {
            let CaptureSession { action } = parse(args)?;
            client.capture_session(action)
        },
        "replay_request" => |client, args| {
            let Replay { request } = parse(args)?;
            let target = client.backend().replay_target()?;
            json(desktop_mitm::replay::send(&target, &request)?)
        },
        "list_debug_mappings" => |client, _| {
            let directory = client.backend().plugins_dir();
            json(mappings::list(&directory)?)
        },
        "save_debug_mapping" => |client, args| {
            let Mapping { entry } = parse(args)?;
            json(client.change_plugins(move |directory| {
                let saved = mappings::save(directory, entry)?;
                Ok(OperationResult::coded(
                    "mapping.saved",
                    format!("Mapping saved: {}", saved.pattern),
                )
                .with("pattern", &saved.pattern))
            })?)
        },
        "remove_debug_mapping" => |client, args| {
            let Id { id } = parse(args)?;
            json(client.change_plugins(move |directory| {
                mappings::remove(directory, &id)?;
                Ok(OperationResult::coded("mapping.removed", "Mapping removed"))
            })?)
        },
        // The patterns decrypted (the list's lines that are on, not exclusions), for the request list.
        "list_decrypt_hosts" => |client, _| {
            let directory = client.backend().plugins_dir();
            json(mappings::load(&directory)?.decrypted())
        },
        // The whole decryption list: `{pattern, exclude, enabled}` per line, in the order added.
        "list_decrypt_domains" => |client, _| {
            let directory = client.backend().plugins_dir();
            json(mappings::load(&directory)?.domains)
        },
        "add_decrypt_host" => |client, args| {
            let Host { host, exclude } = parse(args)?;
            json(client.change_plugins(move |directory| {
                mappings::add_host(directory, &host, exclude)?;
                let name = host.trim().trim_start_matches(['-', '!']);
                let result = if exclude || name.len() < host.trim().len() {
                    OperationResult::coded("decrypt.excluding", format!("Excluding {name}"))
                } else {
                    OperationResult::coded("decrypt.decrypting", format!("Decrypting {name}"))
                };
                Ok(result.with("host", name))
            })?)
        },
        "edit_decrypt_host" => |client, args| {
            let EditHost {
                pattern,
                host,
                exclude,
            } = parse(args)?;
            json(client.change_plugins(move |directory| {
                mappings::edit_host(directory, &pattern, &host, exclude)?;
                Ok(
                    OperationResult::coded("decrypt.saved", format!("Saved {host}"))
                        .with("host", &host),
                )
            })?)
        },
        "set_decrypt_host_enabled" => |client, args| {
            let HostEnabled { pattern, enabled } = parse(args)?;
            json(client.change_plugins(move |directory| {
                mappings::set_host_enabled(directory, &pattern, enabled)?;
                let result = if enabled {
                    OperationResult::coded("decrypt.on", format!("Turned on {pattern}"))
                } else {
                    OperationResult::coded("decrypt.paused", format!("Paused {pattern}"))
                };
                Ok(result.with("pattern", &pattern))
            })?)
        },
        "remove_decrypt_host" => |client, args| {
            let Host { host, .. } = parse(args)?;
            json(client.change_plugins(move |directory| {
                mappings::remove_host(directory, &host)?;
                Ok(
                    OperationResult::coded("decrypt.removed", format!("Removed {host}"))
                        .with("host", &host),
                )
            })?)
        },
        // What decryption did per domain since the engine started (`mitm_hosts`): decrypted and failed
        // connections and why; `enabled: false` with no hosts while nothing runs.
        "mitm_hosts" => |client, _| Ok(client.backend().mitm_hosts(false)),
        "clear_mitm_hosts" => |client, _| Ok(client.backend().mitm_hosts(true)),
        "test_node_delay" => |client, args| {
            let NodeDelayArgs { node_id, url } = parse(args)?;
            let (nodes, id, name) = client
                .backend()
                .delay_target(&node_id)
                .ok_or_else(crate::backend::node_missing)?;
            let delay = engine::node_delay(&nodes, &id, &url)?;
            json(
                OperationResult::coded("delay.node", format!("{name}: {}", format_delay(delay)))
                    .with("node", &name)
                    .with("delay", format_delay(delay)),
            )
        },
        // Node checkup (节点体检): latency rounds and an optional download through one node. The
        // options are the kernel's `CheckupOptions`; unknown fields are rejected. The answer carries the
        // report and the rows every window shows (`client_view::checkup`).
        "node_checkup" => |client, args| {
            let NodeCheckupArgs { node_id, options } = parse(args)?;
            let (nodes, id, name) = client
                .backend()
                .delay_target(&node_id)
                .ok_or_else(crate::backend::node_missing)?;
            let report = engine::node_checkup(&nodes, &id, &options)?;
            let mut answer = json(NodeCheckup { name, report })?;
            let mut shown = client_view::checkup::checkup(&answer);
            answer["sections"] = shown["sections"].take();
            Ok(answer)
        },
        "test_node_delays" => |client, args| {
            let NodeDelaysArgs { node_ids, url } = parse(args)?;
            let nodes = client.backend().delay_nodes(&node_ids);
            let results = node_delays(engine::node_delays(&nodes, &url));
            client.remember_delays(&results);
            json(results)
        },
        "network_probe" => |client, args| {
            let Connected { connected } = parse(args)?;
            json(client.network_probe(connected))
        },
        "node_share_link" => |client, args| {
            let NodeId { node_id } = parse(args)?;
            json(client.backend().share_link(&node_id)?)
        },
        "traffic_stats" => |client, args| {
            let Clear { clear } = parse(args)?;
            json(client.backend().traffic_stats(clear))
        },
        "app_logs" => |client, args| {
            let Clear { clear } = parse(args)?;
            json(client.backend().logs(clear)?)
        },
        "notification_settings" => |client, args| {
            let Notifications { update, language } = parse(args)?;
            json(client.backend().notification_settings(update, &language)?)
        },
        "traffic_redirects" => |client, _| json(client.backend().traffic_redirects()),
        "routing_warnings" => |client, _| json(client.backend().routing_warnings()),
        "data_sources" => |client, _| json(client.backend().data_sources()),
        "save_data_source" => |client, args| {
            let DataSource { name, url } = parse(args)?;
            json(client.backend().save_data_source(&name, &url)?)
        },
        "remove_data_source" => |client, args| {
            let DataSourceName { name } = parse(args)?;
            json(client.backend().remove_data_source(&name)?)
        },
        "devices" => |client, _| json(client.backend().devices()),
        "save_device" => |client, args| {
            let Device { address, name } = parse(args)?;
            json(client.backend().save_device(&address, &name)?)
        },
        "remove_device" => |client, args| {
            let DeviceAddress { address } = parse(args)?;
            json(client.backend().remove_device(&address)?)
        },
        "add_traffic_redirect" => |client, args| {
            let RedirectDraft { draft, replacing } = parse(args)?;
            json(
                client
                    .backend()
                    .add_redirect(&draft, replacing.as_deref())?,
            )
        },
        "save_traffic_redirects" => |client, args| {
            let Redirects { entries } = parse(args)?;
            json(client.backend().save_redirects(entries)?)
        },
        "take_app_links" => |client, _| json(std::mem::take(&mut *lock(&client.links))),
        "on_demand_settings" => |client, args| {
            let OnDemandUpdate { update } = parse(args)?;
            json(client.backend().on_demand_settings(update)?)
        },
        "current_network" => |client, args| {
            let Ask { ask } = parse(args)?;
            json(
                client
                    .platform
                    .current_network(ask)
                    .map(|network| network.current())
                    .ok_or("this platform does not read the Wi-Fi network")?,
            )
        },
        // What the windows' node dialog shows: name, server and port, whether they can be changed or the
        // node removed, and the node as a share link and a QR code.
        "node_details" => |client, args| {
            let NodeId { node_id } = parse(args)?;
            json(client.backend().node_details(&node_id)?)
        },
        // Whether a name, server and port can be saved for a node, before `edit_node` is asked:
        // `{ "problem": null | "server.empty" | "server.invalid" | "port.invalid" }`.
        "check_node" => |_, args| {
            let CheckNode { name, server, port } = parse(args)?;
            let problem = match node_edits::check(&name, &server, &port) {
                Ok(_) => None,
                Err(node_edits::SERVER_EMPTY) => Some("server.empty"),
                Err(node_edits::SERVER_INVALID) => Some("server.invalid"),
                Err(_) => Some("port.invalid"),
            };
            Ok(serde_json::json!({ "problem": problem }))
        },
        "node_form" => |client, args| {
            let NodeId { node_id } = parse(args)?;
            json(client.backend().node_form(&node_id)?)
        },
        "edit_node" => |client, args| {
            let EditNode {
                node_id,
                name,
                server,
                port,
            } = parse(args)?;
            json(
                client
                    .backend()
                    .edit_node(&node_id, &name, &server, &port)?,
            )
        },
        "remove_node" => |client, args| {
            let NodeId { node_id } = parse(args)?;
            json(client.backend().remove_node(&node_id)?)
        },
        "select_fastest_node" => |client, _| json(client.select_fastest_node()?),
        "save_preferences" => |client, args| {
            let Preferences { preferences } = parse(args)?;
            json(client.backend().save_preferences(&preferences)?)
        },
        "create_diagnostics" => |client, _| json(client.create_diagnostics()?),
        "health_status" => |client, _| Ok(client.health.status()),
        "set_crash_reports" => |client, args| {
            let Enabled { enabled } = parse(args)?;
            json(client.set_crash_reports(enabled)?)
        },
        "set_metrics" => |client, args| {
            let Enabled { enabled } = parse(args)?;
            json(client.set_metrics(enabled)?)
        },
        "import_subscription" => |client, args| {
            let Location { location } = parse(args)?;
            json(client.import_subscription(&location)?)
        },
        // A subscription file the window read as the user, for a core that opens no path a window names.
        "import_subscription_text" => |client, args| {
            let SubscriptionText { name, content } = parse(args)?;
            json(client.import_subscription_text(&name, &content)?)
        },
        "update_subscription" => |client, _| {
            let (id, location) = client.backend().refreshable_location().ok_or_else(|| {
                refusal::coded(
                    NOT_UPDATABLE,
                    "the current subscription is not a link that can be updated",
                )
            })?;
            json(client.refresh_source(&id, &location)?)
        },
        "update_subscription_source" => |client, args| {
            let Id { id } = parse(args)?;
            let location = client.backend().source_location(&id).ok_or_else(|| {
                refusal::coded(
                    NOT_UPDATABLE,
                    "this subscription is not a link that can be updated",
                )
            })?;
            json(client.refresh_source(&id, &location)?)
        },
        "select_subscription" => |client, args| {
            let Id { id } = parse(args)?;
            json(client.backend().select_subscription(&id)?)
        },
        "remove_subscription" => |client, args| {
            let Id { id } = parse(args)?;
            json(client.backend().remove_subscription(&id)?)
        },
        "rename_subscription" => |client, args| {
            let Rename { id, name } = parse(args)?;
            json(client.backend().rename_subscription(&id, &name)?)
        },
        "set_subscription_interval" => |client, args| {
            let Interval { id, hours } = parse(args)?;
            json(client.backend().set_subscription_interval(&id, hours)?)
        },
        "restore_network_settings" => {
            |client, _| json(client.backend().restore_network_settings()?)
        }
        "set_routing_mode" => |client, args| {
            let Mode { mode } = parse(args)?;
            let mode = RoutingMode::parse(&mode).ok_or("unknown routing mode")?;
            json(client.backend().set_routing_mode(mode)?)
        },
        "set_dns_preset" => |client, args| {
            let Preset { preset } = parse(args)?;
            let preset = DnsPreset::parse(&preset).ok_or("unknown DNS choice")?;
            json(client.backend().set_dns_preset(preset)?)
        },
        "set_tun_mode" => |client, args| {
            let Enabled { enabled } = parse(args)?;
            if !cfg!(any(target_os = "linux", windows)) {
                return Err(UNAVAILABLE.to_string());
            }
            json(client.backend().set_tun_mode(enabled)?)
        },
        // What AI tools may read (设置 › 高级 › 外部工具访问): only a window reads and changes it here; an AI tool
        // reads it through `Client::agent`.
        "agent_access" => |client, _| json(client.backend().agent_access()),
        "set_agent_access" => |client, args| {
            let AgentAccessArgs { read, content } = parse(args)?;
            json(client.backend().set_agent_access(read, &content)?)
        },
        "set_block_quic" => |client, args| {
            let Enabled { enabled } = parse(args)?;
            json(client.backend().set_block_quic(enabled)?)
        },
        "set_direct_egress" => |client, args| {
            let Enabled { enabled } = parse(args)?;
            json(client.backend().set_direct_egress(enabled)?)
        },
        "my_rules" => |client, _| json(client.backend().my_rules()),
        "save_my_rules" => |client, args| {
            let Text { text } = parse(args)?;
            json(client.backend().save_my_rules(&text)?)
        },
        "custom_dns" => |client, _| json(client.backend().custom_dns()?),
        "save_custom_dns" => |client, args| {
            let DnsText { text } = parse(args)?;
            json(client.backend().save_custom_dns(&text)?)
        },
        "enable_https_mitm" => |client, _| json(client.enable_https_mitm()?),
        "disable_https_mitm" => |client, _| {
            // The root stays trusted, so turning decryption on again needs no new manual trust; the user
            // removes it with `remove_mitm_root_trust`.
            json(client.backend().set_https_decryption(false)?)
        },
        "mitm_root_status" => |client, _| json(client.mitm_root_status()?),
        "open_mitm_root" => |client, _| json(client.open_mitm_root()?),
        "remove_mitm_root_trust" => |client, _| json(client.remove_mitm_root_trust()?),
        "list_mitm_plugins" => |client, _| {
            let directory = client.backend().plugins_dir();
            json(plugins::list(&directory)?)
        },
        "import_mitm_plugin" => |client, args| {
            let Url { url } = parse(args)?;
            let directory = client.backend().plugins_dir();
            let plugin = plugins::import(&directory, &url, plugins::download)?;
            json(client.plugin_imported(&plugin)?)
        },
        "import_mitm_plugin_text" => |client, args| {
            let PluginText { text, source } = parse(args)?;
            let directory = client.backend().plugins_dir();
            let plugin = plugins::import_text(&directory, &source, &text, plugins::download)?;
            json(client.plugin_imported(&plugin)?)
        },
        "mitm_plugin_details" => |client, args| {
            let Id { id } = parse(args)?;
            let directory = client.backend().plugins_dir();
            json(plugins::details(&directory, &id)?)
        },
        "set_mitm_plugin_enabled" => |client, args| {
            let PluginEnabled { id, enabled } = parse(args)?;
            json(client.change_plugins(move |directory| {
                plugins::set_enabled(directory, &id, enabled)?;
                Ok(if enabled {
                    OperationResult::coded("plugins.enabled", "Plugin enabled")
                } else {
                    OperationResult::coded("plugins.disabled", "Plugin disabled")
                })
            })?)
        },
        "remove_mitm_plugin" => |client, args| {
            let Id { id } = parse(args)?;
            json(client.change_plugins(move |directory| {
                plugins::remove(directory, &id)?;
                Ok(OperationResult::coded("plugins.removed", "Plugin removed"))
            })?)
        },
        "phone_qr_code" => |_, args| {
            let Text { text } = parse(args)?;
            json(desktop_mitm::phone::qr_svg(&text)?)
        },
        "import_mitm_root" => |client, args| {
            let RootImport {
                certificate_pem,
                private_key_pem,
            } = parse(args)?;
            json(client.import_mitm_root(&certificate_pem, &private_key_pem)?)
        },
        "mitm_root_fingerprint" => |client, _| json(client.backend().mitm_root_fingerprint()),
        _ => return None,
    };
    Some(handler)
}

/// The name the decryption root gets in Downloads: the first eight hex digits of its SHA-256 fingerprint
/// (`AB:CD:…`) keep two roots apart; without a fingerprint the plain name.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn root_download_name(fingerprint: &str) -> String {
    let digits: String = fingerprint
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(8)
        .collect::<String>()
        .to_ascii_uppercase();
    if digits.is_empty() {
        "AnixOps-Net-HTTPS-root.crt".to_string()
    } else {
        format!("AnixOps-Net-HTTPS-root-{digits}.crt")
    }
}

/// What a command that saves a file answers: the message the other clients show, plus, where a file went to
/// Downloads (Android), its location, so the app can say it in its own language. The extra fields are left out
/// when absent, so the answer reads as an [`OperationResult`] everywhere else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFileResult {
    pub message: String,
    /// The code and its values a window words the result by, as in [`OperationResult`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: std::collections::BTreeMap<&'static str, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_error: Option<String>,
}

fn json<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|error| format!("invalid arguments: {error}"))
}

/// What trusting or removing the root reached, coded by the stores (`count` browser stores, the system store) so
/// the window words it. A note (a store that could not be changed) is in English only, so with one the whole
/// sentence stays the backend's.
#[cfg(not(target_os = "android"))]
fn trust_result(outcome: &trust::Outcome, removed: bool) -> OperationResult {
    let message = outcome.summary(if removed { "removed" } else { "trusted" });
    if !outcome.notes.is_empty() {
        return OperationResult::plain(message);
    }
    let code = match (removed, outcome.browsers > 0, outcome.system) {
        (false, true, true) => "root.trusted_browsers_system",
        (false, true, false) => "root.trusted_browsers",
        (false, false, true) => "root.trusted_system",
        (false, false, false) => "root.trusted_nowhere",
        (true, true, true) => "root.removed_browsers_system",
        (true, true, false) => "root.removed_browsers",
        (true, false, true) => "root.removed_system",
        (true, false, false) => "root.removed_nowhere",
    };
    OperationResult::coded(code, message).with("count", outcome.browsers)
}

/// What importing a plugin did, coded: its name, how many scripts it brought, and its id, by which the window opens
/// the plugin for the user to look at before turning it on. A plugin is saved off unless it is one the user had on
/// and nothing in it changed (`plugins::import`).
fn plugin_imported(plugin: &plugins::PluginSummary) -> OperationResult {
    let result = if plugin.enabled {
        OperationResult::coded(
            "plugins.imported",
            format!(
                "Plugin imported: {} ({} scripts)",
                plugin.name, plugin.script_count
            ),
        )
    } else {
        OperationResult::coded(
            "plugins.imported_off",
            format!(
                "Plugin imported: {} ({} scripts); it is off until you look at what it does and turn it on",
                plugin.name, plugin.script_count
            ),
        )
    };
    result
        .with("name", &plugin.name)
        .with("scripts", plugin.script_count)
        .with("id", &plugin.id)
}

/// The search of a command that takes one; no arguments at all is the empty search.
fn query(args: Value) -> Result<String, String> {
    if args.is_null() {
        return Ok(String::new());
    }
    let Query { query } = parse(args)?;
    Ok(query)
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

// Argument objects, keyed as the interface sends them (camelCase).

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeId {
    node_id: String,
}

#[derive(Deserialize)]
struct ConnectionId {
    id: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupOutbound {
    group_tag: String,
    outbound_tag: String,
}

#[derive(Deserialize)]
struct Request {
    request: Value,
}

#[derive(Deserialize)]
struct Required {
    required: bool,
}

#[derive(Deserialize)]
struct DecodeBody {
    request: desktop_mitm::capture::DecodeRequest,
}

#[derive(Deserialize)]
struct Body {
    file: desktop_mitm::capture::BodyFile,
}

#[derive(Deserialize)]
struct ExportCapture {
    format: String,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    options: Option<desktop_mitm::capture::ExportOptions>,
}

#[derive(Deserialize)]
struct CaptureSession {
    action: desktop_mitm::sessions::SessionAction,
}

#[derive(Deserialize)]
struct Replay {
    request: desktop_mitm::replay::ReplayRequest,
}

#[derive(Deserialize)]
struct Mapping {
    entry: mappings::Entry,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct Host {
    host: String,
    /// Add it as an exclusion; a leading `-` in `host` says the same.
    #[serde(default)]
    exclude: bool,
}

#[derive(Deserialize)]
struct EditHost {
    pattern: String,
    host: String,
    #[serde(default)]
    exclude: bool,
}

#[derive(Deserialize)]
struct HostEnabled {
    pattern: String,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeDelayArgs {
    node_id: String,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NodeCheckupArgs {
    node_id: String,
    #[serde(default)]
    options: CheckupOptions,
}

#[derive(Serialize)]
struct NodeCheckup {
    name: String,
    report: CheckupReport,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeDelaysArgs {
    node_ids: Vec<String>,
    url: String,
}

#[derive(Deserialize)]
struct Connected {
    connected: bool,
}

#[derive(Deserialize)]
struct Clear {
    clear: bool,
}

#[derive(Deserialize)]
struct Notifications {
    #[serde(default)]
    update: Option<NotificationSettings>,
    language: String,
}

#[derive(Deserialize)]
struct OnDemandUpdate {
    #[serde(default)]
    update: Option<OnDemandSettings>,
}

#[derive(Deserialize)]
struct RedirectDraft {
    draft: desktop_dns::redirects::Draft,
    #[serde(default)]
    replacing: Option<String>,
}

#[derive(Deserialize)]
struct DataSource {
    name: String,
    url: String,
}

#[derive(Deserialize)]
struct DataSourceName {
    name: String,
}

#[derive(Deserialize)]
struct Device {
    address: String,
    name: String,
}

#[derive(Deserialize)]
struct DeviceAddress {
    address: String,
}

#[derive(Deserialize)]
struct Redirects {
    entries: Vec<desktop_dns::redirects::Entry>,
}

#[derive(Deserialize)]
struct Ask {
    ask: bool,
}

#[derive(Deserialize)]
struct CheckNode {
    name: String,
    server: String,
    port: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditNode {
    node_id: String,
    name: String,
    server: String,
    port: String,
}

#[derive(Deserialize)]
struct Preferences {
    preferences: PreferencesInput,
}

/// A search, for the commands that take one; no `query` is the empty search.
#[derive(Deserialize)]
struct Query {
    #[serde(default)]
    query: String,
}

#[derive(Deserialize)]
struct Enabled {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentAccessArgs {
    read: bool,
    content: String,
}

#[derive(Deserialize)]
struct Location {
    location: String,
}

#[derive(Deserialize)]
struct SubscriptionText {
    name: String,
    content: String,
}

#[derive(Deserialize)]
struct Rename {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct Interval {
    id: String,
    hours: u32,
}

#[derive(Deserialize)]
struct Mode {
    mode: String,
}

#[derive(Deserialize)]
struct Preset {
    preset: String,
}

#[derive(Deserialize)]
struct Text {
    text: String,
}

#[derive(Deserialize)]
struct DnsText {
    text: desktop_dns::custom::DnsText,
}

#[derive(Deserialize)]
struct Url {
    url: String,
}

#[derive(Deserialize)]
struct PluginText {
    text: String,
    source: String,
}

#[derive(Deserialize)]
struct PluginEnabled {
    id: String,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RootImport {
    certificate_pem: String,
    private_key_pem: String,
}

// Answers.

/// Where a capture export was saved.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedFile {
    location: String,
}

/// One node's result in a batch delay test, as the Windows backend reports it.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeDelay {
    node_id: String,
    delay_ms: Option<u64>,
    error: Option<String>,
}

fn node_delays(results: Vec<(String, Result<Duration, String>)>) -> Vec<NodeDelay> {
    results
        .into_iter()
        .map(|(node_id, delay)| match delay {
            Ok(delay) => NodeDelay {
                node_id,
                delay_ms: Some(u64::try_from(delay.as_millis()).unwrap_or(u64::MAX)),
                error: None,
            },
            Err(error) => NodeDelay {
                node_id,
                delay_ms: None,
                error: Some(error),
            },
        })
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MitmRootStatus {
    generated: bool,
    trusted: bool,
}

/// What the overview shows about the path: the address websites see through the proxy, and how long a mainland
/// site takes to answer without it.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkProbe {
    exit_ip: Option<String>,
    /// The exit's two-letter country code.
    exit_location: Option<String>,
    direct_ms: Option<u64>,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(6);
const EXIT_TRACE_URL: &str = "https://www.cloudflare.com/cdn-cgi/trace";
const DIRECT_PROBE_URL: &str = "https://www.baidu.com/";

/// `ip` and `loc` from Cloudflare's `key=value` trace lines.
fn parse_trace(body: &str) -> (Option<String>, Option<String>) {
    let field = |name: &str| {
        body.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    (field("ip"), field("loc"))
}

/// Asks through the local proxy on `proxy_port` (when connected) which address the exit has, and times one
/// request to a mainland site that goes around the proxy. A probe that fails leaves its fields empty.
fn probe_network(proxy_port: Option<u16>) -> NetworkProbe {
    let (exit_ip, exit_location) = proxy_port
        .and_then(|port| {
            let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{port}")).ok()?;
            let body = reqwest::blocking::Client::builder()
                .timeout(PROBE_TIMEOUT)
                .proxy(proxy)
                .build()
                .ok()?
                .get(EXIT_TRACE_URL)
                .send()
                .ok()?
                .text()
                .ok()?;
            Some(parse_trace(&body))
        })
        .unwrap_or((None, None));
    let direct_ms = reqwest::blocking::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .no_proxy()
        .build()
        .ok()
        .and_then(|client| {
            let started = std::time::Instant::now();
            client.head(DIRECT_PROBE_URL).send().ok()?;
            Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))
        });
    NetworkProbe {
        exit_ip,
        exit_location,
        direct_ms,
    }
}

/// The body and what the subscription headers say about it. A panel that licenses ANX gets this device's
/// key and answers with its license (`anx_license`); the first time it says so, the request goes once more.
fn http_get_with_meta(url: &str) -> Result<(String, ResponseMeta), String> {
    let licenses = engine::license_store();
    let (body, meta, offered) = http_get_once(url, licenses.as_ref())?;
    if offered {
        if let Ok((body, meta, _)) = http_get_once(url, licenses.as_ref()) {
            return Ok((body, meta));
        }
    }
    Ok((body, meta))
}

/// One download. Errors are [`download_failure`]'s or [`crate::download::LimitedReadError::describe`]'s text,
/// without the URL. The body is read at most [`MAX_SUBSCRIPTION_FILE_BYTES`] (audit 2026-11-25 NET-02): a larger
/// answer fails the download, and the caller keeps the last good subscription.
fn http_get_once(
    url: &str,
    licenses: Option<&anx_license::LicenseStore>,
) -> Result<(String, ResponseMeta, bool), String> {
    // The panel picks the subscription format by User-Agent (same token as the iOS app).
    let mut request = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("AnixOpsNet/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| download_failure(&error))?
        .get(url);
    if let Some(device) = licenses.and_then(|store| store.request_header(url).ok().flatten()) {
        request = request.header(anx_license::HEADER_DEVICE, device);
    }
    let response = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| download_failure(&error))?;
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let meta = ResponseMeta::from_headers(header);
    // A license problem never fails the subscription.
    let offered = licenses
        .and_then(|store| store.record(url, header).ok())
        .unwrap_or(false);
    let declared = response.content_length();
    let body = crate::download::read_subscription_text(response, declared)
        .map_err(|error| error.describe())?;
    Ok((body, meta, offered))
}

/// The largest subscription read, from a file or a download: far above any real one, and what keeps `/dev/zero`,
/// a huge file or an endless answer from filling memory.
const MAX_SUBSCRIPTION_FILE_BYTES: u64 = crate::download::MAX_SUBSCRIPTION_BYTES;

fn too_large() -> String {
    format!(
        "the file is larger than {} MiB, which no subscription is",
        MAX_SUBSCRIPTION_FILE_BYTES / (1024 * 1024)
    )
}

/// What a core in a system service answers for a path: it reads no file for a window.
const NO_PATHS_IN_SERVICE: &str =
    "the core service does not open files: import the file from the window, which reads it as you";

/// The label of a source imported from a file's content: the file's name without its folder, never one that
/// reads as a link (which would make the source updatable) or as pasted links.
fn file_label(name: &str) -> String {
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    if name.is_empty() || name.contains("://") || name == PASTED_LOCATION {
        "subscription file".to_string()
    } else {
        name.chars().take(128).collect()
    }
}

/// A URL's body with its headers' meta, the pasted share links themselves, or, with `read_files`, a local file.
/// Without `read_files` (a core in a system service, and the updates, which only fetch links) a path is refused
/// before anything on disk is touched, so a window cannot learn whether a file exists either.
fn fetch(location: &str, read_files: bool) -> Result<(String, ResponseMeta), String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return http_get_with_meta(location)
            .map_err(|reason| format!("the subscription could not be downloaded: {reason}"));
    }
    if location.contains("://") {
        return Ok((location.to_string(), ResponseMeta::default()));
    }
    if !read_files {
        return Err(NO_PATHS_IN_SERVICE.to_string());
    }
    read_subscription_file(location).map(|content| (content, ResponseMeta::default()))
}

/// Why a download failed, without the address: reqwest's own text names the full URL, and a subscription URL
/// carries its token, while this text reaches the window, the notification, the saved state and the log. Same
/// reasons as the Windows client's `describe_download_error`.
fn download_failure(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the download timed out".to_string()
    } else if let Some(status) = error.status() {
        format!("the server answered HTTP status {}", status.as_u16())
    } else if error.is_connect() {
        "the server could not be reached".to_string()
    } else if error.is_body() || error.is_decode() {
        "the answer could not be read".to_string()
    } else {
        "the download failed".to_string()
    }
}

/// A local subscription file, in the user's own process (the Tauri app, Android): only a regular file, at most
/// [`MAX_SUBSCRIPTION_FILE_BYTES`]. Network and device paths (`\\server\share`, `\\.\`) are not opened on
/// Windows.
fn read_subscription_file(location: &str) -> Result<String, String> {
    use std::io::Read;

    if cfg!(windows) && (location.starts_with("\\\\") || location.starts_with("//")) {
        return Err(
            "network and device paths are not opened: copy the file to this computer first"
                .to_string(),
        );
    }
    let path = match location.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map_or_else(
            || PathBuf::from(location),
            |home| PathBuf::from(home).join(rest),
        ),
        None => PathBuf::from(location),
    };
    let unreadable =
        |error: std::io::Error| format!("{} could not be read: {error}", path.display());
    let not_a_file = || format!("{} is not a file", path.display());
    // Checked before opening: opening a FIFO would wait for a writer.
    if !fs::metadata(&path).map_err(unreadable)?.is_file() {
        return Err(not_a_file());
    }
    let file = fs::File::open(&path).map_err(unreadable)?;
    if !file.metadata().map_err(unreadable)?.is_file() {
        return Err(not_a_file());
    }
    let mut content = String::new();
    file.take(MAX_SUBSCRIPTION_FILE_BYTES + 1)
        .read_to_string(&mut content)
        .map_err(unreadable)?;
    if content.len() as u64 > MAX_SUBSCRIPTION_FILE_BYTES {
        return Err(too_large());
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_file_is_named_after_its_fingerprint() {
        assert_eq!(
            root_download_name("ab:cd:01:23:45:67:89:EF:00"),
            "AnixOps-Net-HTTPS-root-ABCD0123.crt"
        );
        // A new root never takes an old root's name.
        assert_ne!(
            root_download_name("AB:CD:01:23:45:67:89:EF"),
            root_download_name("AB:CD:01:24:45:67:89:EF")
        );
        assert_eq!(root_download_name(""), "AnixOps-Net-HTTPS-root.crt");
    }

    #[test]
    fn saved_file_answers_read_as_operation_results() {
        let plain = json(SavedFileResult {
            message: "Saved".to_string(),
            ..SavedFileResult::default()
        })
        .unwrap();
        assert_eq!(plain, serde_json::json!({"message": "Saved"}));
        let located = json(SavedFileResult {
            message: "Saved".to_string(),
            location: Some("Download/a.log".to_string()),
            save_error: Some("denied".to_string()),
            ..SavedFileResult::default()
        })
        .unwrap();
        assert_eq!(
            located,
            serde_json::json!({"message": "Saved", "location": "Download/a.log", "saveError": "denied"})
        );
    }

    #[test]
    fn trace_gives_the_exit_address_and_country() {
        let body = "fl=1\nip=203.0.113.7\nts=1\nloc=JP\n";
        assert_eq!(
            parse_trace(body),
            (Some("203.0.113.7".to_string()), Some("JP".to_string()))
        );
        assert_eq!(parse_trace("ip=\n"), (None, None));
    }

    #[test]
    fn batch_delays_keep_the_interface_shape() {
        let answer = json(node_delays(vec![
            ("a".to_string(), Ok(Duration::from_millis(42))),
            ("b".to_string(), Err("timed out".to_string())),
        ]))
        .unwrap();
        assert_eq!(
            answer,
            serde_json::json!([
                {"nodeId": "a", "delayMs": 42, "error": null},
                {"nodeId": "b", "delayMs": null, "error": "timed out"},
            ])
        );
    }

    #[test]
    fn arguments_are_read_as_the_interface_sends_them() {
        let NodeDelaysArgs { node_ids, url } =
            parse(serde_json::json!({"nodeIds": ["a", "b"], "url": "https://x"})).unwrap();
        assert_eq!(node_ids, ["a", "b"]);
        assert_eq!(url, "https://x");
        let ExportCapture { id, options, .. } =
            parse(serde_json::json!({"format": "har", "id": null})).unwrap();
        assert_eq!(id, None);
        assert!(options.is_none());
        let OnDemandUpdate { update } = parse(serde_json::json!({})).unwrap();
        assert!(update.is_none());
        assert!(parse::<NodeId>(serde_json::json!({"node_id": "a"})).is_err());
    }

    /// Every command the shared interface invokes (`bridge.ts`) has a handler here or is a Windows-only one,
    /// so a command added for Windows and forgotten here fails this test instead of the Linux and Android
    /// windows at run time.
    #[test]
    fn every_interface_command_is_answered() {
        let bridge = include_str!("../../../apps/windows-gui/ui/src/bridge.ts");
        let mut names = Vec::new();
        for line in bridge.lines() {
            let mut rest = line;
            while let Some(at) = rest.find("invoke") {
                rest = &rest[at + "invoke".len()..];
                let Some(open) = rest.find("(\"") else {
                    break;
                };
                let name: String = rest[open + 2..]
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
        assert!(
            names.len() > 60,
            "found only {} commands in bridge.ts",
            names.len()
        );
        let missing: Vec<&String> = names
            .iter()
            .filter(|name| handler(name).is_none() && !WINDOWS_ONLY.contains(&name.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "commands without a handler: {missing:?}"
        );
    }

    struct NoPlatform;

    impl Platform for NoPlatform {}

    #[test]
    fn commands_answer_by_name() {
        let dir = std::env::temp_dir().join(format!(
            "anixops-net-client-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        let client = Client::open(
            Config {
                dir: dir.clone(),
                executable: dir.join("anixops-net"),
                shared_service: false,
            },
            Arc::new(NoPlatform),
        );
        let snapshot = client.invoke("runtime_snapshot", Value::Null).unwrap();
        assert!(snapshot.get("connection").is_some());
        // The overview page's answer: a client that was never given anything is not connected.
        let overview = client.invoke("overview", Value::Null).unwrap();
        assert_eq!(overview["state"], "disconnected");
        assert_eq!(overview["node"], Value::Null);
        assert_eq!(overview["error"], Value::Null);
        assert_eq!(overview["traffic"], Value::Null);
        assert!(overview["canConnect"].is_boolean());
        // The proxies page's answer: no subscription, so no node, searched or not.
        for args in [
            serde_json::json!({}),
            serde_json::json!({ "query": "tokyo" }),
        ] {
            let proxies = client.invoke("proxies", args).unwrap();
            assert_eq!(
                proxies,
                serde_json::json!({ "total": 0, "shown": 0, "sections": [] })
            );
        }
        assert_eq!(
            client
                .invoke("subscriptions", serde_json::json!({}))
                .unwrap(),
            serde_json::json!({ "subscriptions": [] })
        );
        assert_eq!(
            client.invoke("groups", serde_json::json!({})).unwrap(),
            serde_json::json!({ "groups": [] })
        );
        // No engine runs: there is nothing to list, searched or not, and nothing to close.
        for args in [Value::Null, serde_json::json!({ "query": "google" })] {
            assert_eq!(
                client.invoke("connections", args).unwrap(),
                serde_json::json!({ "running": false, "total": 0, "connections": [] })
            );
        }
        assert!(client
            .invoke("close_connection", serde_json::json!({ "id": 1 }))
            .is_err());
        assert!(client
            .invoke("close_connection", serde_json::json!({}))
            .is_err());
        assert!(client.invoke("close_connections", Value::Null).is_err());
        for (server, port, problem) in [
            ("hk.example", "443", None),
            ("", "443", Some("server.empty")),
            ("hk example", "443", Some("server.invalid")),
            ("hk.example", "0", Some("port.invalid")),
        ] {
            assert_eq!(
                client
                    .invoke(
                        "check_node",
                        serde_json::json!({ "name": "HK", "server": server, "port": port })
                    )
                    .unwrap(),
                serde_json::json!({ "problem": problem })
            );
        }
        assert_eq!(
            client
                .invoke("test_proxies", serde_json::json!({}))
                .unwrap(),
            serde_json::json!({ "tested": 0, "answered": 0 })
        );
        // The windows' shared logic answers through the same door.
        assert_eq!(
            client.invoke(
                "view.parse_app_link",
                serde_json::json!({ "link": "anixops://connect" })
            ),
            Ok(serde_json::json!({ "action": "connect" }))
        );
        assert_eq!(
            client
                .invoke("list_subscriptions", serde_json::json!({}))
                .unwrap(),
            serde_json::json!([])
        );
        assert_eq!(
            client.invoke("install_driver", Value::Null),
            Err(UNAVAILABLE.to_string())
        );
        assert_eq!(
            client.invoke("no_such_command", Value::Null),
            Err("unknown command no_such_command".to_string())
        );
        assert!(client
            .invoke("switch_node", serde_json::json!({}))
            .unwrap_err()
            .starts_with("invalid arguments"));
        // Node checkup: unknown nodes and unknown options are refused before anything is sent.
        assert_eq!(
            client.invoke("node_checkup", serde_json::json!({ "nodeId": "gone" })),
            Err("error.node_missing: the node is not in any subscription".to_string())
        );
        assert!(client
            .invoke(
                "node_checkup",
                serde_json::json!({ "nodeId": "gone", "options": { "rounds": 3 } })
            )
            .unwrap_err()
            .starts_with("invalid arguments"));
        // A change to the decryption list says what was changed, then what that did to the connection.
        assert_eq!(
            client
                .invoke(
                    "add_decrypt_host",
                    serde_json::json!({ "host": "-ads.example.com" })
                )
                .unwrap(),
            serde_json::json!({
                "message": "Excluding ads.example.com; turn on HTTPS decryption for it to take effect",
                "code": "decrypt.excluding",
                "params": { "host": "ads.example.com" },
                "follow": "plugins.need_decryption",
            })
        );
        assert!(!client.receive_app_links(vec!["https://example.com".to_string()]));
        assert!(client.receive_app_links(vec!["anixops://connect".to_string()]));
        assert_eq!(
            client.invoke("take_app_links", Value::Null).unwrap(),
            serde_json::json!(["anixops://connect"])
        );
        assert_eq!(
            client.invoke("take_app_links", Value::Null).unwrap(),
            serde_json::json!([])
        );
        client.shutdown();
        let _ = fs::remove_dir_all(dir);
    }

    /// The LAN share commands answer the kept credentials and change them. Linux keeps them in a file here (one is
    /// already there, so the test never reaches the runner's keyring).
    #[cfg(all(not(windows), not(target_os = "android")))]
    #[test]
    fn lan_share_credentials_are_kept_shown_and_changed() {
        let dir = scratch("lan-share");
        let kept = desktop_mitm::phone::LanShareAuth {
            required: true,
            username: "anx-abcdef".to_string(),
            password: "abcdefghjkmnpqrstuvw".to_string(),
        };
        desktop_mitm::sessions::write_private(
            &dir.join("lan-share-auth.json"),
            &kept.to_bytes().unwrap(),
        )
        .unwrap();
        let client = Client::open(
            Config {
                dir: dir.clone(),
                executable: dir.join("anixops-net"),
                shared_service: false,
            },
            Arc::new(NoPlatform),
        );
        assert_eq!(
            client.invoke("lan_share_auth", Value::Null).unwrap(),
            serde_json::json!({
                "required": true,
                "username": "anx-abcdef",
                "password": "abcdefghjkmnpqrstuvw",
            })
        );
        let off = client
            .invoke(
                "set_lan_share_auth_required",
                serde_json::json!({ "required": false }),
            )
            .unwrap();
        assert_eq!(off["required"], false);
        assert_eq!(off["password"], kept.password);
        let fresh = client
            .invoke("regenerate_lan_share_auth", Value::Null)
            .unwrap();
        assert_eq!(fresh["required"], false);
        assert_ne!(fresh["password"], kept.password);
        assert_eq!(client.invoke("lan_share_auth", Value::Null).unwrap(), fresh);
        client.shutdown();
        let _ = fs::remove_dir_all(dir);
    }

    const LINK: &str = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@82.47.34.99:11111#HK";

    /// AI tools (docs/mcp-desktop.md): nothing but the settings while reading is off, only the policy's engine
    /// requests, never a window's command, and nodes without their addresses or credentials.
    #[test]
    fn ai_tools_read_only_what_the_user_allows() {
        use client_mcp::agent as wire;

        let dir = scratch("agent");
        let open = || {
            Client::open(
                Config {
                    dir: dir.clone(),
                    executable: dir.join("anixops-net"),
                    shared_service: false,
                },
                Arc::new(NoPlatform),
            )
        };
        let client = open();
        client
            .invoke(
                "import_subscription_text",
                serde_json::json!({ "name": "Home", "content": LINK }),
            )
            .unwrap();

        // Off by default: the settings are answered, nothing else is.
        let access = client.agent(wire::ACCESS, serde_json::json!({})).unwrap();
        assert_eq!(
            access,
            serde_json::json!({ "read": false, "act": false, "content": "summary" })
        );
        for call in [wire::STATUS, wire::NODES, wire::LOGS, wire::ENGINE] {
            assert_eq!(
                client.agent(call, serde_json::json!({})),
                Err(client_mcp::ACCESS_OFF.to_string()),
                "{call}"
            );
        }

        // Only a window turns it on; a level that does not exist is refused.
        assert!(client
            .invoke(
                "set_agent_access",
                serde_json::json!({ "read": true, "content": "everything" })
            )
            .is_err());
        let set = client
            .invoke(
                "set_agent_access",
                serde_json::json!({ "read": true, "content": "headers" }),
            )
            .unwrap();
        assert_eq!(
            set,
            serde_json::json!({ "read": true, "act": false, "content": "headers" })
        );
        assert_eq!(client.invoke("agent_access", Value::Null).unwrap(), set);

        let status = client.agent(wire::STATUS, serde_json::json!({})).unwrap();
        assert_eq!(status["connection"], "disconnected");
        assert_eq!(status["mode"], "rule");
        assert_eq!(status["subscription"], "Home");
        assert_eq!(status["node"], "HK");
        let nodes = client.agent(wire::NODES, serde_json::json!({})).unwrap();
        assert_eq!(
            nodes,
            serde_json::json!([{
                "name": "Home",
                "current": true,
                "nodes": [{ "name": "HK", "protocol": "shadowsocks", "latency_ms": null, "selected": true }]
            }])
        );
        let text = format!("{status}{nodes}");
        assert!(
            !text.contains("82.47.34.99") && !text.contains("YWVz"),
            "{text}"
        );

        // Engine requests: the policy first, then the engine, which does not run.
        assert_eq!(
            client.agent(
                wire::ENGINE,
                serde_json::json!({ "request": { "cmd": "set_capture", "enabled": true } })
            ),
            Err("set_capture is not available to AI tools.".to_string())
        );
        assert_eq!(
            client.agent(
                wire::ENGINE,
                serde_json::json!({ "request": { "cmd": "traffic" } })
            ),
            Err(client_mcp::NOT_CONNECTED.to_string())
        );
        // A window's command is not a request of an AI tool, whatever it is called.
        for call in [
            "set_agent_access",
            "agent.set_agent_access",
            "connect",
            "agent.",
        ] {
            assert!(client.agent(call, serde_json::json!({})).is_err(), "{call}");
        }
        let lines = client
            .agent(wire::LOGS, serde_json::json!({ "limit": 1 }))
            .unwrap();
        assert!(lines.as_array().is_some_and(|lines| lines.len() <= 1));

        // The settings are kept across runs.
        drop(client);
        let reopened = open();
        assert_eq!(
            reopened.agent(wire::ACCESS, serde_json::json!({})).unwrap(),
            set
        );
        drop(reopened);
        let _ = fs::remove_dir_all(dir);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anixops-net-client-{name}-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A loopback HTTP server for subscription downloads; what it answers is set through the returned mode
    /// (0: [`LINK`] with a length, 1: one byte over the limit without a length, 2: a `Content-Length` over the limit,
    /// 3: exactly the limit without a length).
    fn subscription_server() -> (String, Arc<std::sync::atomic::AtomicU8>) {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::atomic::Ordering;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/sub?token=s3cret-token",
            listener.local_addr().unwrap()
        );
        let mode = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let serving = mode.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|read| read > 0) && line != "\r\n" {
                    line.clear();
                }
                let limit = usize::try_from(MAX_SUBSCRIPTION_FILE_BYTES).unwrap();
                let _ = match serving.load(Ordering::SeqCst) {
                    0 => write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{LINK}",
                        LINK.len()
                    ),
                    1 | 3 => {
                        let size = if serving.load(Ordering::SeqCst) == 1 {
                            limit + 1
                        } else {
                            limit
                        };
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                        let chunk = vec![b'#'; 64 * 1024];
                        let mut left = size;
                        while left > 0 {
                            let take = left.min(chunk.len());
                            if stream.write_all(&chunk[..take]).is_err() {
                                break;
                            }
                            left -= take;
                        }
                        Ok(())
                    }
                    _ => write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{LINK}",
                        MAX_SUBSCRIPTION_FILE_BYTES * 4
                    ),
                };
            }
        });
        (url, mode)
    }

    /// Audit 2026-11-25 NET-02: a download is read at most 16 MiB, with or without a `Content-Length`; a larger
    /// answer fails the manual and the scheduled update without the URL in the error, and the last good
    /// subscription stays.
    #[test]
    fn subscription_downloads_are_bounded_and_keep_the_last_good_one() {
        use std::sync::atomic::Ordering;

        let (url, mode) = subscription_server();
        // The boundary: exactly the limit, without a length, is downloaded whole.
        mode.store(3, Ordering::SeqCst);
        let (body, _) = fetch(&url, false).unwrap();
        assert_eq!(body.len() as u64, MAX_SUBSCRIPTION_FILE_BYTES);

        let dir = scratch("download-limit");
        let client = Client::open(
            Config {
                dir: dir.join("data"),
                executable: dir.join("anixops-net"),
                shared_service: false,
            },
            Arc::new(NoPlatform),
        );
        mode.store(0, Ordering::SeqCst);
        client
            .invoke(
                "import_subscription",
                serde_json::json!({ "location": url }),
            )
            .unwrap();
        let nodes = |client: &Client| client.backend().nodes().len();
        assert_eq!(nodes(&client), 1);

        for oversized in [1, 2] {
            mode.store(oversized, Ordering::SeqCst);
            // Manual update.
            let error = client
                .invoke("update_subscription", Value::Null)
                .unwrap_err();
            assert!(error.contains("larger than 16 MiB"), "{error}");
            assert!(
                !error.contains("s3cret") && !error.contains("127.0.0.1"),
                "{error}"
            );
            // The scheduled update takes the same download and the same refusal.
            let (id, location) = client.backend().refreshable_location().unwrap();
            let fetched = fetch(&location, false).map(|(content, _)| content);
            let error = fetched.clone().unwrap_err();
            assert!(error.contains("larger than 16 MiB"), "{error}");
            assert!(!error.contains("s3cret"), "{error}");
            assert!(client.backend().finish_refresh(&id, fetched).is_err());
            assert_eq!(nodes(&client), 1, "the last good subscription stays");
        }
        client.shutdown();
        let _ = fs::remove_dir_all(dir);
    }

    /// A core in a system service reads no path a window names, as SYSTEM or root (client-core-ipc-02): the
    /// window sends the file's content instead.
    #[test]
    fn a_shared_service_opens_no_path_a_window_names() {
        let dir = scratch("shared");
        let file = dir.join("nodes.txt");
        fs::write(&file, LINK).unwrap();
        let client = Client::open(
            Config {
                dir: dir.join("data"),
                executable: dir.join("anixops-net"),
                shared_service: true,
            },
            Arc::new(NoPlatform),
        );
        let import = |location: &str| {
            client.invoke(
                "import_subscription",
                serde_json::json!({ "location": location }),
            )
        };
        // A file that exists and one that does not get the same answer: nothing is learnt about either.
        let readable = import(&file.display().to_string()).unwrap_err();
        let missing = import(&dir.join("missing.txt").display().to_string()).unwrap_err();
        assert_eq!(readable, NO_PATHS_IN_SERVICE);
        assert_eq!(missing, NO_PATHS_IN_SERVICE);
        assert_eq!(
            import("\\\\host\\share\\x.yaml").unwrap_err(),
            NO_PATHS_IN_SERVICE
        );
        assert_eq!(import("~/nodes.txt").unwrap_err(), NO_PATHS_IN_SERVICE);
        assert!(client.backend().subscriptions().is_empty());
        // Pasted share links still work.
        import(LINK).unwrap();
        assert_eq!(client.backend().subscriptions().len(), 1);

        // The window read the file as the user: its name labels the source, which cannot be updated.
        client
            .invoke(
                "import_subscription_text",
                serde_json::json!({ "name": "/home/someone/nodes.txt", "content": LINK.replace("#HK", "#JP") }),
            )
            .unwrap();
        let subscriptions = client.backend().subscriptions();
        let from_file = subscriptions
            .iter()
            .find(|subscription| subscription.location == "nodes.txt")
            .expect("the file's source");
        assert!(!from_file.refreshable);
        let oversized = "x".repeat(usize::try_from(MAX_SUBSCRIPTION_FILE_BYTES).unwrap() + 1);
        assert!(client
            .invoke(
                "import_subscription_text",
                serde_json::json!({ "name": "big.txt", "content": oversized }),
            )
            .unwrap_err()
            .contains("larger than"));
        client.shutdown();
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn trusting_the_root_is_coded_unless_a_store_left_a_note() {
        let both = trust::Outcome {
            browsers: 2,
            system: true,
            notes: Vec::new(),
        };
        let result = trust_result(&both, false);
        assert_eq!(result.code, Some("root.trusted_browsers_system"));
        assert_eq!(result.params.get("count").map(String::as_str), Some("2"));
        assert_eq!(
            trust_result(&trust::Outcome::default(), true).code,
            Some("root.removed_nowhere")
        );
        let noted = trust::Outcome {
            notes: vec!["certutil is not installed".to_string()],
            ..both
        };
        assert_eq!(trust_result(&noted, false).code, None);
    }

    #[test]
    fn a_file_label_never_reads_as_a_link() {
        assert_eq!(file_label("C:\\Users\\a\\nodes.yaml"), "nodes.yaml");
        assert_eq!(file_label("/home/a/nodes.yaml"), "nodes.yaml");
        assert_eq!(file_label("https://panel.example/s"), "s");
        assert_eq!(file_label("https://"), "subscription file");
        assert_eq!(file_label("  "), "subscription file");
        assert_eq!(file_label(PASTED_LOCATION), "subscription file");
    }

    /// In the user's own process a path is still read, but only a regular file and not without limit.
    #[test]
    fn only_regular_files_of_a_sane_size_are_read() {
        let dir = scratch("files");
        let file = dir.join("nodes.txt");
        fs::write(&file, LINK).unwrap();
        let (content, _) = fetch(&file.display().to_string(), true).unwrap();
        assert_eq!(content, LINK);
        // The same path is refused when files are not read, before the disk is touched.
        assert_eq!(
            fetch(&file.display().to_string(), false).unwrap_err(),
            NO_PATHS_IN_SERVICE
        );
        // A folder is not a file.
        assert!(fetch(&dir.display().to_string(), true)
            .unwrap_err()
            .contains("is not a file"));
        // A device would never end.
        #[cfg(unix)]
        assert!(fetch("/dev/zero", true)
            .unwrap_err()
            .contains("is not a file"));
        // A file past the limit is refused; a sparse one costs no disk.
        let big = dir.join("big.txt");
        fs::File::create(&big)
            .unwrap()
            .set_len(MAX_SUBSCRIPTION_FILE_BYTES + 1)
            .unwrap();
        assert!(fetch(&big.display().to_string(), true)
            .unwrap_err()
            .contains("larger than"));
        let _ = fs::remove_dir_all(dir);
    }
}
