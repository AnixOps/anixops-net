//! The Tauri window: the shared React interface (`apps/windows-gui/ui`) over the Linux backend. Every command
//! the interface invokes goes to [`Client::invoke`] by name (`crate::client`, names and argument shapes as in the
//! interface's `bridge.ts`); this file only adds what needs Tauri: the window, plugins, `anixops://` links and
//! system notifications.
//!
//! The Android app (`apps/android`) runs this same window and backend. There the engine carries the system
//! VPN's TUN interface instead of a local proxy, and [`TunnelHost`] opens and closes that VPN.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use app_notifications::Notice;
use client_health::startup;
use tauri::ipc::{Invoke, InvokeBody};
use tauri::plugin::TauriPlugin;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Context, Emitter, Manager, RunEvent, Wry};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_notification::NotificationExt;

use crate::client::{self, Client, Platform};

/// The network a [`TunnelHost`] reports.
pub use crate::client::{Network, TunnelInterface};

/// For the cold start measurement; call first thing in `main`.
pub use client_health::startup::mark_process_start;

const NO_WINDOW: &str = "the window is not running yet";

/// Where the window runs: the data directory, the program autostart starts, extra plugins, and the system VPN
/// when the platform carries traffic through one.
pub struct Host {
    pub dir: PathBuf,
    pub executable: PathBuf,
    pub plugins: Vec<TauriPlugin<Wry>>,
    pub tunnel: Option<Box<dyn TunnelHost>>,
}

/// Opens the system VPN and hands back its TUN descriptor (Android's VpnService), and closes it again.
/// `open` may wait for the user to allow the VPN, so it is called off the async runtime.
pub trait TunnelHost: Send + Sync + 'static {
    fn open(&self, app: &AppHandle, interface: &TunnelInterface) -> Result<i32, String>;
    fn close(&self, app: &AppHandle);

    /// Puts a text file where the user can open it from the system settings (Android: Downloads) and returns
    /// where it went.
    fn save_to_downloads(
        &self,
        app: &AppHandle,
        name: &str,
        contents: &str,
    ) -> Result<String, String> {
        let _ = (app, name, contents);
        Err("this platform has no shared Downloads folder".to_string())
    }

    /// The key that seals saved capture sessions, as base64, kept by the platform's key store (Android: the
    /// Keystore), made on first use.
    fn capture_session_key(&self, app: &AppHandle) -> Result<String, String> {
        let _ = app;
        Err("this platform keeps the capture session key itself".to_string())
    }

    /// Puts a binary file, given as base64, in the same place as [`TunnelHost::save_to_downloads`] and
    /// returns where it went.
    fn save_base64_to_downloads(
        &self,
        app: &AppHandle,
        name: &str,
        mime_type: &str,
        data_base64: &str,
    ) -> Result<String, String> {
        let _ = (app, name, mime_type, data_base64);
        Err("this platform has no shared Downloads folder".to_string())
    }

    /// The network the device is on, for the Wi-Fi rules; `ask` asks for the permission that reveals the
    /// Wi-Fi name when it is withheld. `None` where the platform does not read it (Linux desktop).
    fn current_network(&self, app: &AppHandle, ask: bool) -> Option<Network> {
        let _ = (app, ask);
        None
    }
}

/// The client's [`Platform`] through Tauri: notifications from the notification plugin, everything else from
/// the [`TunnelHost`] when there is one. The app handle arrives once the window is set up.
struct TauriPlatform {
    app: OnceLock<AppHandle>,
    tunnel: Option<Box<dyn TunnelHost>>,
}

impl TauriPlatform {
    fn host(&self) -> Result<(&AppHandle, &dyn TunnelHost), String> {
        let app = self.app.get().ok_or(NO_WINDOW)?;
        let tunnel = self
            .tunnel
            .as_deref()
            .ok_or("this platform has no system VPN")?;
        Ok((app, tunnel))
    }
}

impl Platform for TauriPlatform {
    fn has_system_vpn(&self) -> bool {
        self.tunnel.is_some()
    }

    fn open_tunnel(&self, interface: &TunnelInterface) -> Result<i32, String> {
        let (app, tunnel) = self.host()?;
        tunnel.open(app, interface)
    }

    fn close_tunnel(&self) {
        if let Ok((app, tunnel)) = self.host() {
            tunnel.close(app);
        }
    }

    fn save_to_downloads(&self, name: &str, contents: &str) -> Result<String, String> {
        let (app, tunnel) = self.host()?;
        tunnel.save_to_downloads(app, name, contents)
    }

    fn save_base64_to_downloads(
        &self,
        name: &str,
        mime_type: &str,
        data_base64: &str,
    ) -> Result<String, String> {
        let (app, tunnel) = self.host()?;
        tunnel.save_base64_to_downloads(app, name, mime_type, data_base64)
    }

    fn capture_session_key(&self) -> Result<String, String> {
        let (app, tunnel) = self.host()?;
        tunnel.capture_session_key(app)
    }

    fn current_network(&self, ask: bool) -> Option<Network> {
        let (app, tunnel) = self.host().ok()?;
        tunnel.current_network(app, ask)
    }

    /// Shows one system notification; one the system refuses (no permission, no notification service) is
    /// dropped.
    fn post_notice(&self, notice: &Notice) {
        if let Some(app) = self.app.get() {
            let _ = app
                .notification()
                .builder()
                .title(&notice.title)
                .body(&notice.body)
                .show();
        }
    }
}

/// Answers an interface command on a blocking thread, so the window keeps polling meanwhile.
fn dispatch(client: &Arc<Client>, invoke: Invoke<Wry>) -> bool {
    let Invoke {
        message, resolver, ..
    } = invoke;
    let command = message.command().to_string();
    let args = match message.payload() {
        InvokeBody::Json(args) => args.clone(),
        InvokeBody::Raw(_) => {
            resolver.reject(format!("{command} takes JSON arguments"));
            return true;
        }
    };
    let client = client.clone();
    tauri::async_runtime::spawn_blocking(move || match client.invoke(&command, args) {
        Ok(answer) => resolver.resolve(answer),
        Err(error) => resolver.reject(error),
    });
    true
}

/// Keeps the links, brings the window forward and tells it to read them (`take_app_links`).
fn receive_app_links(app: &AppHandle, client: &Client, urls: Vec<String>) {
    if !client.receive_app_links(urls) {
        return;
    }
    // Android brings the activity forward itself when it opens the link.
    #[cfg(not(target_os = "android"))]
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("app-link", ());
}

/// Takes a link the app was started with and receives later ones. The desktop registers `anixops://` for this
/// user (a `.desktop` handler); a second start hands its link over through the single instance plugin. Android
/// declares the scheme in its manifest.
fn start_app_links(app: &AppHandle, client: &Arc<Client>) {
    let deep_link = app.deep_link();
    #[cfg(not(target_os = "android"))]
    let _ = deep_link.register_all();
    if let Ok(Some(urls)) = deep_link.get_current() {
        receive_app_links(app, client, urls.iter().map(ToString::to_string).collect());
    }
    let handle = app.clone();
    let client = client.clone();
    deep_link.on_open_url(move |event| {
        receive_app_links(
            &handle,
            &client,
            event.urls().iter().map(ToString::to_string).collect(),
        );
    });
}

pub fn run(context: Context<Wry>, host: Host) -> Result<(), String> {
    let Host {
        dir,
        executable,
        plugins,
        tunnel,
    } = host;
    let platform = Arc::new(TauriPlatform {
        app: OnceLock::new(),
        tunnel,
    });
    let client = Client::open(client::Config { dir, executable }, platform.clone());

    let mut builder = tauri::Builder::default();
    // One window per user: a second start (an `anixops://` link) hands over to the running one.
    #[cfg(target_os = "linux")]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    }
    builder = builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init());
    for plugin in plugins {
        builder = builder.plugin(plugin);
    }
    let app = builder
        .setup({
            let client = client.clone();
            move |app| {
                let _ = platform.app.set(app.handle().clone());
                start_app_links(app.handle(), &client);
                client.start_background();
                Ok(())
            }
        })
        .on_page_load(|webview, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            let ready = startup::first_page_loaded();
            // CI's cold start job: record the time and quit.
            if let Some(path) = startup::probe_path() {
                let _ = startup::write_probe(&path, ready);
                webview.app_handle().exit(0);
            }
        })
        .invoke_handler({
            let client = client.clone();
            move |invoke| dispatch(&client, invoke)
        })
        .build(context)
        .map_err(|error| error.to_string())?;
    app.run(move |_, event| {
        // Put the desktop's proxy settings back when the window closes; a crash leaves them for the next start.
        if let RunEvent::Exit = event {
            client.shutdown();
        }
    });
    Ok(())
}
