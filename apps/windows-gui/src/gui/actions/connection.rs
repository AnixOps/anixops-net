use crate::gui::{
    append_managed_log, load_validated_managed_configuration, mesh, mitm_plugins, owned_proxy,
    startup::{
        my_rules_path, owns_current_proxy, rule_set_cache_dir, save_desktop_state, DesktopState,
    },
    tun_mode::DesktopTun,
    ui_state::ConnectionState,
};
use desktop_mitm::envelope_with_mitm;
use engine_embedded::local_proxy::LocalProxy;
use inbound_mixed::MixedSettings;
use platform_windows::managed::{
    windows_managed_config_path, WindowsManagedConfig, WindowsManagedEmbeddedConfig,
    WindowsManagedNativeMitmConfig, WindowsProxySettings, WindowsProxySnapshot,
};
use platform_windows::system_integration::{
    read_current_user_system_proxy, NativeWindowsSystemIntegration, WindowsServiceState,
    WindowsSystemIntegration,
};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ConnectedProxy {
    pub snapshot: WindowsProxySnapshot,
    pub applied_proxy: WindowsProxySettings,
}

struct DesktopConnectionPlan {
    proxy: WindowsProxySettings,
    embedded: Option<WindowsManagedEmbeddedConfig>,
    native_mitm: Option<WindowsManagedNativeMitmConfig>,
}

/// The core the GUI runs itself in desktop mode: the embedded engine inside
/// the GUI process. When the profile enables HTTPS decryption the engine
/// decrypts itself, on the local proxy and in TUN mode alike, with the same
/// MITM the iOS app uses (the GUI always runs elevated).
#[derive(Default)]
pub struct DesktopCore {
    embedded: Option<LocalProxy>,
    tun: Option<DesktopTun>,
}

impl DesktopCore {
    pub fn is_running(&mut self) -> Result<bool, String> {
        Ok(self.embedded.as_ref().is_some_and(LocalProxy::is_running)
            && self.tun.as_ref().is_none_or(DesktopTun::is_running))
    }

    /// Whether TUN mode is capturing system traffic in this window.
    pub fn tun_running(&self) -> bool {
        self.tun.as_ref().is_some_and(DesktopTun::is_running)
    }

    /// Starts TUN mode on the profile's envelope. Called before the local
    /// proxy starts, so every engine socket is pinned to the physical
    /// interface from the first connection on.
    fn start_tun(&mut self, envelope: &[u8]) -> Result<(), String> {
        self.stop_tun();
        self.tun = Some(DesktopTun::start(envelope)?);
        Ok(())
    }

    fn stop_tun(&mut self) {
        if let Some(tun) = self.tun.take() {
            tun.stop();
        }
    }

    /// The listener is bound when this returns.
    fn start_embedded(&mut self, envelope: &[u8], listen: SocketAddr) -> Result<(), String> {
        self.stop_embedded();
        let proxy = LocalProxy::start(envelope, listen, MixedSettings::default())
            .map_err(|error| format!("embedded engine could not start: {}", error.message))?;
        self.embedded = Some(proxy);
        // The lists read the new engine now; a session the viewer held after the last stop is let go.
        desktop_mitm::sessions::close_viewer();
        Ok(())
    }

    /// The local proxy of the engine this window runs.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.embedded
            .as_ref()
            .filter(|proxy| proxy.is_running())
            .map(LocalProxy::local_addr)
    }

    /// Answers one kernel control request while this window runs the engine. With no engine running, the capture
    /// commands go to the kernel's capture viewer (`desktop_mitm::sessions::viewer_control`): saved sessions open,
    /// and the one saved at the last disconnect stays on screen.
    pub fn control(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        let Some(proxy) = self.embedded.as_ref().filter(|proxy| proxy.is_running()) else {
            return desktop_mitm::sessions::viewer_control(request).unwrap_or_else(|| {
                Err("the embedded engine is not running in this window".to_string())
            });
        };
        proxy
            .control_json(request)
            .map_err(|error| error.message.to_string())
    }

    /// Makes `member` the choice of policy group `group` in the running engine, in TUN mode in both of its
    /// engines: new connections take it, open ones and the system proxy stay as they are. An error (a profile
    /// without policy groups, a group the running envelope lacks) leaves the caller to restart the engine, which
    /// also brings both engines back to one choice.
    pub fn select(&self, group: &str, member: &str) -> Result<(), String> {
        let request =
            serde_json::json!({ "cmd": "select", "group": group, "member": member }).to_string();
        let chose = |answer: Vec<u8>| -> Result<(), String> {
            let answer: serde_json::Value =
                serde_json::from_slice(&answer).map_err(|error| error.to_string())?;
            if answer.get("now").and_then(serde_json::Value::as_str) == Some(member) {
                Ok(())
            } else {
                Err(format!("the engine did not choose {member} in {group}"))
            }
        };
        if let Some(tun) = self.tun.as_ref().filter(|tun| tun.is_running()) {
            chose(tun.control(request.as_bytes())?)?;
        }
        chose(self.control(request.as_bytes())?)
    }

    fn stop_embedded(&mut self) {
        if let Some(mut proxy) = self.embedded.take() {
            proxy.stop();
        }
    }

    fn stop(&mut self) {
        self.stop_tun();
        self.stop_embedded();
    }
}

/// The request bytes for an activity page command, or why it is refused: the
/// same window command list as the client-core backends, so the two cannot
/// drift apart again.
pub use client_core::backend::engine::activity_request;

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

pub fn connect_direct(
    config_path: PathBuf,
    desktop: DesktopState,
    core: &mut DesktopCore,
) -> Result<ConnectedProxy, String> {
    if desktop.direct_egress {
        return connect_direct_egress(&config_path, desktop, core);
    }
    let plan = direct_connection_plan(load_validated_daily_managed_configuration(&config_path)?)?;
    let embedded = plan.embedded.as_ref().ok_or_else(|| {
        "Desktop mode requires an imported profile. Import a subscription first.".to_string()
    })?;
    let mitm = plan.native_mitm.as_ref();
    let envelope = desktop_envelope(embedded, mitm)?;
    let listen = desktop_listener(embedded, mitm)?;
    // The decryption root is never trusted here: the user trusts it by hand before turning
    // decryption on (TERMS.md section 4).
    if desktop.tun_mode_enabled {
        core.start_tun(&envelope)?;
    }
    if let Err(error) = core.start_embedded(&envelope, listen) {
        core.stop();
        return Err(error);
    }
    apply_proxy_after_direct_readiness(&plan.proxy, desktop, core)
}

/// Where the engine listens and the system proxy points when nothing was
/// imported: the ports an imported profile would get.
pub const DIRECT_EGRESS_LISTEN_PORT: u16 = 7890;

/// Debug mode's direct egress: the engine runs a profile with no node (the
/// engine's `direct` outbound), so capture, HTTPS decryption and rewrites work
/// before anything is imported. HTTPS decryption and the system proxy come
/// from the managed configuration when there is one.
fn connect_direct_egress(
    config_path: &Path,
    desktop: DesktopState,
    core: &mut DesktopCore,
) -> Result<ConnectedProxy, String> {
    let managed = if config_path.exists() {
        Some(load_validated_daily_managed_configuration(config_path)?)
    } else {
        None
    };
    let mitm = managed
        .as_ref()
        .and_then(|managed| managed.native_mitm.clone())
        .filter(|mitm| mitm.enabled);
    if mitm
        .as_ref()
        .is_some_and(|mitm| mitm.script_runtime.is_some())
    {
        return Err(
            "Clear the legacy managed script runtime before enabling HTTPS MITM; Windows script execution requires a no-network sandbox."
                .to_string(),
        );
    }
    let proxy = managed
        .and_then(|managed| managed.system_proxy)
        .filter(|proxy| proxy.enabled)
        .unwrap_or_else(|| WindowsProxySettings {
            enabled: true,
            server: format!("127.0.0.1:{DIRECT_EGRESS_LISTEN_PORT}"),
            bypass: "<local>".to_string(),
        });
    let envelope = direct_egress_envelope(desktop.dns_preset, desktop.block_quic)?;
    let envelope = match mitm.as_ref() {
        Some(mitm) => with_native_mitm(envelope.into_bytes(), mitm)?,
        None => envelope.into_bytes(),
    };
    let listen = match mitm.as_ref() {
        Some(mitm) => loopback_listener(&format!("{}:{}", mitm.listen_host, mitm.listen_port))?,
        None => SocketAddr::from(([127, 0, 0, 1], DIRECT_EGRESS_LISTEN_PORT)),
    };
    // The decryption root is trusted by the user by hand, never here (TERMS.md section 4).
    if desktop.tun_mode_enabled {
        core.start_tun(&envelope)?;
    }
    if let Err(error) = core.start_embedded(&envelope, listen) {
        core.stop();
        return Err(error);
    }
    apply_proxy_after_direct_readiness(&proxy, desktop, core)
}

/// The node-less profile of debug mode's direct egress, with the DNS choice
/// and QUIC blocking in it.
pub fn direct_egress_envelope(
    dns: desktop_dns::DnsPreset,
    block_quic: bool,
) -> Result<String, String> {
    let profile = serde_json::json!({
        "schema_version": 1,
        "profile_id": "desktop",
        "outbound": { "type": "direct" },
    })
    .to_string();
    // The user's own rules still apply: REJECT and DIRECT as written, PROXY direct since there is no node.
    let profile =
        desktop_dns::rules::apply(&profile, &desktop_dns::rules::load_lines(&my_rules_path()))?;
    let envelope = desktop_dns::apply(&profile, dns)?;
    let envelope = desktop_dns::apply_block_quic(&envelope, block_quic)?;
    let envelope = desktop_dns::redirects::apply(&envelope)?;
    desktop_dns::rules::with_rule_set_cache(&envelope, &rule_set_cache_dir())
}

/// Stops the engine and puts back the proxy this window set. It reads no configuration: a managed
/// configuration that went missing or invalid must never keep the window from disconnecting, nor leave the proxy
/// pointing at the stopped engine (connecting and restarting still validate it).
pub fn disconnect_direct(desktop: DesktopState, core: &mut DesktopCore) -> Result<String, String> {
    disconnect_steps(&desktop, &mut owned_proxy::NativeProxySystem, || {
        core.stop()
    })
}

/// The system proxy as disconnecting sees it (both halves, see `owned_proxy::ProxyHalves`), so that the steps
/// can be tested without the real settings.
pub trait ProxyAccess: owned_proxy::ProxyHalves {
    /// Drops the owner record (`owned_proxy`) once the proxy is no longer the window's.
    fn forget_owner_record(&mut self) -> Result<(), String>;
}

impl ProxyAccess for owned_proxy::NativeProxySystem {
    fn forget_owner_record(&mut self) -> Result<(), String> {
        owned_proxy::forget_current_user()
    }
}

/// Puts back each half of the window's proxy that is still the window's (`owned_proxy::put_back`): the user's
/// WinINet settings, and the machine's WinHTTP proxy even when the user changed their own settings since. Then
/// stops the engine whatever happened to the proxy. A failed restore is reported after the engine stopped and
/// keeps the owner record, so that the next start of the window tries again; the caller must treat the
/// connection as stopped either way.
pub fn disconnect_steps(
    desktop: &DesktopState,
    proxy: &mut impl ProxyAccess,
    stop: impl FnOnce(),
) -> Result<String, String> {
    let restored = match (
        desktop.proxy_snapshot.as_ref(),
        desktop.applied_proxy.as_ref(),
    ) {
        (Some(snapshot), Some(applied)) => owned_proxy::put_back(&mut *proxy, applied, snapshot),
        _ => Ok(owned_proxy::PutBack::default()),
    };
    stop();
    let restored = restored?;
    if let Err(error) = proxy.forget_owner_record() {
        let _ = append_managed_log(
            "tauri-gui",
            &format!("proxy owner record could not be removed: {error}"),
        );
    }
    Ok(if restored.user {
        STOPPED_PROXY_RESTORED.to_string()
    } else if restored.winhttp {
        STOPPED_WINHTTP_RESTORED.to_string()
    } else {
        STOPPED_PROXY_LEFT.to_string()
    })
}

/// `disconnect_direct`'s answer when the proxy this window set was put back.
pub const STOPPED_PROXY_RESTORED: &str =
    "Desktop core stopped and the GUI-owned proxy snapshot was restored.";
/// `disconnect_direct`'s answer when the user changed their own proxy since (left alone), and only the
/// machine-wide WinHTTP proxy this window set was put back.
pub const STOPPED_WINHTTP_RESTORED: &str = "Desktop core stopped. The current-user proxy was changed since and left as it is; the machine-wide WinHTTP proxy was restored.";
/// `disconnect_direct`'s answer when the proxy was no longer the one this window set, and was left alone.
pub const STOPPED_PROXY_LEFT: &str = "Desktop core stopped. The current-user proxy was left unchanged because it was not owned by this GUI session.";

fn load_validated_daily_managed_configuration(
    config_path: &Path,
) -> Result<WindowsManagedConfig, String> {
    let managed_config_path = windows_managed_config_path();
    if config_path != managed_config_path.as_path() {
        return Err(
            "Apply this configuration before connecting or restarting so the engine uses the validated file."
                .to_string(),
        );
    }
    load_validated_managed_configuration(config_path)
}

/// The connection plan for a profile without HTTPS decryption. The profile's
/// `system_proxy_owner` dates from the Windows service and is ignored: the
/// window always runs the engine and owns the proxy.
fn desktop_connection_plan(managed: WindowsManagedConfig) -> Result<DesktopConnectionPlan, String> {
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

/// The envelope the desktop engines run on: the profile's, with the rule
/// set cache folder and the joined 组网 networks (`mesh::with_joined`), plus a
/// `mitm` section when HTTPS decryption is on. The root key and the networks'
/// secrets only join it in memory; the saved envelope never holds them.
fn desktop_envelope(
    embedded: &WindowsManagedEmbeddedConfig,
    mitm: Option<&WindowsManagedNativeMitmConfig>,
) -> Result<Vec<u8>, String> {
    let envelope = fs::read_to_string(&embedded.envelope_path)
        .map_err(|error| format!("embedded engine configuration could not be read: {error}"))?;
    // Added here rather than in the saved file, so an envelope saved by an earlier version gets it too.
    let envelope = desktop_dns::rules::with_rule_set_cache(&envelope, &rule_set_cache_dir())?;
    let envelope = mesh::with_joined(envelope).into_bytes();
    match mitm {
        Some(mitm) => with_native_mitm(envelope, mitm),
        None => Ok(envelope),
    }
}

/// `envelope` with the `mitm` section of HTTPS decryption.
fn with_native_mitm(
    envelope: Vec<u8>,
    mitm: &WindowsManagedNativeMitmConfig,
) -> Result<Vec<u8>, String> {
    let certificate = fs::read_to_string(&mitm.ca_certificate_path)
        .map_err(|error| format!("HTTPS decryption root certificate could not be read: {error}"))?;
    let private_key = fs::read_to_string(&mitm.ca_private_key_path)
        .map_err(|error| format!("HTTPS decryption root key could not be read: {error}"))?;
    envelope_with_mitm(
        &envelope,
        &certificate,
        &private_key,
        NATIVE_MITM_POLICY,
        Some(&mitm_plugins::directory()),
    )
}

/// Where the engine listens in desktop mode. With HTTPS decryption the
/// engine takes the system proxy port the MITM listener used to hold; the
/// profile's own port stays reserved for the Windows service layout.
fn desktop_listener(
    embedded: &WindowsManagedEmbeddedConfig,
    mitm: Option<&WindowsManagedNativeMitmConfig>,
) -> Result<SocketAddr, String> {
    let listener = match mitm {
        Some(mitm) => format!("{}:{}", mitm.listen_host, mitm.listen_port),
        None => embedded.listener(),
    };
    loopback_listener(&listener)
}

fn loopback_listener(listener: &str) -> Result<SocketAddr, String> {
    listener
        .parse::<SocketAddr>()
        .ok()
        .filter(|address| address.ip().is_loopback())
        .ok_or_else(|| "embedded engine listener must be a loopback socket address".to_string())
}

/// Desktop mode: the GUI runs the embedded engine, which also decrypts HTTPS
/// when the profile enables it, so a MITM-enabled profile no longer needs
/// the Windows service.
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
    let current = match read_current_user_system_proxy() {
        Ok(current) => current,
        Err(error) => {
            core.stop();
            return Err(error.to_string());
        }
    };
    let existing_snapshot = owns_current_proxy(&desktop, &current)
        .then(|| desktop.proxy_snapshot.clone())
        .flatten();
    let reused = existing_snapshot.is_some();
    // What to put back, recorded before the proxy changes so that a crash right after still finds it.
    let planned = match existing_snapshot {
        Some(snapshot) => snapshot,
        None => match owned_proxy::snapshot_now(&current, proxy) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                core.stop();
                return Err(error);
            }
        },
    };
    let planned = owned_proxy::sanitized(planned, proxy);
    if let Err(error) = owned_proxy::record_before_apply(proxy, &planned) {
        core.stop();
        return Err(format!(
            "the system proxy was not changed because its owner record could not be saved: {error}"
        ));
    }
    let applied_snapshot = match integration.apply_system_proxy(proxy) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = owned_proxy::forget_current_user();
            core.stop();
            return Err(error.to_string());
        }
    };
    let snapshot = if reused {
        planned.clone()
    } else {
        owned_proxy::sanitized(applied_snapshot, proxy)
    };
    if snapshot != planned {
        // The settings changed between the two reads: the record follows what applying found.
        let _ = owned_proxy::record_before_apply(proxy, &snapshot);
    }
    desktop.proxy_snapshot = Some(snapshot.clone());
    desktop.applied_proxy = Some(proxy.clone());
    if let Err(error) = save_desktop_state(&desktop) {
        if integration.restore_system_proxy(&snapshot).is_ok() {
            let _ = owned_proxy::forget_current_user();
        }
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

/// The HTTPS decryption policy besides the user's imported plugins: none. Public builds ship no built-in
/// plugin, rewrite or script (docs/no-presets.md); only plugins the user imports decrypt or rewrite anything.
pub const NATIVE_MITM_POLICY: &str = "";

/// What [`remove_legacy_service`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LegacyServiceRemoval {
    pub removed: bool,
    /// The service was running with the proxy this window had set, so the
    /// proxy was put back as it was before connecting.
    pub proxy_restored: bool,
}

/// Earlier versions could run the engine in the "AnixOps NetworkCore"
/// Windows service. That mode is gone: an MSI upgrade removes the service,
/// and this removes one a portable copy registered. When it was connected,
/// the proxy it served is put back, so the computer is not left pointing at
/// a listener that no longer exists; automatic connection then connects in
/// the window.
pub fn remove_legacy_service(desktop: &DesktopState) -> Result<LegacyServiceRemoval, String> {
    let integration = NativeWindowsSystemIntegration::new();
    let status = integration
        .service_status()
        .map_err(|error| error.to_string())?;
    if status.state == WindowsServiceState::NotInstalled {
        return Ok(LegacyServiceRemoval::default());
    }
    let was_running = status.state != WindowsServiceState::Stopped;
    integration
        .uninstall_service()
        .map_err(|error| error.to_string())?;
    let mut proxy_restored = false;
    if was_running {
        if let Some(snapshot) = desktop.proxy_snapshot.as_ref() {
            let current = read_current_user_system_proxy().map_err(|error| error.to_string())?;
            if owns_current_proxy(desktop, &current) {
                integration
                    .restore_system_proxy(snapshot)
                    .map_err(|error| error.to_string())?;
                proxy_restored = true;
            }
        }
    }
    Ok(LegacyServiceRemoval {
        removed: true,
        proxy_restored,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_windows::managed::{
        WindowsSystemProxyOwner, WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION,
    };
    use platform_windows::system_integration::WindowsWinHttpProxy;

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
    fn the_direct_egress_envelope_runs_without_a_node() {
        let envelope = direct_egress_envelope(desktop_dns::DnsPreset::default(), true)
            .expect("direct envelope");
        assert!(envelope.contains("\"direct\""), "{envelope}");
        assert!(
            engine_embedded::config::RuntimeConfig::from_json(envelope.as_bytes()).is_ok(),
            "{envelope}"
        );
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
        assert!(activity_request(&serde_json::json!({"cmd": "mitm_hosts"})).is_ok());
        assert!(
            activity_request(&serde_json::json!({"cmd": "set_capture", "enabled": true})).is_ok()
        );
        assert!(
            activity_request(&serde_json::json!({"cmd": "capture_connection", "id": 7})).is_ok()
        );
        assert!(
            activity_request(&serde_json::json!({"cmd": "set_mode", "mode": "global"})).is_err()
        );
        assert!(activity_request(&serde_json::json!({"id": 7})).is_err());
        assert!(
            activity_request(&serde_json::json!({"cmd": "start_lan_share", "port": 8899})).is_ok()
        );
        assert!(activity_request(&serde_json::json!({"cmd": "stop_lan_share"})).is_ok());
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
        // A profile written for the removed Windows service runs in the window too.
        assert!(desktop_connection_plan(managed_config(
            WindowsSystemProxyOwner::Service,
            Some(proxy),
        ))
        .is_ok());
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
            script_runtime: None,
        });
        let plan = direct_connection_plan(managed.clone())
            .expect("desktop mode should accept a MITM-enabled profile");
        assert_eq!(plan.proxy, proxy);
        assert_eq!(
            plan.native_mitm.map(|mitm| mitm.upstream_socks_port),
            Some(7891)
        );

        managed.native_mitm = None;
        assert!(direct_connection_plan(managed.clone()).is_ok());
        managed.system_proxy = None;
        assert!(direct_connection_plan(managed).is_err());
    }

    fn mitm_config(listen_port: u16) -> WindowsManagedNativeMitmConfig {
        WindowsManagedNativeMitmConfig {
            enabled: true,
            listen_host: "127.0.0.1".to_string(),
            listen_port,
            upstream_socks_host: "127.0.0.1".to_string(),
            upstream_socks_port: 7891,
            ca_certificate_path: PathBuf::from("root-ca.pem"),
            ca_private_key_path: PathBuf::from("root-ca-key.pem"),
            log_path: PathBuf::from("native-mitm.log"),
            script_runtime: None,
        }
    }

    #[test]
    fn desktop_engine_takes_the_system_proxy_port_when_it_decrypts() {
        let embedded = WindowsManagedEmbeddedConfig {
            enabled: true,
            envelope_path: PathBuf::from("embedded.json"),
            listen_host: "127.0.0.1".to_string(),
            listen_port: 7891,
        };
        assert_eq!(
            desktop_listener(&embedded, None).unwrap().to_string(),
            "127.0.0.1:7891"
        );
        assert_eq!(
            desktop_listener(&embedded, Some(&mitm_config(7890)))
                .unwrap()
                .to_string(),
            "127.0.0.1:7890"
        );
        let mut exposed = mitm_config(7890);
        exposed.listen_host = "0.0.0.0".to_string();
        assert!(desktop_listener(&embedded, Some(&exposed)).is_err());
    }

    /// The managed root, as `ensure_mitm_ca_material` writes it, is accepted
    /// by the engine: the local proxy starts with HTTPS decryption on.
    #[test]
    fn the_engine_decrypts_with_the_managed_root() {
        use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair, KeyUsagePurpose};

        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let key = KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key).unwrap();
        const ENVELOPE: &[u8] =
            br#"{"schema_version":1,"profile_id":"desktop","outbound":{"type":"direct"}}"#;

        let envelope = envelope_with_mitm(
            ENVELOPE,
            &certificate.pem(),
            &key.serialize_pem(),
            NATIVE_MITM_POLICY,
            None,
        )
        .unwrap();
        let listen = "127.0.0.1:0".parse().unwrap();
        let proxy = LocalProxy::start(&envelope, listen, MixedSettings::default())
            .unwrap_or_else(|error| panic!("engine refused the envelope: {}", error.message));
        assert!(proxy.decrypts_https());

        assert!(envelope_with_mitm(ENVELOPE, "no pem", &key.serialize_pem(), "", None).is_err());
        assert!(
            envelope_with_mitm(b"[]", &certificate.pem(), &key.serialize_pem(), "", None).is_err()
        );
        let plain = LocalProxy::start(ENVELOPE, listen, MixedSettings::default()).unwrap();
        assert!(!plain.decrypts_https());

        // An imported plugin with a script: the engine pins the script file
        // and starts its QuickJS runtime.
        let plugins =
            std::env::temp_dir().join(format!("anixops-desktop-plugins-{}", std::process::id()));
        let _ = fs::remove_dir_all(&plugins);
        let fetch = |url: &str, _limit: usize| -> Result<Vec<u8>, String> {
            Ok(if url.ends_with(".js") {
                b"$done({});".to_vec()
            } else {
                concat!(
                    "#!name = scripted\n",
                    "[Script]\n",
                    "http-response ^https?:\\/\\/ipleak\\.net\\/ script-path=https://plugins.example/a.js, requires-body=true, tag=a\n",
                    "[MITM]\n",
                    "hostname = ipleak.net\n",
                )
                .as_bytes()
                .to_vec()
            })
        };
        let imported =
            mitm_plugins::import(&plugins, "https://plugins.example/a.plugin", fetch).unwrap();
        // Imported off; the user turns it on after looking at it.
        mitm_plugins::set_enabled(&plugins, &imported.id, true).unwrap();
        let envelope = envelope_with_mitm(
            ENVELOPE,
            &certificate.pem(),
            &key.serialize_pem(),
            NATIVE_MITM_POLICY,
            Some(&plugins),
        )
        .unwrap();
        let section: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
        assert_eq!(
            section["mitm"]["script_assets"].as_object().unwrap().len(),
            1
        );
        let scripted = LocalProxy::start(&envelope, listen, MixedSettings::default())
            .unwrap_or_else(|error| panic!("engine refused the plugin: {}", error.message));
        assert!(scripted.decrypts_https());
        drop(scripted);
        let _ = fs::remove_dir_all(&plugins);
    }

    struct FakeProxy {
        current: Result<WindowsProxySettings, String>,
        winhttp: WindowsWinHttpProxy,
        restore_error: Option<String>,
        restored_user: Vec<WindowsProxySettings>,
        restored_winhttp: Vec<WindowsWinHttpProxy>,
        forgotten: bool,
    }

    impl FakeProxy {
        fn new(
            current: Result<WindowsProxySettings, String>,
            winhttp: WindowsWinHttpProxy,
        ) -> Self {
            Self {
                current,
                winhttp,
                restore_error: None,
                restored_user: Vec::new(),
                restored_winhttp: Vec::new(),
                forgotten: false,
            }
        }
    }

    impl owned_proxy::ProxyHalves for FakeProxy {
        fn current_proxy(&self) -> Result<WindowsProxySettings, String> {
            self.current.clone()
        }

        fn winhttp(&self) -> Result<WindowsWinHttpProxy, String> {
            Ok(self.winhttp.clone())
        }

        fn restore_user(&mut self, previous: &WindowsProxySnapshot) -> Result<(), String> {
            if let Some(error) = &self.restore_error {
                return Err(error.clone());
            }
            self.restored_user
                .push(owned_proxy::user_settings(previous));
            Ok(())
        }

        fn restore_winhttp(&mut self, previous: &WindowsWinHttpProxy) -> Result<(), String> {
            self.restored_winhttp.push(previous.clone());
            self.winhttp = previous.clone();
            Ok(())
        }
    }

    impl ProxyAccess for FakeProxy {
        fn forget_owner_record(&mut self) -> Result<(), String> {
            self.forgotten = true;
            Ok(())
        }
    }

    fn applied_winhttp() -> WindowsWinHttpProxy {
        WindowsWinHttpProxy {
            access_type: owned_proxy::WINHTTP_NAMED_PROXY,
            server: "127.0.0.1:7890".to_string(),
            bypass: "<local>".to_string(),
        }
    }

    fn no_winhttp() -> WindowsWinHttpProxy {
        WindowsWinHttpProxy {
            access_type: owned_proxy::WINHTTP_NO_PROXY,
            server: String::new(),
            bypass: String::new(),
        }
    }

    fn applied_proxy() -> WindowsProxySettings {
        WindowsProxySettings {
            enabled: true,
            server: "127.0.0.1:7890".to_string(),
            bypass: "<local>".to_string(),
        }
    }

    fn original_proxy() -> WindowsProxySnapshot {
        WindowsProxySnapshot {
            enabled: false,
            server: String::new(),
            bypass: String::new(),
            winhttp_access_type: 1,
            winhttp_server: String::new(),
            winhttp_bypass: String::new(),
            winhttp_untouched: false,
            applied: None,
            connections: Vec::new(),
        }
    }

    fn connected_desktop() -> DesktopState {
        DesktopState {
            proxy_snapshot: Some(original_proxy()),
            applied_proxy: Some(applied_proxy()),
            ..DesktopState::default()
        }
    }

    #[test]
    fn disconnecting_reads_no_configuration_and_puts_the_proxy_back() {
        // No managed configuration is read: disconnecting works with it missing or invalid.
        let mut proxy = FakeProxy::new(Ok(applied_proxy()), applied_winhttp());
        let mut stopped = false;
        let message = disconnect_steps(&connected_desktop(), &mut proxy, || stopped = true)
            .expect("disconnect");
        assert!(message.contains("restored"), "{message}");
        assert_eq!(
            proxy.restored_user,
            [owned_proxy::user_settings(&original_proxy())]
        );
        assert_eq!(
            proxy.restored_winhttp,
            [owned_proxy::winhttp_settings(&original_proxy())]
        );
        assert!(stopped);
        assert!(proxy.forgotten);
    }

    #[test]
    fn a_failed_restore_still_stops_the_engine_and_keeps_the_owner_record() {
        // The engine is stopped even when the proxy could not be put back: the caller marks the connection as
        // stopped either way (`disconnect_blocking`), or the recovery monitor would take the stop for a lost
        // connection and reconnect.
        let mut proxy = FakeProxy::new(Ok(applied_proxy()), no_winhttp());
        proxy.restore_error = Some("registry unavailable".to_string());
        let mut stopped = false;
        let error = disconnect_steps(&connected_desktop(), &mut proxy, || stopped = true)
            .expect_err("the failed restore is reported");
        assert_eq!(error, "registry unavailable");
        assert!(stopped);
        assert!(!proxy.forgotten);

        // Reading the current settings failing stops the engine too, and WinHTTP still goes back.
        let mut proxy = FakeProxy::new(Err("read failed".to_string()), applied_winhttp());
        let mut stopped = false;
        assert!(disconnect_steps(&connected_desktop(), &mut proxy, || stopped = true).is_err());
        assert!(stopped);
        assert!(!proxy.forgotten);
        assert_eq!(proxy.winhttp, no_winhttp());
    }

    #[test]
    fn a_proxy_changed_by_the_user_is_left_alone_on_disconnect() {
        let changed = WindowsProxySettings {
            enabled: true,
            server: "corp.example:8080".to_string(),
            bypass: String::new(),
        };
        // Both halves changed since: nothing is touched.
        let mut proxy = FakeProxy::new(Ok(changed.clone()), no_winhttp());
        let mut stopped = false;
        disconnect_steps(&connected_desktop(), &mut proxy, || stopped = true).unwrap();
        assert!(proxy.restored_user.is_empty());
        assert!(proxy.restored_winhttp.is_empty());
        assert!(stopped);
        assert!(proxy.forgotten);

        // Only the user's own settings changed: WinHTTP still names the stopped engine and goes back.
        let mut proxy = FakeProxy::new(Ok(changed), applied_winhttp());
        let message =
            disconnect_steps(&connected_desktop(), &mut proxy, || {}).expect("disconnect");
        assert!(message.contains("WinHTTP"), "{message}");
        assert!(proxy.restored_user.is_empty());
        assert_eq!(
            proxy.restored_winhttp,
            [owned_proxy::winhttp_settings(&original_proxy())]
        );
        assert!(proxy.forgotten);
    }
}
