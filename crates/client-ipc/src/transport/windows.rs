//! A named pipe.
//!
//! Every handle is opened for overlapped I/O. A pipe handle opened the plain way lets only one operation
//! run at a time, so a thread waiting to read would hold up every thread that wants to write, and the two
//! sides could not both speak (`crate::peer`). Each operation here still waits for its own result; it just
//! does not make the others wait.

use std::io::{self, Read, Write};
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_BROKEN_PIPE, ERROR_INSUFFICIENT_BUFFER,
    ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetKernelObjectSecurity, GetSecurityDescriptorOwner, GetTokenInformation, TokenUser,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateEventW, GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

/// Where the service listens.
pub const DEFAULT_ENDPOINT: &str = r"\\.\pipe\AnixOpsNet.Core";

/// Who may open the pipe: the system, administrators and the pipe's owner (the core itself, also when it runs
/// in a console as an ordinary user) in full; users signed in at this machine to read and write. Nobody else,
/// and nothing inherited.
///
/// The signed-in users get [`SIGNED_IN_ACCESS`], not `GRGW`: on a pipe `GENERIC_WRITE` includes
/// `FILE_APPEND_DATA`, which is `FILE_CREATE_PIPE_INSTANCE`, so any of them could serve further instances of
/// the core's pipe and take the windows that connect to them.
const ACCESS: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;0x12019b;;;IU)";

/// What users signed in at the machine may do with the pipe: `FILE_GENERIC_READ | FILE_GENERIC_WRITE`
/// (0x12019f) without `FILE_CREATE_PIPE_INSTANCE` (0x4). Connecting needs no more.
const SIGNED_IN_ACCESS: u32 = 0x0012_019b;

/// `FILE_CREATE_PIPE_INSTANCE`: serving another instance of an existing pipe.
const CREATE_PIPE_INSTANCE: u32 = 0x0000_0004;

// The signed-in users can connect but never serve an instance of their own.
const _: () = assert!(SIGNED_IN_ACCESS & CREATE_PIPE_INSTANCE == 0);

/// The pipe's own buffers. Larger messages go through in pieces.
const BUFFER: u32 = 64 * 1024;

/// How often and how long the window waits when every instance of the pipe is busy with another window.
const BUSY_TRIES: u32 = 40;
const BUSY_WAIT: Duration = Duration::from_millis(50);

/// A handle this module closes.
struct Owned(HANDLE);

// SAFETY: a kernel handle may be used and closed from any thread.
unsafe impl Send for Owned {}
// SAFETY: the operations made on it here (overlapped reads and writes, each with its own event) may run at
// the same time.
unsafe impl Sync for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is open and nothing else closes it.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> u32 {
    // SAFETY: reads this thread's last error code.
    unsafe { GetLastError() }
}

fn os_error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(code as i32)
}

/// An event that stays set until the next operation resets it; one operation at a time waits on it.
fn event() -> io::Result<Owned> {
    // SAFETY: no name and no security attributes; the result is checked.
    let handle = unsafe { CreateEventW(null(), 1, 0, null()) };
    if handle.is_null() {
        return Err(os_error(last_error()));
    }
    Ok(Owned(handle))
}

/// Starts an overlapped operation with `start` and waits for it. Returns the bytes moved.
fn overlapped(
    handle: HANDLE,
    event: &Owned,
    start: impl FnOnce(*mut OVERLAPPED) -> i32,
) -> Result<u32, u32> {
    // SAFETY: an all-zero OVERLAPPED is its documented starting state.
    let mut state: OVERLAPPED = unsafe { zeroed() };
    state.hEvent = event.0;
    if start(&mut state) == 0 {
        let code = last_error();
        if code != ERROR_IO_PENDING {
            return Err(code);
        }
    }
    let mut moved = 0_u32;
    // SAFETY: `state` is the structure the operation was started with and lives until this returns, which
    // with the wait flag set is not before the operation has finished.
    if unsafe { GetOverlappedResult(handle, &state, &mut moved, 1) } == 0 {
        return Err(last_error());
    }
    Ok(moved)
}

/// One end of a connected pipe.
pub struct Stream {
    pipe: Arc<Owned>,
    /// This clone's own event, so a clone that reads and a clone that writes do not wake each other.
    event: Owned,
}

impl Stream {
    fn new(pipe: Arc<Owned>) -> io::Result<Self> {
        Ok(Self {
            pipe,
            event: event()?,
        })
    }

    /// Another handle on the same end, for a thread of its own.
    pub fn try_clone(&self) -> io::Result<Self> {
        Self::new(Arc::clone(&self.pipe))
    }
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        let pipe = self.pipe.0;
        let result = overlapped(pipe, &self.event, |state| {
            // SAFETY: `buffer` is valid for writes of `length` bytes until the operation has finished,
            // which `overlapped` waits for.
            unsafe { ReadFile(pipe, buffer.as_mut_ptr(), length, null_mut(), state) }
        });
        match result {
            Ok(moved) => Ok(moved as usize),
            // The other end closed: the end of the stream, not a failure.
            Err(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED | ERROR_NO_DATA) => Ok(0),
            Err(code) => Err(os_error(code)),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        let pipe = self.pipe.0;
        overlapped(pipe, &self.event, |state| {
            // SAFETY: `buffer` is valid for reads of `length` bytes until the operation has finished,
            // which `overlapped` waits for.
            unsafe { WriteFile(pipe, buffer.as_ptr(), length, null_mut(), state) }
        })
        .map(|moved| moved as usize)
        .map_err(os_error)
    }

    fn flush(&mut self) -> io::Result<()> {
        // A pipe hands bytes to the other end as they are written.
        Ok(())
    }
}

/// The access list as Windows wants it, freed when dropped.
struct Security(PSECURITY_DESCRIPTOR);

// SAFETY: the descriptor is a block of memory that is only read after it is made.
unsafe impl Send for Security {}
// SAFETY: as above.
unsafe impl Sync for Security {}

impl Security {
    fn new() -> io::Result<Self> {
        let text = wide(ACCESS);
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
            return Err(os_error(last_error()));
        }
        Ok(Self(descriptor))
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by the conversion above, which says to free it this way.
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub struct Listener {
    name: Vec<u16>,
    security: Security,
    /// The instance the next window connects to. There is always one, so a window never finds the pipe
    /// missing while the core is busy with another window.
    waiting: Mutex<Owned>,
}

impl Listener {
    /// Listens at `endpoint`, a pipe name such as [`DEFAULT_ENDPOINT`]. A name another program already
    /// serves is an error.
    pub fn bind(endpoint: &str) -> io::Result<Self> {
        let name = wide(endpoint);
        let security = Security::new()?;
        let first = instance(&name, &security, true)?;
        Ok(Self {
            name,
            security,
            waiting: Mutex::new(first),
        })
    }

    /// Waits for the next window.
    pub fn accept(&self) -> io::Result<Stream> {
        let mut waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
        let ready = event()?;
        loop {
            let pipe = waiting.0;
            let connected = overlapped(pipe, &ready, |state| {
                // SAFETY: `pipe` is an instance of this listener's pipe and `state` lives until the
                // operation has finished, which `overlapped` waits for.
                unsafe { ConnectNamedPipe(pipe, state) }
            });
            match connected {
                // A window that connected between two calls is reported as already there.
                Ok(_) | Err(ERROR_PIPE_CONNECTED) => break,
                // A window connected and closed again before it was accepted (a probe, or anyone opening
                // and closing the pipe in a loop): the instance is closing. It is made ready again and the
                // next window waited for; this is not a failure of the listener.
                Err(ERROR_NO_DATA) => self.recycle(&mut waiting)?,
                Err(code) => return Err(os_error(code)),
            }
        }
        let next = instance(&self.name, &self.security, false)?;
        let connected = std::mem::replace(&mut *waiting, next);
        Stream::new(Arc::new(connected))
    }

    /// Makes the instance a window left closing listen again. When it cannot be, a new instance takes its
    /// place, made before the old one is closed: without an instance the name would be free for a moment,
    /// and later instances do not have the "first instance" flag that would notice someone taking it.
    fn recycle(&self, waiting: &mut Owned) -> io::Result<()> {
        // SAFETY: `waiting` is an open instance of this listener's pipe, not in use by any operation.
        if unsafe { DisconnectNamedPipe(waiting.0) } != 0 {
            return Ok(());
        }
        let fresh = instance(&self.name, &self.security, false)?;
        // The old instance is closed when it is dropped here.
        drop(std::mem::replace(waiting, fresh));
        Ok(())
    }
}

/// One instance of the pipe, waiting for a window.
fn instance(name: &[u16], security: &Security, first: bool) -> io::Result<Owned> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        // Fails when the name is already served, by an earlier run or by a program posing as the core.
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: `name` is NUL-terminated and `attributes` with its descriptor outlives the call.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER,
            BUFFER,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(os_error(last_error()));
    }
    Ok(Owned(handle))
}

/// Connects to the core at `endpoint`. The core learns who the caller is but cannot act as them, and the window
/// makes sure it is the core ([`verify_server`]) before anything is said.
pub fn connect(endpoint: &str) -> io::Result<Stream> {
    let stream = open(endpoint)?;
    verify_server(stream.pipe.0, endpoint)?;
    Ok(stream)
}

/// Whether the pipe's owner may serve `endpoint`. Only the installed service (LocalSystem) or an elevated
/// administrator can make a pipe owned by the system (`SY`) or the administrators (`BA`); an ordinary user
/// cannot give a pipe such an owner. A pipe some other user made while the service was stopped is theirs,
/// and from them the window would take orders (`session.apply_proxy`, `session.trust_root`). For an endpoint
/// other than [`DEFAULT_ENDPOINT`] (a core in a console, as tests run it) a pipe of the window's own user is
/// accepted too: it is no one the user does not trust already.
fn trusted_owner(owner: &str, endpoint: &str, current_user: Option<&str>) -> bool {
    if matches!(owner, "SY" | "BA") {
        return true;
    }
    let default = endpoint.eq_ignore_ascii_case(DEFAULT_ENDPOINT);
    !default && current_user.is_some_and(|user| user.eq_ignore_ascii_case(owner))
}

/// Refuses a pipe the core did not make: see [`trusted_owner`].
fn verify_server(pipe: HANDLE, endpoint: &str) -> io::Result<()> {
    let owner = pipe_owner(pipe)?;
    if trusted_owner(&owner, endpoint, current_user_sid().as_deref()) {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("the pipe is not the AnixOps Net core service's: it belongs to {owner}"),
    ))
}

/// The owner of the open pipe `pipe` ([`owner_name`]): `SY`, `BA`, or a SID such as `S-1-5-21-…`.
fn pipe_owner(pipe: HANDLE) -> io::Result<String> {
    let mut needed = 0_u32;
    // SAFETY: with no buffer the call only reports the size needed.
    let sized = unsafe {
        GetKernelObjectSecurity(pipe, OWNER_SECURITY_INFORMATION, null_mut(), 0, &mut needed)
    };
    if sized == 0 && last_error() != ERROR_INSUFFICIENT_BUFFER {
        return Err(os_error(last_error()));
    }
    // Descriptors hold pointer-sized fields; a buffer of `u64` is aligned for them.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8).max(1)];
    let descriptor: PSECURITY_DESCRIPTOR = buffer.as_mut_ptr().cast();
    // SAFETY: `pipe` is open with `READ_CONTROL` (part of `GENERIC_READ`); `descriptor` is valid for the
    // length passed.
    let read = unsafe {
        GetKernelObjectSecurity(
            pipe,
            OWNER_SECURITY_INFORMATION,
            descriptor,
            (buffer.len() * 8) as u32,
            &mut needed,
        )
    };
    if read == 0 {
        return Err(os_error(last_error()));
    }
    let mut owner = null_mut();
    let mut defaulted = 0;
    // SAFETY: `descriptor` was filled above and stays alive in `buffer`; the owner SID points into it.
    if unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) } == 0 {
        return Err(os_error(last_error()));
    }
    if owner.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the pipe has no owner",
        ));
    }
    let mut text: *mut u16 = null_mut();
    // SAFETY: `owner` is a valid SID inside `buffer`; `text` is valid for the pointer written.
    if unsafe { ConvertSidToStringSidW(owner, &mut text) } == 0 || text.is_null() {
        return Err(os_error(last_error()));
    }
    // SAFETY: on success `text` is a NUL-terminated string allocated with `LocalAlloc`.
    let sid = unsafe { take_local_string(text) };
    Ok(owner_name(&sid).to_string())
}

/// The owner as [`trusted_owner`] compares it: the system and the administrators by their SDDL names, any other
/// owner by its full SID. The SID is read as such rather than from the descriptor's SDDL text, which writes
/// machine-relative accounts by alias (the built-in Administrator as `LA`), and those would never match the
/// window's own SID.
fn owner_name(sid: &str) -> &str {
    match sid {
        "S-1-5-18" => "SY",
        "S-1-5-32-544" => "BA",
        other => other,
    }
}

/// The SID of the user this process runs as, such as `S-1-5-21-…-1001`.
fn current_user_sid() -> Option<String> {
    let mut token: HANDLE = null_mut();
    // SAFETY: the pseudo handle of this process needs no closing; `token` is valid for the handle written.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let token = Owned(token);
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

/// Reads a NUL-terminated UTF-16 string the system allocated, then frees it the way the system says to.
///
/// # Safety
///
/// `text` must be a non-null, NUL-terminated string allocated with `LocalAlloc`, not used afterwards.
unsafe fn take_local_string(text: *mut u16) -> String {
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

/// Opens the pipe at `endpoint`, waiting a while when every instance is busy.
fn open(endpoint: &str) -> io::Result<Stream> {
    let name = wide(endpoint);
    for _ in 0..BUSY_TRIES {
        // SAFETY: `name` is NUL-terminated; no security attributes and no template file.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            return Stream::new(Arc::new(Owned(handle)));
        }
        let code = last_error();
        if code != ERROR_PIPE_BUSY {
            return Err(os_error(code));
        }
        thread::sleep(BUSY_WAIT);
    }
    Err(os_error(ERROR_PIPE_BUSY))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No one but the system, administrators and the pipe's owner may serve an instance of the core's pipe
    /// (client-core-ipc-03).
    #[test]
    fn signed_in_users_may_connect_but_not_serve_the_pipe() {
        let aces: Vec<&str> = ACCESS
            .trim_start_matches("D:P")
            .split(['(', ')'])
            .filter(|ace| !ace.is_empty())
            .collect();
        assert_eq!(aces.len(), 4, "{ACCESS}");
        for ace in aces {
            let fields: Vec<&str> = ace.split(';').collect();
            assert_eq!(fields[0], "A", "{ace}");
            let (rights, trustee) = (fields[2], fields[5]);
            if matches!(trustee, "SY" | "BA" | "OW") {
                continue;
            }
            // Everyone else gets a plain mask, never a generic right that maps to more.
            let mask = rights
                .strip_prefix("0x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .unwrap_or_else(|| panic!("{trustee} has {rights}, not a mask"));
            assert_eq!(mask, SIGNED_IN_ACCESS, "{ace}");
            assert_eq!(mask & CREATE_PIPE_INSTANCE, 0, "{ace}");
            // WRITE_DAC and WRITE_OWNER.
            assert_eq!(mask & 0x000c_0000, 0, "{ace}");
        }
    }

    #[test]
    fn only_the_system_or_administrators_serve_the_default_pipe() {
        let user = Some("S-1-5-21-1-2-3-1001");
        assert!(trusted_owner("SY", DEFAULT_ENDPOINT, user));
        assert!(trusted_owner("BA", DEFAULT_ENDPOINT, user));
        // Another user's pipe, or even this user's, is not the installed core.
        assert!(!trusted_owner(
            "S-1-5-21-1-2-3-1002",
            DEFAULT_ENDPOINT,
            user
        ));
        assert!(!trusted_owner(
            "S-1-5-21-1-2-3-1001",
            DEFAULT_ENDPOINT,
            user
        ));
        assert!(!trusted_owner(
            "S-1-5-21-1-2-3-1001",
            &DEFAULT_ENDPOINT.to_ascii_lowercase(),
            user
        ));
        // A core this user runs in a console on a pipe of its own.
        let test = r"\\.\pipe\anixops-net-test";
        assert!(trusted_owner("S-1-5-21-1-2-3-1001", test, user));
        assert!(!trusted_owner("S-1-5-21-1-2-3-1002", test, user));
        assert!(!trusted_owner("S-1-5-21-1-2-3-1001", test, None));
    }

    #[test]
    fn the_owner_is_named_by_its_sid() {
        assert_eq!(owner_name("S-1-5-18"), "SY");
        assert_eq!(owner_name("S-1-5-32-544"), "BA");
        // The built-in Administrator (RID 500) stays a full SID, so it can match the window's own SID.
        assert_eq!(owner_name("S-1-5-21-1-2-3-500"), "S-1-5-21-1-2-3-500");
        assert_eq!(owner_name("S-1-5-21-1-2-3-1001"), "S-1-5-21-1-2-3-1001");
    }

    /// The pipe this process serves is accepted: its owner is this process's (elevated: the administrators;
    /// otherwise this user), and the endpoint is not the default one.
    #[test]
    fn a_pipe_this_process_serves_is_accepted() {
        let endpoint = format!(r"\\.\pipe\anixops-net-owner-test-{}", std::process::id());
        let _listener = Listener::bind(&endpoint).unwrap();
        // The listener's waiting instance takes this one window.
        let stream = connect(&endpoint).unwrap();
        let owner = pipe_owner(stream.pipe.0).unwrap();
        assert!(
            owner == "BA" || owner == "SY" || current_user_sid().as_deref() == Some(owner.as_str()),
            "{owner}"
        );
    }
}
