//! The LAN share's proxy username and password (`desktop_mitm::phone::LanShareAuth`) at rest on the desktop.
//! Windows seals them with Windows Data Protection (DPAPI) in the current user's scope; Linux keeps them in the
//! desktop's Secret Service (GNOME Keyring, KWallet) through `secret-tool`, or, where there is none, in a file
//! only this user can read, like the capture session key ([`super::session_key`]).
//!
//! Unlike that key, credentials that cannot be read back are replaced by new ones: nothing else depends on them,
//! and the share must never start without a password because a keyring was locked. The phones then need the new
//! pair, which the window shows.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use desktop_mitm::phone::LanShareAuth;
use desktop_mitm::sessions::write_private;

/// Serialises the read-modify-write below, so two first uses at once cannot each keep a different pair.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The credentials kept in `dir`, made and kept on first use. Users who had the share on before it asked for a
/// password get theirs here, the first time they start it, with the password required.
pub fn load(dir: &Path) -> Result<LanShareAuth, String> {
    let _guard = lock();
    load_locked(dir)
}

fn load_locked(dir: &Path) -> Result<LanShareAuth, String> {
    if let Some(auth) = read(dir)? {
        return Ok(auth);
    }
    let auth = LanShareAuth::generate()?;
    save(dir, &auth)?;
    Ok(auth)
}

/// New credentials in place of the kept ones; whether they are required stays as it was.
pub fn regenerate(dir: &Path) -> Result<LanShareAuth, String> {
    let _guard = lock();
    let auth = load_locked(dir)?.regenerated()?;
    save(dir, &auth)?;
    Ok(auth)
}

/// Whether phones must send the credentials; the kept pair stays for when it is turned on again.
pub fn set_required(dir: &Path, required: bool) -> Result<LanShareAuth, String> {
    let _guard = lock();
    let mut auth = load_locked(dir)?;
    auth.required = required;
    save(dir, &auth)?;
    Ok(auth)
}

#[cfg(windows)]
const FILE: &str = "lan-share-auth.dpapi";
#[cfg(windows)]
const WHAT: &str = "LAN share password";

/// What is kept, or `None` when nothing is or it cannot be opened any more.
#[cfg(windows)]
fn read(dir: &Path) -> Result<Option<LanShareAuth>, String> {
    match std::fs::read(file(dir)) {
        Ok(sealed) => Ok(super::session_key::crypt(&sealed, false, WHAT)
            .ok()
            .and_then(|bytes| LanShareAuth::from_bytes(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("the {WHAT} could not be read: {error}")),
    }
}

/// Keeps `auth`, sealed for this Windows account.
#[cfg(windows)]
pub fn save(dir: &Path, auth: &LanShareAuth) -> Result<(), String> {
    let sealed = super::session_key::crypt(&auth.to_bytes()?, true, WHAT)?;
    write_private(&file(dir), &sealed)
}

/// The Secret Service item's attributes.
#[cfg(not(windows))]
const ATTRIBUTES: [&str; 4] = ["application", "anixops-net", "purpose", "lan-share-auth"];
#[cfg(not(windows))]
const LABEL: &str = "AnixOps Net LAN share";
/// The credentials themselves, when there is no Secret Service.
#[cfg(not(windows))]
const FILE: &str = "lan-share-auth.json";
/// Says the credentials are in the Secret Service.
#[cfg(not(windows))]
const MARKER_FILE: &str = "lan-share-auth.secret-service";

/// What is kept, or `None` when nothing is or it cannot be read (a damaged file, a locked keyring).
#[cfg(not(windows))]
fn read(dir: &Path) -> Result<Option<LanShareAuth>, String> {
    match std::fs::read(file(dir)) {
        Ok(bytes) => return Ok(LanShareAuth::from_bytes(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("the LAN share password could not be read: {error}")),
    }
    if !dir.join(MARKER_FILE).exists() {
        return Ok(None);
    }
    Ok(super::session_key::secret_lookup(&ATTRIBUTES)
        .and_then(|text| LanShareAuth::from_bytes(text.as_bytes())))
}

/// Keeps `auth`: in the Secret Service when it takes and returns it (and no file was made before), otherwise in
/// a file only this user can read.
#[cfg(not(windows))]
pub fn save(dir: &Path, auth: &LanShareAuth) -> Result<(), String> {
    let bytes = auth.to_bytes()?;
    let text = String::from_utf8(bytes.clone()).map_err(|error| error.to_string())?;
    let file = file(dir);
    let marker = dir.join(MARKER_FILE);
    if !file.exists()
        && super::session_key::secret_store(LABEL, &ATTRIBUTES, &text)
        && super::session_key::secret_lookup(&ATTRIBUTES).as_deref() == Some(text.as_str())
    {
        return write_private(&marker, b"");
    }
    write_private(&file, &bytes)?;
    // The file is read first from now on; the marker would only send a later read to an old item.
    let _ = std::fs::remove_file(marker);
    Ok(())
}

fn file(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anixops-lan-share-{name}-{}-{}",
            std::process::id(),
            desktop_mitm::sessions::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A kept pair is used as it is; a damaged one is replaced by a new, required pair rather than leaving the
    /// share without a password.
    #[cfg(not(windows))]
    #[test]
    fn a_kept_file_is_read_and_a_damaged_one_replaced() {
        let dir = temporary_dir("file");
        let kept = LanShareAuth {
            required: false,
            username: "anx-abcdef".to_string(),
            password: "abcdefghjkmnpqrstuvw".to_string(),
        };
        write_private(&file(&dir), &kept.to_bytes().unwrap()).unwrap();
        assert_eq!(load(&dir).unwrap(), kept);
        let on = set_required(&dir, true).unwrap();
        assert!(on.required);
        assert_eq!(on.password, kept.password);
        assert_eq!(load(&dir).unwrap(), on);
        let fresh = regenerate(&dir).unwrap();
        assert!(fresh.required);
        assert_ne!(fresh.password, kept.password);
        assert_eq!(load(&dir).unwrap(), fresh);

        write_private(&file(&dir), b"damaged").unwrap();
        let replaced = load(&dir).unwrap();
        assert!(replaced.required);
        assert!(replaced.is_valid());
        // Written to the file again (a file was there), so the next read finds the same pair.
        assert_eq!(load(&dir).unwrap(), replaced);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn the_pair_is_sealed_read_back_and_replaced_when_damaged() {
        let dir = temporary_dir("dpapi");
        let first = load(&dir).unwrap();
        assert!(first.required);
        let sealed = std::fs::read(file(&dir)).unwrap();
        assert!(!sealed
            .windows(first.password.len())
            .any(|window| window == first.password.as_bytes()));
        assert_eq!(load(&dir).unwrap(), first);
        let off = set_required(&dir, false).unwrap();
        assert!(!off.required);
        assert_eq!(off.password, first.password);
        assert_eq!(load(&dir).unwrap(), off);
        std::fs::write(file(&dir), b"damaged").unwrap();
        let replaced = load(&dir).unwrap();
        assert!(replaced.required);
        assert_ne!(replaced.password, first.password);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
