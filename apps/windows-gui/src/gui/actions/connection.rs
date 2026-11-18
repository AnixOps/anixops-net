use crate::gui::{
    load_validated_managed_configuration, mitm_plugins,
    startup::{my_rules_path, owns_current_proxy, save_desktop_state, DesktopState},
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
        Ok(())
    }

    /// The local proxy of the engine this window runs.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.embedded
            .as_ref()
            .filter(|proxy| proxy.is_running())
            .map(LocalProxy::local_addr)
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
        self.stop_tun();
        self.stop_embedded();
    }
}

/// Kernel control commands the client may send: reading traffic, connections
/// and rule sets, closing connections, refreshing remote rule sets and packet
/// capture. Mode and group changes go through the saved profile instead.
const ACTIVITY_COMMANDS: [&str; 16] = [
    "traffic",
    "connections",
    "close_connection",
    "close_all",
    "rule_sets",
    "update_rule_sets",
    "capture",
    "set_capture",
    "clear_capture",
    "capture_connection",
    "export_capture",
    "capture_requests",
    "capture_exchange",
    "capture_h2_frames",
    // Per-domain decryption results for the decryption list.
    "mitm_hosts",
    "clear_mitm_hosts",
];

/// The request bytes for an activity page command, or why it is refused.
pub fn activity_request(request: &serde_json::Value) -> Result<Vec<u8>, String> {
    let command = request
        .get("cmd")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !ACTIVITY_COMMANDS.contains(&command)
        && !desktop_mitm::phone::CONTROL_COMMANDS.contains(&command)
    {
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
    desktop_dns::redirects::apply(&envelope)
}

pub fn disconnect_direct(
    config_path: PathBuf,
    desktop: DesktopState,
    core: &mut DesktopCore,
) -> Result<String, String> {
    // Debug mode's direct egress runs without an imported profile.
    if !desktop.direct_egress || config_path.exists() {
        load_validated_daily_managed_configuration(&config_path)?;
    }
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

/// The envelope the desktop engines run on: the profile's, plus a `mitm`
/// section when HTTPS decryption is on. The root key only joins it in
/// memory; the saved envelope never holds it.
fn desktop_envelope(
    embedded: &WindowsManagedEmbeddedConfig,
    mitm: Option<&WindowsManagedNativeMitmConfig>,
) -> Result<Vec<u8>, String> {
    let envelope = fs::read(&embedded.envelope_path)
        .map_err(|error| format!("embedded engine configuration could not be read: {error}"))?;
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
        mitm_plugins::import(&plugins, "https://plugins.example/a.plugin", fetch).unwrap();
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
}
