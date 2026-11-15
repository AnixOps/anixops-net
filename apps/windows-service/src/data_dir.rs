//! The service's data folder, closed to everyone but the system and administrators.
//!
//! `%ProgramData%` lets any user make folders in it, and a folder made there is open to all users unless
//! told otherwise. The service's folder holds subscriptions, keys and captured traffic, and the service
//! acts on what it reads there with the system's rights. So the folder is made with an access list of its
//! own, and one that is already there is only used when the system or an administrator owns it: a folder
//! an ordinary user put in the way first is refused.

use std::io;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{
    GetLastError, LocalFree, ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetFileSecurityW, SetFileSecurityW, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

/// Owned by administrators; the system and administrators in full, for the folder and everything made in
/// it later; nothing inherited from the folder above.
const ACCESS: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// The same access list without the owner, for a folder that already has a trusted one.
const ACCESS_ONLY: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

/// A security descriptor made from its text form, freed when dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn new(text: &str) -> io::Result<Self> {
        let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `text` is NUL-terminated and `descriptor` is valid for the pointer written to it.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if converted == 0 {
            return Err(last_error());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by the conversion above, which says to free it this way.
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// The owner and access list of `path` in their text form, such as `O:BAD:P(A;OICI;FA;;;SY)`.
pub fn describe(path: &Path) -> io::Result<String> {
    let name = wide(path);
    let wanted = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut needed = 0_u32;
    // SAFETY: with no buffer the call only reports the size needed.
    let sized = unsafe { GetFileSecurityW(name.as_ptr(), wanted, null_mut(), 0, &mut needed) };
    // SAFETY: reads this thread's last error code.
    if sized == 0 && unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(last_error());
    }
    // Descriptors hold pointers-sized fields; a buffer of `u64` is aligned for them.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8).max(1)];
    let descriptor: PSECURITY_DESCRIPTOR = buffer.as_mut_ptr().cast();
    // SAFETY: `buffer` is valid for writes of the `needed` bytes passed.
    let read = unsafe {
        GetFileSecurityW(
            name.as_ptr(),
            wanted,
            descriptor,
            (buffer.len() * 8) as u32,
            &mut needed,
        )
    };
    if read == 0 {
        return Err(last_error());
    }
    let mut text: *mut u16 = null_mut();
    let mut length = 0_u32;
    // SAFETY: `descriptor` was filled by the call above; `text` and `length` are valid for what is
    // written to them.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            wanted,
            &mut text,
            &mut length,
        )
    };
    if converted == 0 {
        return Err(last_error());
    }
    // SAFETY: on success `text` points to a NUL-terminated string, read up to its terminator and then
    // freed the way the conversion says to.
    let described = unsafe {
        let mut end = 0;
        while *text.add(end) != 0 {
            end += 1;
        }
        let described = String::from_utf16_lossy(std::slice::from_raw_parts(text, end));
        LocalFree(text.cast());
        described
    };
    Ok(described)
}

/// Whether the system or the administrators group owns what `described` (from [`describe`]) is about.
pub fn trusted_owner(described: &str) -> bool {
    described.starts_with("O:BA") || described.starts_with("O:SY")
}

/// Whether `described` (from [`describe`]) is a folder as [`secure`] leaves it: owned by administrators,
/// nothing inherited from above, and the system and administrators the only ones let in.
pub fn closed(described: &str) -> bool {
    described.starts_with("O:BAD:P")
        && described.contains("(A;OICI;FA;;;SY)")
        && described.contains("(A;OICI;FA;;;BA)")
        && described.matches('(').count() == 2
}

/// Makes `path` (and the folders above it) when missing, closed to ordinary users. A folder that is
/// already there has its access list set again, unless someone other than the system or an administrator
/// owns it, which is an error.
pub fn secure(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        // The folders above are shared with other programs and keep the access the system gives them.
        std::fs::create_dir_all(parent)?;
    }
    let name = wide(path);
    let descriptor = Descriptor::new(ACCESS)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: `name` is NUL-terminated and `attributes` with its descriptor outlives the call.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } != 0 {
        return Ok(());
    }
    // SAFETY: reads this thread's last error code.
    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return Err(last_error());
    }
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is not a folder", path.display()),
        ));
    }
    if !trusted_owner(&describe(path)?) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} was made by someone other than the system or an administrator and is not used",
                path.display()
            ),
        ));
    }
    let descriptor = Descriptor::new(ACCESS_ONLY)?;
    // SAFETY: `name` is NUL-terminated and `descriptor` is a valid security descriptor.
    let set = unsafe {
        SetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    };
    if set == 0 {
        return Err(last_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_other_than_system_and_administrators_are_not_trusted() {
        assert!(trusted_owner("O:BAD:P(A;OICI;FA;;;SY)"));
        assert!(trusted_owner("O:SYD:(A;;FA;;;WD)"));
        assert!(!trusted_owner(
            "O:S-1-5-21-1-2-3-1001D:(A;OICI;FA;;;S-1-5-21-1-2-3-1001)"
        ));
        assert!(!trusted_owner("O:BUD:(A;;FA;;;WD)"));
        assert!(!trusted_owner("D:(A;;FA;;;WD)"));
    }

    #[test]
    fn a_folder_that_lets_anyone_else_in_is_not_closed() {
        assert!(closed("O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"));
        assert!(closed("O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"));
        assert!(!closed(
            "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)"
        ));
        assert!(!closed("O:BAD:(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"));
        assert!(!closed("O:BAD:P(A;OICI;FA;;;SY)"));
    }

    #[test]
    fn the_folder_is_closed_to_ordinary_users_and_stays_so() {
        let dir = std::env::temp_dir()
            .join(format!("anixops-net-service-dir-{}", std::process::id()))
            .join("AnixOps")
            .join("Net");
        let _ = std::fs::remove_dir_all(&dir);
        secure(&dir).unwrap();
        assert!(closed(&describe(&dir).unwrap()));
        // A second start finds the folder there and leaves it closed.
        secure(&dir).unwrap();
        assert!(closed(&describe(&dir).unwrap()));
        // What is made in it later is closed too.
        let file = dir.join("state.json");
        std::fs::write(&file, "{}").unwrap();
        let described = describe(&file).unwrap();
        assert!(!described.contains(";;;WD)"), "{described}");
        assert!(!described.contains(";;;BU)"), "{described}");
        assert!(!described.contains(";;;AU)"), "{described}");
        std::fs::remove_dir_all(dir.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_file_in_the_way_is_an_error() {
        let path =
            std::env::temp_dir().join(format!("anixops-net-service-file-{}", std::process::id()));
        std::fs::write(&path, "").unwrap();
        assert!(secure(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
