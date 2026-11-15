//! Saved capture sessions on Windows (`desktop_mitm::sessions`). Sessions and their key live in the user's local
//! application data folder; the key is sealed with Windows Data Protection (DPAPI) by the shared client core, so
//! only this Windows account on this computer can open it, and the window never sees it.

use std::path::PathBuf;

use desktop_mitm::sessions::{self, SessionKey};

/// `%LOCALAPPDATA%\AnixOps Net\capture`: the sealed key and `sessions\`.
pub fn capture_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(|dir| PathBuf::from(dir).join("AnixOps Net").join("capture"))
        .ok_or_else(|| "the local application data folder could not be found".to_string())
}

pub fn sessions_dir() -> Result<PathBuf, String> {
    capture_dir().map(|dir| dir.join("sessions"))
}

/// The session key, made and sealed on first use (`client_core::backend::session_key`, the same file as before).
pub fn load_key() -> Result<SessionKey, String> {
    client_core::backend::session_key::load(&capture_dir()?)
}

/// Deletes expired sessions; called at start, since the engine only does it while it runs.
pub fn purge_expired() {
    if let Ok(dir) = sessions_dir() {
        sessions::purge(&dir, sessions::now_ms());
    }
}
