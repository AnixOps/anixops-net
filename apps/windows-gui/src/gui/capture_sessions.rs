//! Saved capture sessions on Windows (`desktop_mitm::sessions`). Sessions and their key live in the user's local
//! application data folder; the key is sealed with Windows Data Protection (DPAPI), so only this Windows account
//! on this computer can open it, and the window never sees it.

use std::path::PathBuf;
use std::ptr::{null, null_mut};

use desktop_mitm::sessions::{self, Protection, SessionKey};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

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

/// The session key, made and sealed on first use.
pub fn load_key() -> Result<SessionKey, String> {
    let path = capture_dir()?.join("session-key.dpapi");
    let key = sessions::load_or_create_sealed(&path, protect, unprotect)?;
    SessionKey::new(&key, Protection::Dpapi)
}

/// Deletes expired sessions; called at start, since the engine only does it while it runs.
pub fn purge_expired() {
    if let Ok(dir) = sessions_dir() {
        sessions::purge(&dir, sessions::now_ms());
    }
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
