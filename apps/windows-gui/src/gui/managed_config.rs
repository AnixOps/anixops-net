//! `managed-config.json` as the desktop client reads and writes it.
//!
//! The kernel's [`WindowsManagedConfig::validate`] still carries one rule from the removed Windows service:
//! enabled HTTPS decryption required the service to own the system proxy (`system_proxy_owner: service`),
//! because the service ran a separate decryption listener and put the proxy back itself. In desktop mode the
//! window runs the engine, which decrypts in process on the system proxy port, and the window keeps the proxy
//! snapshot ([`super::actions::connection`]); every profile the window imports is `desktop`-owned. With the
//! kernel's rule, turning HTTPS decryption on could never be saved, and a saved desktop profile with decryption
//! on could not be read back.
//!
//! So a `desktop`-owned configuration is checked here with every kernel rule except that one: the same
//! validation runs on a copy that names the service as owner, which changes nothing else. A `service`-owned
//! file (written by an earlier version) and a schema 1 file go through the kernel's own reader unchanged.
//! Once the pinned kernel accepts desktop-owned decryption itself this module can go back to
//! `read_managed_config` / `write_managed_config`.

use control_domain::{DomainError, DomainResult};
use platform_windows::managed::{
    read_managed_config, write_managed_text_atomic, WindowsManagedConfig, WindowsSystemProxyOwner,
    WINDOWS_MANAGED_CONFIG_INVALID_CODE, WINDOWS_MANAGED_CONFIG_IO_CODE,
    WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION,
};
use std::fs;
use std::path::Path;

/// The kernel's validation, without the service-era rule that decryption needs a service-owned proxy when the
/// window owns the proxy.
pub fn validate(config: &WindowsManagedConfig) -> DomainResult<()> {
    if config.system_proxy_owner != WindowsSystemProxyOwner::Desktop {
        return config.validate();
    }
    let mut checked = config.clone();
    checked.system_proxy_owner = WindowsSystemProxyOwner::Service;
    checked.validate()
}

/// Reads and validates the configuration at `path`. A current-schema, `desktop`-owned file is checked with
/// [`validate`]; anything else (a `service`-owned file, a schema 1 file the kernel migrates) is read by the
/// kernel as before.
pub fn read(path: &Path) -> DomainResult<WindowsManagedConfig> {
    let bytes = fs::read(path).map_err(|_| {
        DomainError::new(
            WINDOWS_MANAGED_CONFIG_IO_CODE,
            "managed configuration could not be read",
        )
    })?;
    match serde_json::from_slice::<WindowsManagedConfig>(&bytes) {
        Ok(config)
            if config.schema_version == WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION
                && config.system_proxy_owner == WindowsSystemProxyOwner::Desktop =>
        {
            validate(&config)?;
            Ok(config)
        }
        _ => read_managed_config(path),
    }
}

/// Validates `config` with [`validate`] and replaces the file at `path` in one step, as the kernel's
/// `write_managed_config` does (the same pretty JSON and the same temporary file).
pub fn write(path: &Path, config: &WindowsManagedConfig) -> DomainResult<()> {
    validate(config)?;
    let text = serde_json::to_string_pretty(config).map_err(|_| {
        DomainError::new(
            WINDOWS_MANAGED_CONFIG_INVALID_CODE,
            "managed JSON could not be serialized",
        )
    })?;
    write_managed_text_atomic(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_windows::managed::{
        WindowsManagedEmbeddedConfig, WindowsManagedNativeMitmConfig, WindowsProxySettings,
    };
    use std::path::PathBuf;

    fn decrypting(owner: WindowsSystemProxyOwner) -> WindowsManagedConfig {
        WindowsManagedConfig {
            schema_version: WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION,
            system_proxy: Some(WindowsProxySettings {
                enabled: true,
                server: "127.0.0.1:7890".to_string(),
                bypass: "<local>".to_string(),
            }),
            system_proxy_owner: owner,
            root_certificate_path: None,
            driver_package: None,
            native_mitm: Some(WindowsManagedNativeMitmConfig {
                enabled: true,
                listen_host: "127.0.0.1".to_string(),
                listen_port: 7890,
                upstream_socks_host: "127.0.0.1".to_string(),
                upstream_socks_port: 7891,
                ca_certificate_path: PathBuf::from(r"C:\data\mitm\root-ca.pem"),
                ca_private_key_path: PathBuf::from(r"C:\data\mitm\root-ca-key.pem"),
                log_path: PathBuf::from(r"C:\data\logs\native-mitm.log"),
                script_runtime: None,
            }),
            embedded: Some(WindowsManagedEmbeddedConfig {
                enabled: true,
                envelope_path: PathBuf::from(r"C:\data\embedded\envelope.json"),
                listen_host: "127.0.0.1".to_string(),
                listen_port: 7891,
            }),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anixops-managed-config-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("managed-config.json")
    }

    /// What turning HTTPS decryption on writes: the window owns the proxy and decryption is on. The kernel's
    /// own check refuses it (the bug: decryption could never be turned on); the desktop check accepts it.
    #[test]
    fn desktop_owned_decryption_is_accepted() {
        let config = decrypting(WindowsSystemProxyOwner::Desktop);
        assert!(
            config.validate().is_err(),
            "the pinned kernel still refuses desktop-owned decryption; drop this module once it does not"
        );
        validate(&config).expect("desktop-owned decryption is valid");
    }

    #[test]
    fn every_other_rule_still_applies() {
        let mut config = decrypting(WindowsSystemProxyOwner::Desktop);
        config.native_mitm.as_mut().unwrap().listen_port = 0;
        assert!(validate(&config).is_err());

        let mut config = decrypting(WindowsSystemProxyOwner::Desktop);
        config.native_mitm.as_mut().unwrap().ca_private_key_path = PathBuf::new();
        assert!(validate(&config).is_err());

        let mut config = decrypting(WindowsSystemProxyOwner::Desktop);
        config.schema_version = 99;
        assert!(validate(&config).is_err());

        let mut config = decrypting(WindowsSystemProxyOwner::Desktop);
        config.system_proxy.as_mut().unwrap().server = "127.0.0.1:7890\n".to_string();
        assert!(validate(&config).is_err());
    }

    #[test]
    fn a_service_owned_file_keeps_the_kernel_check() {
        let config = decrypting(WindowsSystemProxyOwner::Service);
        assert_eq!(validate(&config).is_ok(), config.validate().is_ok());
    }

    /// Enabling decryption writes the file and every later read (connect, snapshot, plugins) takes it back.
    #[test]
    fn desktop_owned_decryption_round_trips_through_the_file() {
        let path = scratch("round-trip");
        let config = decrypting(WindowsSystemProxyOwner::Desktop);
        write(&path, &config).expect("decryption can be turned on");
        assert_eq!(read(&path).expect("and read back"), config);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// A portable copy left a service-era file with decryption on; importing a profile again makes the window
    /// the owner and keeps decryption on, which must save.
    #[test]
    fn a_service_era_decrypting_file_saves_once_the_window_owns_it() {
        let path = scratch("service-era");
        let legacy = decrypting(WindowsSystemProxyOwner::Service);
        fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        let mut managed = read(&path).expect("the service-era file reads as before");
        assert_eq!(managed.system_proxy_owner, WindowsSystemProxyOwner::Service);
        managed.system_proxy_owner = WindowsSystemProxyOwner::Desktop;
        write(&path, &managed).expect("the desktop-owned copy saves");
        assert_eq!(read(&path).unwrap(), managed);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// Schema 1 files still migrate through the kernel's reader (owner `service`).
    #[test]
    fn a_schema_one_file_is_migrated_by_the_kernel() {
        let path = scratch("schema-one");
        fs::write(
            &path,
            br#"{"schema_version":1,"system_proxy":null,"root_certificate_path":null,"driver_package":null}"#,
        )
        .unwrap();
        let config = read(&path).expect("schema 1 still reads");
        assert_eq!(config.schema_version, WINDOWS_MANAGED_CONFIG_SCHEMA_VERSION);
        assert_eq!(config.system_proxy_owner, WindowsSystemProxyOwner::Service);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_broken_desktop_file_is_still_refused() {
        let path = scratch("broken");
        let mut config = decrypting(WindowsSystemProxyOwner::Desktop);
        config.native_mitm.as_mut().unwrap().log_path = PathBuf::new();
        fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        assert!(read(&path).is_err());
        assert!(write(&path, &config).is_err());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
