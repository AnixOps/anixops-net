//! The Tauri window: the shared React interface (`apps/windows-gui/ui`) over the Linux backend. Command names
//! and argument shapes match the interface's `bridge.ts`; Windows-only commands answer that they are not
//! available, and the snapshot's `platform` field hides them in the interface.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use client_health::{startup, Health};
use desktop_mitm::plugins::{self, PluginSummary};
use engine_embedded::profile::RoutingMode;
use networkcore_linux_gui::backend::engine;
use networkcore_linux_gui::backend::state::Store;
use networkcore_linux_gui::backend::system_proxy::Desktop;
use networkcore_linux_gui::backend::trust;
use networkcore_linux_gui::backend::{
    autostart, format_delay, Backend, DnsPreset, GroupSummary, NodeSummary, OperationResult,
    PreferencesInput, RuntimeSnapshot, SubscriptionSummary, PASTED_LOCATION,
};
use subscription_meta::ResponseMeta;
use tauri::webview::PageLoadEvent;
use tauri::{Manager, RunEvent, State};

/// The URL the delay tests fetch, as on Windows.
const DELAY_URL: &str = "https://www.gstatic.com/generate_204";
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const UNAVAILABLE: &str = "Not available on Linux yet";

struct AppState(Arc<Mutex<Backend>>);

struct HealthState(Arc<Health>);

fn lock(backend: &Mutex<Backend>) -> MutexGuard<'_, Backend> {
    backend.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `task` on a blocking thread with the backend locked, so the window keeps polling meanwhile.
async fn with_backend<T, F>(state: &AppState, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&mut Backend) -> Result<T, String> + Send + 'static,
{
    let backend = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || task(&mut lock(&backend)))
        .await
        .map_err(|error| error.to_string())?
}

/// Runs `task` on a blocking thread without the lock, for network work.
async fn off_lock<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| error.to_string())?
}

fn http_get(url: &str) -> Result<String, reqwest::Error> {
    http_get_with_meta(url).map(|(body, _)| body)
}

/// The body and what the subscription headers say about it.
fn http_get_with_meta(url: &str) -> Result<(String, ResponseMeta), reqwest::Error> {
    // The panel picks the subscription format by User-Agent (same token as the iOS app).
    let response = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("AnixOpsNet/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)?;
    let meta = ResponseMeta::from_headers(|name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    });
    Ok((response.text()?, meta))
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

/// Downloads the public rule lists `mode` needs (all of them with `force`, else the missing ones) outside the
/// lock and caches them.
async fn download_rule_lists(
    state: &AppState,
    mode: RoutingMode,
    force: bool,
) -> Result<usize, String> {
    let (store, lists) = {
        let backend = lock(&state.0);
        (backend.store(), backend.rule_lists_to_download(mode, force))
    };
    off_lock(move || {
        for list in &lists {
            let content = http_get(list.url()).map_err(|error| {
                format!(
                    "the routing rules could not be downloaded from {}: {error}",
                    list.url()
                )
            })?;
            let entries = list.parse(&content);
            if entries.is_empty() {
                return Err(format!("{} returned no usable entries", list.url()));
            }
            store
                .write_rule_list(*list, &entries)
                .map_err(|error| format!("the routing rules could not be saved: {error}"))?;
        }
        Ok(lists.len())
    })
    .await
}

/// Connects after fetching any rule list the routing mode still lacks.
async fn connect_with_rules(state: &AppState) -> Result<OperationResult, String> {
    let mode = lock(&state.0).routing_mode();
    if let Err(error) = download_rule_lists(state, mode, false).await {
        lock(&state.0).record_error(&error);
        return Err(error);
    }
    with_backend(state, Backend::connect).await
}

#[tauri::command]
fn runtime_snapshot(state: State<'_, AppState>) -> RuntimeSnapshot {
    lock(&state.0).snapshot()
}

#[tauri::command]
fn list_nodes(state: State<'_, AppState>) -> Vec<NodeSummary> {
    lock(&state.0).nodes()
}

#[tauri::command]
fn list_subscriptions(state: State<'_, AppState>) -> Vec<SubscriptionSummary> {
    lock(&state.0).subscriptions()
}

#[tauri::command]
fn list_native_groups(state: State<'_, AppState>) -> Vec<GroupSummary> {
    lock(&state.0).groups()
}

#[tauri::command]
async fn select_native_group_outbound(
    state: State<'_, AppState>,
    group_tag: String,
    outbound_tag: String,
) -> Result<OperationResult, String> {
    with_backend(&state, move |backend| {
        backend.select_group_member(&group_tag, &outbound_tag)
    })
    .await
}

#[tauri::command]
async fn set_routing_mode(
    state: State<'_, AppState>,
    mode: String,
) -> Result<OperationResult, String> {
    let mode = RoutingMode::parse(&mode).ok_or("unknown routing mode")?;
    download_rule_lists(&state, mode, false).await?;
    with_backend(&state, move |backend| backend.set_routing_mode(mode)).await
}

#[tauri::command]
async fn set_dns_preset(
    state: State<'_, AppState>,
    preset: String,
) -> Result<OperationResult, String> {
    let preset = DnsPreset::parse(&preset).ok_or("unknown DNS choice")?;
    with_backend(&state, move |backend| backend.set_dns_preset(preset)).await
}

/// Downloads the current mode's public rule lists again; they apply on the next connection.
#[tauri::command]
async fn refresh_routing_rules(state: State<'_, AppState>) -> Result<OperationResult, String> {
    let mode = lock(&state.0).routing_mode();
    let count = download_rule_lists(&state, mode, true).await?;
    Ok(OperationResult {
        message: if count == 0 {
            "The current routing mode uses no public rule lists".to_string()
        } else {
            format!("Routing rules refreshed ({count} lists); they apply on the next connection")
        },
    })
}

#[tauri::command]
async fn connect(state: State<'_, AppState>) -> Result<OperationResult, String> {
    connect_with_rules(&state).await
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<OperationResult, String> {
    with_backend(&state, Backend::disconnect).await
}

/// "Restart core": starts the engine again with the current node.
#[tauri::command]
async fn restart_service(state: State<'_, AppState>) -> Result<OperationResult, String> {
    connect_with_rules(&state).await
}

#[tauri::command]
async fn validate_configuration(state: State<'_, AppState>) -> Result<OperationResult, String> {
    with_backend(&state, |backend| backend.validate()).await
}

#[tauri::command]
async fn check_profile_runtime(state: State<'_, AppState>) -> Result<OperationResult, String> {
    with_backend(&state, |backend| backend.validate()).await
}

#[tauri::command]
async fn switch_node(
    state: State<'_, AppState>,
    node_id: String,
) -> Result<OperationResult, String> {
    with_backend(&state, move |backend| backend.switch_node(&node_id)).await
}

#[tauri::command]
async fn runtime_control(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    with_backend(&state, move |backend| backend.runtime_control(&request)).await
}

#[tauri::command]
async fn test_node_delay(
    state: State<'_, AppState>,
    node_id: String,
    url: String,
) -> Result<OperationResult, String> {
    let (nodes, id, name) = lock(&state.0)
        .delay_target(&node_id)
        .ok_or("the node is not in any subscription")?;
    let delay = off_lock(move || engine::node_delay(&nodes, &id, &url)).await?;
    Ok(OperationResult {
        message: format!("{name}: {}", format_delay(delay)),
    })
}

/// One node's result in a batch delay test, as the Windows backend reports it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeDelay {
    node_id: String,
    delay_ms: Option<u64>,
    error: Option<String>,
}

/// What the overview shows about the path: the address websites see through the proxy, and how long a mainland
/// site takes to answer without it.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
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
async fn network_probe(
    state: State<'_, AppState>,
    connected: bool,
) -> Result<NetworkProbe, String> {
    let port = lock(&state.0).listen_port();
    off_lock(move || Ok(probe_network(connected.then_some(port)))).await
}

#[tauri::command]
async fn test_node_delays(
    state: State<'_, AppState>,
    node_ids: Vec<String>,
    url: String,
) -> Result<Vec<NodeDelay>, String> {
    let nodes = lock(&state.0).delay_nodes(&node_ids);
    let results = off_lock(move || Ok(engine::node_delays(&nodes, &url))).await?;
    Ok(results
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
        .collect())
}

#[tauri::command]
async fn node_share_link(state: State<'_, AppState>, node_id: String) -> Result<String, String> {
    lock(&state.0).share_link(&node_id)
}

#[tauri::command]
async fn select_fastest_node(state: State<'_, AppState>) -> Result<OperationResult, String> {
    let nodes = lock(&state.0).node_list();
    let (id, delay) = off_lock(move || {
        engine::fastest_node(&nodes, DELAY_URL)
            .ok_or_else(|| "no node answered the delay test".to_string())
    })
    .await?;
    let switched = with_backend(&state, move |backend| backend.switch_node(&id)).await?;
    Ok(OperationResult {
        message: format!("{} ({})", switched.message, format_delay(delay)),
    })
}

#[tauri::command]
async fn save_preferences(
    state: State<'_, AppState>,
    preferences: PreferencesInput,
) -> Result<OperationResult, String> {
    with_backend(&state, move |backend| {
        backend.save_preferences(&preferences)
    })
    .await
}

#[tauri::command]
async fn create_diagnostics(
    state: State<'_, AppState>,
    health: State<'_, HealthState>,
) -> Result<OperationResult, String> {
    let section = health.0.diagnostics();
    with_backend(&state, move |backend| backend.diagnostics(&section)).await
}

/// Crash reports, the metrics endpoint, the install check and the cold start, for the settings page.
#[tauri::command]
fn health_status(health: State<'_, HealthState>) -> serde_json::Value {
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
        .map_err(|error| format!("the setting could not be saved: {error}"))?;
    Ok(OperationResult {
        message: if enabled {
            "Crash reports on".to_string()
        } else {
            "Crash reports off; kept reports deleted".to_string()
        },
    })
}

#[tauri::command]
async fn import_subscription(
    state: State<'_, AppState>,
    location: String,
) -> Result<OperationResult, String> {
    let location = location.trim().to_string();
    if location.is_empty() {
        return Err("enter a subscription link, share links or a file path".to_string());
    }
    let label = if location.contains("://") && !location.starts_with("http") {
        PASTED_LOCATION.to_string()
    } else {
        location.clone()
    };
    let (content, meta) = off_lock(move || fetch(&location)).await?;
    with_backend(&state, move |backend| {
        let result = backend.import(&label, &content)?;
        backend.record_response(None, meta);
        Ok(result)
    })
    .await
}

#[tauri::command]
async fn update_subscription(state: State<'_, AppState>) -> Result<OperationResult, String> {
    let (id, location) = lock(&state.0)
        .refreshable_location()
        .ok_or("the current subscription is not a link that can be updated")?;
    refresh_source(&state.0, id, location).await
}

#[tauri::command]
async fn update_subscription_source(
    state: State<'_, AppState>,
    id: String,
) -> Result<OperationResult, String> {
    let location = lock(&state.0)
        .source_location(&id)
        .ok_or("this subscription is not a link that can be updated")?;
    refresh_source(&state.0, id, location).await
}

/// Downloads source `id` again outside the lock and records the result.
async fn refresh_source(
    backend: &Arc<Mutex<Backend>>,
    id: String,
    location: String,
) -> Result<OperationResult, String> {
    let fetched = off_lock(move || Ok(fetch(&location))).await?;
    let backend = backend.clone();
    off_lock(move || {
        let mut backend = lock(&backend);
        let (content, meta) = match fetched {
            Ok((content, meta)) => (Ok(content), Some(meta)),
            Err(error) => (Err(error), None),
        };
        let result = backend.finish_refresh(&id, content);
        if let (Ok(_), Some(meta)) = (&result, meta) {
            backend.record_response(Some(&id), meta);
        }
        result
    })
    .await
}

#[tauri::command]
async fn rename_subscription(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<OperationResult, String> {
    with_backend(&state, move |backend| {
        backend.rename_subscription(&id, &name)
    })
    .await
}

#[tauri::command]
async fn set_subscription_interval(
    state: State<'_, AppState>,
    id: String,
    hours: u32,
) -> Result<OperationResult, String> {
    with_backend(&state, move |backend| {
        backend.set_subscription_interval(&id, hours)
    })
    .await
}

/// Rebuilds the engine's connections when the network changes or the computer wakes (`network_watch`).
fn start_network_watch(backend: Arc<Mutex<Backend>>) {
    thread::spawn(move || {
        let mut watch = network_watch::NetworkWatch::new();
        loop {
            let path = network_watch::current_path();
            if let Some(reset) = watch.observe(std::time::SystemTime::now(), path) {
                lock(&backend).reset_network(reset);
            }
            thread::sleep(network_watch::POLL);
        }
    });
}

/// Updates every subscription whose interval has passed, checking once a minute.
fn start_subscription_updates(backend: Arc<Mutex<Backend>>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(60));
        let due = lock(&backend).due_sources();
        for (id, location) in due {
            let fetched = fetch(&location);
            let mut backend = lock(&backend);
            let (content, meta) = match fetched {
                Ok((content, meta)) => (Ok(content), Some(meta)),
                Err(error) => (Err(error), None),
            };
            if let (Ok(_), Some(meta)) = (backend.finish_refresh(&id, content), meta) {
                backend.record_response(Some(&id), meta);
            }
        }
    });
}

#[tauri::command]
async fn select_subscription(
    state: State<'_, AppState>,
    id: String,
) -> Result<OperationResult, String> {
    with_backend(&state, move |backend| backend.select_subscription(&id)).await
}

#[tauri::command]
async fn remove_subscription(
    state: State<'_, AppState>,
    id: String,
) -> Result<OperationResult, String> {
    with_backend(&state, move |backend| backend.remove_subscription(&id)).await
}

#[tauri::command]
async fn restore_network_settings(state: State<'_, AppState>) -> Result<OperationResult, String> {
    with_backend(&state, Backend::restore_network_settings).await
}

/// Generates the root on first use, has the browsers and the system trust it (polkit asks for the system
/// store), then restarts a running engine with decryption on.
#[tauri::command]
async fn enable_https_mitm(state: State<'_, AppState>) -> Result<OperationResult, String> {
    let root = lock(&state.0).ensure_mitm_root()?;
    let trusted = off_lock(move || Ok(trust::install(&root.certificate_path))).await?;
    let result = with_backend(&state, |backend| backend.set_https_decryption(true)).await?;
    Ok(OperationResult {
        message: format!("{}. {}", result.message, trusted.summary("trusted")),
    })
}

#[tauri::command]
async fn disable_https_mitm(state: State<'_, AppState>) -> Result<OperationResult, String> {
    let result = with_backend(&state, |backend| backend.set_https_decryption(false)).await?;
    let removed = off_lock(|| Ok(trust::remove())).await?;
    Ok(OperationResult {
        message: format!("{}. {}", result.message, removed.summary("removed")),
    })
}

#[tauri::command]
async fn list_mitm_plugins(state: State<'_, AppState>) -> Result<Vec<PluginSummary>, String> {
    let directory = lock(&state.0).plugins_dir();
    off_lock(move || plugins::list(&directory)).await
}

/// Changes the plugin library outside the lock, then applies it to a running engine.
async fn change_plugins<F>(state: &AppState, change: F) -> Result<OperationResult, String>
where
    F: FnOnce(&std::path::Path) -> Result<String, String> + Send + 'static,
{
    let directory = lock(&state.0).plugins_dir();
    let message = off_lock(move || change(&directory)).await?;
    let applied = with_backend(state, |backend| backend.plugins_changed()).await?;
    Ok(OperationResult {
        message: format!("{message}; {applied}"),
    })
}

#[tauri::command]
async fn import_mitm_plugin(
    state: State<'_, AppState>,
    url: String,
) -> Result<OperationResult, String> {
    change_plugins(&state, move |directory| {
        let plugin = plugins::import(directory, &url, plugins::download)?;
        Ok(format!("Plugin imported: {}", plugin.name))
    })
    .await
}

#[tauri::command]
async fn set_mitm_plugin_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<OperationResult, String> {
    change_plugins(&state, move |directory| {
        plugins::set_enabled(directory, &id, enabled)?;
        Ok(if enabled {
            "Plugin enabled"
        } else {
            "Plugin disabled"
        }
        .to_string())
    })
    .await
}

#[tauri::command]
async fn remove_mitm_plugin(
    state: State<'_, AppState>,
    id: String,
) -> Result<OperationResult, String> {
    change_plugins(&state, move |directory| {
        plugins::remove(directory, &id)?;
        Ok("Plugin removed".to_string())
    })
    .await
}

/// Commands the Windows client has and Linux does not yet; the interface hides their controls.
macro_rules! unavailable {
    ($($name:ident),* $(,)?) => {
        $(
            #[tauri::command]
            fn $name() -> Result<(), String> {
                Err(UNAVAILABLE.to_string())
            }
        )*
    };
}

unavailable!(
    install_service,
    start_service,
    stop_service,
    configure_script_runtime,
    clear_script_runtime,
    install_certificate,
    remove_certificate,
    install_driver,
    remove_driver,
);

pub fn run() -> Result<(), String> {
    let dir = Store::default_dir().ok_or("HOME is not set")?;
    // Inside an AppImage the running file is a temporary mount; autostart must point at the image itself.
    let executable = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)
        .map_err(|error| error.to_string())?;
    let health = Health::start(
        &dir,
        "linux",
        env!("CARGO_PKG_VERSION"),
        std::env::current_exe().unwrap_or_else(|_| executable.clone()),
    );
    let backend = Backend::open(
        Store::new(dir),
        Desktop::from_env(),
        autostart::autostart_dir(),
        executable,
    );
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
    if auto_connect {
        let state = AppState(shared.clone());
        thread::spawn(move || {
            let _ = tauri::async_runtime::block_on(connect_with_rules(&state));
        });
    }

    start_subscription_updates(shared.clone());
    start_network_watch(shared.clone());

    let app = tauri::Builder::default()
        .manage(AppState(shared.clone()))
        .manage(HealthState(health))
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
        .invoke_handler(tauri::generate_handler![
            runtime_snapshot,
            list_nodes,
            list_native_groups,
            list_subscriptions,
            connect,
            disconnect,
            restart_service,
            validate_configuration,
            check_profile_runtime,
            switch_node,
            runtime_control,
            test_node_delay,
            test_node_delays,
            network_probe,
            node_share_link,
            select_fastest_node,
            save_preferences,
            create_diagnostics,
            import_subscription,
            update_subscription,
            select_subscription,
            remove_subscription,
            update_subscription_source,
            rename_subscription,
            set_subscription_interval,
            restore_network_settings,
            select_native_group_outbound,
            set_routing_mode,
            set_dns_preset,
            refresh_routing_rules,
            install_service,
            start_service,
            stop_service,
            configure_script_runtime,
            clear_script_runtime,
            enable_https_mitm,
            disable_https_mitm,
            list_mitm_plugins,
            import_mitm_plugin,
            set_mitm_plugin_enabled,
            remove_mitm_plugin,
            install_certificate,
            remove_certificate,
            install_driver,
            remove_driver,
            health_status,
            set_crash_reports,
        ])
        .build(tauri::generate_context!())
        .map_err(|error| error.to_string())?;
    app.run(move |_, event| {
        // Put the desktop's proxy settings back when the window closes; a crash leaves them for the next start.
        if let RunEvent::Exit = event {
            lock(&shared).shutdown();
        }
    });
    Ok(())
}
