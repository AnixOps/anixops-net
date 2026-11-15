//! The key that seals saved capture sessions on Windows (`desktop_mitm::sessions`). It is sealed with Windows
//! Data Protection (DPAPI) in the current user's scope, so only this Windows account on this computer can open
//! it, and the window never sees it.
//!
//! A key file that is there but cannot be opened is an error, never replaced: a new key would leave every
//! saved session unreadable.

use std::path::Path;
use std::ptr::{null, null_mut};

use desktop_mitm::sessions::{self, Protection, SessionKey};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

/// The sealed key, next to the saved sessions.
const KEY_FILE: &str = "session-key.dpapi";

/// The session key for the capture folder `dir`, made and sealed on first use.
pub fn load(dir: &Path) -> Result<SessionKey, String> {
    let key = sessions::load_or_create_sealed(&dir.join(KEY_FILE), protect, unprotect)?;
    SessionKey::new(&key, Protection::Dpapi)
}

fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    crypt(data, true)
}

fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    crypt(data, false)
}

/// DPAPI in the current user's scope, with no prompt.
fn crypt(data: &[u8], seal: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).map_err(|_| "the capture session key is too large")?,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // SAFETY: `input` points at `data` for the call; DPAPI only reads it. `output` is filled by the call and
    // freed with LocalFree below, after its bytes are copied.
    let ok = unsafe {
        if seal {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(if seal {
            "the capture session key could not be sealed with Windows Data Protection".to_string()
        } else {
            "the capture session key could not be opened by this Windows account".to_string()
        });
    }
    // SAFETY: on success DPAPI returns `cbData` bytes at `pbData`, allocated with LocalAlloc.
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    // SAFETY: `pbData` came from DPAPI and is freed once.
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_protection_opens_what_it_sealed() {
        let sealed = protect(b"capture session key").unwrap();
        assert_ne!(sealed, b"capture session key");
        assert_eq!(unprotect(&sealed).unwrap(), b"capture session key");
        assert!(unprotect(b"not sealed by data protection").is_err());
    }

    #[test]
    fn the_key_is_made_once_and_read_back() {
        let dir = std::env::temp_dir().join(format!(
            "anixops-session-key-{}-{}",
            std::process::id(),
            sessions::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).is_ok());
        let sealed = std::fs::read(dir.join(KEY_FILE)).unwrap();
        assert!(load(&dir).is_ok());
        assert_eq!(std::fs::read(dir.join(KEY_FILE)).unwrap(), sealed);
        std::fs::write(dir.join(KEY_FILE), b"damaged").unwrap();
        assert!(load(&dir).is_err());
        assert_eq!(std::fs::read(dir.join(KEY_FILE)).unwrap(), b"damaged");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
