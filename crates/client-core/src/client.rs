//! The client service behind every window that drives this backend, free of Tauri: one [`Client`] answers the
//! interface's commands by name ([`Client::invoke`], names and JSON shapes as in the shared interface's
//! `bridge.ts`) and runs the background work (subscription updates, Wi-Fi rules, traffic counting, network
//! changes). The Tauri window (`app.rs`) forwards its IPC here; the native Android app calls the same commands
//! through JNI, so both read one contract.
//!
//! What the platform does itself (the system VPN, the shared Downloads folder, the key store, the current
//! network, system notifications) sits behind [`Platform`].

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use app_notifications::{Notice, Settings as NotificationSettings};
use client_health::Health;
use desktop_mitm::mappings;
use desktop_mitm::plugins;
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
const NO_DOWNLOADS: &str = "this platform has no shared Downloads folder";

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

    /// Puts a text file where the user can open it from the system settings (Android: Downloads) and returns
    /// where it went.
    fn save_to_downloads(&self, name: &str, contents: &str) -> Result<String, String> {
        let _ = (name, contents);
        Err(NO_DOWNLOADS.to_string())
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
        let Config { dir, executable } = config;
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
            self.backend().stop_engine();
            self.platform.close_tunnel();
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
        let cached = lock(&self.session_key).clone();
        let key = match cached {
            Some(key) => key,
            None => {
                let key = self.load_session_key(&capture_dir)?;
                *lock(&self.session_key) = Some(key.clone());
                key
            }
        };
        let storage = desktop_mitm::sessions::storage_request(&capture_dir.join("sessions"), &key);
        let protection = key.protection;
        let answer = {
            let backend = self.backend();
            backend.runtime_control(&storage)?;
            backend.runtime_control(&request)?
        };
        Ok(desktop_mitm::sessions::with_protection(answer, protection))
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

    /// Exports the packet capture (`format` pcapng or har; one connection with `id`, else all) to Downloads
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
        let (id, delay) = engine::fastest_node(&nodes, DELAY_URL)
            .ok_or_else(|| "no node answered the delay test".to_string())?;
        let switched = self.backend().switch_node(&id)?;
        Ok(OperationResult {
            message: format!("{} ({})", switched.message, format_delay(delay)),
        })
    }

    fn create_diagnostics(&self) -> Result<OperationResult, String> {
        let section = self.health.diagnostics();
        // Android: the app's own directory is out of the user's reach, so the report goes to Downloads.
        if self.platform.has_system_vpn() {
            let report = self.backend().diagnostics_report(&section);
            let name = format!("anixops-net-diagnostics-{}.log", unix_seconds());
            let saved = self.platform.save_to_downloads(&name, &report)?;
            return Ok(OperationResult {
                message: format!("Diagnostics report saved to {saved}"),
            });
        }
        self.backend().diagnostics(&section)
    }

    fn set_crash_reports(&self, enabled: bool) -> Result<OperationResult, String> {
        self.health
            .set_crash_reports(enabled)
            .map_err(|error| format!("the setting could not be saved: {error}"))?;
        Ok(OperationResult {
            message: if enabled {
                "Crash reports on".to_string()
            } else {
                "Crash reports off; kept reports deleted".to_string()
            },
        })
    }

    fn import_subscription(&self, location: &str) -> Result<OperationResult, String> {
        let location = location.trim();
        if location.is_empty() {
            return Err("enter a subscription link, share links or a file path".to_string());
        }
        let label = if location.contains("://") && !location.starts_with("http") {
            PASTED_LOCATION.to_string()
        } else {
            location.to_string()
        };
        let (content, meta) = fetch(location)?;
        let mut backend = self.backend();
        let result = backend.import(&label, &content)?;
        backend.record_response(None, meta);
        Ok(result)
    }

    /// Downloads source `id` again outside the lock and records the result.
    fn refresh_source(&self, id: &str, location: &str) -> Result<OperationResult, String> {
        let (content, meta) = match fetch(location) {
            Ok((content, meta)) => (Ok(content), Some(meta)),
            Err(error) => (Err(error), None),
        };
        let mut backend = self.backend();
        let result = backend.finish_refresh(id, content);
        if let (Ok(_), Some(meta)) = (&result, meta) {
            backend.record_response(Some(id), meta);
        }
        result
    }

    /// Changes the plugin library outside the lock, then applies it to a running engine.
    fn change_plugins<F>(&self, change: F) -> Result<OperationResult, String>
    where
        F: FnOnce(&Path) -> Result<String, String>,
    {
        let directory = self.backend().plugins_dir();
        let message = change(&directory)?;
        let applied = self.backend().plugins_changed()?;
        Ok(OperationResult {
            message: format!("{message}; {applied}"),
        })
    }

    /// Generates the root on first use and restarts a running engine with decryption on. The root is not
    /// trusted here: the user does that with `open_mitm_root` (TERMS.md section 4).
    #[cfg(not(target_os = "android"))]
    fn enable_https_mitm(&self) -> Result<OperationResult, String> {
        let root = self.backend().ensure_mitm_root()?;
        let trusted = self.root_trusted(&root);
        let result = self.backend().set_https_decryption(true)?;
        let note = if trusted {
            ""
        } else {
            ". The root certificate is not trusted yet: choose Trust root certificate under Extensions"
        };
        Ok(OperationResult {
            message: format!("{}{note}", result.message),
        })
    }

    /// Android has no trust store an app may change: the root is saved to Downloads, the user installs it in
    /// the system settings, and only apps that trust user certificates (most browsers) accept it.
    #[cfg(target_os = "android")]
    fn enable_https_mitm(&self) -> Result<OperationResult, String> {
        let root = self.backend().ensure_mitm_root()?;
        let saved = fs::read_to_string(&root.certificate_path)
            .map_err(|error| format!("the root certificate could not be read: {error}"))
            .and_then(|pem| self.platform.save_to_downloads(ROOT_DOWNLOAD_NAME, &pem));
        let result = self.backend().set_https_decryption(true)?;
        let install = match saved {
            Ok(location) => format!(
                "The root certificate is saved as {location}; install it under Settings > Security > \
                 Encryption & credentials > Install a certificate > CA certificate"
            ),
            Err(error) => {
                format!("The root certificate could not be saved to Downloads: {error}")
            }
        };
        Ok(OperationResult {
            message: format!(
                "{}. {install}. Apps that ignore user certificates are not decrypted",
                result.message
            ),
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
        session.trusts_root(&pem).unwrap_or_else(trust::is_trusted)
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
            return Ok(OperationResult {
                message: trusted.summary("trusted"),
            });
        }
        let thumbprint = desktop_mitm::thumbprint(&pem)?;
        // Noted before the user answers: a root that was noted and then refused costs nothing, one that
        // was trusted and not noted would stay behind when the client is removed.
        self.backend().note_trusted_root(&thumbprint);
        session.trust_root(&pem)?;
        Ok(OperationResult {
            message: "Root certificate trusted for this user".to_string(),
        })
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
                Ok(OperationResult {
                    message: "Root certificate removed for this user".to_string(),
                })
            }
            None => Ok(OperationResult {
                message: trust::remove().summary("removed"),
            }),
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
                if path != last_path || samples % NETWORK_READ_EVERY == 0 {
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
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(10));
            if !client.backend().on_demand_enabled() {
                continue;
            }
            let Some(network) = client.platform.current_network(false) else {
                continue;
            };
            let action = client.backend().on_demand_action(network);
            match action {
                Some(OnDemandAction::Connect) => {
                    let _ = client.connect_through_tunnel();
                }
                Some(OnDemandAction::Disconnect) => {
                    let _ = client.backend().disconnect();
                    client.close_tunnel();
                }
                _ => {}
            }
        });
    }

    /// Updates every subscription whose interval has passed, checking once a minute; a failed update is
    /// notified.
    fn start_subscription_updates(self: &Arc<Self>) {
        let client = self.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(60));
            let due = client.backend().due_sources();
            for (id, location) in due {
                let fetched = fetch(&location);
                let mut backend = client.backend();
                let (content, meta) = match fetched {
                    Ok((content, meta)) => (Ok(content), Some(meta)),
                    Err(error) => (Err(error), None),
                };
                match (backend.finish_refresh(&id, content), meta) {
                    (Ok(_), Some(meta)) => backend.record_response(Some(&id), meta),
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
            client.backend().runtime_control(&request)
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
                Ok(format!("Mapping saved: {}", saved.pattern))
            })?)
        },
        "remove_debug_mapping" => |client, args| {
            let Id { id } = parse(args)?;
            json(client.change_plugins(move |directory| {
                mappings::remove(directory, &id)?;
                Ok("Mapping removed".to_string())
            })?)
        },
        "list_decrypt_hosts" => |client, _| {
            let directory = client.backend().plugins_dir();
            json(mappings::load(&directory)?.hosts)
        },
        "add_decrypt_host" => |client, args| {
            let Host { host } = parse(args)?;
            json(client.change_plugins(move |directory| {
                mappings::add_host(directory, &host)?;
                Ok(format!("Decrypting {host}"))
            })?)
        },
        "remove_decrypt_host" => |client, args| {
            let Host { host } = parse(args)?;
            json(client.change_plugins(move |directory| {
                mappings::remove_host(directory, &host)?;
                Ok(format!("No longer decrypting {host}"))
            })?)
        },
        "test_node_delay" => |client, args| {
            let NodeDelayArgs { node_id, url } = parse(args)?;
            let (nodes, id, name) = client
                .backend()
                .delay_target(&node_id)
                .ok_or("the node is not in any subscription")?;
            let delay = engine::node_delay(&nodes, &id, &url)?;
            json(OperationResult {
                message: format!("{name}: {}", format_delay(delay)),
            })
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
        "import_subscription" => |client, args| {
            let Location { location } = parse(args)?;
            json(client.import_subscription(&location)?)
        },
        "update_subscription" => |client, _| {
            let (id, location) = client
                .backend()
                .refreshable_location()
                .ok_or("the current subscription is not a link that can be updated")?;
            json(client.refresh_source(&id, &location)?)
        },
        "update_subscription_source" => |client, args| {
            let Id { id } = parse(args)?;
            let location = client
                .backend()
                .source_location(&id)
                .ok_or("this subscription is not a link that can be updated")?;
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
            json(client.change_plugins(move |directory| {
                let plugin = plugins::import(directory, &url, plugins::download)?;
                Ok(format!("Plugin imported: {}", plugin.name))
            })?)
        },
        "import_mitm_plugin_text" => |client, args| {
            let PluginText { text, source } = parse(args)?;
            json(client.change_plugins(move |directory| {
                let plugin = plugins::import_text(directory, &source, &text, plugins::download)?;
                Ok(format!("Plugin imported: {}", plugin.name))
            })?)
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
                    "Plugin enabled"
                } else {
                    "Plugin disabled"
                }
                .to_string())
            })?)
        },
        "remove_mitm_plugin" => |client, args| {
            let Id { id } = parse(args)?;
            json(client.change_plugins(move |directory| {
                plugins::remove(directory, &id)?;
                Ok("Plugin removed".to_string())
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
            json(
                client
                    .backend()
                    .import_mitm_root(&certificate_pem, &private_key_pem)?,
            )
        },
        "mitm_root_fingerprint" => |client, _| json(client.backend().mitm_root_fingerprint()),
        _ => return None,
    };
    Some(handler)
}

/// The name the decryption root gets in Downloads.
#[cfg(target_os = "android")]
const ROOT_DOWNLOAD_NAME: &str = "AnixOps-Net-HTTPS-root.crt";

fn json<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|error| format!("invalid arguments: {error}"))
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
struct DecodeBody {
    request: desktop_mitm::capture::DecodeRequest,
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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeDelayArgs {
    node_id: String,
    url: String,
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
struct Location {
    location: String,
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
fn http_get_with_meta(url: &str) -> Result<(String, ResponseMeta), reqwest::Error> {
    let licenses = engine::license_store();
    let (body, meta, offered) = http_get_once(url, licenses.as_ref())?;
    if offered {
        if let Ok((body, meta, _)) = http_get_once(url, licenses.as_ref()) {
            return Ok((body, meta));
        }
    }
    Ok((body, meta))
}

fn http_get_once(
    url: &str,
    licenses: Option<&anx_license::LicenseStore>,
) -> Result<(String, ResponseMeta, bool), reqwest::Error> {
    // The panel picks the subscription format by User-Agent (same token as the iOS app).
    let mut request = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("AnixOpsNet/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(url);
    if let Some(device) = licenses.and_then(|store| store.request_header(url).ok().flatten()) {
        request = request.header(anx_license::HEADER_DEVICE, device);
    }
    let response = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)?;
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
    Ok((response.text()?, meta, offered))
}

/// A URL's body with its headers' meta, a local file, or the pasted share links themselves.
fn fetch(location: &str) -> Result<(String, ResponseMeta), String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return http_get_with_meta(location)
            .map_err(|error| format!("the subscription could not be downloaded: {error}"));
    }
    if location.contains("://") {
        return Ok((location.to_string(), ResponseMeta::default()));
    }
    let path = match location.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map_or_else(
            || PathBuf::from(location),
            |home| PathBuf::from(home).join(rest),
        ),
        None => PathBuf::from(location),
    };
    fs::read_to_string(&path)
        .map(|content| (content, ResponseMeta::default()))
        .map_err(|error| format!("{} could not be read: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
