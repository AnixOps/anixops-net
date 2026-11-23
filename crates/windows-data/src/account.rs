//! Who this process runs as, and the rights it turns on to take back what others own.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, TokenUser,
    LUID_AND_ATTRIBUTES, SE_BACKUP_NAME, SE_PRIVILEGE_ENABLED, SE_RESTORE_NAME,
    SE_TAKE_OWNERSHIP_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::descriptor::take_local_string;

/// This process's token, closed when dropped.
struct Token(HANDLE);

impl Token {
    fn open(access: u32) -> Option<Self> {
        let mut token: HANDLE = null_mut();
        // SAFETY: the pseudo handle of this process needs no closing; `token` is valid for the handle written.
        if unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) } == 0 {
            return None;
        }
        Some(Self(token))
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken and is closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// The SID of the account this process runs as, such as `S-1-5-21-…-1001`.
pub fn current_user_sid() -> Option<String> {
    let token = Token::open(TOKEN_QUERY)?;
    let mut needed = 0_u32;
    // SAFETY: with no buffer the call only reports the size needed.
    unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed) };
    if needed == 0 {
        return None;
    }
    // TOKEN_USER holds a pointer; a buffer of `u64` is aligned for it.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    // SAFETY: `buffer` is valid for writes of the bytes passed.
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut needed,
        )
    };
    if read == 0 {
        return None;
    }
    // SAFETY: the call above filled the buffer with a TOKEN_USER, whose SID points into the same buffer.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text: *mut u16 = null_mut();
    // SAFETY: the SID is valid while `buffer` lives; `text` is valid for the pointer written.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 || text.is_null() {
        return None;
    }
    // SAFETY: on success `text` is a NUL-terminated string allocated with `LocalAlloc`.
    Some(unsafe { take_local_string(text) })
}

/// Rights turned on by [`enable_ownership_privileges`]; dropping it puts each back as it was.
#[must_use = "the rights are turned off again when this is dropped"]
pub(crate) struct OwnershipPrivileges {
    token: Option<Token>,
    /// What each right was before, for the rights this changed, in the order they were changed.
    previous: Vec<TOKEN_PRIVILEGES>,
}

impl Drop for OwnershipPrivileges {
    fn drop(&mut self) {
        let Some(token) = &self.token else {
            return;
        };
        for before in self.previous.iter().rev() {
            // SAFETY: `before` is the state AdjustTokenPrivileges reported; no previous state is asked for.
            unsafe {
                AdjustTokenPrivileges(token.0, 0, before, 0, null_mut(), null_mut());
            }
        }
    }
}

/// Turns on the administrators' rights to open any file for backup and restore, to take ownership of any
/// file and to set any owner, which taking back something an ordinary user put in the data folder needs,
/// for as long as the value returned lives: the rest of the program runs without them. A right the token
/// does not hold stays off; the change that needs it then fails and says so.
pub(crate) fn enable_ownership_privileges() -> OwnershipPrivileges {
    let Some(token) = Token::open(TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY) else {
        return OwnershipPrivileges {
            token: None,
            previous: Vec::new(),
        };
    };
    let mut previous = Vec::new();
    for name in [SE_BACKUP_NAME, SE_RESTORE_NAME, SE_TAKE_OWNERSHIP_NAME] {
        let mut luid = LUID::default();
        // SAFETY: `name` is a NUL-terminated constant and `luid` is valid for the value written.
        if unsafe { LookupPrivilegeValueW(null(), name, &mut luid) } == 0 {
            continue;
        }
        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let mut before = TOKEN_PRIVILEGES {
            PrivilegeCount: 0,
            Privileges: [LUID_AND_ATTRIBUTES::default()],
        };
        let mut length = 0_u32;
        // SAFETY: `privileges` is a valid TOKEN_PRIVILEGES; `before` holds the one entry a change of one
        // right reports, and its size is passed.
        let changed = unsafe {
            AdjustTokenPrivileges(
                token.0,
                0,
                &privileges,
                std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                &mut before,
                &mut length,
            )
        };
        // A right that was already on, or that the token does not hold, reports nothing to put back.
        if changed != 0 && before.PrivilegeCount > 0 {
            previous.push(before);
        }
    }
    OwnershipPrivileges {
        token: Some(token),
        previous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_has_an_account() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
    }
}
