//! What the client changes in the signed-in user's own session: the system proxy settings and the entry that
//! starts the client after login.
//!
//! These belong to the user, not to the machine. A client whose engine runs in the window's own process (the
//! Tauri desktop app, the portable build) does them itself, through [`LocalSession`]. Where the engine runs
//! in a background service, that service cannot reach one user's settings, so the window process answers
//! this trait for it (`docs/architecture/desktop-native.md`).

use std::path::PathBuf;

use super::autostart;
use super::system_proxy::{self, Desktop, ProxyBackup};

pub trait UserSession: Send + Sync {
    /// The name of what keeps the system proxy settings here, such as `GNOME`; `None` where the client cannot
    /// set them and the user points applications at the local proxy by hand.
    fn proxy_settings(&self) -> Option<&'static str>;

    /// Points the system proxy at the local proxy on `port` and returns what to put back afterwards.
    fn apply_proxy(&self, port: u16) -> Result<ProxyBackup, String>;

    /// Puts the system proxy back to what `backup` recorded.
    fn restore_proxy(&self, backup: &ProxyBackup) -> Result<(), String>;

    /// Whether the client starts after login.
    fn starts_after_login(&self) -> bool;

    /// Adds or removes the entry that starts the client after login. Where the session has no such entry
    /// nothing is changed.
    fn set_start_after_login(&self, enabled: bool) -> Result<(), String>;
}

/// The session of the process the backend runs in.
pub struct LocalSession {
    desktop: Option<Desktop>,
    autostart_dir: Option<PathBuf>,
    executable: PathBuf,
}

impl LocalSession {
    /// `desktop` is what keeps the proxy settings, `autostart_dir` where the login entry goes (`None`: the
    /// session has none) and `executable` the program that entry starts.
    pub fn new(
        desktop: Option<Desktop>,
        autostart_dir: Option<PathBuf>,
        executable: PathBuf,
    ) -> Self {
        Self {
            desktop,
            autostart_dir,
            executable,
        }
    }
}

impl UserSession for LocalSession {
    fn proxy_settings(&self) -> Option<&'static str> {
        self.desktop.map(Desktop::name)
    }

    fn apply_proxy(&self, port: u16) -> Result<ProxyBackup, String> {
        let desktop = self
            .desktop
            .ok_or("this desktop's proxy settings are not supported")?;
        system_proxy::apply(desktop, port)
    }

    fn restore_proxy(&self, backup: &ProxyBackup) -> Result<(), String> {
        system_proxy::restore(backup)
    }

    fn starts_after_login(&self) -> bool {
        self.autostart_dir
            .as_deref()
            .is_some_and(autostart::is_enabled)
    }

    fn set_start_after_login(&self, enabled: bool) -> Result<(), String> {
        let Some(dir) = &self.autostart_dir else {
            return Ok(());
        };
        autostart::set_enabled(dir, enabled, &self.executable)
            .map_err(|error| format!("the autostart entry could not be changed: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(name: &str, autostart: bool) -> (LocalSession, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("anixops-net-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let session = LocalSession::new(
            None,
            autostart.then(|| dir.join("autostart")),
            PathBuf::from("/usr/bin/anixops-net"),
        );
        (session, dir)
    }

    #[test]
    fn login_entry_is_added_and_removed() {
        let (session, dir) = session("login", true);
        assert!(!session.starts_after_login());
        session.set_start_after_login(true).unwrap();
        assert!(session.starts_after_login());
        session.set_start_after_login(false).unwrap();
        assert!(!session.starts_after_login());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_session_without_login_entries_changes_nothing() {
        let (session, dir) = session("no-login", false);
        session.set_start_after_login(true).unwrap();
        assert!(!session.starts_after_login());
        assert!(!dir.exists());
    }

    #[test]
    fn an_unsupported_desktop_is_not_given_a_proxy() {
        let (session, _) = session("no-proxy", false);
        assert_eq!(session.proxy_settings(), None);
        assert!(session.apply_proxy(7890).is_err());
    }
}
