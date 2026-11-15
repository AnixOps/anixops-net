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
    CloseHandle, GetLastError, LocalFree, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE,
    HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::CreateEventW;
use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

/// Where the service listens.
pub const DEFAULT_ENDPOINT: &str = r"\\.\pipe\AnixOpsNet.Core";

/// Who may open the pipe: the system and administrators in full, users signed in at this machine to read
/// and write. Nobody else, and nothing inherited.
const ACCESS: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

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
        let pipe = waiting.0;
        let connected = overlapped(pipe, &ready, |state| {
            // SAFETY: `pipe` is an instance of this listener's pipe and `state` lives until the operation
            // has finished, which `overlapped` waits for.
            unsafe { ConnectNamedPipe(pipe, state) }
        });
        match connected {
            // A window that connected between two calls is reported as already there.
            Ok(_) | Err(ERROR_PIPE_CONNECTED) => {}
            Err(code) => return Err(os_error(code)),
        }
        let next = instance(&self.name, &self.security, false)?;
        let connected = std::mem::replace(&mut *waiting, next);
        Stream::new(Arc::new(connected))
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

/// Connects to the core at `endpoint`. The core learns who the caller is but cannot act as them.
pub fn connect(endpoint: &str) -> io::Result<Stream> {
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
