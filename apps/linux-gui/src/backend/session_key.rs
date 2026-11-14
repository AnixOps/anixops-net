//! The key that seals saved capture sessions on a Linux desktop (`desktop_mitm::sessions`). It goes to the
//! desktop's Secret Service (GNOME Keyring, KWallet) through `secret-tool`; where there is none, to a file only
//! this user can read, and the window says so.
//!
//! Once a key is stored one way it stays there: a key that cannot be read (a locked keyring) is an error, never
//! replaced, because a new key would leave every saved session unreadable.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use desktop_mitm::capture::{decode_base64, encode_base64};
use desktop_mitm::sessions::{self, Protection, SessionKey, KEY_LEN};

/// The Secret Service item's attributes.
const ATTRIBUTES: [&str; 4] = [
    "application",
    "anixops-net",
    "purpose",
    "capture-session-key",
];
const LABEL: &str = "AnixOps Net capture sessions";
/// The key itself, when there is no Secret Service.
const KEY_FILE: &str = "session-key";
/// Says the key is in the Secret Service.
const MARKER_FILE: &str = "session-key.secret-service";

/// The session key for the capture folder `dir` (`<data>/capture`), made on first use.
pub fn load(dir: &Path) -> Result<SessionKey, String> {
    let file = dir.join(KEY_FILE);
    if file.exists() {
        let key = std::fs::read(&file)
            .map_err(|error| format!("the capture session key could not be read: {error}"))?;
        return SessionKey::new(&key, Protection::File);
    }
    let marker = dir.join(MARKER_FILE);
    if marker.exists() {
        let key = lookup().ok_or(
            "the capture session key is in the system keyring, which is locked or not running; unlock it and try again",
        )?;
        return SessionKey::new(&key, Protection::SecretService);
    }
    let key = sessions::new_key()?;
    if store(&key) && lookup().as_deref() == Some(&key[..]) {
        sessions::write_private(&marker, b"")?;
        return SessionKey::new(&key, Protection::SecretService);
    }
    sessions::write_private(&file, &key)?;
    SessionKey::new(&key, Protection::File)
}

fn lookup() -> Option<Vec<u8>> {
    let output = Command::new("secret-tool")
        .arg("lookup")
        .args(ATTRIBUTES)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let key = decode_base64(String::from_utf8(output.stdout).ok()?.trim()).ok()?;
    (key.len() == KEY_LEN).then_some(key)
}

fn store(key: &[u8]) -> bool {
    let Ok(mut child) = Command::new("secret-tool")
        .arg("store")
        .arg(format!("--label={LABEL}"))
        .args(ATTRIBUTES)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(encode_base64(key).as_bytes()).is_ok());
    child.wait().is_ok_and(|status| status.success()) && written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_existing_key_file_is_used_as_it_is() {
        let dir = std::env::temp_dir().join(format!("anixops-linux-key-{}", sessions::now_ms()));
        sessions::write_private(&dir.join(KEY_FILE), &[9u8; KEY_LEN]).unwrap();
        let key = load(&dir).unwrap();
        assert_eq!(key.protection, Protection::File);
        let request = sessions::storage_request(Path::new("/x"), &key);
        assert_eq!(
            decode_base64(request["key_base64"].as_str().unwrap()).unwrap(),
            vec![9u8; KEY_LEN]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_key_file_is_an_error() {
        let dir =
            std::env::temp_dir().join(format!("anixops-linux-bad-key-{}", sessions::now_ms()));
        sessions::write_private(&dir.join(KEY_FILE), &[1u8; 5]).unwrap();
        assert!(load(&dir).is_err());
        assert_eq!(std::fs::read(dir.join(KEY_FILE)).unwrap().len(), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
