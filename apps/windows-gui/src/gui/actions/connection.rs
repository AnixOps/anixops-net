use crate::gui::{
    load_validated_managed_configuration,
    runtime_status::{read_runtime_status, ManagedCoreStatus},
    startup::{owns_current_proxy, save_desktop_state, DesktopState},
    tun_mode::DesktopTun,
    ui_state::ConnectionState,
};
use engine_embedded::local_proxy::LocalProxy;
use inbound_mixed::MixedSettings;
use networkcore_windows_service::DesktopNativeMitm;
use platform_windows::managed::{
    read_managed_state, windows_managed_config_path, windows_managed_state_path,
    write_managed_state, WindowsManagedConfig, WindowsManagedEmbeddedConfig,
    WindowsManagedNativeMitmConfig, WindowsProxySettings, WindowsProxySnapshot,
};
use platform_windows::system_integration::{
    managed_proxy_listener_ready, read_current_user_system_proxy, NativeWindowsSystemIntegration,
    WindowsServiceState, WindowsSystemIntegration,
};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ConnectedProxy {
    pub snapshot: WindowsProxySnapshot,
    pub applied_proxy: WindowsProxySettings,
}

#[derive(Debug)]
pub enum RestartedService {
    Desktop(ConnectedProxy),
    ServiceManaged,
}

struct DesktopConnectionPlan {
    proxy: WindowsProxySettings,
    embedded: Option<WindowsManagedEmbeddedConfig>,
    native_mitm: Option<WindowsManagedNativeMitmConfig>,
}

/// The core the GUI runs itself in desktop mode: the embedded engine inside
/// the GUI process, and the native HTTPS MITM proxy in front of it when the
/// profile enables HTTPS decryption (the GUI always runs elevated).
#[derive(Default)]
pub struct DesktopCore {
    embedded: Option<LocalProxy>,
    native_mitm: Option<DesktopNativeMitm>,
    tun: Option<DesktopTun>,
}

impl DesktopCore {
    pub fn is_running(&mut self) -> Result<bool, String> {
        Ok(self.embedded.as_ref().is_some_and(LocalProxy::is_running)
            && self
                .native_mitm
                .as_ref()
                .is_none_or(DesktopNativeMitm::is_running)
            && self.tun.as_ref().is_none_or(DesktopTun::is_running))
    }

    /// Whether TUN mode is capturing system traffic in this window.
    pub fn tun_running(&self) -> bool {
        self.tun.as_ref().is_some_and(DesktopTun::is_running)
    }

    /// Starts TUN mode on the profile's envelope. Called before the local
    /// proxy starts, so every engine socket is pinned to the physical
    /// interface from the first connection on.
    fn start_tun(&mut self, config: &WindowsManagedEmbeddedConfig) -> Result<(), String> {
        self.stop_tun();
        let envelope = fs::read(&config.envelope_path)
            .map_err(|error| format!("TUN mode configuration could not be read: {error}"))?;
        self.tun = Some(DesktopTun::start(&envelope)?);
        Ok(())
    }

    fn stop_tun(&mut self) {
        if let Some(tun) = self.tun.take() {
            tun.stop();
        }
    }

    /// Trusts the managed root CA, records its thumbprint where disabling
    /// HTTPS decryption removes it, then starts the MITM listener.
    fn start_native_mitm(&mut self, config: &WindowsManagedNativeMitmConfig) -> Result<(), String> {
        self.stop_native_mitm();
        let thumbprint = NativeWindowsSystemIntegration::new()
            .install_root_certificate(&config.ca_certificate_path)
            .map_err(|error| {
                format!("HTTPS decryption root certificate could not be trusted: {error}")
            })?;
        let state_path = windows_managed_state_path();
        let mut state = read_managed_state(&state_path).unwrap_or_default();
        if state.native_mitm_certificate_sha1.as_deref() != Some(thumbprint.as_str()) {
            state.native_mitm_certificate_sha1 = Some(thumbprint);
            write_managed_state(&state_path, &state).map_err(|error| {
                format!("HTTPS decryption certificate could not be recorded: {error}")
            })?;
        }
        let mitm = DesktopNativeMitm::start(config)
            .map_err(|error| format!("HTTPS decryption could not start: {}", error.message))?;
        self.native_mitm = Some(mitm);
        Ok(())
    }

    fn stop_native_mitm(&mut self) {
        if let Some(mitm) = self.native_mitm.take() {
            let _ = mitm.stop();
        }
    }

    /// The listener is bound when this returns.
    fn start_embedded(&mut self, config: &WindowsManagedEmbeddedConfig) -> Result<(), String> {
        self.stop_embedded();
        let envelope = fs::read(&config.envelope_path)
            .map_err(|error| format!("embedded engine configuration could not be read: {error}"))?;
        let listen = config.listener().parse::<SocketAddr>().map_err(|_| {
            "embedded engine listener must be a loopback socket address".to_string()
        })?;
        let proxy = LocalProxy::start(&envelope, listen, MixedSettings::default())
            .map_err(|error| format!("embedded engine could not start: {}", error.message))?;
        self.embedded = Some(proxy);
        Ok(())
    }

    /// Answers one kernel control request while this window runs the engine.
    pub fn control(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        let proxy = self
            .embedded
            .as_ref()
            .filter(|proxy| proxy.is_running())
            .ok_or("the embedded engine is not running in this window")?;
        proxy
            .control_json(request)
            .map_err(|error| error.message.to_string())
    }

    fn stop_embedded(&mut self) {
        if let Some(mut proxy) = self.embedded.take() {
            proxy.stop();
        }
    }

    fn stop(&mut self) {
        self.stop_native_mitm();
        self.stop_tun();
        self.stop_embedded();
    }
}

/// Kernel control commands the client may send: reading traffic, connections
/// and rule sets, closing connections and refreshing remote rule sets. Mode
/// and group changes go through the saved profile instead.
const ACTIVITY_COMMANDS: [&str; 6] = [
    "traffic",
    "connections",
    "close_connection",
    "close_all",
    "rule_sets",
    "update_rule_sets",
];

/// The request bytes for an activity page command, or why it is refused.
pub fn activity_request(request: &serde_json::Value) -> Result<Vec<u8>, String> {
    let command = request
        .get("cmd")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !ACTIVITY_COMMANDS.contains(&command) {
        return Err(format!(
            "runtime control command {command:?} is not available"
        ));
    }
    serde_json::to_vec(request).map_err(|error| error.to_string())
}

pub const fn can_connect(connection: ConnectionState) -> bool {
    !matches!(
        connection,
        ConnectionState::Connected | ConnectionState::Connecting
    )
}

pub const fn can_disconnect(connection: ConnectionState, has_gui_owned_proxy: bool) -> bool {
    has_gui_owned_proxy || !matches!(connection, ConnectionState::Disconnected)
}

pub const fn should_auto_connect(enabled: bool, already_attempted: bool, connected: bool) -> bool {
    enabled && !already_attempted && !connected
}

pub const fn should_restart_gui_started_core(
    enabled: bool,
    already_attempted: bool,
    gui_started_connection: bool,
    connection: ConnectionState,
) -> bool {
    enabled
        && !already_attempted
        && gui_started_connection
        && matches!(connection, ConnectionState::CoreError)
}

pub fn should_restore_abandoned_owned_proxy(
    has_proxy_snapshot: bool,
    service_state: WindowsServiceState,
    core: &ManagedCoreStatus,
    already_attempted: bool,
) -> bool {
    has_proxy_snapshot
        && !already_attempted
        && matches!(
            service_state,
            WindowsServiceState::NotInstalled | WindowsServiceState::Stopped
        )
        && matches!(
            core,
            ManagedCoreStatus::NotConfigured | ManagedCoreStatus::Failed { .. }
        )
}

pub fn connect(config_path: PathBuf, desktop: DesktopState) -> Result<ConnectedProxy, String> {
    let plan = load_desktop_connection_plan(&config_path)?;
    start_desktop_connection(plan, desktop)
}

pub fn connect_direct(
    config_path: PathBuf,
    desktop: DesktopState,
    core: &mut DesktopCore,
) -> Result<ConnectedProxy, String> {
    let plan = direct_connection_plan(load_validated_daily_managed_configuration(&config_path)?)?;
    let embedded = plan.embedded.as_ref().ok_or_else(|| {
        "Desktop mode requires an imported profile. Import a subscription first.".to_string()
    })?;
    if desktop.tun_mode_enabled {
        core.start_tun(embedded)?;
    }
    if let Err(error) = core.start_embedded(embedded) {
        core.stop();
        return Err(error);
    }
    if let Some(native_mitm) = plan.native_mitm.as_ref() {
        if let Err(error) = core.start_native_mitm(native_mitm) {
            core.stop();
            return Err(error);
        }
    }
    apply_proxy_after_direct_readiness(&plan.proxy, desktop, core)
}

pub fn disconnect_direct(
    config_path: PathBuf,
    desktop: DesktopState,
    core: &mut DesktopCore,
) -> Result<String, String> {
    load_validated_daily_managed_configuration(&config_path)?;
    let integration = NativeWindowsSystemIntegration::new();
    let mut proxy_restored = false;
    if let Some(snapshot) = desktop.proxy_snapshot.as_ref() {
        let current = read_current_user_system_proxy().map_err(|error| error.to_string())?;
        if owns_current_proxy(&desktop, &current) {
            integration
                .restore_system_proxy(snapshot)
                .map_err(|error| error.to_string())?;
            proxy_restored = true;
        }
    }
    core.stop();
    Ok(if proxy_restored {
        "Desktop core stopped and the GUI-owned proxy snapshot was restored.".to_string()
    } else {
        "Desktop core stopped. The current-user proxy was left unchanged because it was not owned by this GUI session.".to_string()
    })
}

pub fn restart(config_path: PathBuf, desktop: DesktopState) -> Result<RestartedService, String> {
    let managed = load_validated_daily_managed_configuration(&config_path)?;
    if managed.system_proxy_owner.is_service_managed() {
        NativeWindowsSystemIntegration::new()
            .restart_service()
            .map_err(|error| error.to_string())?;
        return Ok(RestartedService::ServiceManaged);
    }
    let plan = desktop_connection_plan(managed)?;
    disconnect(desktop.clone())?;
    start_desktop_connection(plan, desktop).map(RestartedService::Desktop)
}

fn load_desktop_connection_plan(config_path: &Path) -> Result<DesktopConnectionPlan, String> {
    desktop_connection_plan(load_validated_daily_managed_configuration(config_path)?)
}

fn load_validated_daily_managed_configuration(
    config_path: &Path,
) -> Result<WindowsManagedConfig, String> {
    let managed_config_path = windows_managed_config_path();
    if config_path != managed_config_path.as_path() {
        return Err(
            "Apply this configuration before connecting or restarting so the Windows service uses the validated file."
                .to_string(),
        );
    }
    load_validated_managed_configuration(config_path)
}

fn desktop_connection_plan(managed: WindowsManagedConfig) -> Result<DesktopConnectionPlan, String> {
    if managed.system_proxy_owner.is_service_managed() {
        return Err(
            "This configuration is managed by the Windows service. Use the explicit advanced workflow or import a daily desktop profile before connecting."
                .to_string(),
        );
    }
    if managed
        .native_mitm
        .as_ref()
        .is_some_and(|mitm| mitm.enabled)
    {
        return Err(
            "This profile enables HTTPS MITM and must run in Windows service mode.".to_string(),
        );
    }
    let proxy = managed
        .system_proxy
        .filter(|proxy| proxy.enabled)
        .ok_or_else(|| {
            "Connection requires an enabled managed system proxy. Import a profile or configure one first."
                .to_string()
        })?;
    let embedded = managed.embedded.filter(|embedded| embedded.enabled);
    Ok(DesktopConnectionPlan {
        proxy,
        embedded,
        native_mitm: None,
    })
}

/// Desktop mode: the GUI runs the embedded engine and, when the profile
/// enables HTTPS decryption, the MITM listener in front of it, so a
/// MITM-enabled profile no longer needs the Windows service.
fn direct_connection_plan(
    mut managed: WindowsManagedConfig,
) -> Result<DesktopConnectionPlan, String> {
    let Some(native_mitm) = managed.native_mitm.take().filter(|mitm| mitm.enabled) else {
        return desktop_connection_plan(managed);
    };
    if native_mitm.script_runtime.is_some() {
        return Err(
            "Clear the legacy managed script runtime before enabling HTTPS MITM; Windows script execution requires a no-network sandbox."
                .to_string(),
        );
    }
    let proxy = managed
        .system_proxy
        .filter(|proxy| proxy.enabled)
        .ok_or_else(|| {
            "Connection requires an enabled managed system proxy. Import a profile or configure one first."
                .to_string()
        })?;
    let embedded = managed.embedded.filter(|embedded| embedded.enabled);
    Ok(DesktopConnectionPlan {
        proxy,
        embedded,
        native_mitm: Some(native_mitm),
    })
}

fn apply_proxy_after_direct_readiness(
    proxy: &WindowsProxySettings,
    mut desktop: DesktopState,
    core: &mut DesktopCore,
) -> Result<ConnectedProxy, String> {
    let integration = NativeWindowsSystemIntegration::new();
    let current = read_current_user_system_proxy().map_err(|error| error.to_string())?;
    let existing_snapshot = owns_current_proxy(&desktop, &current)
        .then(|| desktop.proxy_snapshot.clone())
        .flatten();
    let applied_snapshot = match integration.apply_system_proxy(proxy) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            core.stop();
            return Err(error.to_string());
        }
    };
    let snapshot = existing_snapshot.unwrap_or(applied_snapshot);
    desktop.proxy_snapshot = Some(snapshot.clone());
    desktop.applied_proxy = Some(proxy.clone());
    if let Err(error) = save_desktop_state(&desktop) {
        let _ = integration.restore_system_proxy(&snapshot);
        core.stop();
        return Err(format!(
            "desktop proxy ownership could not be saved after connection: {error}"
        ));
    }
    Ok(ConnectedProxy {
        snapshot,
        applied_proxy: proxy.clone(),
    })
}

fn start_desktop_connection(
    plan: DesktopConnectionPlan,
    desktop: DesktopState,
) -> Result<ConnectedProxy, String> {
    let DesktopConnectionPlan { proxy, .. } = plan;
    let integration = NativeWindowsSystemIntegration::new();
    integration
        .start_service()
        .map_err(|error| rollback_failed_connection(&integration, &desktop, error.to_string()))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let waiting_for = format!("local proxy listener at {}", proxy.server);
    loop {
        let runtime = read_runtime_status();
        if runtime.service_state == WindowsServiceState::Running
            && runtime.core.liveness_confirmed() == Some(true)
        {
            let listener_ready = managed_proxy_listener_ready(&proxy, Duration::from_millis(500))
                .map_err(|error| {
                rollback_failed_connection(&integration, &desktop, error.to_string())
            })?;
            if listener_ready {
                return apply_proxy_after_readiness(&integration, proxy, desktop);
            }
        }
        if matches!(
            runtime.connection,
            ConnectionState::CoreError | ConnectionState::ConfigurationError
        ) {
            let message = runtime
                .last_error
                .or(runtime.configuration_error)
                .unwrap_or_else(|| "managed runtime failed before the core was ready".to_string());
            return Err(rollback_failed_connection(&integration, &desktop, message));
        }
        if Instant::now() >= deadline {
            return Err(rollback_failed_connection(
                &integration,
                &desktop,
                format!("timed out waiting for {waiting_for}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn apply_proxy_after_readiness(
    integration: &NativeWindowsSystemIntegration,
    proxy: WindowsProxySettings,
    mut desktop: DesktopState,
) -> Result<ConnectedProxy, String> {
    let current = read_current_user_system_proxy().map_err(|error| {
        rollback_failed_connection(
            integration,
            &desktop,
            format!("current-user proxy could not be read before connection: {error}"),
        )
    })?;
    let existing_snapshot = owns_current_proxy(&desktop, &current)
        .then(|| desktop.proxy_snapshot.clone())
        .flatten();
    let applied_snapshot = integration.apply_system_proxy(&proxy).map_err(|error| {
        rollback_failed_connection(
            integration,
            &desktop,
            format!("system proxy could not be applied after core readiness verification: {error}"),
        )
    })?;
    let snapshot = existing_snapshot.unwrap_or(applied_snapshot);
    desktop.proxy_snapshot = Some(snapshot.clone());
    desktop.applied_proxy = Some(proxy.clone());
    if let Err(error) = save_desktop_state(&desktop) {
        let restore = integration.restore_system_proxy(&snapshot);
        let stopped = integration.stop_service();
        return Err(match (restore, stopped) {
            (Ok(()), Ok(_)) => format!(
                "proxy ownership could not be saved after connection; the proxy was restored: {error}"
            ),
            (restore, stopped) => format!(
                "proxy ownership could not be saved after connection: {error}; rollback result proxy={restore:?} service={stopped:?}"
            ),
        });
    }
    Ok(ConnectedProxy {
        snapshot,
        applied_proxy: proxy,
    })
}

fn rollback_failed_connection(
    integration: &NativeWindowsSystemIntegration,
    desktop: &DesktopState,
    message: String,
) -> String {
    let proxy_result = desktop.proxy_snapshot.as_ref().map_or(Ok(()), |snapshot| {
        let current = read_current_user_system_proxy().map_err(|error| error.to_string())?;
        if owns_current_proxy(desktop, &current) {
            integration
                .restore_system_proxy(snapshot)
                .map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    });
    let service_result = integration
        .stop_service()
        .map(|_| ())
        .map_err(|error| error.to_string());
    match (proxy_result, service_result) {
        (Ok(()), Ok(())) => message,
        (proxy, service) => {
            format!("{message}; cleanup result proxy={proxy:?} service={service:?}")
        }
    }
}

pub fn disconnect(desktop: DesktopState) -> Result<String, String> {
    let integration = NativeWindowsSystemIntegration::new();
    let mut proxy_restored = false;
    if let Some(snapshot) = desktop.proxy_snapshot.clone() {
        let current = read_current_user_system_proxy().map_err(|error| error.to_string())?;
        if owns_current_proxy(&desktop, &current) {
            integration
                .restore_system_proxy(&snapshot)
                .map_err(|error| error.to_string())?;
            proxy_restored = true;
        }
    }
    integration
        .stop_service()
        .map_err(|error| error.to_string())?;
    Ok(if proxy_restored {
        "Service stopped and the GUI-owned desktop proxy snapshot was restored.".to_string()
    } else {
        "Service stopped. The current-user proxy was left unchanged because it was not owned by this GUI session."
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_windows::managed::{
        WindowsSystemProxyOwner, WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION,
    };

    fn managed_config(
        owner: WindowsSystemProxyOwner,
        proxy: Option<WindowsProxySettings>,
    ) -> WindowsManagedConfig {
        WindowsManagedConfig {
            schema_version: WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION,
            system_proxy: proxy,
            system_proxy_owner: owner,
            root_certificate_path: None,
            driver_package: None,
            native_mitm: None,
            embedded: None,
        }
    }

    #[test]
    fn activity_requests_only_read_and_close_connections() {
        let request = activity_request(&serde_json::json!({"cmd": "close_connection", "id": 7}))
            .expect("closing one connection is allowed");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&request).unwrap(),
            serde_json::json!({"cmd": "close_connection", "id": 7})
        );
        assert!(activity_request(&serde_json::json!({"cmd": "traffic"})).is_ok());
        assert!(activity_request(&serde_json::json!({"cmd": "update_rule_sets"})).is_ok());
        assert!(
            activity_request(&serde_json::json!({"cmd": "set_mode", "mode": "global"})).is_err()
        );
        assert!(activity_request(&serde_json::json!({"id": 7})).is_err());
    }

    #[test]
    fn connection_actions_respect_the_aggregated_runtime_state() {
        assert!(!can_connect(ConnectionState::Connected));
        assert!(!can_connect(ConnectionState::Connecting));
        assert!(can_connect(ConnectionState::CoreError));
        assert!(!can_disconnect(ConnectionState::Disconnected, false));
        assert!(can_disconnect(ConnectionState::CoreError, false));
        assert!(can_disconnect(ConnectionState::Disconnected, true));
    }

    #[test]
    fn controlled_recovery_requires_a_connection_started_by_this_gui_run() {
        assert!(!should_restart_gui_started_core(
            true,
            false,
            false,
            ConnectionState::CoreError,
        ));
        assert!(should_restart_gui_started_core(
            true,
            false,
            true,
            ConnectionState::CoreError,
        ));
        assert!(!should_restart_gui_started_core(
            true,
            true,
            true,
            ConnectionState::CoreError,
        ));
    }

    #[test]
    fn abandoned_gui_proxy_recovery_runs_once_only_after_the_runtime_is_gone() {
        assert!(should_restore_abandoned_owned_proxy(
            true,
            WindowsServiceState::Stopped,
            &ManagedCoreStatus::Failed {
                reason: "listener failed".to_string(),
            },
            false,
        ));
        assert!(!should_restore_abandoned_owned_proxy(
            true,
            WindowsServiceState::Stopped,
            &ManagedCoreStatus::Failed {
                reason: "listener failed".to_string(),
            },
            true,
        ));
        assert!(!should_restore_abandoned_owned_proxy(
            true,
            WindowsServiceState::Running,
            &ManagedCoreStatus::EmbeddedRunning {
                endpoint: "127.0.0.1:7890".to_string(),
            },
            false,
        ));
    }

    #[test]
    fn daily_desktop_restart_requires_an_owned_enabled_proxy_plan() {
        let proxy = WindowsProxySettings {
            enabled: true,
            server: "127.0.0.1:7890".to_string(),
            bypass: "<local>".to_string(),
        };
        let plan = desktop_connection_plan(managed_config(
            WindowsSystemProxyOwner::Desktop,
            Some(proxy.clone()),
        ))
        .expect("daily desktop restart should have a readiness plan");
        assert_eq!(plan.proxy, proxy);

        assert!(
            desktop_connection_plan(managed_config(WindowsSystemProxyOwner::Desktop, None,))
                .is_err()
        );
        assert!(desktop_connection_plan(managed_config(
            WindowsSystemProxyOwner::Service,
            Some(proxy),
        ))
        .is_err());
    }

    #[test]
    fn desktop_mode_runs_a_mitm_profile_itself() {
        let proxy = WindowsProxySettings {
            enabled: true,
            server: "127.0.0.1:7890".to_string(),
            bypass: "<local>".to_string(),
        };
        let mut managed = managed_config(WindowsSystemProxyOwner::Service, Some(proxy.clone()));
        managed.native_mitm = Some(WindowsManagedNativeMitmConfig {
            enabled: true,
            listen_host: "127.0.0.1".to_string(),
            listen_port: 7890,
            upstream_socks_host: "127.0.0.1".to_string(),
            upstream_socks_port: 7891,
            ca_certificate_path: PathBuf::from("root-ca.pem"),
            ca_private_key_path: PathBuf::from("root-ca-key.pem"),
            log_path: PathBuf::from("native-mitm.log"),
            bilibili_web_ad_block_enabled: false,
            script_runtime: None,
        });
        assert!(desktop_connection_plan(managed.clone()).is_err());
        let plan = direct_connection_plan(managed.clone())
            .expect("desktop mode should accept a MITM-enabled profile");
        assert_eq!(plan.proxy, proxy);
        assert_eq!(
            plan.native_mitm.map(|mitm| mitm.upstream_socks_port),
            Some(7891)
        );

        managed.native_mitm = None;
        assert!(direct_connection_plan(managed).is_err());
    }
}
