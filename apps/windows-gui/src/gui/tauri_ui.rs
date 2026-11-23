use super::actions::connection;
use super::capture_sessions;
use super::linked_sources::{self, CachedNode, OtherSource};
use super::mitm_plugins::{self, PluginSummary};
use super::root_trust;
use super::runtime_status::{read_runtime_status, WindowsRuntimeStatus};
use super::startup::{
    desktop_state_path, dns_settings_path, load_desktop_state, my_rules_path, owns_current_proxy,
    save_desktop_state, set_start_after_login, start_after_login_wanted, startup_repair,
    DesktopProfileNode, DesktopRoutingMode, DesktopState, DesktopSubscriptionSource, StartupRepair,
};
use super::ui_state::ConnectionState;
use super::{
    append_managed_log, is_trunk_node, load_validated_managed_configuration,
    managed_config_or_default, profile_node_options, write_diagnostic_report_at,
};
use app_notifications::{DisconnectWatch, Notice, Settings as NotificationSettings};
use client_health::{startup as cold_start, Health};
use config_core::routing_import::import_subscription_routing;
use config_core::CoreSubscriptionService;
use control_domain::{NodeDescriptor, SubscriptionService, SubscriptionSource};
use desktop_dns::DnsPreset;
use desktop_mitm::mappings;
use engine_embedded::config::{RuntimeConfig, CONFIG_SCHEMA_VERSION};
use engine_embedded::delay::measure_delay;
use engine_embedded::nodes::{outbound_for_node, routing_envelope_with_sets, single_node_envelope};
use engine_embedded::profile::{envelope_for_mode, RoutingMode};
use node_edits::{NodeEdits, NodeForm};
use on_demand::{
    Action as OnDemandAction, Current as CurrentNetwork, Network, Settings as OnDemandSettings,
    Watch as OnDemandWatch,
};
use platform_windows::managed::{
    read_managed_state, windows_managed_config_path, windows_managed_data_directory,
    windows_managed_log_directory, windows_managed_state_path, write_managed_config,
    write_managed_state, write_managed_text_atomic, WindowsManagedConfig,
    WindowsManagedEmbeddedConfig, WindowsManagedNativeMitmConfig, WindowsProxySettings,
    WindowsSystemProxyOwner,
};
use platform_windows::mitm_security::{
    protect_windows_managed_mitm_private_key, remove_windows_managed_mitm_private_key,
    windows_managed_mitm_private_key_path,
};
use platform_windows::system_integration::{
    current_user_startup_enabled, disable_current_user_startup, enable_current_user_startup,
    read_current_user_system_proxy, NativeWindowsSystemIntegration, WindowsSystemIntegration,
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::{
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_notification::NotificationExt;
use traffic_stats::{Counter as TrafficCounter, TrafficStats};

const APP_LOG_SCOPE: &str = "tauri-gui";
const MITM_CA_SUBJECT: &str = "AnixOps NetworkCore Windows HTTPS MITM CA";
const NODE_SELECTION_INTERVAL: Duration = Duration::from_secs(30 * 60);
const DEFAULT_NODE_SELECTION_URL: &str = "https://www.gstatic.com/generate_204";
const EMBEDDED_PROFILE_ID: &str = "windows-tauri-profile";
/// The embedded engine's listener, which is the system proxy unless HTTPS
/// MITM takes that port.
const EMBEDDED_LISTEN_PORT: u16 = 7890;
/// The embedded engine's listener behind HTTPS MITM, which relays to it.
const EMBEDDED_MITM_UPSTREAM_PORT: u16 = 7891;
const EMBEDDED_DELAY_TIMEOUT: Duration = Duration::from_secs(5);
/// Nodes the fastest-node search measures at once.
const EMBEDDED_DELAY_PARALLELISM: usize = 16;
/// How often the Wi-Fi rules read the Wi-Fi name when the network path stays the same.
const ON_DEMAND_POLL: Duration = Duration::from_secs(12);

#[derive(Clone)]
struct DesktopAppState {
    desktop: Arc<Mutex<DesktopState>>,
    lifecycle: Arc<RuntimeLifecycle>,
    direct_core: Arc<Mutex<connection::DesktopCore>>,
    /// Seven days of traffic by hour, host and policy (`traffic_stats`).
    traffic: Arc<Mutex<TrafficRecorder>>,
    /// The notification switches (`notification_settings`).
    notifications: Arc<Mutex<NotificationSettings>>,
    /// The Wi-Fi rules (`on_demand_settings`); a changed set is applied to the current network again.
    on_demand: Arc<Mutex<(OnDemandSettings, OnDemandWatch)>>,
}

/// The traffic statistics and what the last connection list held.
struct TrafficRecorder {
    stats: TrafficStats,
    counter: TrafficCounter,
    saved_at: Instant,
    /// Traffic counted since the statistics were last written; kept until a write succeeds.
    unsaved: bool,
}

impl TrafficRecorder {
    fn load() -> Self {
        Self {
            stats: TrafficStats::load(&traffic_stats_path()),
            counter: TrafficCounter::default(),
            saved_at: Instant::now(),
            unsaved: false,
        }
    }

    /// Takes in one connection list; traffic counted earlier is written once the minute is up even when this
    /// list added nothing, so a connection that went quiet is not left in memory only.
    fn ingest(&mut self, list: &Value, now: u64) {
        if self.counter.ingest(list, &mut self.stats, now) {
            self.unsaved = true;
        }
        if self.unsaved && self.saved_at.elapsed() >= Duration::from_secs(60) {
            self.save();
        }
    }

    /// Only a successful write clears the unsaved mark and starts the next minute; a failed one is tried again.
    fn save(&mut self) {
        if self.stats.save(&traffic_stats_path()).is_ok() {
            self.unsaved = false;
            self.saved_at = Instant::now();
        }
    }
}

fn on_demand_settings_path() -> PathBuf {
    windows_managed_data_directory().join("on-demand.json")
}

/// The network under the connection: the Wi-Fi name from `netsh`, otherwise Ethernet or another interface,
/// or offline when no route leaves the computer.
fn current_network_now(path: Option<&String>) -> Network {
    match on_demand::windows_wifi() {
        Some(ssid) => Network::Wifi(ssid),
        None if path.is_none() => Network::Offline,
        None => Network::Other,
    }
}

/// The Wi-Fi rules, changed to `update` first when given.
#[tauri::command]
async fn on_demand_settings(
    update: Option<OnDemandSettings>,
    state: State<'_, DesktopAppState>,
) -> Result<OnDemandSettings, String> {
    let mut guard = state
        .on_demand
        .lock()
        .map_err(|_| "Wi-Fi rules lock failed")?;
    if let Some(update) = update {
        let next = update
            .checked()
            .map_err(|name| format!("\"{name}\" is not a Wi-Fi name (1 to 32 bytes)"))?;
        if next != guard.0 {
            next.save(&on_demand_settings_path())
                .map_err(|error| format!("the Wi-Fi rules could not be saved: {error}"))?;
            guard.0 = next;
            guard.1.reset();
        }
    }
    Ok(guard.0.clone())
}

/// The network the computer is on, for "add the current Wi-Fi". Windows 11 withholds the Wi-Fi name until
/// location access is allowed in the system settings; `ask` has no prompt to show here.
#[tauri::command]
async fn current_network(ask: bool) -> Result<CurrentNetwork, String> {
    let _ = ask;
    run_blocking(|| {
        let path = network_watch::current_path();
        Ok(current_network_now(path.as_ref()).current())
    })
    .await
}

/// What the Wi-Fi rules want now, when the network is new and the connection is not already that way.
fn on_demand_action(state: &DesktopAppState, path: Option<&String>) -> Option<OnDemandAction> {
    let enabled = state
        .on_demand
        .lock()
        .map(|guard| guard.0.enabled)
        .unwrap_or(false);
    if !enabled {
        return None;
    }
    let network = current_network_now(path);
    let action = {
        let mut guard = state.on_demand.lock().ok()?;
        let (settings, watch) = &mut *guard;
        watch.observe(settings, network)?
    };
    let running = node_operations_runtime_running(state).ok()?;
    match action {
        OnDemandAction::Connect if !running => Some(action),
        OnDemandAction::Disconnect if running => Some(action),
        _ => None,
    }
}

fn notification_settings_path() -> PathBuf {
    windows_managed_data_directory().join("notifications.json")
}

/// The name the subscription at `location` is shown with.
fn subscription_name(state: &DesktopAppState, location: &str) -> String {
    state
        .desktop
        .lock()
        .ok()
        .and_then(|desktop| {
            desktop
                .subscription_sources
                .iter()
                .find(|source| source.location == location)
                .map(|source| {
                    subscription_meta::display_name(source.name.as_deref(), &source.location)
                })
        })
        .unwrap_or_else(|| subscription_meta::display_name(None, location))
}

fn notification_settings_now(state: &DesktopAppState) -> NotificationSettings {
    state
        .notifications
        .lock()
        .map(|settings| settings.clone())
        .unwrap_or_default()
}

/// Shows one system notification; one the system refuses is dropped.
fn post_notice(app: &AppHandle, notice: &Notice) {
    let _ = app
        .notification()
        .builder()
        .title(&notice.title)
        .body(&notice.body)
        .show();
}

/// The notification switches, changed to `update` first when given, with the texts in `language`.
#[tauri::command]
async fn notification_settings(
    update: Option<NotificationSettings>,
    language: String,
    state: State<'_, DesktopAppState>,
) -> Result<NotificationSettings, String> {
    let mut settings = state
        .notifications
        .lock()
        .map_err(|_| "notification settings lock failed")?;
    let mut next = update.unwrap_or_else(|| settings.clone());
    if !language.is_empty() {
        next.language = language;
    }
    if next != *settings {
        next.save(&notification_settings_path())
            .map_err(|error| format!("the notification settings could not be saved: {error}"))?;
        *settings = next;
    }
    Ok(settings.clone())
}

fn traffic_stats_path() -> PathBuf {
    windows_managed_data_directory().join("traffic-stats.json")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Counts the engine's connections into the traffic statistics every five seconds (saved once a minute and on
/// quit), and posts what scripts posted.
fn start_traffic_stats(app: AppHandle, state: DesktopAppState) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let (list, scripts) = state
            .direct_core
            .lock()
            .map(|core| {
                let read = |request: &[u8]| {
                    core.control(request)
                        .ok()
                        .and_then(|answer| serde_json::from_slice::<Value>(&answer).ok())
                };
                (
                    read(br#"{"cmd":"connections"}"#),
                    read(br#"{"cmd":"script_notifications"}"#),
                )
            })
            .unwrap_or_default();
        if let Some(answer) = scripts {
            for notice in notification_settings_now(&state).script_notices(&answer) {
                post_notice(&app, &notice);
            }
        }
        let Ok(mut recorder) = state.traffic.lock() else {
            continue;
        };
        let Some(list) = list else {
            recorder.counter.reset();
            continue;
        };
        recorder.ingest(&list, unix_now());
    });
}

/// The app's log files, by module: the window, the app and the native HTTPS decryption.
fn app_log_files() -> [(&'static str, PathBuf); 3] {
    let directory = windows_managed_log_directory();
    [
        ("gui", directory.join("gui.log")),
        ("app", directory.join(format!("{APP_LOG_SCOPE}.log"))),
        ("mitm", directory.join("native-mitm.log")),
    ]
}

/// The last lines of the app's logs, oldest first; `clear` empties them first.
#[tauri::command]
async fn app_logs(clear: bool) -> Result<Vec<app_log::Entry>, String> {
    let files = app_log_files();
    if clear {
        app_log::clear(&files)
            .map_err(|error| format!("the logs could not be cleared: {error}"))?;
    }
    Ok(app_log::read(&files, 500))
}

/// Seven days of traffic by hour, host and policy; `clear` empties it first.
#[tauri::command]
async fn traffic_stats(
    clear: bool,
    state: State<'_, DesktopAppState>,
) -> Result<TrafficStats, String> {
    let mut recorder = state
        .traffic
        .lock()
        .map_err(|_| "traffic statistics lock failed")?;
    if clear {
        recorder.stats.clear();
        recorder.save();
    }
    recorder.stats.prune(unix_now());
    Ok(recorder.stats.clone())
}

/// Crash reports, the metrics endpoint, the install check and the cold start.
struct HealthState(Arc<Health>);

struct DesktopTray {
    _icon: TrayIcon,
}

#[derive(Default)]
struct RuntimeLifecycle {
    gui_started_connection: Mutex<bool>,
    core_recovery_attempted: Mutex<bool>,
    subscription_refresh_active: Arc<Mutex<bool>>,
}

struct SubscriptionRefreshGuard {
    active: Arc<Mutex<bool>>,
}

impl Drop for SubscriptionRefreshGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            *active = false;
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSnapshot {
    connection: String,
    connection_label: String,
    core: StatusFact,
    proxy: StatusFact,
    selected_node: Option<String>,
    subscription: Option<String>,
    subscription_last_updated: Option<String>,
    subscription_last_attempt: Option<String>,
    subscription_next_attempt: Option<String>,
    subscription_refresh_result: String,
    subscription_added_node_count: usize,
    subscription_removed_node_count: usize,
    subscription_changed_node_count: usize,
    subscription_refresh_error_code: Option<String>,
    subscription_error: Option<String>,
    last_error: Option<String>,
    configuration_error: Option<String>,
    start_after_login: bool,
    auto_connect: bool,
    auto_recover_core: bool,
    auto_subscription_refresh: bool,
    auto_select_fastest_node: bool,
    tun_mode_enabled: bool,
    script_runtime_configured: bool,
    /// HTTPS decryption is on for the managed profile.
    https_decryption_enabled: bool,
    dark_theme: bool,
    routing_mode: String,
    dns_preset: &'static str,
    /// UDP to port 443 is refused so apps fall back from QUIC to TCP.
    block_quic: bool,
    /// Debug mode's direct egress (the engine runs with no node); only in
    /// desktop mode, where this window runs the engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    direct_egress: Option<bool>,
    /// The imported profile runs on the embedded engine.
    embedded_engine: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusFact {
    label: String,
    detail: Option<String>,
    tone: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeSummary {
    id: String,
    label: String,
    protocol: String,
    selected: bool,
    /// The subscription the node comes from.
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<String>,
    /// A node of the imported profile, which can be edited and removed (`node_edits`).
    editable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionSummary {
    id: String,
    location: String,
    selected: bool,
    last_successful_update: Option<String>,
    last_attempt: Option<String>,
    next_attempt: Option<String>,
    result: String,
    added_node_count: usize,
    removed_node_count: usize,
    changed_node_count: usize,
    error_code: Option<String>,
    last_update_error: Option<String>,
    name: String,
    node_count: Option<usize>,
    upload: Option<u64>,
    download: Option<u64>,
    total: Option<u64>,
    /// Unix seconds.
    expire: Option<u64>,
    update_interval_hours: u32,
    refreshable: bool,
    /// Its saved nodes were imported by an earlier version, which left the subscription's certificate pins out:
    /// they apply once it is updated or imported again.
    needs_reimport: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeGroupSummary {
    tag: String,
    group_type: String,
    selected: Option<String>,
    outbounds: Vec<String>,
    /// Display names for `outbounds`; empty when they are the names.
    outbound_labels: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationResult {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreferenceValues {
    start_after_login: bool,
    auto_connect: bool,
    auto_recover_core: bool,
    auto_subscription_refresh: bool,
    auto_select_fastest_node: bool,
    dark_theme: bool,
}

pub(super) fn run(debug: bool) -> Result<(), String> {
    let mut desktop = load_desktop_state()?;
    let _ = append_managed_log(APP_LOG_SCOPE, &format!("startup debug={debug}"));
    // Upgrades from the betas' MSI removed every user's logon task and reset
    // managed-config.json; both are put back from what the user chose.
    if reconcile_start_after_login(&mut desktop) {
        let _ = save_desktop_state(&desktop);
    }
    repair_managed_config(&desktop);
    // Saved capture sessions expire after a week even when the engine does not run to delete them.
    capture_sessions::purge_expired();
    desktop_dns::custom::use_file(dns_settings_path());
    desktop_dns::redirects::use_file(windows_managed_data_directory().join("redirects.json"));
    desktop_dns::data_sources::use_file(windows_managed_data_directory().join("data-sources.json"));
    desktop_dns::devices::use_file(windows_managed_data_directory().join("devices.json"));
    // The removed 国内外分流 DNS choice becomes the user's own settings (docs/no-presets.md). Until that
    // works it is applied as "自定义", never as a preset.
    if let Ok(adopted) = desktop_dns::adopt_legacy_split(desktop.dns_preset) {
        if adopted != desktop.dns_preset {
            desktop.dns_preset = adopted;
            let _ = save_desktop_state(&desktop);
        }
    }
    refresh_imported_profile(&mut desktop);
    let health_dir = desktop_state_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let health = Health::start(
        &health_dir,
        "windows",
        env!("CARGO_PKG_VERSION"),
        std::env::current_exe().unwrap_or_default(),
    );
    let direct_core = Arc::new(Mutex::new(connection::DesktopCore::default()));
    // The endpoint skips the engine while another command holds the core, rather than waiting on it.
    let probe = Arc::downgrade(&direct_core);
    health.serve_metrics(Arc::new(move || {
        let text = probe.upgrade().and_then(|core| {
            let answer = core
                .try_lock()
                .ok()?
                .control(br#"{"cmd":"metrics"}"#)
                .ok()?;
            let answer: Value = serde_json::from_slice(&answer).ok()?;
            client_health::metrics::engine_text(&answer)
        });
        let connected = text.is_some();
        (text, connected)
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .manage(AppLinks::default())
        .manage(DesktopAppState {
            desktop: Arc::new(Mutex::new(desktop)),
            lifecycle: Arc::new(RuntimeLifecycle::default()),
            direct_core,
            traffic: Arc::new(Mutex::new(TrafficRecorder::load())),
            notifications: Arc::new(Mutex::new(NotificationSettings::load(
                &notification_settings_path(),
            ))),
            on_demand: Arc::new(Mutex::new((
                OnDemandSettings::load(&on_demand_settings_path()),
                OnDemandWatch::default(),
            ))),
        })
        .manage(HealthState(health))
        .setup(|app| {
            install_tray(app)?;
            if let Some(window) = app.get_webview_window("main") {
                super::backdrop::apply(&window);
            }
            show_main_window(app);
            start_app_links(app.handle());
            let state = app.state::<DesktopAppState>().inner().clone();
            remove_legacy_service(&state);
            start_automatic_connection(state.clone());
            start_network_watch(state.clone());
            start_core_recovery_monitor(app.handle().clone(), state);
            start_subscription_refresh_monitor(
                app.handle().clone(),
                app.state::<DesktopAppState>().inner().clone(),
            );
            start_fastest_node_monitor(app.state::<DesktopAppState>().inner().clone());
            start_traffic_stats(
                app.handle().clone(),
                app.state::<DesktopAppState>().inner().clone(),
            );
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                if let Some(script) = super::backdrop::page_script() {
                    let _ = webview.eval(script);
                }
            }
            let _ = append_managed_log(
                APP_LOG_SCOPE,
                &format!("webview page load {:?}: {}", payload.event(), payload.url()),
            );
            if payload.event() == PageLoadEvent::Finished {
                let ready = cold_start::first_page_loaded();
                // CI's cold start job: record the time and quit.
                if let Some(path) = cold_start::probe_path() {
                    let _ = cold_start::write_probe(&path, ready);
                    webview.app_handle().exit(0);
                }
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            runtime_snapshot,
            list_nodes,
            list_native_groups,
            list_subscriptions,
            connect,
            disconnect,
            restart_service,
            validate_configuration,
            switch_node,
            select_native_group_outbound,
            runtime_control,
            export_capture,
            decode_body,
            save_body,
            open_body,
            capture_session,
            replay_request,
            list_debug_mappings,
            save_debug_mapping,
            remove_debug_mapping,
            list_decrypt_hosts,
            list_decrypt_domains,
            add_decrypt_host,
            edit_decrypt_host,
            set_decrypt_host_enabled,
            remove_decrypt_host,
            test_node_delay,
            test_node_delays,
            network_probe,
            node_share_link,
            traffic_stats,
            app_logs,
            notification_settings,
            on_demand_settings,
            current_network,
            traffic_redirects,
            add_traffic_redirect,
            save_traffic_redirects,
            data_sources,
            save_data_source,
            remove_data_source,
            devices,
            save_device,
            remove_device,
            take_app_links,
            node_form,
            edit_node,
            remove_node,
            select_fastest_node,
            save_preferences,
            create_diagnostics,
            health_status,
            set_crash_reports,
            import_subscription,
            update_subscription,
            select_subscription,
            remove_subscription,
            update_subscription_source,
            rename_subscription,
            set_subscription_interval,
            check_profile_runtime,
            restore_network_settings,
            set_routing_mode,
            set_dns_preset,
            set_block_quic,
            set_direct_egress,
            my_rules,
            save_my_rules,
            routing_warnings,
            custom_dns,
            save_custom_dns,
            configure_script_runtime,
            clear_script_runtime,
            enable_https_mitm,
            disable_https_mitm,
            mitm_root_status,
            open_mitm_root,
            list_mitm_plugins,
            import_mitm_plugin,
            import_mitm_plugin_text,
            mitm_plugin_details,
            set_mitm_plugin_enabled,
            remove_mitm_plugin,
            phone_qr_code,
            import_mitm_root,
            mitm_root_fingerprint,
            set_tun_mode,
            install_certificate,
            remove_certificate,
            install_driver,
            remove_driver,
        ])
        .run(tauri::generate_context!())
        .map_err(|error| error.to_string())
}

fn install_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show NetworkCore", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("Tauri default window icon is not configured")?;
    let tray = TrayIconBuilder::with_id("networkcore")
        .icon(icon)
        .tooltip("AnixOps Net")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                if let Some(state) = app.try_state::<DesktopAppState>() {
                    if let Ok(mut recorder) = state.traffic.lock() {
                        recorder.save();
                    }
                    stop_desktop_core_for_exit(state.inner().clone());
                }
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    app.manage(DesktopTray { _icon: tray });
    Ok(())
}

/// `anixops://` links the window has not read yet (`take_app_links`); the window acts on them.
#[derive(Default)]
struct AppLinks(Mutex<Vec<String>>);

/// Keeps the links, brings the window forward and tells it to read them.
fn receive_app_links(app: &AppHandle, urls: Vec<String>) {
    let urls: Vec<String> = urls
        .into_iter()
        .filter(|url| url.to_ascii_lowercase().starts_with("anixops:"))
        .collect();
    if urls.is_empty() {
        return;
    }
    if let Ok(mut links) = app.state::<AppLinks>().0.lock() {
        links.extend(urls);
    }
    show_main_window(app);
    let _ = app.emit("app-link", ());
}

/// Hands the window the `anixops://` links received since it last asked.
#[tauri::command]
fn take_app_links(links: State<'_, AppLinks>) -> Vec<String> {
    links
        .0
        .lock()
        .map(|mut links| std::mem::take(&mut *links))
        .unwrap_or_default()
}

/// Registers `anixops://` for this user, takes a link the app was started with, and receives later ones (a second
/// start hands its link over through the single instance plugin).
fn start_app_links(app: &AppHandle) {
    let deep_link = app.deep_link();
    if let Err(error) = deep_link.register_all() {
        let _ = append_managed_log(
            APP_LOG_SCOPE,
            &format!("anixops:// registration failed: {error}"),
        );
    }
    if let Ok(Some(urls)) = deep_link.get_current() {
        receive_app_links(app, urls.iter().map(ToString::to_string).collect());
    }
    let handle = app.clone();
    deep_link.on_open_url(move |event| {
        receive_app_links(
            &handle,
            event.urls().iter().map(ToString::to_string).collect(),
        );
    });
}

fn show_main_window<R: Runtime, M: Manager<R>>(app: &M) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn start_automatic_connection(state: DesktopAppState) {
    std::thread::spawn(move || {
        let desktop = match state.desktop.lock() {
            Ok(desktop) => desktop.clone(),
            Err(_) => {
                let _ = append_managed_log(APP_LOG_SCOPE, "automatic connection state lock failed");
                return;
            }
        };
        let connected = match node_operations_runtime_running(&state) {
            Ok(connected) => connected,
            Err(error) => {
                let _ = append_managed_log(
                    APP_LOG_SCOPE,
                    &format!("automatic connection skipped: {error}"),
                );
                return;
            }
        };
        if !connection::should_auto_connect(desktop.auto_connect, false, connected) {
            return;
        }
        match connect_window_core(&state, desktop) {
            Ok(connected) => match state.desktop.lock() {
                Ok(mut persisted) => {
                    persisted.proxy_snapshot = Some(connected.snapshot);
                    persisted.applied_proxy = Some(connected.applied_proxy);
                    if let Err(error) = save_desktop_state(&persisted) {
                        let _ = append_managed_log(
                            APP_LOG_SCOPE,
                            &format!("automatic connection state save failed: {error}"),
                        );
                    } else {
                        mark_gui_started_connection(&state);
                        let _ = append_managed_log(APP_LOG_SCOPE, "automatic connection completed");
                    }
                }
                Err(_) => {
                    let _ = append_managed_log(
                        APP_LOG_SCOPE,
                        "automatic connection completed but state lock failed",
                    );
                }
            },
            Err(error) => {
                let _ = append_managed_log(
                    APP_LOG_SCOPE,
                    &format!("automatic connection failed: {error}"),
                );
            }
        }
    });
}

/// Rebuilds the connections of the engine this window runs when the network
/// changes or the computer wakes (`network_watch`). The service-mode core and
/// the TUN core are in another process or keep their own sockets; they are
/// left alone. It also applies the Wi-Fi rules, reading the Wi-Fi name when
/// the path changes and every twelve seconds (a switch between two Wi-Fi
/// networks can keep the same address).
fn start_network_watch(state: DesktopAppState) {
    std::thread::spawn(move || {
        let mut watch = network_watch::NetworkWatch::new();
        let mut last_path = None;
        let mut wifi_read_at: Option<Instant> = None;
        loop {
            let path = network_watch::current_path();
            let due = wifi_read_at.is_none_or(|at| at.elapsed() >= ON_DEMAND_POLL);
            if path != last_path || due {
                last_path = path.clone();
                wifi_read_at = Some(Instant::now());
                match on_demand_action(&state, path.as_ref()) {
                    Some(OnDemandAction::Connect) => {
                        let _ = append_managed_log(APP_LOG_SCOPE, "Wi-Fi rules: connecting");
                        let result = connect_blocking(state.clone());
                        log_operation("Wi-Fi rules connect", &result);
                    }
                    Some(OnDemandAction::Disconnect) => {
                        let _ = append_managed_log(APP_LOG_SCOPE, "Wi-Fi rules: disconnecting");
                        let result = disconnect_blocking(state.clone());
                        log_operation("Wi-Fi rules disconnect", &result);
                    }
                    _ => {}
                }
            }
            if let Some(reset) = watch.observe(std::time::SystemTime::now(), path) {
                let answer = state
                    .direct_core
                    .lock()
                    .map_err(|_| "desktop core lock failed".to_string())
                    .and_then(|core| core.control(&reset.request()));
                if let Ok(answer) = answer {
                    let _ = append_managed_log(
                        APP_LOG_SCOPE,
                        &format!(
                            "{}: engine reset {}",
                            reset.reason.as_str(),
                            String::from_utf8_lossy(&answer)
                        ),
                    );
                }
            }
            std::thread::sleep(network_watch::POLL);
        }
    });
}

fn start_core_recovery_monitor(app: AppHandle, state: DesktopAppState) {
    let mut watch = DisconnectWatch::default();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        let desktop = match state.desktop.lock() {
            Ok(desktop) => desktop.clone(),
            Err(_) => continue,
        };
        // The engine runs in this window: it needs recovering when this run connected and it is no
        // longer running.
        let engine = match node_operations_runtime_running(&state) {
            Ok(true) => ConnectionState::Connected,
            Ok(false) => ConnectionState::CoreError,
            Err(_) => continue,
        };
        let gui_started = state
            .lifecycle
            .gui_started_connection
            .lock()
            .map(|started| *started)
            .unwrap_or(false);
        let should_recover = match state.lifecycle.core_recovery_attempted.lock() {
            Ok(mut attempted)
                if connection::should_restart_gui_started_core(
                    desktop.auto_recover_core,
                    *attempted,
                    gui_started,
                    engine,
                ) =>
            {
                *attempted = true;
                true
            }
            _ => false,
        };
        // A stop between a connect of this window and a disconnect is notified once.
        let running = engine == ConnectionState::Connected;
        if !gui_started {
            watch.disconnected();
        } else if running {
            watch.connected();
        } else if watch.stopped_unexpectedly(running) {
            let _ = append_managed_log(APP_LOG_SCOPE, "the connection stopped unexpectedly");
            if let Some(notice) = notification_settings_now(&state).disconnected(should_recover) {
                post_notice(&app, &notice);
            }
        }
        if !should_recover {
            continue;
        }
        let _ = append_managed_log(APP_LOG_SCOPE, "one controlled core recovery was requested");
        match connect_window_core(&state, desktop) {
            Ok(connected) => match state.desktop.lock() {
                Ok(mut persisted) => {
                    persisted.proxy_snapshot = Some(connected.snapshot);
                    persisted.applied_proxy = Some(connected.applied_proxy);
                    if let Err(error) = save_desktop_state(&persisted) {
                        let _ = append_managed_log(
                            APP_LOG_SCOPE,
                            &format!("core recovery state save failed: {error}"),
                        );
                    } else {
                        let _ = append_managed_log(
                            APP_LOG_SCOPE,
                            "one controlled core recovery completed",
                        );
                    }
                }
                Err(_) => {
                    let _ = append_managed_log(
                        APP_LOG_SCOPE,
                        "core recovery completed but state lock failed",
                    );
                }
            },
            Err(error) => {
                let _ = append_managed_log(
                    APP_LOG_SCOPE,
                    &format!("one controlled core recovery failed: {error}"),
                );
            }
        }
    });
}

/// Updates the subscription in use when its schedule says so (`subscription_next_update`), the same schedule the
/// interface shows, read from the saved state so it holds across restarts. A failure is retried after a growing
/// wait and notified once until the subscription updates again.
fn start_subscription_refresh_monitor(app: AppHandle, state: DesktopAppState) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        let due = match state.desktop.lock() {
            Ok(desktop) => desktop.profile_source_url.clone().filter(|location| {
                match desktop
                    .subscription_sources
                    .iter()
                    .find(|source| source.location == *location)
                {
                    Some(source) => subscription_next_update(&desktop, source)
                        .is_some_and(|at| unix_now() >= at),
                    // Not in the catalog yet: the first check brings it in.
                    None => desktop.auto_subscription_refresh,
                }
            }),
            Err(_) => None,
        };
        let Some(location) = due else {
            continue;
        };
        let name = subscription_name(&state, &location);
        match import_subscription_blocking(
            state.clone(),
            location.clone(),
            "Automatic subscription update completed.",
        ) {
            Ok(_) => {
                let _ =
                    append_managed_log(APP_LOG_SCOPE, "automatic subscription update completed");
            }
            Err(error) => {
                let _ = append_managed_log(
                    APP_LOG_SCOPE,
                    &format!("automatic subscription update skipped or failed: {error}"),
                );
                let first_failure = state.desktop.lock().is_ok_and(|desktop| {
                    desktop
                        .subscription_sources
                        .iter()
                        .find(|source| source.location == location)
                        .is_some_and(|source| subscription_meta::notify_failure(source.failures))
                });
                if first_failure {
                    if let Some(notice) =
                        notification_settings_now(&state).subscription_failed(&name, &error)
                    {
                        post_notice(&app, &notice);
                    }
                }
            }
        }
    });
}

/// When `source` updates on its own next, Unix seconds: only the subscription in use updates automatically, and
/// only while automatic updates are on. The interface shows this time and the monitor acts on it.
fn subscription_next_update(
    desktop: &DesktopState,
    source: &DesktopSubscriptionSource,
) -> Option<u64> {
    let in_use = desktop.profile_source_url.as_deref() == Some(source.location.as_str());
    if !desktop.auto_subscription_refresh || !in_use {
        return None;
    }
    source.schedule().next_update()
}

/// [`subscription_next_update`] as the interface reads it.
fn next_attempt_text(at: Option<u64>) -> Option<String> {
    at.map(|seconds| format!("unix-epoch-seconds:{seconds}"))
}

/// The next automatic update of the subscription in use, as the interface reads it.
fn profile_next_attempt(desktop: &DesktopState) -> Option<String> {
    let location = desktop.profile_source_url.as_deref()?;
    let source = desktop
        .subscription_sources
        .iter()
        .find(|source| source.location == location)?;
    next_attempt_text(subscription_next_update(desktop, source))
}

fn start_fastest_node_monitor(state: DesktopAppState) {
    std::thread::spawn(move || {
        let mut next_selection = Instant::now() + NODE_SELECTION_INTERVAL;
        loop {
            std::thread::sleep(Duration::from_secs(30));
            if Instant::now() < next_selection {
                continue;
            }
            next_selection = Instant::now() + NODE_SELECTION_INTERVAL;
            let enabled = state
                .desktop
                .lock()
                .map(|desktop| desktop.auto_select_fastest_node)
                .unwrap_or(false);
            if !enabled || !node_operations_runtime_running(&state).unwrap_or(false) {
                continue;
            }
            match select_fastest_node_blocking(state.clone()) {
                Ok(result) => {
                    let _ = append_managed_log(APP_LOG_SCOPE, &result.message);
                }
                Err(error) => {
                    let _ = append_managed_log(
                        APP_LOG_SCOPE,
                        &format!("automatic fastest-node selection skipped or failed: {error}"),
                    );
                }
            }
        }
    });
}

fn mark_gui_started_connection(state: &DesktopAppState) {
    if let Ok(mut started) = state.lifecycle.gui_started_connection.lock() {
        *started = true;
    }
}

fn mark_gui_connection_stopped(state: &DesktopAppState) {
    if let Ok(mut started) = state.lifecycle.gui_started_connection.lock() {
        *started = false;
    }
}

async fn run_blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("background desktop operation failed: {error}"))?
}

#[tauri::command]
async fn runtime_snapshot(state: State<'_, DesktopAppState>) -> Result<RuntimeSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || runtime_snapshot_blocking(state)).await
}

fn runtime_snapshot_blocking(state: DesktopAppState) -> Result<RuntimeSnapshot, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let direct_running = state
        .direct_core
        .lock()
        .map_err(|_| "desktop core lock failed")?
        .is_running()?;
    Ok(snapshot(&read_runtime_status(), &desktop, direct_running))
}

#[tauri::command]
fn list_nodes(state: State<'_, DesktopAppState>) -> Result<Vec<NodeSummary>, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let selected = |id: &str| desktop.profile_node_id.as_deref() == Some(id);
    let group = active_source_id(&desktop).and_then(|id| {
        desktop
            .subscription_sources
            .iter()
            .find(|source| source.id == id)
            .map(|source| subscription_meta::display_name(source.name.as_deref(), &source.location))
    });
    let mut nodes: Vec<NodeSummary> = desktop
        .profile_node_catalog
        .iter()
        .map(|node| NodeSummary {
            id: node.id.clone(),
            label: node.label.clone(),
            protocol: node.protocol.clone(),
            selected: selected(&node.id),
            group: group.clone(),
            editable: true,
        })
        .collect();
    // Other subscriptions follow, each under its own name.
    for other in other_sources(&desktop) {
        for node in &other.nodes {
            let id = other.outbound_id(&node.id);
            nodes.push(NodeSummary {
                selected: selected(&id),
                id,
                label: node.label.clone(),
                protocol: node.protocol.clone(),
                group: Some(other.name.clone()),
                editable: false,
            });
        }
    }
    Ok(nodes)
}

fn source_cache_directory() -> PathBuf {
    embedded_directory().join("sources")
}

/// The saved subscription the imported profile came from.
fn active_source_id(desktop: &DesktopState) -> Option<String> {
    desktop
        .profile_source_url
        .clone()
        .or_else(|| {
            desktop
                .profile_source_path
                .as_ref()
                .map(|path| path.display().to_string())
        })
        .map(|location| subscription_source_id(&location))
}

/// Every other saved subscription whose nodes were cached when it was last imported.
fn other_sources(desktop: &DesktopState) -> Vec<OtherSource> {
    let active = active_source_id(desktop);
    let directory = source_cache_directory();
    desktop
        .subscription_sources
        .iter()
        .filter(|source| Some(&source.id) != active.as_ref())
        .filter_map(|source| {
            Some(OtherSource {
                id: source.id.clone(),
                name: subscription_meta::display_name(source.name.as_deref(), &source.location),
                nodes: linked_sources::read_cache(&directory, &source.id)?,
            })
        })
        .filter(|other| !other.nodes.is_empty())
        .collect()
}

/// Caches the just imported profile's nodes under subscription `location`, so they can be listed and linked
/// while another subscription is in use.
fn cache_imported_nodes(location: &str, desktop: &DesktopState) -> Result<(), String> {
    let content = fs::read_to_string(embedded_nodes_path()).map_err(|error| error.to_string())?;
    let outbounds: serde_json::Map<String, Value> =
        serde_json::from_str(&content).map_err(|error| error.to_string())?;
    let nodes: Vec<CachedNode> = desktop
        .profile_node_catalog
        .iter()
        .filter_map(|node| {
            Some(CachedNode {
                id: node.id.clone(),
                label: node.label.clone(),
                protocol: node.protocol.clone(),
                outbound: outbounds.get(&node.id)?.clone(),
            })
        })
        .collect();
    linked_sources::write_cache(
        &source_cache_directory(),
        &subscription_source_id(location),
        &nodes,
    )
}

/// Links the other subscriptions into the imported profile again, when it routes by rules, and rewrites its
/// envelope. Group choices of the profile, then of `previous` (the profile an import replaced), are kept
/// where the group and member still exist.
fn relink_active_profile(
    desktop: &mut DesktopState,
    previous: Option<&Value>,
) -> Result<(), String> {
    let Some(embedded) = active_embedded_config()? else {
        return Ok(());
    };
    let Ok(current) = read_embedded_profile() else {
        return Ok(());
    };
    if current.get("routing").is_none() {
        return Ok(());
    }
    let mut profile = current.clone();
    linked_sources::unlink(&mut profile);
    linked_sources::link(&mut profile, &other_sources(desktop));
    carry_routed_group_defaults(&current, &mut profile);
    if let Some(previous) = previous {
        carry_routed_group_defaults(previous, &mut profile);
    }
    let rendered = profile.to_string();
    write_managed_text_atomic(&embedded_profile_path(), &rendered)
        .map_err(|error| error.to_string())?;
    let envelope = embedded_envelope_or_profile(
        rendered,
        desktop.routing_mode,
        desktop.dns_preset,
        desktop.block_quic,
    );
    write_managed_text_atomic(&embedded.envelope_path, &envelope)
        .map_err(|error| error.to_string())?;
    desktop.profile_node_id = routed_selected_node(&profile);
    Ok(())
}

/// Relinks after a saved subscription changed (renamed or removed); a running core picks it up when it
/// next connects.
fn relink_saved_profile(state: &DesktopAppState) -> Result<(), String> {
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    relink_active_profile(&mut desktop, None)?;
    save_desktop_state(&desktop)
}

#[tauri::command]
async fn list_native_groups(
    state: State<'_, DesktopAppState>,
) -> Result<Vec<NativeGroupSummary>, String> {
    let state = state.inner().clone();
    run_blocking(move || list_native_groups_blocking(state)).await
}

fn list_native_groups_blocking(state: DesktopAppState) -> Result<Vec<NativeGroupSummary>, String> {
    if active_embedded_config()?.is_none() {
        return Ok(Vec::new());
    }
    let Ok(profile) = read_embedded_profile() else {
        return Ok(Vec::new());
    };
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let mut nodes = desktop.profile_node_catalog.clone();
    for other in other_sources(&desktop) {
        nodes.extend(other.nodes.iter().map(|node| DesktopProfileNode {
            id: other.outbound_id(&node.id),
            label: node.label.clone(),
            protocol: node.protocol.clone(),
            outbound_tag: other.outbound_id(&node.id),
            trunk: false,
        }));
    }
    Ok(embedded_groups(&profile, &nodes))
}

#[tauri::command]
async fn select_native_group_outbound(
    group_tag: String,
    outbound_tag: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || select_native_group_outbound_blocking(group_tag, outbound_tag, state))
        .await
}

fn select_native_group_outbound_blocking(
    group_tag: String,
    outbound_tag: String,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    let embedded = require_embedded_config()?;
    select_embedded_group_member(&state, &embedded, &group_tag, &outbound_tag)
}

/// Live traffic and connections from the engine this window runs; the
/// service-mode core is in another process and answers nothing here.
#[tauri::command]
async fn runtime_control(
    request: Value,
    state: State<'_, DesktopAppState>,
) -> Result<Value, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let request = connection::activity_request(&request)?;
        let answer = state
            .direct_core
            .lock()
            .map_err(|_| "desktop core lock failed")?
            .control(&request)?;
        serde_json::from_slice(&answer).map_err(|error| error.to_string())
    })
    .await
}

/// Sends an edited copy of a captured request through the engine this
/// window runs (`desktop_mitm::replay`), trusting the HTTPS decryption root
/// while decryption is on.
#[tauri::command]
async fn replay_request(
    request: desktop_mitm::replay::ReplayRequest,
    state: State<'_, DesktopAppState>,
) -> Result<desktop_mitm::replay::ReplayResponse, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let proxy = state
            .direct_core
            .lock()
            .map_err(|_| "desktop core lock failed")?
            .local_addr()
            .ok_or(
                "connect in desktop mode first: replayed requests go through the running engine",
            )?;
        let root = managed_config_or_default()?
            .native_mitm
            .filter(|mitm| mitm.enabled)
            .and_then(|mitm| fs::read_to_string(mitm.ca_certificate_path).ok());
        let target = desktop_mitm::replay::Target {
            proxy,
            credentials: None,
            root_pem: root,
        };
        desktop_mitm::replay::send(&target, &request)
    })
    .await
}

/// Where a capture export was saved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedFile {
    location: String,
}

/// Exports the packet capture (`format` pcapng or har; one connection with `id`, else all) to the Downloads
/// folder without replacing an older file, and answers where it went.
#[tauri::command]
async fn export_capture(
    format: String,
    id: Option<u64>,
    options: Option<desktop_mitm::capture::ExportOptions>,
    state: State<'_, DesktopAppState>,
) -> Result<SavedFile, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let request =
            desktop_mitm::capture::export_request(&format, id, options.unwrap_or_default());
        let request = connection::activity_request(&request)?;
        let answer = state
            .direct_core
            .lock()
            .map_err(|_| "desktop core lock failed")?
            .control(&request)?;
        let answer: Value = serde_json::from_slice(&answer).map_err(|error| error.to_string())?;
        let export = desktop_mitm::capture::Export::from_answer(&answer)?;
        let dir = desktop_mitm::capture::downloads_dir()
            .ok_or("the Downloads folder could not be found")?;
        let path = desktop_mitm::capture::save_new_file(&dir, &export.file_name, &export.bytes()?)?;
        Ok(SavedFile {
            location: path.display().to_string(),
        })
    })
    .await
}

/// Undoes a captured body's `Content-Encoding` for the inspector with the kernel's decoder
/// (`desktop_mitm::capture::decode_body`): gzip, deflate, br and zstd, the same as on Apple and in exports.
#[tauri::command]
async fn decode_body(request: desktop_mitm::capture::DecodeRequest) -> Result<Value, String> {
    run_blocking(move || desktop_mitm::capture::decode_body(&request)).await
}

/// Saves a body as the inspector shows it (decoded) to the Downloads folder without replacing an older file, and
/// answers where it went.
#[tauri::command]
async fn save_body(file: desktop_mitm::capture::BodyFile) -> Result<SavedFile, String> {
    run_blocking(move || {
        let dir = desktop_mitm::capture::downloads_dir()
            .ok_or("the Downloads folder could not be found")?;
        let path = desktop_mitm::capture::save_body(&file, &dir)?;
        Ok(SavedFile {
            location: path.display().to_string(),
        })
    })
    .await
}

/// Opens a body with the app Windows uses for its type (`desktop_mitm::capture::open_body`: only types that run
/// no code where they open).
#[tauri::command]
async fn open_body(file: desktop_mitm::capture::BodyFile) -> Result<SavedFile, String> {
    run_blocking(move || {
        let path = desktop_mitm::capture::open_body(&file)?;
        Ok(SavedFile {
            location: path.display().to_string(),
        })
    })
    .await
}

/// Saved capture sessions: list, save what capture holds, load one for viewing, delete
/// (`desktop_mitm::sessions`). The engine is told where sessions go and the key, sealed with DPAPI,
/// before every action, since it forgets both when it restarts.
#[tauri::command]
async fn capture_session(
    action: desktop_mitm::sessions::SessionAction,
    state: State<'_, DesktopAppState>,
) -> Result<Value, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let request = action.request()?;
        let key = capture_sessions::load_key()?;
        let storage =
            desktop_mitm::sessions::storage_request(&capture_sessions::sessions_dir()?, &key);
        let to_bytes = |value: &Value| serde_json::to_vec(value).map_err(|error| error.to_string());
        let core = state
            .direct_core
            .lock()
            .map_err(|_| "desktop core lock failed")?;
        core.control(&to_bytes(&storage)?)?;
        let answer = core.control(&to_bytes(&request)?)?;
        drop(core);
        let answer: Value = serde_json::from_slice(&answer).map_err(|error| error.to_string())?;
        Ok(desktop_mitm::sessions::with_protection(
            answer,
            key.protection,
        ))
    })
    .await
}

#[tauri::command]
fn list_subscriptions(
    state: State<'_, DesktopAppState>,
) -> Result<Vec<SubscriptionSummary>, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    Ok(desktop
        .subscription_sources
        .iter()
        .map(|source| SubscriptionSummary {
            id: source.id.clone(),
            location: source.location.clone(),
            selected: desktop.profile_source_url.as_deref() == Some(source.location.as_str())
                || desktop
                    .profile_source_path
                    .as_ref()
                    .is_some_and(|path| path.display().to_string() == source.location),
            last_successful_update: source.last_successful_update.clone(),
            last_attempt: source.last_attempt.clone(),
            next_attempt: next_attempt_text(subscription_next_update(&desktop, source)),
            result: source.result.clone(),
            added_node_count: source.added_node_count,
            removed_node_count: source.removed_node_count,
            changed_node_count: source.changed_node_count,
            error_code: source.error_code.clone(),
            last_update_error: source.last_update_error.clone(),
            name: subscription_meta::display_name(source.name.as_deref(), &source.location),
            node_count: source.node_count,
            upload: source.user_info.and_then(|info| info.upload),
            download: source.user_info.and_then(|info| info.download),
            total: source.user_info.and_then(|info| info.total),
            expire: source.user_info.and_then(|info| info.expire),
            update_interval_hours: source.effective_interval_hours(),
            refreshable: is_subscription_url(&source.location),
            needs_reimport: linked_sources::predates_certificate_pins(
                &source_cache_directory(),
                &source.id,
            ),
        })
        .collect())
}

fn is_subscription_url(location: &str) -> bool {
    location.starts_with("https://") || location.starts_with("http://")
}

#[tauri::command]
fn rename_subscription(
    id: String,
    name: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let name = subscription_meta::clean_name(&name).ok_or("Enter a name.")?;
    change_subscription_source(&state, &id, |source| source.name = Some(name))?;
    // Linked policy groups are named after their subscriptions.
    relink_saved_profile(&state)?;
    Ok(OperationResult {
        message: "Subscription renamed.".to_string(),
    })
}

#[tauri::command]
fn set_subscription_interval(
    id: String,
    hours: u32,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let hours = hours.min(subscription_meta::MAX_UPDATE_INTERVAL_HOURS);
    change_subscription_source(&state, &id, |source| {
        source.update_interval_hours = Some(hours)
    })?;
    Ok(OperationResult {
        message: if hours == 0 {
            "Automatic updates are off for this subscription.".to_string()
        } else {
            format!("This subscription updates every {hours} hours while it is in use.")
        },
    })
}

fn change_subscription_source(
    state: &DesktopAppState,
    id: &str,
    change: impl FnOnce(&mut DesktopSubscriptionSource),
) -> Result<(), String> {
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let source = desktop
        .subscription_sources
        .iter_mut()
        .find(|source| source.id == id)
        .ok_or("The subscription was not found.")?;
    change(source);
    save_desktop_state(&desktop)
}

#[tauri::command]
async fn connect(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    let result = run_blocking(move || connect_blocking(state)).await;
    log_operation("connect", &result);
    result
}

/// Desktop mode runs the core inside this process, so the service state file
/// says nothing about it: record each connect / disconnect outcome in the app
/// log that the diagnostics report includes.
fn log_operation(operation: &str, result: &Result<OperationResult, String>) {
    let line = match result {
        Ok(done) => format!("{operation} ok: {}", done.message),
        Err(error) => format!("{operation} failed: {error}"),
    };
    let _ = append_managed_log(APP_LOG_SCOPE, &line);
}

fn connect_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    let connected = connect_window_core(&state, desktop)?;
    let mut persisted = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    persisted.proxy_snapshot = Some(connected.snapshot);
    persisted.applied_proxy = Some(connected.applied_proxy);
    save_desktop_state(&persisted)?;
    mark_gui_started_connection(&state);
    Ok(OperationResult {
        message: "Connected. The desktop-owned core and current-user proxy are verified."
            .to_string(),
    })
}

/// Starts the engine in this window on the saved profile and applies the proxy.
fn connect_window_core(
    state: &DesktopAppState,
    desktop: DesktopState,
) -> Result<connection::ConnectedProxy, String> {
    connection::connect_direct(
        windows_managed_config_path(),
        desktop,
        &mut *state
            .direct_core
            .lock()
            .map_err(|_| "desktop core lock failed")?,
    )
}

/// Removes the Windows service earlier versions could run the engine in
/// (`connection::remove_legacy_service`) and forgets the proxy it had set.
fn remove_legacy_service(state: &DesktopAppState) {
    let Ok(mut desktop) = state.desktop.lock() else {
        return;
    };
    match connection::remove_legacy_service(&desktop) {
        Ok(removal) if removal.removed => {
            if removal.proxy_restored {
                desktop.proxy_snapshot = None;
                desktop.applied_proxy = None;
                if let Err(error) = save_desktop_state(&desktop) {
                    let _ = append_managed_log(
                        APP_LOG_SCOPE,
                        &format!("legacy service proxy state save failed: {error}"),
                    );
                }
            }
            let _ = append_managed_log(
                APP_LOG_SCOPE,
                &format!(
                    "legacy Windows service removed; proxy restored={}",
                    removal.proxy_restored
                ),
            );
        }
        Ok(_) => {}
        Err(error) => {
            let _ = append_managed_log(
                APP_LOG_SCOPE,
                &format!("legacy Windows service could not be removed: {error}"),
            );
        }
    }
}

#[tauri::command]
async fn disconnect(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    let result = run_blocking(move || disconnect_blocking(state)).await;
    log_operation("disconnect", &result);
    result
}

fn disconnect_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    let message = connection::disconnect_direct(
        windows_managed_config_path(),
        desktop,
        &mut *state
            .direct_core
            .lock()
            .map_err(|_| "desktop core lock failed")?,
    )?;
    let mut persisted = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    persisted.proxy_snapshot = None;
    persisted.applied_proxy = None;
    save_desktop_state(&persisted)?;
    mark_gui_connection_stopped(&state);
    Ok(OperationResult { message })
}

fn stop_desktop_core_for_exit(state: DesktopAppState) {
    let desktop = match state.desktop.lock() {
        Ok(desktop) => desktop.clone(),
        Err(_) => return,
    };
    let result = state
        .direct_core
        .lock()
        .map_err(|_| "desktop core lock failed".to_string())
        .and_then(|mut core| {
            connection::disconnect_direct(windows_managed_config_path(), desktop, &mut core)
        });
    if let Err(error) = result {
        let _ = append_managed_log(
            APP_LOG_SCOPE,
            &format!("desktop exit cleanup failed: {error}"),
        );
    }
}

#[tauri::command]
async fn restart_service(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || restart_service_blocking(state)).await
}

fn restart_service_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    let _ = connection::disconnect_direct(
        windows_managed_config_path(),
        desktop.clone(),
        &mut *state
            .direct_core
            .lock()
            .map_err(|_| "desktop core lock failed")?,
    );
    let connected = connect_window_core(&state, desktop)?;
    let mut persisted = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    persisted.proxy_snapshot = Some(connected.snapshot);
    persisted.applied_proxy = Some(connected.applied_proxy);
    save_desktop_state(&persisted)?;
    Ok(OperationResult {
        message: "Desktop core restarted and proxy settings were reapplied.".to_string(),
    })
}

#[tauri::command]
async fn validate_configuration() -> Result<OperationResult, String> {
    run_blocking(validate_configuration_blocking).await
}

fn validate_configuration_blocking() -> Result<OperationResult, String> {
    load_validated_managed_configuration(&windows_managed_config_path())?;
    Ok(OperationResult {
        message: "Managed configuration and enabled core preflight succeeded.".to_string(),
    })
}

#[tauri::command]
async fn switch_node(
    node_id: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || switch_node_blocking(node_id, state)).await
}

fn switch_node_blocking(
    node_id: String,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    let update_running_core = node_operations_runtime_running(&state)?;
    if selected_catalog_node(&desktop, &node_id).is_err() {
        if let Some(other) = other_sources(&desktop)
            .into_iter()
            .find(|other| other.node(&node_id).is_some())
        {
            return switch_other_source_node(
                &state,
                &desktop,
                &other,
                &node_id,
                update_running_core,
            );
        }
    }
    let node = selected_catalog_node(&desktop, &node_id)?;
    let embedded = require_embedded_config()?;
    switch_embedded_node(&state, &desktop, &embedded, &node.id, update_running_core)?;
    Ok(OperationResult {
        message: if update_running_core {
            "Active node switched and the embedded engine restarted with it.".to_string()
        } else {
            "Active node saved. It will be applied when the core next connects.".to_string()
        },
    })
}

/// Switches to a node of another subscription: through its linked policy group when the imported profile
/// has one, otherwise by importing that subscription (disconnected only, as every import).
fn switch_other_source_node(
    state: &DesktopAppState,
    desktop: &DesktopState,
    other: &OtherSource,
    listed_id: &str,
    update_running_core: bool,
) -> Result<OperationResult, String> {
    let node = other
        .node(listed_id)
        .ok_or_else(|| "The node is not in any subscription.".to_string())?;
    let group = other.group_id();
    if let Ok(mut profile) = read_embedded_profile() {
        if set_routed_group_default(&mut profile, &group, listed_id).is_ok() {
            if let Some(main) = routed_switch_group_for_node(&profile, &group) {
                set_routed_group_default(&mut profile, &main, &group)?;
            }
            let embedded = require_embedded_config()?;
            let rendered = profile.to_string();
            let envelope = embedded_envelope_for_mode(
                &rendered,
                desktop.routing_mode,
                desktop.dns_preset,
                desktop.block_quic,
            )?;
            write_managed_text_atomic(&embedded_profile_path(), &rendered)
                .map_err(|error| error.to_string())?;
            write_managed_text_atomic(&embedded.envelope_path, &envelope)
                .map_err(|error| error.to_string())?;
            {
                let mut persisted = state
                    .desktop
                    .lock()
                    .map_err(|_| "desktop state lock failed")?;
                persisted.profile_node_id = routed_selected_node(&profile);
                save_desktop_state(&persisted)?;
            }
            if update_running_core {
                restart_service_blocking(state.clone())?;
            }
            return Ok(OperationResult {
                message: format!("Switched to {} through policy group {group}.", node.label),
            });
        }
    }
    if update_running_core {
        return Err(format!(
            "Disconnect first to use {}: its subscription is not part of the current profile's policy groups.",
            node.label
        ));
    }
    let location = desktop
        .subscription_sources
        .iter()
        .find(|source| source.id == other.id)
        .map(|source| source.location.clone())
        .ok_or_else(|| "The subscription was not found.".to_string())?;
    import_subscription_blocking(state.clone(), location, "Subscription selected.")?;
    // A routed subscription whose main group cannot pick the node runs on its own choices.
    Ok(
        switch_node_blocking(node.id.clone(), state.clone()).unwrap_or_else(|_| OperationResult {
            message: format!(
                "Switched to {}; its policy groups choose the node.",
                other.name
            ),
        }),
    )
}

#[tauri::command]
async fn test_node_delay(
    node_id: String,
    url: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || test_node_delay_blocking(state, node_id, url)).await
}

/// The link other clients import for a node of the profile or of another
/// subscription; Snell gives a Surge proxy line.
#[tauri::command]
async fn node_share_link(
    node_id: String,
    state: State<'_, DesktopAppState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?
            .clone();
        let (label, outbound) = match selected_catalog_node(&desktop, &node_id) {
            Ok(node) => {
                let outbound = embedded_node_outbound(&node.id)?;
                (node.label, outbound)
            }
            Err(error) => {
                let other = other_sources(&desktop);
                let node = other
                    .iter()
                    .find_map(|other| other.node(&node_id))
                    .ok_or(error)?;
                (node.label.clone(), node.outbound.clone())
            }
        };
        node_import::share_link(&label, &outbound)
            .map_err(|error| format!("This node cannot be shared: {error}"))
    })
    .await
}

/// The name, server and port of a node of the imported profile, and whether it can be removed.
#[tauri::command]
async fn node_form(node_id: String, state: State<'_, DesktopAppState>) -> Result<NodeForm, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?
            .clone();
        node_form_of(&desktop, &node_id)
    })
    .await
}

fn node_form_of(desktop: &DesktopState, node_id: &str) -> Result<NodeForm, String> {
    let node = selected_catalog_node(desktop, node_id)
        .map_err(|_| "Only nodes of the subscription in use can be edited.".to_string())?;
    let outbound = embedded_node_outbound(node_id)?;
    let profile = read_embedded_profile().ok();
    Ok(NodeForm {
        name: node.label,
        server: outbound["server"].as_str().unwrap_or_default().to_string(),
        port: outbound["port"]
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .unwrap_or_default(),
        keep_reason: node_edits::keep_reason(
            desktop.profile_node_catalog.len(),
            profile.as_ref(),
            node_id,
        ),
    })
}

#[tauri::command]
async fn edit_node(
    node_id: String,
    name: String,
    server: String,
    port: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let change = node_edits::check(&name, &server, &port)?;
        change_node_edits(&state, &node_id, |edits| edits.change(&node_id, change))?;
        Ok(OperationResult {
            message: "Node saved.".to_string(),
        })
    })
    .await
}

#[tauri::command]
async fn remove_node(
    node_id: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        change_node_edits(&state, &node_id, |edits| edits.remove(&node_id))?;
        Ok(OperationResult {
            message: "Node removed.".to_string(),
        })
    })
    .await
}

/// Changes the saved node edits, applies them to the imported profile from its kept content, and restarts a
/// running core on the result. Nothing changes when they cannot be applied.
fn change_node_edits(
    state: &DesktopAppState,
    node_id: &str,
    change: impl FnOnce(&mut NodeEdits),
) -> Result<(), String> {
    let _refresh_guard = acquire_subscription_refresh(state)?;
    // Everything here is local, so it runs on the state itself under the lock: a copy written back afterwards
    // would undo what another command saved meanwhile (third-party review F07).
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let original = desktop.clone();
    let form = node_form_of(&desktop, node_id)?;
    let path = node_edits_path();
    let before = NodeEdits::load(&path);
    let mut edits = before.clone();
    change(&mut edits);
    if edits.removed.contains(node_id) {
        if let Some(reason) = form.keep_reason {
            return Err(reason.to_string());
        }
    }
    edits.save(&path).map_err(|error| error.to_string())?;
    if let Err(error) = reapply_node_edits(&mut desktop) {
        let _ = before.save(&path);
        *desktop = original;
        let _ = reapply_node_edits(&mut desktop);
        return Err(error);
    }
    save_desktop_state(&desktop)?;
    drop(desktop);
    if node_operations_runtime_running(state)? {
        restart_service_blocking(state.clone())?;
    }
    Ok(())
}

/// Imports the kept content of the profile again with the saved node edits, keeping the routing mode, and
/// links the other subscriptions in as an import does.
fn reapply_node_edits(desktop: &mut DesktopState) -> Result<(), String> {
    let payload = fs::read_to_string(profile_source_text_path())
        .map_err(|_| "Update the subscription once before editing its nodes.".to_string())?;
    let previous = read_embedded_profile().ok();
    import_embedded_subscription(
        &payload,
        desktop.profile_source_path.clone(),
        desktop.profile_source_url.clone(),
        desktop,
        false,
        &NodeEdits::load(&node_edits_path()),
    )?;
    let location = desktop.profile_source_url.clone().or_else(|| {
        desktop
            .profile_source_path
            .as_ref()
            .map(|path| path.display().to_string())
    });
    if let Some(location) = location {
        // As in an import, a failed cache write only leaves the nodes out of other profiles' lists.
        let _ = cache_imported_nodes(&location, desktop);
    }
    relink_active_profile(desktop, previous.as_ref())
}

#[tauri::command]
async fn select_fastest_node(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || select_fastest_node_blocking(state)).await
}

fn select_fastest_node_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    let test_url = desktop
        .delay_test_url
        .as_deref()
        .unwrap_or(DEFAULT_NODE_SELECTION_URL);
    let embedded = require_embedded_config()?;
    select_fastest_embedded_node(&state, &desktop, &embedded, test_url)
}

fn test_node_delay_blocking(
    state: DesktopAppState,
    node_id: String,
    url: String,
) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    require_embedded_config()?;
    let delay = listed_node_delay(&desktop, &node_id, &url)?;
    Ok(OperationResult {
        message: format!("{} ms via {url}", delay.as_millis()),
    })
}

/// Delay through a node of the profile or of another subscription.
fn listed_node_delay(desktop: &DesktopState, node_id: &str, url: &str) -> Result<Duration, String> {
    match selected_catalog_node(desktop, node_id) {
        Ok(node) => measure_embedded_node_delay(&node.id, url),
        Err(error) => {
            let other = other_sources(desktop);
            let node = other
                .iter()
                .find_map(|other| other.node(node_id))
                .ok_or(error)?;
            let profile =
                linked_sources::node_profile(EMBEDDED_PROFILE_ID, CONFIG_SCHEMA_VERSION, node);
            measure_delay(profile.as_bytes(), url, EMBEDDED_DELAY_TIMEOUT)
                .map_err(|error| error.to_string())
        }
    }
}

/// One node's result in a batch delay test.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeDelay {
    node_id: String,
    delay_ms: Option<u64>,
    error: Option<String>,
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

/// The exit address and the direct delay for the overview; `connected` says whether the local proxy is up.
#[tauri::command]
async fn network_probe(connected: bool) -> Result<NetworkProbe, String> {
    run_blocking(move || Ok(probe_network(connected.then_some(EMBEDDED_LISTEN_PORT)))).await
}

/// Tests the listed nodes, up to 16 at a time, and reports each one.
#[tauri::command]
async fn test_node_delays(
    node_ids: Vec<String>,
    url: String,
    state: State<'_, DesktopAppState>,
) -> Result<Vec<NodeDelay>, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?
            .clone();
        require_embedded_config()?;
        let (desktop, url) = (&desktop, url.as_str());
        let mut results = Vec::with_capacity(node_ids.len());
        for chunk in node_ids.chunks(EMBEDDED_DELAY_PARALLELISM) {
            let delays: Vec<_> = std::thread::scope(|scope| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|id| scope.spawn(move || listed_node_delay(desktop, id, url)))
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .unwrap_or_else(|_| Err("delay test stopped".to_string()))
                    })
                    .collect()
            });
            for (node_id, delay) in chunk.iter().zip(delays) {
                results.push(match delay {
                    Ok(delay) => NodeDelay {
                        node_id: node_id.clone(),
                        delay_ms: Some(u64::try_from(delay.as_millis()).unwrap_or(u64::MAX)),
                        error: None,
                    },
                    Err(error) => NodeDelay {
                        node_id: node_id.clone(),
                        delay_ms: None,
                        error: Some(error),
                    },
                });
            }
        }
        Ok(results)
    })
    .await
}

/// The embedded engine config when an imported profile runs on it.
fn active_embedded_config() -> Result<Option<WindowsManagedEmbeddedConfig>, String> {
    Ok(managed_config_or_default()?
        .embedded
        .filter(|embedded| embedded.enabled))
}

fn require_embedded_config() -> Result<WindowsManagedEmbeddedConfig, String> {
    active_embedded_config()?.ok_or_else(|| "Import a profile first.".to_string())
}

/// The imported embedded profile (`profile.json`).
fn read_embedded_profile() -> Result<Value, String> {
    let content = fs::read_to_string(embedded_profile_path())
        .map_err(|error| format!("Import the profile again before using it: {error}"))?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

fn routed_groups(profile: &Value) -> &[Value] {
    profile
        .pointer("/routing/groups")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn routed_group<'a>(profile: &'a Value, id: &str) -> Option<&'a Value> {
    routed_groups(profile)
        .iter()
        .find(|group| group.get("id").and_then(Value::as_str) == Some(id))
}

fn group_members(group: &Value) -> Vec<String> {
    group
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// The member a group routes to now: a select group's default (when it is a
/// member) and otherwise the first member, as the engine decides.
fn group_choice(group: &Value) -> Option<String> {
    let members = group_members(group);
    let default = group
        .get("default")
        .and_then(Value::as_str)
        .filter(|_| group.get("type").and_then(Value::as_str) == Some("select"))
        .filter(|default| members.iter().any(|member| member == default));
    default
        .map(str::to_string)
        .or_else(|| members.into_iter().next())
}

/// The groups FINAL passes through, following each group's choice, and the
/// target it ends at.
fn routed_final_chain(profile: &Value) -> (Vec<String>, Option<String>) {
    let mut target = profile
        .pointer("/routing/rules")
        .and_then(Value::as_array)
        .and_then(|rules| rules.last())
        .and_then(Value::as_str)
        .and_then(|rule| rule.strip_prefix("FINAL,"))
        // `FINAL,<target>,dns-failed` keeps its option after the target.
        .and_then(|rest| rest.split(',').next())
        .map(|target| target.trim().to_string());
    let mut chain = Vec::new();
    while let Some(id) = target.clone() {
        if chain.contains(&id) {
            break;
        }
        let Some(group) = routed_group(profile, &id) else {
            break;
        };
        chain.push(id);
        target = group_choice(group);
    }
    (chain, target)
}

/// The node FINAL ends at, if it ends at a node.
fn routed_selected_node(profile: &Value) -> Option<String> {
    let (_, target) = routed_final_chain(profile);
    target.filter(|id| {
        profile
            .pointer(&format!("/routing/outbounds/{}", json_pointer_escape(id)))
            .is_some()
    })
}

fn json_pointer_escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

/// The first select group on FINAL's route that has `node_id` as a member.
fn routed_switch_group_for_node(profile: &Value, node_id: &str) -> Option<String> {
    let (chain, _) = routed_final_chain(profile);
    chain.into_iter().find(|id| {
        routed_group(profile, id).is_some_and(|group| {
            group.get("type").and_then(Value::as_str) == Some("select")
                && group_members(group).iter().any(|member| member == node_id)
        })
    })
}

/// Makes `member` the default of the select group `group_id`.
fn set_routed_group_default(
    profile: &mut Value,
    group_id: &str,
    member: &str,
) -> Result<(), String> {
    let group = profile
        .pointer_mut("/routing/groups")
        .and_then(Value::as_array_mut)
        .and_then(|groups| {
            groups
                .iter_mut()
                .find(|group| group.get("id").and_then(Value::as_str) == Some(group_id))
        })
        .ok_or_else(|| format!("Policy group {group_id} is not part of the imported profile."))?;
    if group.get("type").and_then(Value::as_str) != Some("select") {
        return Err(format!(
            "Policy group {group_id} chooses automatically; only select groups can be switched."
        ));
    }
    if !group_members(group)
        .iter()
        .any(|candidate| candidate == member)
    {
        return Err(format!(
            "{member} is not a member of policy group {group_id}."
        ));
    }
    group
        .as_object_mut()
        .ok_or_else(|| "policy group must be a JSON object".to_string())?
        .insert("default".to_string(), Value::String(member.to_string()));
    Ok(())
}

/// Copies select-group defaults from `previous` into `profile` where the
/// group and the member still exist.
fn carry_routed_group_defaults(previous: &Value, profile: &mut Value) {
    let defaults: Vec<(String, String)> = routed_groups(previous)
        .iter()
        .filter_map(|group| {
            Some((
                group.get("id")?.as_str()?.to_string(),
                group.get("default")?.as_str()?.to_string(),
            ))
        })
        .collect();
    for (group, member) in defaults {
        let _ = set_routed_group_default(profile, &group, &member);
    }
}

/// The routed profile's groups in the shape the policy group card shows,
/// with node ids labelled by their names.
fn embedded_groups(profile: &Value, nodes: &[DesktopProfileNode]) -> Vec<NativeGroupSummary> {
    let label = |member: &str| {
        nodes
            .iter()
            .find(|node| node.id == member)
            .map(|node| node.label.clone())
            .unwrap_or_else(|| member.to_string())
    };
    routed_groups(profile)
        .iter()
        .filter_map(|group| {
            let tag = group.get("id")?.as_str()?.to_string();
            let members = group_members(group);
            let group_type = match group.get("type")?.as_str()? {
                "select" => "selector",
                "url-test" => "urltest",
                other => other,
            }
            .to_string();
            Some(NativeGroupSummary {
                tag,
                group_type,
                selected: group_choice(group),
                outbound_labels: members.iter().map(|member| label(member)).collect(),
                outbounds: members,
            })
        })
        .collect()
}

/// Makes `member` the choice of the select group `group_id`, rebuilds the
/// envelope for the routing mode and restarts a running core on it.
fn select_embedded_group_member(
    state: &DesktopAppState,
    embedded: &WindowsManagedEmbeddedConfig,
    group_id: &str,
    member: &str,
) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    let mut profile = read_embedded_profile()?;
    set_routed_group_default(&mut profile, group_id, member)?;
    let rendered = profile.to_string();
    let envelope = embedded_envelope_for_mode(
        &rendered,
        desktop.routing_mode,
        desktop.dns_preset,
        desktop.block_quic,
    )?;
    write_managed_text_atomic(&embedded_profile_path(), &rendered)
        .map_err(|error| error.to_string())?;
    write_managed_text_atomic(&embedded.envelope_path, &envelope)
        .map_err(|error| error.to_string())?;
    {
        let mut persisted = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?;
        persisted.profile_node_id = routed_selected_node(&profile);
        save_desktop_state(&persisted)?;
    }
    let running = node_operations_runtime_running(state)?;
    if running {
        restart_service_blocking(state.clone())?;
    }
    Ok(OperationResult {
        message: if running {
            format!(
                "Policy group {group_id} now uses {member}; the embedded engine restarted with it."
            )
        } else {
            format!("Policy group {group_id} now uses {member}. It applies when the core next connects.")
        },
    })
}

/// Envelope with the one outbound saved for `node_id` at import.
/// The engine outbound entry of one node of the imported profile.
fn embedded_node_outbound(node_id: &str) -> Result<Value, String> {
    let content = fs::read_to_string(embedded_nodes_path())
        .map_err(|error| format!("Import the profile again before using its nodes: {error}"))?;
    let mut nodes: serde_json::Map<String, Value> =
        serde_json::from_str(&content).map_err(|error| error.to_string())?;
    nodes
        .remove(node_id)
        .ok_or_else(|| "Selected node is not part of the current imported profile.".to_string())
}

fn embedded_node_profile(node_id: &str) -> Result<String, String> {
    let outbound = embedded_node_outbound(node_id)?;
    Ok(json!({
        "schema_version": CONFIG_SCHEMA_VERSION,
        "profile_id": EMBEDDED_PROFILE_ID,
        "outbound": outbound,
    })
    .to_string())
}

/// Delay of `url` through the node's own outbound, measured in this process,
/// so it works while disconnected and never goes through the system proxy.
fn measure_embedded_node_delay(node_id: &str, url: &str) -> Result<Duration, String> {
    let profile = embedded_node_profile(node_id)?;
    measure_delay(profile.as_bytes(), url, EMBEDDED_DELAY_TIMEOUT)
        .map_err(|error| error.to_string())
}

/// Makes `node_id` the profile's outbound, reapplies the routing mode and,
/// when the core runs, restarts it on the new envelope.
fn switch_embedded_node(
    state: &DesktopAppState,
    desktop: &DesktopState,
    embedded: &WindowsManagedEmbeddedConfig,
    node_id: &str,
    update_running_core: bool,
) -> Result<(), String> {
    let current = read_embedded_profile()?;
    let profile = if current.get("routing").is_some() {
        // A routed profile keeps its rules: the node becomes the choice of the
        // select group it belongs to on the route FINAL takes.
        let mut routed = current;
        let group = routed_switch_group_for_node(&routed, node_id).ok_or_else(|| {
            "This node is not in the profile's main policy group; choose it under Policy groups."
                .to_string()
        })?;
        set_routed_group_default(&mut routed, &group, node_id)?;
        routed.to_string()
    } else {
        embedded_node_profile(node_id)?
    };
    write_managed_text_atomic(&embedded_profile_path(), &profile)
        .map_err(|error| error.to_string())?;
    let envelope = embedded_envelope_or_profile(
        profile,
        desktop.routing_mode,
        desktop.dns_preset,
        desktop.block_quic,
    );
    write_managed_text_atomic(&embedded.envelope_path, &envelope)
        .map_err(|error| error.to_string())?;
    {
        let mut persisted = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?;
        persisted.profile_node_id = Some(node_id.to_string());
        save_desktop_state(&persisted)?;
    }
    if update_running_core {
        restart_service_blocking(state.clone())?;
    }
    Ok(())
}

fn select_fastest_embedded_node(
    state: &DesktopAppState,
    desktop: &DesktopState,
    embedded: &WindowsManagedEmbeddedConfig,
    test_url: &str,
) -> Result<OperationResult, String> {
    // On a routed profile only nodes of the main policy group can be chosen.
    let routed = read_embedded_profile()
        .ok()
        .filter(|profile| profile.get("routing").is_some());
    let candidates: Vec<DesktopProfileNode> = desktop
        .profile_node_catalog
        .iter()
        .filter(|node| {
            routed
                .as_ref()
                .is_none_or(|profile| routed_switch_group_for_node(profile, &node.id).is_some())
        })
        .cloned()
        .collect();
    // Trunk nodes win over direct ones whenever one of them answers (anx
    // FUTURE-IMPROVEMENTS F-51); direct nodes are only picked when no trunk
    // node does.
    let mut fastest: Option<(&DesktopProfileNode, Duration)> = None;
    let mut fastest_trunk: Option<(&DesktopProfileNode, Duration)> = None;
    for chunk in candidates.chunks(EMBEDDED_DELAY_PARALLELISM) {
        let delays: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|node| scope.spawn(move || measure_embedded_node_delay(&node.id, test_url)))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().ok().and_then(Result::ok))
                .collect()
        });
        for (node, delay) in chunk.iter().zip(delays) {
            let Some(delay) = delay else { continue };
            if fastest.is_none_or(|(_, best)| delay < best) {
                fastest = Some((node, delay));
            }
            if node.trunk && fastest_trunk.is_none_or(|(_, best)| delay < best) {
                fastest_trunk = Some((node, delay));
            }
        }
    }
    let (node, delay) = fastest_trunk
        .or(fastest)
        .ok_or_else(|| "No imported node returned a successful delay test.".to_string())?;
    let delay_millis = delay.as_millis();
    if desktop.profile_node_id.as_deref() == Some(node.id.as_str()) {
        return Ok(OperationResult {
            message: format!(
                "{} is already the fastest responding node at {delay_millis} ms.",
                node.label
            ),
        });
    }
    let update_running_core = node_operations_runtime_running(state)?;
    switch_embedded_node(state, desktop, embedded, &node.id, update_running_core)?;
    Ok(OperationResult {
        message: format!("Selected fastest node: {} ({delay_millis} ms).", node.label),
    })
}

/// Whether the engine runs in this window.
fn node_operations_runtime_running(state: &DesktopAppState) -> Result<bool, String> {
    state
        .direct_core
        .lock()
        .map_err(|_| "desktop core lock failed")?
        .is_running()
}

#[tauri::command]
async fn save_preferences(
    preferences: PreferenceValues,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || save_preferences_blocking(preferences, state)).await
}

/// Makes the current user's logon task match what they chose: one they
/// turned on but that is gone (the betas' MSI removed every user's task when
/// it was upgraded) is registered again. Afterwards `start_after_login`
/// shows whether the task is there, which the settings page reads. Returns
/// whether the per-user choices changed and need saving.
fn reconcile_start_after_login(desktop: &mut DesktopState) -> bool {
    let (Ok(executable), Some(sid)) = (
        std::env::current_exe(),
        windows_data::account::current_user_sid(),
    ) else {
        return false;
    };
    let before = desktop.start_after_login_users.clone();
    let wanted = start_after_login_wanted(desktop, &sid);
    let present = match current_user_startup_enabled(&executable) {
        Ok(present) => present,
        Err(error) => {
            let _ = append_managed_log(
                APP_LOG_SCOPE,
                &format!("sign-in startup could not be checked: {}", error.message),
            );
            desktop.start_after_login = wanted;
            return desktop.start_after_login_users != before;
        }
    };
    let present = match startup_repair(wanted, present) {
        StartupRepair::Keep => present,
        StartupRepair::Register => match enable_current_user_startup(&executable) {
            Ok(()) => {
                let _ = append_managed_log(APP_LOG_SCOPE, "sign-in startup registered again");
                true
            }
            Err(error) => {
                let _ = append_managed_log(
                    APP_LOG_SCOPE,
                    &format!(
                        "sign-in startup could not be registered again: {}",
                        error.message
                    ),
                );
                false
            }
        },
    };
    if present && !wanted {
        // A task the user has is their choice, whatever the shared file said.
        set_start_after_login(desktop, Some(&sid), true);
    }
    desktop.start_after_login = present;
    desktop.start_after_login_users != before
}

/// Whether managed-config.json lost the imported profile's engine block while
/// the profile itself is still there: the betas' MSI removed the user's
/// managed-config.json when it was upgraded and put the empty default back.
fn embedded_block_lost(
    managed: &WindowsManagedConfig,
    desktop: &DesktopState,
    profile_files_present: bool,
) -> bool {
    let imported = desktop.profile_source_url.is_some()
        || desktop.profile_source_path.is_some()
        || !desktop.profile_node_catalog.is_empty();
    let running = managed
        .embedded
        .as_ref()
        .is_some_and(|embedded| embedded.enabled);
    imported && profile_files_present && !running
}

/// Builds the saved envelope again for this version, before anything connects. The envelope on disk is only
/// rewritten when the user changes something, so without this an upgrade would keep running what the earlier
/// version wrote:
/// - A profile an earlier importer built has no certificate pins in its outbounds: it is imported again from
///   the kept subscription content with the saved node edits, as a node edit does. Without kept content the
///   subscription list asks for an update instead (`needs_reimport`).
/// - Otherwise the envelope is written again from the imported profile, so the routing mode is the engine's
///   current one (the direct mode now starts the engine in its own direct mode, DNS included).
///
/// A failure goes to the log. The import writes its files one by one, so a failure part way can leave some of
/// them new: the desktop state is kept as the import left it and saved, never put back over newer files, and
/// the import revision is removed so the next start builds the profile again.
fn refresh_imported_profile(desktop: &mut DesktopState) {
    if !matches!(active_embedded_config(), Ok(Some(_))) || !embedded_profile_path().is_file() {
        return;
    }
    let current = fs::read_to_string(profile_import_revision_path())
        .is_ok_and(|revision| revision.trim() == linked_sources::IMPORT_REVISION);
    let outcome = if !current && profile_source_text_path().is_file() {
        // The import changes the desktop state only together with the files, so what it left is saved either
        // way.
        let built = reapply_node_edits(desktop);
        let saved = save_desktop_state(desktop);
        match built.and(saved) {
            Ok(()) => "imported profile built again with certificate pins".to_string(),
            Err(error) => {
                let _ = fs::remove_file(profile_import_revision_path());
                format!("imported profile could not be built again: {error}")
            }
        }
    } else {
        match rewrite_embedded_envelope(desktop, desktop.dns_preset) {
            Ok(()) => return,
            Err(error) => format!("saved envelope could not be written again: {error}"),
        }
    };
    let _ = append_managed_log(APP_LOG_SCOPE, &outcome);
}

/// Puts the engine block back into managed-config.json when an upgrade lost
/// it (see [`embedded_block_lost`]). HTTPS decryption is not turned back on:
/// nothing else records that it was on, and it is the user's to turn on.
fn repair_managed_config(desktop: &DesktopState) {
    let envelope_path = embedded_directory().join("envelope.json");
    let present = envelope_path.is_file() && embedded_profile_path().is_file();
    let mut managed = match managed_config_or_default() {
        Ok(managed) => managed,
        Err(error) => {
            let _ = append_managed_log(
                APP_LOG_SCOPE,
                &format!("managed configuration could not be read for repair: {error}"),
            );
            return;
        }
    };
    if !embedded_block_lost(&managed, desktop, present) {
        return;
    }
    set_embedded_listener(&mut managed, envelope_path, false);
    let outcome = match write_managed_config(&windows_managed_config_path(), &managed) {
        Ok(()) => "managed configuration repaired: the imported profile runs again".to_string(),
        Err(error) => format!("managed configuration repair failed: {error}"),
    };
    let _ = append_managed_log(APP_LOG_SCOPE, &outcome);
}

fn save_preferences_blocking(
    preferences: PreferenceValues,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let startup_enabled =
        current_user_startup_enabled(&executable).map_err(|error| error.to_string())?;
    if preferences.start_after_login && !startup_enabled {
        enable_current_user_startup(&executable).map_err(|error| error.to_string())?;
    } else if !preferences.start_after_login && startup_enabled {
        disable_current_user_startup(&executable).map_err(|error| error.to_string())?;
    }
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    set_start_after_login(
        &mut desktop,
        windows_data::account::current_user_sid().as_deref(),
        preferences.start_after_login,
    );
    desktop.auto_connect = preferences.auto_connect;
    desktop.auto_recover_core = preferences.auto_recover_core;
    desktop.auto_subscription_refresh = preferences.auto_subscription_refresh;
    desktop.profile_next_attempt = profile_next_attempt(&desktop);
    desktop.auto_select_fastest_node = preferences.auto_select_fastest_node;
    desktop.dark_theme = preferences.dark_theme;
    save_desktop_state(&desktop)?;
    Ok(OperationResult {
        message: "Desktop preferences saved.".to_string(),
    })
}

#[tauri::command]
async fn create_diagnostics(health: State<'_, HealthState>) -> Result<OperationResult, String> {
    let section = health.0.diagnostics();
    run_blocking(move || create_diagnostics_blocking(&section)).await
}

/// Crash reports, the metrics endpoint, the install check and the cold start, for the settings page.
#[tauri::command]
fn health_status(health: State<'_, HealthState>) -> Value {
    health.0.status()
}

#[tauri::command]
fn set_crash_reports(
    health: State<'_, HealthState>,
    enabled: bool,
) -> Result<OperationResult, String> {
    health
        .0
        .set_crash_reports(enabled)
        .map_err(|error| format!("The setting could not be saved: {error}"))?;
    Ok(OperationResult {
        message: if enabled {
            "Crash reports on.".to_string()
        } else {
            "Crash reports off; kept reports deleted.".to_string()
        },
    })
}

fn create_diagnostics_blocking(health: &str) -> Result<OperationResult, String> {
    let path = write_diagnostic_report_at(&windows_managed_config_path(), health)?;
    Ok(OperationResult {
        message: format!("Diagnostics written to {}", path.display()),
    })
}

#[tauri::command]
async fn import_subscription(
    location: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || import_subscription_blocking(state, location, "Subscription imported."))
        .await
}

#[tauri::command]
async fn update_subscription(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let location = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?
            .profile_source_url
            .clone()
            .ok_or_else(|| {
                "Import an HTTP or HTTPS subscription URL before updating it.".to_string()
            })?;
        import_subscription_blocking(state, location, "Subscription updated.")
    })
    .await
}

/// Windows runs one subscription at a time, so only the one in use can be updated in place.
#[tauri::command]
async fn update_subscription_source(
    id: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let location = {
            let desktop = state
                .desktop
                .lock()
                .map_err(|_| "desktop state lock failed")?;
            let source = desktop
                .subscription_sources
                .iter()
                .find(|source| source.id == id)
                .ok_or_else(|| "The subscription was not found.".to_string())?;
            if desktop.profile_source_url.as_deref() != Some(source.location.as_str()) {
                return Err(
                    "Use this subscription first; it is updated when it is used.".to_string(),
                );
            }
            source.location.clone()
        };
        import_subscription_blocking(state, location, "Subscription updated.")
    })
    .await
}

#[tauri::command]
async fn select_subscription(
    id: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let location = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?
            .subscription_sources
            .iter()
            .find(|source| source.id == id)
            .map(|source| source.location.clone())
            .ok_or_else(|| "Selected subscription source was not found.".to_string())?;
        import_subscription_blocking(state, location, "Subscription selected.")
    })
    .await
}

#[tauri::command]
async fn remove_subscription(
    id: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || remove_subscription_blocking(id, state)).await
}

fn remove_subscription_blocking(
    id: String,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let before = desktop.subscription_sources.len();
    desktop
        .subscription_sources
        .retain(|source| source.id != id);
    if desktop.subscription_sources.len() == before {
        return Err("Selected subscription source was not found.".to_string());
    }
    linked_sources::remove_cache(&source_cache_directory(), &id);
    relink_active_profile(&mut desktop, None)?;
    save_desktop_state(&desktop)?;
    Ok(OperationResult {
        message: "Subscription source removed from the catalog.".to_string(),
    })
}

fn import_subscription_blocking(
    state: DesktopAppState,
    location: String,
    message: &'static str,
) -> Result<OperationResult, String> {
    let _refresh_guard = acquire_subscription_refresh(&state)?;
    // Only the download runs without the lock. The attempt and the result are written into the state as it is
    // then, never a copy taken before the download, so settings another command saved meanwhile stay
    // (third-party review F07).
    {
        let mut desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?;
        record_subscription_attempt(&mut desktop, &location, super::current_local_timestamp());
        save_desktop_state(&desktop)?;
    }
    let fetched = read_subscription(&location);
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    // Removed while it was downloading: it stays removed, and nothing of it is written again.
    let id = subscription_source_id(&location);
    if !desktop
        .subscription_sources
        .iter()
        .any(|source| source.id == id)
    {
        return Err("The subscription was removed while it was updating.".to_string());
    }
    record_subscription_import(&location, fetched, &mut desktop)?;
    Ok(OperationResult {
        message: message.to_string(),
    })
}

fn acquire_subscription_refresh(
    state: &DesktopAppState,
) -> Result<SubscriptionRefreshGuard, String> {
    let mut active = state
        .lifecycle
        .subscription_refresh_active
        .lock()
        .map_err(|_| "subscription refresh lock failed")?;
    if *active {
        return Err(
            "Subscription refresh is already active; wait for the current attempt to finish."
                .to_string(),
        );
    }
    *active = true;
    Ok(SubscriptionRefreshGuard {
        active: state.lifecycle.subscription_refresh_active.clone(),
    })
}

#[tauri::command]
async fn check_profile_runtime(
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || check_profile_runtime_blocking(state)).await
}

/// Checks that the embedded engine accepts the profile it would run.
fn check_profile_runtime_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let embedded = require_embedded_config()?;
    let envelope = fs::read(&embedded.envelope_path)
        .map_err(|error| format!("Import the profile again before checking it: {error}"))?;
    RuntimeConfig::from_json(&envelope).map_err(|error| {
        format!(
            "The embedded engine rejects this profile: {}",
            error.message
        )
    })?;
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let active = desktop
        .profile_node_id
        .as_deref()
        .and_then(|id| {
            desktop
                .profile_node_catalog
                .iter()
                .find(|node| node.id == id)
        })
        .map(|node| node.label.clone())
        .unwrap_or_else(|| "the profile's rules".to_string());
    Ok(OperationResult {
        message: format!(
            "Profile ready: {active} ({} nodes)",
            desktop.profile_node_catalog.len()
        ),
    })
}

/// Applies a finished download (`fetched`) to the state and saves it; the attempt is recorded already.
fn record_subscription_import(
    location: &str,
    fetched: Result<SubscriptionRead, String>,
    desktop: &mut DesktopState,
) -> Result<(), String> {
    let previous_nodes = desktop.profile_node_catalog.clone();
    match fetched.and_then(|read| import_subscription_at(location, read, desktop)) {
        Ok(meta) => {
            let timestamp = super::current_local_timestamp();
            let (added_node_count, removed_node_count, changed_node_count) =
                subscription_node_change_counts(&previous_nodes, &desktop.profile_node_catalog);
            desktop.profile_last_successful_update = Some(timestamp.clone());
            record_subscription_outcome(desktop, location, true);
            desktop.profile_refresh_result = "success".to_string();
            desktop.profile_added_node_count = added_node_count;
            desktop.profile_removed_node_count = removed_node_count;
            desktop.profile_changed_node_count = changed_node_count;
            desktop.profile_refresh_error_code = None;
            desktop.profile_last_update_error = None;
            record_subscription_result(
                desktop,
                location,
                Some(timestamp),
                "success",
                (added_node_count, removed_node_count, changed_node_count),
                None,
            );
            record_subscription_meta(desktop, location, meta);
            save_desktop_state(desktop)
        }
        Err(error) => {
            let error_code = redacted_subscription_error_code(&error);
            record_subscription_outcome(desktop, location, false);
            desktop.profile_refresh_result = "failed".to_string();
            desktop.profile_added_node_count = 0;
            desktop.profile_removed_node_count = 0;
            desktop.profile_changed_node_count = 0;
            desktop.profile_refresh_error_code = Some(error_code.clone());
            desktop.profile_last_update_error =
                Some(format!("Subscription refresh failed ({error_code})."));
            // Only the code goes to the app log, which the diagnostics report includes: the raw error can
            // carry the subscription URL's token or a piece of the payload.
            let _ = append_managed_log(
                APP_LOG_SCOPE,
                &format!("subscription refresh failed: {error_code}"),
            );
            record_subscription_result(
                desktop,
                location,
                None,
                "failed",
                (0, 0, 0),
                Some(error_code.clone()),
            );
            let _ = save_desktop_state(desktop);
            Err(format!("Subscription refresh failed ({error_code})."))
        }
    }
}

/// Keeps what the server said about the subscription. The server's title names it only until the user
/// renames it, and its traffic figures stay when a later download leaves the header out.
fn record_subscription_meta(
    desktop: &mut DesktopState,
    location: &str,
    meta: subscription_meta::ResponseMeta,
) {
    let id = subscription_source_id(location);
    let node_count = desktop.profile_node_catalog.len();
    if let Some(source) = desktop
        .subscription_sources
        .iter_mut()
        .find(|source| source.id == id)
    {
        if source.name.is_none() {
            source.name = meta.title;
        }
        if meta.user_info.is_some() {
            source.user_info = meta.user_info;
        }
        if meta.update_interval_hours.is_some() {
            source.suggested_interval_hours = meta.update_interval_hours;
        }
        source.node_count = Some(node_count);
    }
}

fn record_subscription_attempt(desktop: &mut DesktopState, location: &str, attempt: String) {
    desktop.profile_last_attempt = Some(attempt.clone());
    desktop.profile_refresh_result = "running".to_string();
    desktop.profile_refresh_error_code = None;
    let id = subscription_source_id(location);
    let source = desktop
        .subscription_sources
        .iter_mut()
        .find(|source| source.id == id);
    if let Some(source) = source {
        source.last_attempt = Some(attempt);
        source.last_attempt_unix = Some(unix_now());
        source.result = "running".to_string();
        source.error_code = None;
    } else {
        desktop
            .subscription_sources
            .push(DesktopSubscriptionSource {
                id,
                location: location.to_string(),
                last_attempt: Some(attempt),
                last_successful_update: None,
                next_attempt: None,
                last_success_unix: None,
                last_attempt_unix: Some(unix_now()),
                failures: 0,
                result: "running".to_string(),
                added_node_count: 0,
                removed_node_count: 0,
                changed_node_count: 0,
                error_code: None,
                last_update_error: None,
                name: None,
                user_info: None,
                node_count: None,
                update_interval_hours: None,
                suggested_interval_hours: None,
            });
    }
}

fn record_subscription_result(
    desktop: &mut DesktopState,
    location: &str,
    successful_update: Option<String>,
    result: &str,
    node_counts: (usize, usize, usize),
    error_code: Option<String>,
) {
    let (added_node_count, removed_node_count, changed_node_count) = node_counts;
    let id = subscription_source_id(location);
    let next_attempt = desktop.profile_next_attempt.clone();
    if let Some(source) = desktop
        .subscription_sources
        .iter_mut()
        .find(|source| source.id == id)
    {
        source.last_successful_update = successful_update;
        source.next_attempt = next_attempt;
        source.result = result.to_string();
        source.added_node_count = added_node_count;
        source.removed_node_count = removed_node_count;
        source.changed_node_count = changed_node_count;
        source.error_code = error_code.clone();
        source.last_update_error = error_code.map(|_| "refresh_failed".to_string());
        return;
    }
    desktop
        .subscription_sources
        .push(DesktopSubscriptionSource {
            id,
            location: location.to_string(),
            last_attempt: desktop.profile_last_attempt.clone(),
            last_successful_update: successful_update,
            next_attempt,
            result: result.to_string(),
            added_node_count,
            removed_node_count,
            changed_node_count,
            error_code: error_code.clone(),
            last_update_error: error_code.map(|_| "refresh_failed".to_string()),
            name: None,
            user_info: None,
            node_count: None,
            update_interval_hours: None,
            suggested_interval_hours: None,
            last_success_unix: None,
            last_attempt_unix: Some(unix_now()),
            failures: 0,
        });
}

/// Records how an update of the subscription at `location` ended, in the times its schedule is worked out from,
/// and the next automatic update that follows (`subscription_next_update`).
fn record_subscription_outcome(desktop: &mut DesktopState, location: &str, succeeded: bool) {
    let id = subscription_source_id(location);
    let now = unix_now();
    if let Some(source) = desktop
        .subscription_sources
        .iter_mut()
        .find(|source| source.id == id)
    {
        source.last_attempt_unix = Some(now);
        if succeeded {
            source.last_success_unix = Some(now);
            source.failures = 0;
        } else {
            source.failures = source.failures.saturating_add(1);
        }
    }
    desktop.profile_next_attempt = desktop
        .subscription_sources
        .iter()
        .find(|source| source.id == id)
        .and_then(|source| next_attempt_text(subscription_next_update(desktop, source)));
}

fn redacted_subscription_error_code(error: &str) -> String {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("timed out") || normalized.contains("timeout") {
        "windows.subscription.fetch_timeout".to_string()
    } else if let Some(status) = normalized
        .strip_prefix("subscription server answered http status ")
        .and_then(|status| status.parse::<u16>().ok())
    {
        format!("windows.subscription.http_status_{status}")
    } else if normalized.starts_with("subscription server could not be reached") {
        "windows.subscription.fetch_unreachable".to_string()
    } else if normalized.starts_with("subscription download")
        || normalized.contains("status")
        || normalized.contains("http")
    {
        "windows.subscription.fetch_failed".to_string()
    } else if normalized.contains("disconnect") {
        "windows.subscription.runtime_active".to_string()
    } else if normalized.contains("supported proxy node") || normalized.contains("parse") {
        "windows.subscription.invalid_payload".to_string()
    } else {
        "windows.subscription.refresh_failed".to_string()
    }
}

fn subscription_node_change_counts(
    previous: &[DesktopProfileNode],
    current: &[DesktopProfileNode],
) -> (usize, usize, usize) {
    let added = current
        .iter()
        .filter(|node| !previous.iter().any(|before| before.id == node.id))
        .count();
    let removed = previous
        .iter()
        .filter(|node| !current.iter().any(|after| after.id == node.id))
        .count();
    let changed = current
        .iter()
        .filter(|node| {
            previous
                .iter()
                .find(|before| before.id == node.id)
                .is_some_and(|before| {
                    before.label != node.label
                        || before.protocol != node.protocol
                        || before.outbound_tag != node.outbound_tag
                })
        })
        .count();
    (added, removed, changed)
}

fn subscription_source_id(location: &str) -> String {
    format!(
        "source-{:x}",
        location.bytes().fold(0_u64, |hash, byte| {
            hash.wrapping_mul(16777619).wrapping_add(u64::from(byte))
        })
    )
}

fn import_subscription_at(
    location: &str,
    read: SubscriptionRead,
    desktop: &mut DesktopState,
) -> Result<subscription_meta::ResponseMeta, String> {
    let previous = read_embedded_profile().ok();
    let (payload, source_path, source_url, meta) = read;
    // An update brings back what the provider serves (`node_edits`). The new content is parsed and imported first,
    // without the old edits; only then is it kept (so edits can be applied to it later without fetching it again)
    // and the old edits dropped. A refresh that fails leaves the old content and its edits together
    // (third-party review F09).
    import_embedded_subscription(
        &payload,
        source_path,
        source_url,
        desktop,
        true,
        &NodeEdits::default(),
    )?;
    write_managed_text_atomic(&profile_source_text_path(), &payload)
        .map_err(|error| error.to_string())?;
    match fs::remove_file(node_edits_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("The old node edits could not be dropped: {error}")),
    }
    // Listing and linking work without this subscription's cache, so a failed write only leaves it out.
    let _ = cache_imported_nodes(location, desktop);
    relink_active_profile(desktop, previous.as_ref())?;
    Ok(meta)
}

/// Runs a subscription whose nodes the embedded engine supports in-process:
/// the GUI in desktop mode, the service otherwise.
fn import_embedded_subscription(
    payload: &str,
    source_path: Option<PathBuf>,
    source_url: Option<String>,
    desktop: &mut DesktopState,
    fresh: bool,
    edits: &NodeEdits,
) -> Result<(), String> {
    sync_anx_license();
    let service = CoreSubscriptionService::new();
    let source = SubscriptionSource {
        id: EMBEDDED_PROFILE_ID.to_string(),
        location: format!("inline:{payload}"),
    };
    let raw = service.fetch(&source).map_err(|error| error.to_string())?;
    let document = service.parse(&raw).map_err(|error| error.to_string())?;
    let catalog = service
        .normalize(&document)
        .map_err(|error| error.to_string())?;
    if catalog.nodes.is_empty() {
        return Err("Profile did not contain a supported proxy node".to_string());
    }
    // The subscription's rules and groups name the nodes as served; the engine runs them with the user's edits.
    // Its ANX nodes get the license of the origin it was fetched from, and nothing for a file.
    let mut parsed = catalog.nodes;
    client_core::backend::engine::mark_license_origin(
        &mut parsed,
        source_url.as_deref().unwrap_or_default(),
    );
    let nodes = edits.applied(&parsed);
    // Subscription rules and policy groups drive the route when every node is
    // supported and the engine accepts the result; otherwise one node runs
    // alone.
    // The subscription's own rule sets go with its rules: without them the engine refuses every rule naming one.
    let imported = import_subscription_routing(payload, &parsed);
    let routed = imported
        .spec
        .and_then(|spec| {
            routing_envelope_with_sets(EMBEDDED_PROFILE_ID, &nodes, &spec, &imported.rule_sets).ok()
        })
        // The engine caps the routed node and rule counts.
        .filter(|envelope| RuntimeConfig::from_json(envelope.as_bytes()).is_ok());
    // A single-node profile lists every supported node so it can be switched
    // and delay tested; a routed one is driven by its own groups.
    let routed_import = routed.is_some();
    let (envelope, selected_node_id, node_outbounds) = match routed {
        Some(envelope) => {
            // Group choices made before a refresh survive it where the group
            // and member still exist.
            let mut profile: Value =
                serde_json::from_str(&envelope).map_err(|error| error.to_string())?;
            if let Ok(previous) = read_embedded_profile() {
                carry_routed_group_defaults(&previous, &mut profile);
            }
            let node_outbounds = nodes
                .iter()
                .filter_map(|node| {
                    outbound_for_node(node)
                        .ok()
                        .map(|outbound| (node.clone(), outbound))
                })
                .collect::<Vec<_>>();
            let selected = routed_selected_node(&profile);
            (profile.to_string(), selected, node_outbounds)
        }
        None => {
            // The node picked before stays; otherwise the first trunk node
            // (anx FUTURE-IMPROVEMENTS F-51), and without one the engine's
            // first runnable node.
            let selected = desktop
                .profile_node_id
                .as_deref()
                .filter(|id| nodes.iter().any(|node| node.id == *id))
                .or_else(|| {
                    nodes
                        .iter()
                        .find(|node| is_trunk_node(node) && outbound_for_node(node).is_ok())
                        .map(|node| node.id.as_str())
                });
            let (envelope, node) = single_node_envelope(EMBEDDED_PROFILE_ID, &nodes, selected)
                .map_err(|reason| {
                    format!("The embedded engine cannot run this profile: {reason}")
                })?;
            let node_outbounds = nodes
                .iter()
                .filter_map(|node| {
                    outbound_for_node(node)
                        .ok()
                        .map(|outbound| (node.clone(), outbound))
                })
                .collect::<Vec<_>>();
            (envelope, Some(node.id), node_outbounds)
        }
    };
    let saved_outbounds: serde_json::Map<String, Value> = node_outbounds
        .iter()
        .map(|(node, outbound)| (node.id.clone(), outbound.clone()))
        .collect();
    write_managed_text_atomic(
        &embedded_nodes_path(),
        &Value::Object(saved_outbounds).to_string(),
    )
    .map_err(|error| error.to_string())?;
    let supported_nodes: Vec<NodeDescriptor> =
        node_outbounds.into_iter().map(|(node, _)| node).collect();
    let node_catalog = profile_node_options(&supported_nodes);
    // The imported envelope is kept so routing modes can be reapplied to it.
    write_managed_text_atomic(&embedded_profile_path(), &envelope)
        .map_err(|error| error.to_string())?;
    if routed_import && fresh {
        // A profile with its own rules runs them by default.
        desktop.routing_mode = DesktopRoutingMode::Rule;
    }
    let envelope = embedded_envelope_or_profile(
        envelope,
        desktop.routing_mode,
        desktop.dns_preset,
        desktop.block_quic,
    );
    let envelope_path = embedded_directory().join("envelope.json");
    write_managed_text_atomic(&envelope_path, &envelope).map_err(|error| error.to_string())?;
    let mut managed = managed_config_or_default()?;
    let mitm = managed
        .native_mitm
        .as_ref()
        .is_some_and(|native_mitm| native_mitm.enabled);
    set_embedded_listener(&mut managed, envelope_path, mitm);
    write_managed_config(&windows_managed_config_path(), &managed)
        .map_err(|error| format!("Managed configuration update failed: {error}"))?;
    desktop.profile_source_path = source_path;
    desktop.profile_source_url = source_url;
    desktop.profile_node_id = selected_node_id;
    desktop.profile_node_catalog = node_catalog;
    // Without it the next start only builds the profile again from the kept content.
    let _ = write_managed_text_atomic(
        &profile_import_revision_path(),
        linked_sources::IMPORT_REVISION,
    );
    Ok(())
}

/// Runs the embedded engine from `envelope_path`; the window owns the system
/// proxy. With HTTPS MITM the profile keeps the listener layout the removed
/// Windows service used, and `connection::desktop_listener` puts the engine,
/// which decrypts itself, on the system proxy port.
fn set_embedded_listener(managed: &mut WindowsManagedConfig, envelope_path: PathBuf, mitm: bool) {
    let listen_port = if mitm {
        EMBEDDED_MITM_UPSTREAM_PORT
    } else {
        EMBEDDED_LISTEN_PORT
    };
    managed.system_proxy = Some(WindowsProxySettings {
        enabled: true,
        server: format!("127.0.0.1:{EMBEDDED_LISTEN_PORT}"),
        bypass: "<local>".to_string(),
    });
    managed.system_proxy_owner = WindowsSystemProxyOwner::Desktop;
    managed.embedded = Some(WindowsManagedEmbeddedConfig {
        enabled: true,
        envelope_path,
        listen_host: "127.0.0.1".to_string(),
        listen_port,
    });
}

fn embedded_directory() -> PathBuf {
    windows_managed_data_directory().join("embedded")
}

fn embedded_profile_path() -> PathBuf {
    embedded_directory().join("profile.json")
}

/// The imported subscription's content, so node edits can be applied to it again without fetching it.
fn profile_source_text_path() -> PathBuf {
    embedded_directory().join("source.txt")
}

/// The importer revision (`linked_sources::IMPORT_REVISION`) the imported profile was last built with; absent
/// when an earlier version built it.
fn profile_import_revision_path() -> PathBuf {
    embedded_directory().join("import-revision")
}

/// The user's edits of the imported profile's nodes (`node_edits`), dropped when it is updated.
fn node_edits_path() -> PathBuf {
    embedded_directory().join("node-edits.json")
}

/// `{node id: outbound}` for the nodes of a single-node profile.
fn embedded_nodes_path() -> PathBuf {
    embedded_directory().join("nodes.json")
}

/// The profile with the routing mode applied, or the profile alone (logged)
/// when the mode's rules are unavailable.
fn embedded_envelope_or_profile(
    profile: String,
    mode: DesktopRoutingMode,
    dns: DnsPreset,
    block_quic: bool,
) -> String {
    match embedded_envelope_for_mode(&profile, mode, dns, block_quic) {
        Ok(routed) => routed,
        Err(error) => {
            let _ = append_managed_log(
                APP_LOG_SCOPE,
                &format!("routing mode was not applied to the embedded profile: {error}"),
            );
            profile
        }
    }
}

/// The envelope for `mode` with the DNS choice and QUIC blocking in it.
fn embedded_envelope_for_mode(
    profile: &str,
    mode: DesktopRoutingMode,
    dns: DnsPreset,
    block_quic: bool,
) -> Result<String, String> {
    let rules = desktop_dns::rules::load_lines(&my_rules_path());
    embedded_envelope_with_rules(profile, mode, dns, block_quic, &rules)
}

/// [`embedded_envelope_for_mode`] with `rules` as "我的规则" in place of the saved ones.
fn embedded_envelope_with_rules(
    profile: &str,
    mode: DesktopRoutingMode,
    dns: DnsPreset,
    block_quic: bool,
    rules: &[String],
) -> Result<String, String> {
    let envelope = desktop_dns::rules::apply(&mode_envelope(profile, mode)?, rules)?;
    let envelope = desktop_dns::apply(&envelope, dns)?;
    let envelope = desktop_dns::apply_block_quic(&envelope, block_quic)?;
    let envelope = desktop_dns::redirects::apply(&envelope)?;
    let fake_ip = desktop_dns::custom::current().is_ok_and(|settings| settings.fake_ip);
    let redirects = envelope.contains("\"redirects\"");
    let data_sources = !desktop_dns::data_sources::current().is_empty()
        || !desktop_dns::devices::current().is_empty();
    if dns != DnsPreset::System || fake_ip || !rules.is_empty() || redirects || data_sources {
        RuntimeConfig::from_json(envelope.as_bytes()).map_err(|error| {
            if redirects || data_sources {
                format!(
                    "the engine rejected the settings, redirects, data lists or device names: {}",
                    error.message
                )
            } else if rules.is_empty() {
                format!("the DNS choice was rejected: {}", error.message)
            } else {
                format!("the engine rejected the rules: {}", error.message)
            }
        })?;
    }
    Ok(envelope)
}

/// The imported envelope in `mode`, as the engine builds it (`engine_embedded::profile::envelope_for_mode`, the
/// same as Linux and Android). Rule mode keeps the profile's own rules and groups (or sends everything through its
/// one node); direct mode starts the engine in its direct mode (`"mode": "direct"`), where every connection and
/// every DNS query goes direct and no rule applies, with the rules replaced by `FINAL,DIRECT`. Public builds ship
/// no preset list modes (docs/no-presets.md).
fn mode_envelope(profile: &str, mode: DesktopRoutingMode) -> Result<String, String> {
    let mode = match mode {
        DesktopRoutingMode::Rule => RoutingMode::Rule,
        DesktopRoutingMode::Direct => RoutingMode::Direct,
    };
    envelope_for_mode(profile, mode).map_err(|error| {
        format!(
            "embedded profile was rejected in {} mode: {error}",
            mode.as_str()
        )
    })
}

/// The subscription text, where it came from, and what the server's headers said about it.
type SubscriptionRead = (
    String,
    Option<PathBuf>,
    Option<String>,
    subscription_meta::ResponseMeta,
);

/// This device's ANX license directory (`anx_license`), beside the desktop state.
fn anx_license_store() -> anx_license::LicenseStore {
    anx_license::LicenseStore::new(windows_managed_data_directory().join("anx-license"))
}

/// Hands each subscription origin's device key and license file to the ANX outbounds of that
/// origin's nodes built from now on (`anx_license`, anx spec §2.4.1); an origin that does not
/// offer ANX licensing gives its nodes nothing, and no node carries another origin's key.
fn sync_anx_license() {
    node_import::set_anx_license(None);
    node_import::set_anx_licenses(client_core::backend::engine::anx_licenses(Some(
        &anx_license_store(),
    )));
}

fn download_subscription(
    location: &str,
    licenses: &anx_license::LicenseStore,
) -> Result<(String, subscription_meta::ResponseMeta, bool), String> {
    // The panel picks the subscription format by User-Agent; without one it serves
    // plain v2ray links and leaves out anx:// nodes.
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(SUBSCRIPTION_USER_AGENT)
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.get(location);
    if let Ok(Some(device)) = licenses.request_header(location) {
        request = request.header(anx_license::HEADER_DEVICE, device);
    }
    let response = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(describe_download_error)?;
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let meta = subscription_meta::ResponseMeta::from_headers(header);
    // A license problem never fails the subscription.
    let offered = licenses.record(location, header).unwrap_or(false);
    let payload = response.text().map_err(describe_download_error)?;
    Ok((payload, meta, offered))
}

/// Says what went wrong with a download without the URL, which carries the subscription's token, in
/// words `redacted_subscription_error_code` sorts into a code.
fn describe_download_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "Subscription download timed out".to_string()
    } else if let Some(status) = error.status() {
        format!(
            "Subscription server answered HTTP status {}",
            status.as_u16()
        )
    } else if error.is_connect() {
        "Subscription server could not be reached".to_string()
    } else {
        "Subscription download failed".to_string()
    }
}

/// Same token the iOS app sends, so the panel answers in the AnixOps Net format.
const SUBSCRIPTION_USER_AGENT: &str = concat!("AnixOpsNet/", env!("CARGO_PKG_VERSION"));

fn read_subscription(location: &str) -> Result<SubscriptionRead, String> {
    if is_subscription_url(location) {
        // A panel that licenses ANX gets this device's key and answers with its license
        // (`anx_license`); the first time it says so, the request goes once more.
        let licenses = anx_license_store();
        let (payload, meta, offered) = download_subscription(location, &licenses)?;
        if offered {
            if let Ok((payload, meta, _)) = download_subscription(location, &licenses) {
                return Ok((payload, None, Some(location.to_string()), meta));
            }
        }
        return Ok((payload, None, Some(location.to_string()), meta));
    }
    let path = PathBuf::from(location);
    let payload = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    Ok((
        payload,
        Some(path),
        None,
        subscription_meta::ResponseMeta::default(),
    ))
}

#[tauri::command]
async fn restore_network_settings(
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || restore_network_settings_blocking(state)).await
}

fn restore_network_settings_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let snapshot = desktop
        .proxy_snapshot
        .clone()
        .ok_or_else(|| "No GUI-owned system proxy snapshot is available to restore.".to_string())?;
    let current = read_current_user_system_proxy().map_err(|error| error.to_string())?;
    if !owns_current_proxy(&desktop, &current) {
        desktop.proxy_snapshot = None;
        desktop.applied_proxy = None;
        save_desktop_state(&desktop)?;
        return Ok(OperationResult {
            message: "Current proxy settings no longer match the GUI-owned value and were left unchanged."
                .to_string(),
        });
    }
    NativeWindowsSystemIntegration::new()
        .restore_system_proxy(&snapshot)
        .map_err(|error| error.to_string())?;
    desktop.proxy_snapshot = None;
    desktop.applied_proxy = None;
    save_desktop_state(&desktop)?;
    mark_gui_connection_stopped(&state);
    Ok(OperationResult {
        message: "Network settings restored from the GUI-owned proxy snapshot.".to_string(),
    })
}

#[tauri::command]
async fn set_routing_mode(
    mode: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || set_routing_mode_blocking(&mode, state)).await
}

fn set_routing_mode_blocking(
    mode: &str,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    let mode = parse_routing_mode(mode)?;
    ensure_routing_configuration_stopped(&state)?;
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let embedded = active_embedded_config()?
        .ok_or_else(|| "Import a profile before applying a routing mode.".to_string())?;
    let profile = fs::read_to_string(embedded_profile_path()).map_err(|error| {
        format!("Import the profile again before applying a routing mode: {error}")
    })?;
    let envelope =
        embedded_envelope_for_mode(&profile, mode, desktop.dns_preset, desktop.block_quic)?;
    write_managed_text_atomic(&embedded.envelope_path, &envelope)
        .map_err(|error| error.to_string())?;
    desktop.routing_mode = mode;
    save_desktop_state(&desktop)?;
    Ok(OperationResult {
        message: format!(
            "{} routing mode saved. Connect to apply the updated route.",
            routing_mode_label(mode)
        ),
    })
}

#[tauri::command]
async fn set_dns_preset(
    preset: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || set_dns_preset_blocking(&preset, state)).await
}

fn set_dns_preset_blocking(
    preset: &str,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    let preset =
        DnsPreset::parse(preset).ok_or_else(|| format!("unknown DNS choice {preset:?}"))?;
    ensure_routing_configuration_stopped(&state)?;
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    rewrite_embedded_envelope(&desktop, preset)?;
    desktop.dns_preset = preset;
    save_desktop_state(&desktop)?;
    Ok(OperationResult {
        message: "DNS choice saved. Connect to apply it.".to_string(),
    })
}

/// Writes the imported profile's envelope again with `preset` and the saved
/// "自定义" DNS settings; nothing to do before a profile is imported.
fn rewrite_embedded_envelope(desktop: &DesktopState, preset: DnsPreset) -> Result<(), String> {
    if let Some(embedded) = active_embedded_config()? {
        let profile = fs::read_to_string(embedded_profile_path())
            .map_err(|error| format!("Import the profile again before changing DNS: {error}"))?;
        let envelope =
            embedded_envelope_for_mode(&profile, desktop.routing_mode, preset, desktop.block_quic)?;
        write_managed_text_atomic(&embedded.envelope_path, &envelope)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// The destination rewrites (`redirects`), as the window lists them.
#[tauri::command]
async fn traffic_redirects() -> Result<Vec<desktop_dns::redirects::View>, String> {
    Ok(desktop_dns::redirects::views(
        &desktop_dns::redirects::current(),
    ))
}

/// Adds a rewrite from the editor's fields, or changes the one with id `replacing`.
#[tauri::command]
async fn add_traffic_redirect(
    draft: desktop_dns::redirects::Draft,
    replacing: Option<String>,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let next = desktop_dns::redirects::current().adding(&draft, replacing.as_deref())?;
        replace_redirects(&next, &state)
    })
    .await
}

/// Saves the rewrites as the window changed them: switches, order and deletions.
#[tauri::command]
async fn save_traffic_redirects(
    entries: Vec<desktop_dns::redirects::Entry>,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let next = desktop_dns::redirects::Redirects { entries }.checked()?;
        replace_redirects(&next, &state)
    })
    .await
}

/// Saves `next` once the imported profile's envelope takes it (the engine checks it there), then reconnects a
/// running core. A refused list leaves the saved one and the envelope as they were.
fn replace_redirects(
    next: &desktop_dns::redirects::Redirects,
    state: &DesktopAppState,
) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    desktop_dns::redirects::replace(next, || {
        rewrite_embedded_envelope(&desktop, desktop.dns_preset)
    })?;
    let message = match reconnect_running_direct_core(state) {
        Ok(()) => {
            "Redirects saved. They apply to the running connection, or the next one.".to_string()
        }
        Err(error) => format!("Redirects saved. Reconnect to apply them: {error}"),
    };
    Ok(OperationResult { message })
}

/// The GEOIP and IP-ASN data lists the user added by URL, by name.
#[tauri::command]
async fn data_sources() -> Result<Vec<desktop_dns::data_sources::DataSource>, String> {
    run_blocking(|| Ok(desktop_dns::data_sources::current())).await
}

/// Points the data `name` (`geoip-us`, `asn-13335`) at the list `url`, replacing a list of that name.
#[tauri::command]
async fn save_data_source(
    name: String,
    url: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let next =
            desktop_dns::data_sources::setting(&desktop_dns::data_sources::current(), &name, &url)?;
        replace_data_sources(&next, &state)
    })
    .await
}

/// Removes the data list `name`; its rules then match nothing until a list is added again.
#[tauri::command]
async fn remove_data_source(
    name: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let next =
            desktop_dns::data_sources::removing(&desktop_dns::data_sources::current(), &name);
        replace_data_sources(&next, &state)
    })
    .await
}

/// Saves `next` once the imported profile's envelope takes it, then reconnects a running core, as for redirects.
fn replace_data_sources(
    next: &[desktop_dns::data_sources::DataSource],
    state: &DesktopAppState,
) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    desktop_dns::data_sources::replace(next, || {
        rewrite_embedded_envelope(&desktop, desktop.dns_preset)
    })?;
    let message = match reconnect_running_direct_core(state) {
        Ok(()) => "Data lists saved. The engine downloads them through the proxy.".to_string(),
        Err(error) => format!("Data lists saved. Reconnect to apply them: {error}"),
    };
    Ok(OperationResult { message })
}

/// The names given to local network devices for `DEVICE-NAME` rules, by address.
#[tauri::command]
async fn devices() -> Result<Vec<desktop_dns::devices::Device>, String> {
    run_blocking(|| Ok(desktop_dns::devices::current())).await
}

/// Calls the device at `address` `name`, replacing its earlier name.
#[tauri::command]
async fn save_device(
    address: String,
    name: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let next =
            desktop_dns::devices::setting(&desktop_dns::devices::current(), &address, &name)?;
        replace_devices(&next, &state)
    })
    .await
}

/// Forgets the name of the device at `address`; its `DEVICE-NAME` rules then match it no more.
#[tauri::command]
async fn remove_device(
    address: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let next = desktop_dns::devices::removing(&desktop_dns::devices::current(), &address);
        replace_devices(&next, &state)
    })
    .await
}

/// Saves `next` once the imported profile's envelope takes it, then reconnects a running core, as for data lists.
fn replace_devices(
    next: &[desktop_dns::devices::Device],
    state: &DesktopAppState,
) -> Result<OperationResult, String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    desktop_dns::devices::replace(next, || {
        rewrite_embedded_envelope(&desktop, desktop.dns_preset)
    })?;
    let message = match reconnect_running_direct_core(state) {
        Ok(()) => "Device names saved.".to_string(),
        Err(error) => format!("Device names saved. Reconnect to apply them: {error}"),
    };
    Ok(OperationResult { message })
}

/// The "自定义" DNS text and the Fake-IP switch as saved.
#[tauri::command]
async fn custom_dns() -> Result<desktop_dns::custom::DnsText, String> {
    run_blocking(desktop_dns::custom::text).await
}

/// Checks and saves the "自定义" DNS text and the Fake-IP switch. While the
/// custom choice is selected or Fake-IP changes, the envelope changes too, so
/// that needs a disconnected window, as the other DNS choices do; otherwise
/// the text is only kept for later.
#[tauri::command]
async fn save_custom_dns(
    text: desktop_dns::custom::DnsText,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || save_custom_dns_blocking(&text, state)).await
}

fn save_custom_dns_blocking(
    text: &desktop_dns::custom::DnsText,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    let settings = text.parse()?;
    let fake_ip_changed =
        !matches!(desktop_dns::custom::current(), Ok(saved) if saved.fake_ip == settings.fake_ip);
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    let in_use = desktop.dns_preset == DnsPreset::Custom || fake_ip_changed;
    if in_use {
        ensure_routing_configuration_stopped(&state)?;
    }
    desktop_dns::custom::replace(text, || {
        if in_use {
            rewrite_embedded_envelope(&desktop, desktop.dns_preset)?;
        }
        if desktop.direct_egress {
            let envelope =
                connection::direct_egress_envelope(desktop.dns_preset, desktop.block_quic)?;
            RuntimeConfig::from_json(envelope.as_bytes())
                .map_err(|error| format!("the DNS settings were rejected: {}", error.message))?;
        }
        Ok(())
    })?;
    Ok(OperationResult {
        message: if in_use {
            "DNS settings saved. Connect to apply them.".to_string()
        } else {
            "DNS settings saved. Choose Custom to use them.".to_string()
        },
    })
}

#[tauri::command]
async fn set_block_quic(
    enabled: bool,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || set_block_quic_blocking(enabled, state)).await
}

#[tauri::command]
async fn set_direct_egress(
    enabled: bool,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || set_direct_egress_blocking(enabled, state)).await
}

/// Debug mode's egress: with `enabled` the window's engine runs with no node
/// (`connection::direct_egress_envelope`), else the imported profile again. A
/// running core reconnects on it.
fn set_direct_egress_blocking(
    enabled: bool,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    {
        let mut desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?;
        if desktop.direct_egress == enabled {
            return Ok(OperationResult {
                message: "Egress unchanged.".to_string(),
            });
        }
        desktop.direct_egress = enabled;
        save_desktop_state(&desktop)?;
    }
    let message = match reconnect_running_direct_core(&state) {
        Ok(()) if enabled => "Traffic leaves directly.".to_string(),
        Ok(()) => "Traffic leaves through the selected node.".to_string(),
        Err(error) => format!("Egress saved. Reconnect to apply it: {error}"),
    };
    Ok(OperationResult { message })
}

#[derive(Debug, Serialize)]
struct MyRules {
    text: String,
    count: usize,
}

/// "我的规则": the user's own rule lines as written and how many rules they hold.
#[tauri::command]
async fn my_rules() -> Result<MyRules, String> {
    run_blocking(|| {
        let text = desktop_dns::rules::load(&my_rules_path());
        let count = desktop_dns::rules::parse(&text).map_or(0, |lines| lines.len());
        Ok(MyRules { text, count })
    })
    .await
}

/// What importing the running subscription's rules left out (unsupported rule types, Surge's `RULE-SET,SYSTEM`,
/// ...), one line each, for the rules page; empty without a saved subscription text.
#[tauri::command]
async fn routing_warnings() -> Result<Vec<String>, String> {
    run_blocking(|| {
        let Ok(content) = fs::read_to_string(profile_source_text_path()) else {
            return Ok(Vec::new());
        };
        Ok(match client_core::backend::engine::parse_nodes(&content) {
            Ok(nodes) => client_core::backend::engine::routing_warnings(&content, &nodes),
            Err(_) => Vec::new(),
        })
    })
    .await
}

#[tauri::command]
async fn save_my_rules(
    text: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || save_my_rules_blocking(&text, state)).await
}

/// Checks "我的规则" against the imported profile, saves them and writes the envelope. The engine this window
/// runs takes them in place (`reload_rules`), so open connections stay; TUN mode, or a profile that had no
/// rules before, reconnects instead. The Windows service takes them on its next start.
fn save_my_rules_blocking(text: &str, state: DesktopAppState) -> Result<OperationResult, String> {
    let lines = desktop_dns::rules::parse(text).map_err(|error| error.message())?;
    let mut envelope = None;
    {
        let desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?;
        if let Some(embedded) = active_embedded_config()? {
            // A profile whose mode cannot be applied yet (rule lists not downloaded) has nothing to check the
            // lines against; they are kept and checked when it connects.
            let profile = fs::read_to_string(embedded_profile_path())
                .ok()
                .filter(|profile| {
                    embedded_envelope_with_rules(
                        profile,
                        desktop.routing_mode,
                        desktop.dns_preset,
                        desktop.block_quic,
                        &[],
                    )
                    .is_ok()
                });
            if let Some(profile) = profile {
                let routed = embedded_envelope_with_rules(
                    &profile,
                    desktop.routing_mode,
                    desktop.dns_preset,
                    desktop.block_quic,
                    &lines,
                )?;
                envelope = Some((embedded.envelope_path, routed));
            }
        }
        if desktop.direct_egress {
            // Debug mode's node-less profile: checked the same way.
            let direct = desktop_dns::rules::apply(
                r#"{"schema_version":1,"profile_id":"desktop","outbound":{"type":"direct"}}"#,
                &lines,
            )?;
            RuntimeConfig::from_json(direct.as_bytes())
                .map_err(|error| format!("the engine rejected the rules: {}", error.message))?;
        }
    }
    desktop_dns::rules::save(&my_rules_path(), text)?;
    if let Some((path, routed)) = envelope.as_ref() {
        write_managed_text_atomic(path, routed).map_err(|error| error.to_string())?;
    }
    let count = lines.len();
    let running_rules = {
        let desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?
            .clone();
        if desktop.direct_egress {
            connection::direct_egress_envelope(desktop.dns_preset, desktop.block_quic)
                .ok()
                .and_then(|direct| desktop_dns::rules::rules_of(&direct))
        } else {
            envelope
                .as_ref()
                .and_then(|(_, routed)| desktop_dns::rules::rules_of(routed))
        }
    };
    let reloaded = {
        let core = state
            .direct_core
            .lock()
            .map_err(|_| "desktop core lock failed")?;
        !core.tun_running()
            && running_rules.is_some_and(|rules| {
                serde_json::to_vec(&json!({ "cmd": "reload_rules", "rules": rules }))
                    .ok()
                    .and_then(|request| core.control(&request).ok())
                    .and_then(|answer| serde_json::from_slice::<Value>(&answer).ok())
                    .is_some_and(|answer| answer.get("reloaded") == Some(&Value::Bool(true)))
            })
    };
    let message = if reloaded {
        format!("{count} rules saved and applied; open connections stay.")
    } else {
        match reconnect_running_direct_core(&state) {
            Ok(()) => format!(
                "{count} rules saved. They apply to the running connection, or the next one."
            ),
            Err(error) => format!("{count} rules saved. Reconnect to apply them: {error}"),
        }
    };
    Ok(OperationResult { message })
}

/// Saves QUIC blocking into the envelope and the desktop state. A core this
/// window runs reconnects on it; the Windows service takes it on its next
/// start.
fn set_block_quic_blocking(
    enabled: bool,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    {
        let mut desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?;
        if let Some(embedded) = active_embedded_config()? {
            let profile = fs::read_to_string(embedded_profile_path()).map_err(|error| {
                format!("Import the profile again before changing QUIC blocking: {error}")
            })?;
            let envelope = embedded_envelope_for_mode(
                &profile,
                desktop.routing_mode,
                desktop.dns_preset,
                enabled,
            )?;
            write_managed_text_atomic(&embedded.envelope_path, &envelope)
                .map_err(|error| error.to_string())?;
        }
        desktop.block_quic = enabled;
        save_desktop_state(&desktop)?;
    }
    let message = match reconnect_running_direct_core(&state) {
        Ok(()) => {
            "QUIC setting saved. It applies to the running connection, or the next one.".to_string()
        }
        Err(error) => format!("QUIC setting saved. Reconnect to apply it: {error}"),
    };
    Ok(OperationResult { message })
}

fn ensure_routing_configuration_stopped(state: &DesktopAppState) -> Result<(), String> {
    if state
        .direct_core
        .lock()
        .map_err(|_| "desktop core lock failed")?
        .is_running()?
    {
        return Err("Disconnect before changing the routing mode.".to_string());
    }
    Ok(())
}

fn parse_routing_mode(value: &str) -> Result<DesktopRoutingMode, String> {
    match value {
        "direct" => Ok(DesktopRoutingMode::Direct),
        "rule" => Ok(DesktopRoutingMode::Rule),
        _ => Err("routing mode must be rule or direct".to_string()),
    }
}
fn routing_mode_name(mode: DesktopRoutingMode) -> &'static str {
    match mode {
        DesktopRoutingMode::Direct => "direct",
        DesktopRoutingMode::Rule => "rule",
    }
}
fn routing_mode_label(mode: DesktopRoutingMode) -> &'static str {
    match mode {
        DesktopRoutingMode::Direct => "Direct",
        DesktopRoutingMode::Rule => "Rules",
    }
}

#[tauri::command]
async fn list_mitm_plugins() -> Result<Vec<PluginSummary>, String> {
    run_blocking(|| mitm_plugins::list(&mitm_plugins::directory())).await
}

/// Saves a plugin opened from a file or pasted; `source` is the file name, empty when pasted.
#[tauri::command]
async fn import_mitm_plugin_text(
    text: String,
    source: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let plugin = mitm_plugins::import_text(
            &mitm_plugins::directory(),
            &source,
            &text,
            mitm_plugins::download,
        )?;
        let applied = apply_mitm_plugins(&state);
        Ok(OperationResult {
            message: format!(
                "Plugin imported: {} ({} scripts). {applied}",
                plugin.name, plugin.script_count
            ),
        })
    })
    .await
}

#[tauri::command]
async fn mitm_plugin_details(id: String) -> Result<mitm_plugins::PluginDetails, String> {
    run_blocking(move || mitm_plugins::details(&mitm_plugins::directory(), &id)).await
}

#[tauri::command]
async fn import_mitm_plugin(
    url: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let plugin =
            mitm_plugins::import(&mitm_plugins::directory(), &url, mitm_plugins::download)?;
        let applied = apply_mitm_plugins(&state);
        Ok(OperationResult {
            message: format!(
                "Plugin imported: {} ({} scripts). {applied}",
                plugin.name, plugin.script_count
            ),
        })
    })
    .await
}

#[tauri::command]
async fn set_mitm_plugin_enabled(
    id: String,
    enabled: bool,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mitm_plugins::set_enabled(&mitm_plugins::directory(), &id, enabled)?;
        Ok(OperationResult {
            message: apply_mitm_plugins(&state),
        })
    })
    .await
}

#[tauri::command]
async fn remove_mitm_plugin(
    id: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mitm_plugins::remove(&mitm_plugins::directory(), &id)?;
        Ok(OperationResult {
            message: apply_mitm_plugins(&state),
        })
    })
    .await
}

#[tauri::command]
async fn list_debug_mappings() -> Result<Vec<mappings::Entry>, String> {
    run_blocking(|| mappings::list(&mitm_plugins::directory())).await
}

#[tauri::command]
async fn save_debug_mapping(
    entry: mappings::Entry,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mappings::save(&mitm_plugins::directory(), entry)?;
        Ok(OperationResult {
            message: apply_mitm_plugins(&state),
        })
    })
    .await
}

#[tauri::command]
async fn remove_debug_mapping(
    id: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mappings::remove(&mitm_plugins::directory(), &id)?;
        Ok(OperationResult {
            message: apply_mitm_plugins(&state),
        })
    })
    .await
}

#[tauri::command]
async fn list_decrypt_hosts() -> Result<Vec<String>, String> {
    run_blocking(|| mappings::load(&mitm_plugins::directory()).map(|stored| stored.decrypted()))
        .await
}

/// The whole decryption list: `{pattern, exclude, enabled}` per line.
#[tauri::command]
async fn list_decrypt_domains() -> Result<Vec<mappings::Domain>, String> {
    run_blocking(|| mappings::load(&mitm_plugins::directory()).map(|stored| stored.domains)).await
}

/// Adds a line to the decryption list; `exclude` (or a leading `-`) makes it an exclusion.
#[tauri::command]
async fn add_decrypt_host(
    host: String,
    exclude: Option<bool>,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mappings::add_host(&mitm_plugins::directory(), &host, exclude.unwrap_or(false))?;
        Ok(OperationResult {
            message: apply_mitm_plugins(&state),
        })
    })
    .await
}

#[tauri::command]
async fn edit_decrypt_host(
    pattern: String,
    host: String,
    exclude: bool,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mappings::edit_host(&mitm_plugins::directory(), &pattern, &host, exclude)?;
        Ok(OperationResult {
            message: apply_mitm_plugins(&state),
        })
    })
    .await
}

#[tauri::command]
async fn set_decrypt_host_enabled(
    pattern: String,
    enabled: bool,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mappings::set_host_enabled(&mitm_plugins::directory(), &pattern, enabled)?;
        Ok(OperationResult {
            message: apply_mitm_plugins(&state),
        })
    })
    .await
}

#[tauri::command]
async fn remove_decrypt_host(
    host: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        mappings::remove_host(&mitm_plugins::directory(), &host)?;
        Ok(OperationResult {
            message: apply_mitm_plugins(&state),
        })
    })
    .await
}

/// Restarts a running desktop core so a plugin change takes effect, and
/// says what the user still has to do when it cannot.
fn apply_mitm_plugins(state: &DesktopAppState) -> String {
    if !managed_https_decryption_enabled() {
        return "Turn on HTTPS decryption for plugins to take effect.".to_string();
    }
    match reconnect_running_direct_core(state) {
        Ok(()) => "Plugins are applied to the running connection.".to_string(),
        Err(error) => format!("Reconnect to apply plugins: {error}"),
    }
}

#[tauri::command]
async fn configure_script_runtime(script_runtime_json: String) -> Result<OperationResult, String> {
    run_blocking(move || configure_script_runtime_blocking(script_runtime_json)).await
}

fn configure_script_runtime_blocking(
    script_runtime_json: String,
) -> Result<OperationResult, String> {
    let _ = script_runtime_json;
    Err(
        "Native MITM script execution is unavailable until Windows has a no-network sandbox."
            .to_string(),
    )
}

#[tauri::command]
async fn clear_script_runtime() -> Result<OperationResult, String> {
    run_blocking(clear_script_runtime_blocking).await
}

fn clear_script_runtime_blocking() -> Result<OperationResult, String> {
    let mut managed = managed_config_or_default()?;
    let native_mitm = managed
        .native_mitm
        .as_mut()
        .ok_or_else(|| "HTTPS MITM is not configured.".to_string())?;
    native_mitm.script_runtime = None;
    write_managed_config(&windows_managed_config_path(), &managed)
        .map_err(|error| error.to_string())?;
    Ok(OperationResult {
        message: "Managed script runtime cleared.".to_string(),
    })
}

#[tauri::command]
async fn enable_https_mitm(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || enable_https_mitm_blocking(state)).await
}

fn enable_https_mitm_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let embedded = active_embedded_config()?
        .ok_or_else(|| "Import a profile before enabling HTTPS MITM.".to_string())?;
    if managed_config_or_default()?
        .native_mitm
        .is_some_and(|native_mitm| native_mitm.script_runtime.is_some())
    {
        return Err(
            "Clear the legacy managed script runtime before enabling HTTPS MITM; Windows script execution requires a no-network sandbox."
                .to_string(),
        );
    }
    let (certificate_path, private_key_path) = ensure_protected_mitm_ca_material()?;
    // The user trusts the root by hand in Windows' certificate dialog; it is never added here.
    if !root_trust::is_trusted(&certificate_path)? {
        return Err(format!(
            "Trust the HTTPS decryption root first: choose Trust root certificate under Extensions. {}.",
            root_trust::STEPS
        ));
    }
    let mut managed = managed_config_or_default()?;
    // The engine decrypts itself on the system proxy port (`connection::desktop_listener`).
    managed.native_mitm = Some(WindowsManagedNativeMitmConfig {
        enabled: true,
        listen_host: "127.0.0.1".to_string(),
        listen_port: EMBEDDED_LISTEN_PORT,
        upstream_socks_host: "127.0.0.1".to_string(),
        upstream_socks_port: EMBEDDED_MITM_UPSTREAM_PORT,
        ca_certificate_path: certificate_path.clone(),
        ca_private_key_path: private_key_path,
        log_path: windows_managed_log_directory().join("native-mitm.log"),
        script_runtime: None,
    });
    set_embedded_listener(&mut managed, embedded.envelope_path, true);
    write_managed_config(&windows_managed_config_path(), &managed)
        .map_err(|error| format!("Managed configuration update failed: {error}"))?;
    reconnect_running_direct_core(&state)?;
    Ok(OperationResult {
        message: format!(
            "HTTPS MITM enabled with CA material at {}.",
            certificate_path.display()
        ),
    })
}

#[tauri::command]
async fn disable_https_mitm(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || disable_https_mitm_blocking(state)).await
}

#[tauri::command]
async fn set_tun_mode(
    enabled: bool,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || set_tun_mode_blocking(enabled, state)).await
}

/// Saves the TUN mode switch and reconnects a running desktop core, which
/// creates or removes the Wintun adapter.
fn set_tun_mode_blocking(enabled: bool, state: DesktopAppState) -> Result<OperationResult, String> {
    if enabled {
        let dll = platform_windows::tun::default_wintun_dll_path()
            .map_err(|error| format!("wintun.dll could not be located: {error}"))?;
        if !dll.is_file() {
            return Err(
                "TUN mode needs wintun.dll next to the client. Reinstall the client.".to_string(),
            );
        }
    }
    let previous = {
        let mut desktop = state
            .desktop
            .lock()
            .map_err(|_| "desktop state lock failed")?;
        let previous = desktop.tun_mode_enabled;
        desktop.tun_mode_enabled = enabled;
        save_desktop_state(&desktop)?;
        previous
    };
    if let Err(error) = reconnect_running_direct_core(&state) {
        if let Ok(mut desktop) = state.desktop.lock() {
            desktop.tun_mode_enabled = previous;
            let _ = save_desktop_state(&desktop);
        }
        let _ = reconnect_running_direct_core(&state);
        return Err(error);
    }
    Ok(OperationResult {
        message: if enabled {
            "TUN mode is on.".to_string()
        } else {
            "TUN mode is off.".to_string()
        },
    })
}

/// Desktop mode runs HTTPS decryption inside this process, so a running
/// desktop core picks up an enabled or disabled MITM only by reconnecting.
fn reconnect_running_direct_core(state: &DesktopAppState) -> Result<(), String> {
    let desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?
        .clone();
    let mut core = state
        .direct_core
        .lock()
        .map_err(|_| "desktop core lock failed")?;
    if !core.is_running()? {
        return Ok(());
    }
    let _ =
        connection::disconnect_direct(windows_managed_config_path(), desktop.clone(), &mut core);
    let connected = connection::connect_direct(windows_managed_config_path(), desktop, &mut core)?;
    drop(core);
    let mut persisted = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    persisted.proxy_snapshot = Some(connected.snapshot);
    persisted.applied_proxy = Some(connected.applied_proxy);
    save_desktop_state(&persisted)
}

fn disable_https_mitm_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let mut managed = managed_config_or_default()?;
    managed
        .native_mitm
        .take()
        .ok_or_else(|| "HTTPS MITM is not configured.".to_string())?;
    // The embedded engine takes the system proxy port back.
    if let Some(embedded) = managed.embedded.clone().filter(|embedded| embedded.enabled) {
        set_embedded_listener(&mut managed, embedded.envelope_path, false);
    } else {
        managed.system_proxy = None;
    }
    write_managed_config(&windows_managed_config_path(), &managed)
        .map_err(|error| error.to_string())?;
    let certificate_cleanup_error =
        if let Ok(mut runtime_state) = read_managed_state(&windows_managed_state_path()) {
            if let Some(thumbprint) = runtime_state.native_mitm_certificate_sha1.take() {
                match NativeWindowsSystemIntegration::new().remove_root_certificate(&thumbprint) {
                    Ok(()) => write_managed_state(&windows_managed_state_path(), &runtime_state)
                        .err()
                        .map(|error| error.to_string()),
                    Err(error) => Some(error.to_string()),
                }
            } else {
                None
            }
        } else {
            None
        };
    // The root and its key stay, so turning decryption on again needs no new manual trust. The user
    // removes the root from Windows' certificate store when they stop using decryption.
    reconnect_running_direct_core(&state)?;
    if let Some(error) = certificate_cleanup_error {
        return Err(error);
    }
    Ok(OperationResult {
        message: "HTTPS MITM disabled; the embedded engine is the system proxy again.".to_string(),
    })
}

/// The root and its key, generated on first use, with the key's ACL locked down.
fn ensure_protected_mitm_ca_material() -> Result<(PathBuf, PathBuf), String> {
    let (certificate_path, private_key_path) = ensure_mitm_ca_material()?;
    if let Err(error) = protect_windows_managed_mitm_private_key(&private_key_path) {
        let _ = append_managed_log(
            APP_LOG_SCOPE,
            &format!(
                "native HTTPS MITM private key protection failed: {}",
                error.message
            ),
        );
        let _ = remove_windows_managed_mitm_private_key(&private_key_path);
        return Err(error.message);
    }
    Ok((certificate_path, private_key_path))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MitmRootStatus {
    /// Whether a root exists yet; it is generated the first time the user asks to trust it.
    generated: bool,
    trusted: bool,
}

/// Whether the HTTPS decryption root exists and is in a Windows ROOT store. Read only.
#[tauri::command]
async fn mitm_root_status() -> Result<MitmRootStatus, String> {
    run_blocking(|| {
        let certificate_path = windows_managed_data_directory()
            .join("mitm")
            .join("root-ca.pem");
        if !certificate_path.exists() {
            return Ok(MitmRootStatus {
                generated: false,
                trusted: false,
            });
        }
        Ok(MitmRootStatus {
            generated: true,
            trusted: root_trust::is_trusted(&certificate_path)?,
        })
    })
    .await
}

/// Generates the root if needed and opens it in Windows' certificate dialog, where the user installs and
/// trusts it by hand.
#[tauri::command]
async fn open_mitm_root() -> Result<OperationResult, String> {
    run_blocking(|| {
        let (certificate_path, _) = ensure_protected_mitm_ca_material()?;
        root_trust::open_dialog(&certificate_path)?;
        Ok(OperationResult {
            message: format!("{}.", root_trust::STEPS),
        })
    })
    .await
}

fn ensure_mitm_ca_material() -> Result<(PathBuf, PathBuf), String> {
    let directory = windows_managed_data_directory().join("mitm");
    let certificate_path = directory.join("root-ca.pem");
    let private_key_path = directory.join("root-ca-key.pem");
    if certificate_path.exists() && private_key_path.exists() {
        return Ok((certificate_path, private_key_path));
    }
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, MITM_CA_SUBJECT);
    distinguished_name.push(DnType::OrganizationName, "AnixOps NetworkCore");
    let mut params = CertificateParams::default();
    params.distinguished_name = distinguished_name;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let key_pair = KeyPair::generate().map_err(|error| error.to_string())?;
    let certificate = params
        .self_signed(&key_pair)
        .map_err(|error| error.to_string())?;
    fs::write(&certificate_path, certificate.pem()).map_err(|error| error.to_string())?;
    fs::write(&private_key_path, key_pair.serialize_pem()).map_err(|error| error.to_string())?;
    Ok((certificate_path, private_key_path))
}

/// An SVG QR code for the phone setup page address of the LAN share.
#[tauri::command]
fn phone_qr_code(text: String) -> Result<String, String> {
    desktop_mitm::phone::qr_svg(&text)
}

/// Replaces the HTTPS decryption root with the user's own CA (PEM certificate and PKCS#8 key); it is used the
/// next time HTTPS decryption starts.
#[tauri::command]
async fn import_mitm_root(
    certificate_pem: String,
    private_key_pem: String,
) -> Result<OperationResult, String> {
    run_blocking(move || {
        let directory = windows_managed_data_directory().join("mitm");
        // The root being replaced may be trusted by this or another user:
        // noted, so that removing the client still takes it out of their stores.
        let replaced = directory.join("root-ca.pem");
        if replaced.is_file() {
            match root_trust::thumbprint(&replaced) {
                Ok(thumbprint) => root_trust::note(&thumbprint),
                Err(error) => {
                    let _ = append_managed_log(
                        APP_LOG_SCOPE,
                        &format!("the replaced HTTPS decryption root could not be read: {error}"),
                    );
                }
            }
        }
        desktop_mitm::Root::import(&directory, &certificate_pem, &private_key_pem)?;
        // Closed to everyone but this account and the system at once, not only
        // when decryption is next turned on.
        let private_key_path = windows_managed_mitm_private_key_path();
        if let Err(error) = protect_windows_managed_mitm_private_key(&private_key_path) {
            let _ = append_managed_log(
                APP_LOG_SCOPE,
                &format!(
                    "imported HTTPS decryption key protection failed: {}",
                    error.message
                ),
            );
            // Neither half of the imported root is kept: a certificate left without its key would be
            // overwritten by the next root generated, and trusted meanwhile for nothing.
            let _ = remove_windows_managed_mitm_private_key(&private_key_path);
            let _ = std::fs::remove_file(&replaced);
            return Err(error.message);
        }
        Ok(OperationResult {
            message:
                "Your root certificate is saved. Turn HTTPS decryption off and on again to use it, \
                      then install it again on phones and other devices."
                    .to_string(),
        })
    })
    .await
}

/// SHA-256 of the HTTPS decryption root, for checking it against the phone setup page.
#[tauri::command]
fn mitm_root_fingerprint() -> Option<String> {
    let root = desktop_mitm::Root::at(&windows_managed_data_directory().join("mitm"));
    if root.exists() {
        root.fingerprint().ok()
    } else {
        None
    }
}

#[tauri::command]
async fn install_certificate(
    path: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || install_certificate_blocking(path, state)).await
}

fn install_certificate_blocking(
    path: String,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    // Opened in Windows' certificate dialog: the user installs and trusts it there by hand.
    let _ = state;
    root_trust::open_dialog(&PathBuf::from(path))?;
    Ok(OperationResult {
        message: format!("{}.", root_trust::STEPS),
    })
}

#[tauri::command]
async fn remove_certificate(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || remove_certificate_blocking(state)).await
}

fn remove_certificate_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let thumbprint = desktop
        .certificate_sha1
        .clone()
        .ok_or_else(|| "No GUI-installed certificate is recorded.".to_string())?;
    NativeWindowsSystemIntegration::new()
        .remove_root_certificate(&thumbprint)
        .map_err(|error| error.to_string())?;
    desktop.certificate_sha1 = None;
    save_desktop_state(&desktop)?;
    Ok(OperationResult {
        message: "Certificate removed.".to_string(),
    })
}

#[tauri::command]
async fn install_driver(
    path: String,
    state: State<'_, DesktopAppState>,
) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || install_driver_blocking(path, state)).await
}

fn install_driver_blocking(
    path: String,
    state: DesktopAppState,
) -> Result<OperationResult, String> {
    let installed = NativeWindowsSystemIntegration::new()
        .install_driver(&PathBuf::from(path))
        .map_err(|error| error.to_string())?;
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    desktop.driver_inf_path = Some(installed.inf_path);
    save_desktop_state(&desktop)?;
    Ok(OperationResult {
        message: "Driver installed.".to_string(),
    })
}

#[tauri::command]
async fn remove_driver(state: State<'_, DesktopAppState>) -> Result<OperationResult, String> {
    let state = state.inner().clone();
    run_blocking(move || remove_driver_blocking(state)).await
}

fn remove_driver_blocking(state: DesktopAppState) -> Result<OperationResult, String> {
    let mut desktop = state
        .desktop
        .lock()
        .map_err(|_| "desktop state lock failed")?;
    let path = desktop
        .driver_inf_path
        .clone()
        .ok_or_else(|| "No GUI-installed driver is recorded.".to_string())?;
    NativeWindowsSystemIntegration::new()
        .uninstall_driver(&path)
        .map_err(|error| error.to_string())?;
    desktop.driver_inf_path = None;
    save_desktop_state(&desktop)?;
    Ok(OperationResult {
        message: "Driver removed.".to_string(),
    })
}

fn selected_catalog_node(
    desktop: &DesktopState,
    node_id: &str,
) -> Result<DesktopProfileNode, String> {
    desktop
        .profile_node_catalog
        .iter()
        .find(|node| node.id == node_id)
        .cloned()
        .ok_or_else(|| "Selected node is not part of the current imported profile.".to_string())
}

fn snapshot(
    runtime: &WindowsRuntimeStatus,
    desktop: &DesktopState,
    direct_running: bool,
) -> RuntimeSnapshot {
    let desktop_connected = direct_running
        && read_current_user_system_proxy()
            .ok()
            .is_some_and(|proxy| owns_current_proxy(desktop, &proxy));
    RuntimeSnapshot {
        connection: if desktop_connected {
            "Connected"
        } else {
            "Disconnected"
        }
        .to_string(),
        connection_label: if desktop_connected {
            "The embedded engine is running and managing the current-user proxy.".to_string()
        } else if direct_running {
            "The embedded engine is running; the current-user proxy is not applied.".to_string()
        } else {
            "Connect to start the embedded engine.".to_string()
        },
        core: StatusFact {
            label: if direct_running { "Running" } else { "Stopped" }.to_string(),
            detail: Some("Embedded engine in this app".to_string()),
            tone: if direct_running { "success" } else { "neutral" },
        },
        proxy: StatusFact {
            label: runtime
                .system_proxy_enabled
                .map(|enabled| if enabled { "Enabled" } else { "Disabled" }.to_string())
                .unwrap_or_else(|| "Unavailable".to_string()),
            detail: runtime.system_proxy_server.clone(),
            tone: match runtime.system_proxy_matches_managed {
                Some(true) => "success",
                Some(false) => "warning",
                None => "neutral",
            },
        },
        selected_node: desktop.profile_node_id.clone(),
        subscription: desktop.profile_source_url.clone().or_else(|| {
            desktop
                .profile_source_path
                .as_ref()
                .map(|path| path.display().to_string())
        }),
        subscription_last_updated: desktop.profile_last_successful_update.clone(),
        subscription_last_attempt: desktop.profile_last_attempt.clone(),
        subscription_next_attempt: profile_next_attempt(desktop),
        subscription_refresh_result: desktop.profile_refresh_result.clone(),
        subscription_added_node_count: desktop.profile_added_node_count,
        subscription_removed_node_count: desktop.profile_removed_node_count,
        subscription_changed_node_count: desktop.profile_changed_node_count,
        subscription_refresh_error_code: desktop.profile_refresh_error_code.clone(),
        subscription_error: desktop.profile_last_update_error.clone(),
        // The state file's errors came from the removed Windows service; the window reports its own.
        last_error: None,
        configuration_error: runtime.configuration_error.clone(),
        start_after_login: desktop.start_after_login,
        auto_connect: desktop.auto_connect,
        auto_recover_core: desktop.auto_recover_core,
        auto_subscription_refresh: desktop.auto_subscription_refresh,
        auto_select_fastest_node: desktop.auto_select_fastest_node,
        tun_mode_enabled: desktop.tun_mode_enabled,
        script_runtime_configured: managed_script_runtime_configured(),
        https_decryption_enabled: managed_https_decryption_enabled(),
        dark_theme: desktop.dark_theme,
        routing_mode: routing_mode_name(desktop.routing_mode).to_string(),
        dns_preset: desktop.dns_preset.as_str(),
        block_quic: desktop.block_quic,
        direct_egress: Some(desktop.direct_egress),
        embedded_engine: active_embedded_config().ok().flatten().is_some(),
    }
}

fn managed_https_decryption_enabled() -> bool {
    managed_config_or_default()
        .ok()
        .and_then(|managed| managed.native_mitm)
        .is_some_and(|native_mitm| native_mitm.enabled)
}

fn managed_script_runtime_configured() -> bool {
    managed_config_or_default()
        .ok()
        .and_then(|managed| managed.native_mitm)
        .and_then(|native_mitm| native_mitm.script_runtime)
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_managed_config() -> WindowsManagedConfig {
        // As the MSI installs it: every value empty.
        serde_json::from_str(include_str!(
            "../../../../installer/windows/default-managed-config.json"
        ))
        .unwrap()
    }

    #[test]
    fn an_upgrade_that_reset_the_managed_configuration_is_repaired() {
        let imported = DesktopState {
            profile_source_url: Some("https://sub.example/link".to_string()),
            ..DesktopState::default()
        };
        let reset = default_managed_config();
        assert!(embedded_block_lost(&reset, &imported, true));
        // A profile imported from a file, or known only by its nodes, counts too.
        let from_file = DesktopState {
            profile_source_path: Some(PathBuf::from(r"C:\profiles\nodes.yaml")),
            ..DesktopState::default()
        };
        assert!(embedded_block_lost(&reset, &from_file, true));
        // Nothing imported, or the profile's files gone: nothing to put back.
        assert!(!embedded_block_lost(&reset, &DesktopState::default(), true));
        assert!(!embedded_block_lost(&reset, &imported, false));

        let mut repaired = reset;
        set_embedded_listener(
            &mut repaired,
            PathBuf::from(r"C:\ProgramData\AnixOps\NetworkCore\embedded\envelope.json"),
            false,
        );
        assert!(repaired.validate().is_ok());
        assert!(!embedded_block_lost(&repaired, &imported, true));
        let embedded = repaired.embedded.as_ref().unwrap();
        assert_eq!(embedded.listen_port, EMBEDDED_LISTEN_PORT);
        assert_eq!(
            repaired.system_proxy.as_ref().unwrap().server,
            format!("127.0.0.1:{EMBEDDED_LISTEN_PORT}")
        );
        // HTTPS decryption is left for the user to turn on.
        assert!(repaired.native_mitm.is_none());
    }

    #[test]
    fn trace_gives_the_exit_address_and_country() {
        let body = "fl=1f2\nh=www.cloudflare.com\nip=203.0.113.7\nts=1.5\nloc=HK\ntls=TLSv1.3\n";
        assert_eq!(
            parse_trace(body),
            (Some("203.0.113.7".to_string()), Some("HK".to_string()))
        );
        assert_eq!(parse_trace("ipx=1\nloc=\n"), (None, None));
    }

    fn routed_profile() -> Value {
        json!({
            "schema_version": 1,
            "profile_id": "windows-embedded",
            "routing": {
                "outbounds": {
                    "hk": { "type": "shadowsocks" },
                    "jp": { "type": "shadowsocks" },
                    "us": { "type": "shadowsocks" }
                },
                "rules": ["DOMAIN-SUFFIX,example.jp,jp", "FINAL,Proxy"],
                "groups": [
                    { "id": "Proxy", "type": "select", "members": ["Auto", "jp", "DIRECT"], "default": null },
                    { "id": "Auto", "type": "url-test", "members": ["hk", "us"] },
                    { "id": "Streaming", "type": "select", "members": ["us", "hk"], "default": "hk" }
                ]
            }
        })
    }

    #[test]
    fn routed_profile_final_chain_follows_group_choices() {
        let mut profile = routed_profile();
        assert_eq!(
            routed_final_chain(&profile),
            (
                vec!["Proxy".to_string(), "Auto".to_string()],
                Some("hk".to_string())
            )
        );
        assert_eq!(routed_selected_node(&profile).as_deref(), Some("hk"));

        set_routed_group_default(&mut profile, "Proxy", "DIRECT").unwrap();
        assert_eq!(routed_final_chain(&profile).1.as_deref(), Some("DIRECT"));
        assert_eq!(routed_selected_node(&profile), None);
    }

    #[test]
    fn routed_node_switch_uses_the_main_select_group() {
        let mut profile = routed_profile();
        // jp is a member of Proxy, which FINAL reaches first.
        assert_eq!(
            routed_switch_group_for_node(&profile, "jp").as_deref(),
            Some("Proxy")
        );
        // hk is only in the url-test group and a group off FINAL's route.
        assert_eq!(routed_switch_group_for_node(&profile, "hk"), None);

        set_routed_group_default(&mut profile, "Proxy", "jp").unwrap();
        assert_eq!(routed_selected_node(&profile).as_deref(), Some("jp"));
        assert!(set_routed_group_default(&mut profile, "Auto", "us").is_err());
        assert!(set_routed_group_default(&mut profile, "Proxy", "us").is_err());
        assert!(set_routed_group_default(&mut profile, "Missing", "jp").is_err());
    }

    #[test]
    fn routed_group_defaults_survive_a_refresh() {
        let mut previous = routed_profile();
        set_routed_group_default(&mut previous, "Proxy", "jp").unwrap();
        set_routed_group_default(&mut previous, "Streaming", "us").unwrap();
        let mut refreshed = routed_profile();
        refreshed["routing"]["groups"][0]["members"] = json!(["Auto", "DIRECT"]);

        carry_routed_group_defaults(&previous, &mut refreshed);

        // jp left the Proxy group, so its old default is dropped.
        assert_eq!(routed_selected_node(&refreshed).as_deref(), Some("hk"));
        assert_eq!(
            routed_group(&refreshed, "Streaming")
                .and_then(group_choice)
                .as_deref(),
            Some("us")
        );
    }

    #[test]
    fn embedded_groups_label_nodes_by_name() {
        let nodes = vec![node("hk", "Hong Kong"), node("jp", "Tokyo")];
        let groups = embedded_groups(&routed_profile(), &nodes);

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].tag, "Proxy");
        assert_eq!(groups[0].group_type, "selector");
        assert_eq!(groups[0].selected.as_deref(), Some("Auto"));
        assert_eq!(groups[0].outbound_labels, vec!["Auto", "Tokyo", "DIRECT"]);
        assert_eq!(groups[1].group_type, "urltest");
        assert_eq!(groups[1].selected.as_deref(), Some("hk"));
    }

    #[test]
    fn rule_mode_round_trips() {
        assert_eq!(parse_routing_mode("rule"), Ok(DesktopRoutingMode::Rule));
        assert_eq!(routing_mode_name(DesktopRoutingMode::Rule), "rule");
        assert_eq!(parse_routing_mode("direct"), Ok(DesktopRoutingMode::Direct));
        // No preset list modes in public builds (docs/no-presets.md).
        let presets = ["bypass_china", "gfw_list", "return_china"]; // neutral-terms: allow (values saved by earlier versions)
        for preset in presets {
            assert!(parse_routing_mode(preset).is_err(), "{preset}");
        }
    }

    #[test]
    fn mode_envelopes_add_no_preset_rules() {
        let single = r#"{"schema_version":1,"profile_id":"p","outbound":{"type":"direct"}}"#;
        assert_eq!(
            mode_envelope(single, DesktopRoutingMode::Rule).unwrap(),
            single
        );
        let direct: Value =
            serde_json::from_str(&mode_envelope(single, DesktopRoutingMode::Direct).unwrap())
                .unwrap();
        assert_eq!(direct["routing"]["rules"], json!(["FINAL,DIRECT"]));
        assert_eq!(direct["routing"]["rule_sets"], json!({}));
        // The engine starts in its direct mode: without it DNS servers asked through a node (the default
        // 1.1.1.1:53 among them) still went through the node in "直连".
        assert_eq!(direct["mode"], "direct");
        assert_eq!(
            direct["routing"]["outbounds"]["proxy"],
            json!({ "type": "direct" })
        );

        let routed = r#"{"schema_version":1,"profile_id":"p","routing":{"outbounds":{"n1":{"type":"shadowsocks","server":"192.0.2.1","port":443,"method":"2022-blake3-aes-128-gcm","password":"AAECAwQFBgcICQoLDA0ODw=="}},"groups":[{"id":"Proxy","type":"select","members":["n1","DIRECT"]}],"rules":["DOMAIN,a.test,DIRECT","FINAL,Proxy"]}}"#;
        let rule: Value =
            serde_json::from_str(&mode_envelope(routed, DesktopRoutingMode::Rule).unwrap())
                .unwrap();
        assert!(rule.get("mode").is_none());
        let direct: Value =
            serde_json::from_str(&mode_envelope(routed, DesktopRoutingMode::Direct).unwrap())
                .unwrap();
        assert_eq!(direct["mode"], "direct");
        assert_eq!(direct["routing"]["rules"], json!(["FINAL,DIRECT"]));
        assert_eq!(direct["routing"]["groups"][0]["id"], "Proxy");
        assert!(mode_envelope("{}", DesktopRoutingMode::Direct).is_err());
    }

    fn node(id: &str, label: &str) -> DesktopProfileNode {
        DesktopProfileNode {
            id: id.to_string(),
            label: label.to_string(),
            protocol: "Shadowsocks".to_string(),
            outbound_tag: format!("node-{id}"),
            trunk: false,
        }
    }

    #[test]
    fn refresh_node_counts_use_stable_node_identity() {
        let previous = vec![node("retained", "Old retained"), node("removed", "Removed")];
        let current = vec![node("retained", "New retained"), node("added", "Added")];

        assert_eq!(
            subscription_node_change_counts(&previous, &current),
            (1, 1, 1)
        );
    }

    #[test]
    fn refresh_error_codes_are_stable_and_redacted() {
        assert_eq!(
            redacted_subscription_error_code(
                "request timed out for https://secret.invalid/?token=redacted"
            ),
            "windows.subscription.fetch_timeout"
        );
        assert_eq!(
            redacted_subscription_error_code("unsupported payload password=never-store"),
            "windows.subscription.refresh_failed"
        );
        assert_eq!(
            redacted_subscription_error_code("Subscription server answered HTTP status 403"),
            "windows.subscription.http_status_403"
        );
        assert_eq!(
            redacted_subscription_error_code("Subscription server could not be reached"),
            "windows.subscription.fetch_unreachable"
        );
        assert_eq!(
            redacted_subscription_error_code("Subscription download failed"),
            "windows.subscription.fetch_failed"
        );
        assert_eq!(
            redacted_subscription_error_code("Profile did not contain a supported proxy node"),
            "windows.subscription.invalid_payload"
        );
    }

    #[test]
    fn the_shown_next_update_is_the_one_the_monitor_acts_on() {
        let location = "https://sub.example/link";
        let mut desktop = DesktopState {
            auto_subscription_refresh: true,
            profile_source_url: Some(location.to_string()),
            ..DesktopState::default()
        };
        record_subscription_attempt(&mut desktop, location, "now".to_string());
        record_subscription_outcome(&mut desktop, location, true);
        let source = desktop.subscription_sources[0].clone();
        let success = source.last_success_unix.unwrap();
        // A 24-hour interval: the next update is a day after the success, not an hour.
        let at = subscription_next_update(&desktop, &source).unwrap();
        assert_eq!(at, success + 24 * 3600);
        assert_eq!(
            profile_next_attempt(&desktop),
            Some(format!("unix-epoch-seconds:{at}"))
        );
        assert_eq!(desktop.profile_next_attempt, profile_next_attempt(&desktop));

        // A failure waits for the retry time, and only the first one in a row is notified.
        record_subscription_outcome(&mut desktop, location, false);
        let source = desktop.subscription_sources[0].clone();
        assert_eq!(source.failures, 1);
        assert!(subscription_meta::notify_failure(source.failures));
        assert_eq!(
            subscription_next_update(&desktop, &source),
            Some(success + 24 * 3600)
        );

        // By hand only: nothing is shown and the monitor does nothing.
        desktop.subscription_sources[0].update_interval_hours = Some(0);
        let source = desktop.subscription_sources[0].clone();
        assert_eq!(subscription_next_update(&desktop, &source), None);
        // Automatic updates off: the same.
        desktop.subscription_sources[0].update_interval_hours = None;
        desktop.auto_subscription_refresh = false;
        assert_eq!(profile_next_attempt(&desktop), None);
    }
}
