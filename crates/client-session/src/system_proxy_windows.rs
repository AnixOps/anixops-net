//! Points the signed-in user's Windows proxy settings (WinINet, what browsers and most applications read) at
//! the local proxy and puts them back afterwards.
//!
//! Only the user's own settings are written, which needs no administrator rights. The machine-wide WinHTTP
//! proxy that services read is left alone here; where the engine runs in a system service, that service sets
//! it (`docs/architecture/desktop-native.md`).
//!
//! Before putting the old settings back the current ones are read: when they are no longer what the client
//! wrote, the user or another program changed them since, and they are left as they are.

use platform_windows::managed::WindowsProxySettings;
use platform_windows::system_integration::{
    apply_current_user_system_proxy, read_current_user_system_proxy,
    restore_current_user_system_proxy,
};
use serde::{Deserialize, Serialize};

/// Hosts that never go through the proxy: names without a dot, which are on the local network.
const BYPASS: &str = "<local>";

/// What keeps the proxy settings. Windows has one place for them, whatever the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Desktop {
    Windows,
}

impl Desktop {
    pub fn from_env() -> Option<Self> {
        Some(Self::Windows)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
        }
    }
}

/// The user's proxy settings before the client changed them, and what the client wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyBackup {
    pub previous: WindowsProxySettings,
    pub applied: WindowsProxySettings,
}

/// The settings that send the user's applications to the local proxy on `port`.
pub fn local_proxy(port: u16) -> WindowsProxySettings {
    WindowsProxySettings {
        enabled: true,
        server: format!("127.0.0.1:{port}"),
        bypass: BYPASS.to_string(),
    }
}

/// Whether the settings are still the ones the client wrote, so putting the old ones back overwrites nothing
/// the user or another program set since.
pub fn still_ours(backup: &ProxyBackup, current: &WindowsProxySettings) -> bool {
    *current == backup.applied
}

pub fn apply(_desktop: Desktop, port: u16) -> Result<ProxyBackup, String> {
    let applied = local_proxy(port);
    let previous = apply_current_user_system_proxy(&applied).map_err(|error| error.message)?;
    Ok(ProxyBackup { previous, applied })
}

pub fn restore(backup: &ProxyBackup) -> Result<(), String> {
    let current = read_current_user_system_proxy().map_err(|error| error.message)?;
    if !still_ours(backup, &current) {
        return Ok(());
    }
    restore_current_user_system_proxy(&backup.previous).map_err(|error| error.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backup() -> ProxyBackup {
        ProxyBackup {
            previous: WindowsProxySettings {
                enabled: false,
                server: String::new(),
                bypass: String::new(),
            },
            applied: local_proxy(7890),
        }
    }

    #[test]
    fn the_local_proxy_keeps_local_names_direct() {
        let settings = local_proxy(7890);
        assert!(settings.enabled);
        assert_eq!(settings.server, "127.0.0.1:7890");
        assert_eq!(settings.bypass, "<local>");
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn settings_changed_by_someone_else_are_not_ours() {
        let backup = backup();
        assert!(still_ours(&backup, &local_proxy(7890)));
        // Another program pointed the proxy elsewhere, or the user turned it off.
        assert!(!still_ours(&backup, &local_proxy(8080)));
        let mut off = local_proxy(7890);
        off.enabled = false;
        assert!(!still_ours(&backup, &off));
    }

    #[test]
    fn a_backup_survives_being_saved() {
        let backup = backup();
        let saved = serde_json::to_string(&backup).unwrap();
        assert_eq!(serde_json::from_str::<ProxyBackup>(&saved).unwrap(), backup);
    }

    /// Writes the settings that are already there and puts them back, so the machine is left as found.
    #[test]
    fn settings_nobody_changed_are_put_back() {
        let current = read_current_user_system_proxy().unwrap();
        let previous = apply_current_user_system_proxy(&current).unwrap();
        let backup = ProxyBackup {
            previous,
            applied: current.clone(),
        };
        restore(&backup).unwrap();
        assert_eq!(read_current_user_system_proxy().unwrap(), current);
    }
}
