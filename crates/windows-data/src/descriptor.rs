//! Security descriptors in their text form (SDDL), read from and written to files and folders.

use std::io;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{GetLastError, LocalFree, ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetFileSecurityW, GetKernelObjectSecurity, SetFileSecurityW, SetKernelObjectSecurity,
    DACL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};

/// What [`describe`] reads: the owner and the access list.
const DESCRIBED: OBJECT_SECURITY_INFORMATION =
    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;

pub(crate) fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub(crate) fn last_error() -> io::Error {
    io::Error::last_os_error()
}

/// Reads a NUL-terminated UTF-16 string the system allocated, then frees it the way the system says to.
///
/// # Safety
///
/// `text` must be a non-null, NUL-terminated string allocated with `LocalAlloc`, not used afterwards.
pub(crate) unsafe fn take_local_string(text: *mut u16) -> String {
    let mut end = 0;
    // SAFETY: the caller guarantees a NUL-terminated string, read up to its terminator.
    while unsafe { *text.add(end) } != 0 {
        end += 1;
    }
    // SAFETY: the `end` characters before the terminator were just read.
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, end) });
    // SAFETY: the caller guarantees the string came from `LocalAlloc` and is not used again.
    unsafe {
        LocalFree(text.cast());
    }
    value
}

/// A security descriptor made from its text form, freed when dropped.
pub(crate) struct Descriptor(pub(crate) PSECURITY_DESCRIPTOR);

impl Descriptor {
    pub(crate) fn new(text: &str) -> io::Result<Self> {
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

/// Sets the parts of `path`'s security that `information` names from the descriptor `text`.
pub(crate) fn set(
    path: &Path,
    information: OBJECT_SECURITY_INFORMATION,
    text: &str,
) -> io::Result<()> {
    let name = wide(path);
    let descriptor = Descriptor::new(text)?;
    // SAFETY: `name` is NUL-terminated and `descriptor` is a valid security descriptor.
    if unsafe { SetFileSecurityW(name.as_ptr(), information, descriptor.0) } == 0 {
        return Err(last_error());
    }
    Ok(())
}

/// Sets the parts of the security of the open file or folder `handle` that `information` names from the
/// descriptor `text`. The handle must have been opened with the rights the change needs.
pub(crate) fn set_handle(
    handle: HANDLE,
    information: OBJECT_SECURITY_INFORMATION,
    text: &str,
) -> io::Result<()> {
    let descriptor = Descriptor::new(text)?;
    // SAFETY: the caller passes an open handle; `descriptor` is a valid security descriptor.
    if unsafe { SetKernelObjectSecurity(handle, information, descriptor.0) } == 0 {
        return Err(last_error());
    }
    Ok(())
}

/// The owner and access list of `path` in their text form, such as `O:BAD:P(A;OICI;FA;;;SY)`.
pub fn describe(path: &Path) -> io::Result<String> {
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated; `describe_with` passes a buffer valid for the length it gives.
    describe_with(|descriptor, length, needed| unsafe {
        GetFileSecurityW(name.as_ptr(), DESCRIBED, descriptor, length, needed)
    })
}

/// [`describe`] for the open file or folder `handle`, which must have been opened with `READ_CONTROL`.
pub(crate) fn describe_handle(handle: HANDLE) -> io::Result<String> {
    // SAFETY: the caller passes an open handle; `describe_with` passes a buffer valid for the length it
    // gives.
    describe_with(|descriptor, length, needed| unsafe {
        GetKernelObjectSecurity(handle, DESCRIBED, descriptor, length, needed)
    })
}

/// Reads a descriptor with `read` (called first without a buffer for the size, then with one) and gives
/// its owner and access list in their text form.
fn describe_with(read: impl Fn(PSECURITY_DESCRIPTOR, u32, &mut u32) -> i32) -> io::Result<String> {
    let mut needed = 0_u32;
    // With no buffer the call only reports the size needed.
    let sized = read(null_mut(), 0, &mut needed);
    // SAFETY: reads this thread's last error code.
    if sized == 0 && unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(last_error());
    }
    // Descriptors hold pointers-sized fields; a buffer of `u64` is aligned for them.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8).max(1)];
    let descriptor: PSECURITY_DESCRIPTOR = buffer.as_mut_ptr().cast();
    if read(descriptor, (buffer.len() * 8) as u32, &mut needed) == 0 {
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
            DESCRIBED,
            &mut text,
            &mut length,
        )
    };
    if converted == 0 || text.is_null() {
        return Err(last_error());
    }
    // SAFETY: on success `text` is a NUL-terminated string the conversion allocated with `LocalAlloc`.
    Ok(unsafe { take_local_string(text) })
}

/// The owner in `described` (from [`describe`]): an alias such as `BA` or a SID such as `S-1-5-21-…`.
pub fn owner(described: &str) -> Option<&str> {
    let rest = described.strip_prefix("O:")?;
    let end = rest
        .find("D:")
        .or_else(|| rest.find("G:"))
        .unwrap_or(rest.len());
    let owner = &rest[..end];
    (!owner.is_empty()).then_some(owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owner_is_read_from_the_text_form() {
        assert_eq!(owner("O:BAD:P(A;OICI;FA;;;SY)"), Some("BA"));
        assert_eq!(owner("O:SYD:(A;;FA;;;WD)"), Some("SY"));
        assert_eq!(
            owner("O:S-1-5-21-1-2-3-1001D:(A;OICI;FA;;;S-1-5-21-1-2-3-1001)"),
            Some("S-1-5-21-1-2-3-1001")
        );
        assert_eq!(owner("O:BA"), Some("BA"));
        assert_eq!(owner("D:(A;;FA;;;WD)"), None);
        assert_eq!(owner("O:D:(A;;FA;;;WD)"), None);
    }
}
