//! A Unix socket in the file system.

use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// Where the daemon listens. `/run` is cleared at boot and only root creates folders there.
pub const DEFAULT_ENDPOINT: &str = "/run/anixops-net/core.sock";

/// The socket file's mode: any local user may connect, and what a user may do is decided from
/// [`peer_uid`]. The folder the file is in can narrow who reaches it.
const SOCKET_MODE: u32 = 0o666;

pub type Stream = UnixStream;

pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
}

impl Listener {
    /// Listens at `endpoint`, a path. A socket file left by a run that did not clean up is replaced; one
    /// that another program still listens on is an error.
    pub fn bind(endpoint: &str) -> io::Result<Self> {
        let path = PathBuf::from(endpoint);
        if path.exists() {
            if UnixStream::connect(&path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("another program is listening on {endpoint}"),
                ));
            }
            fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(SOCKET_MODE))?;
        Ok(Self { listener, path })
    }

    /// Waits for the next window.
    pub fn accept(&self) -> io::Result<Stream> {
        self.listener.accept().map(|(stream, _)| stream)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn connect(endpoint: &str) -> io::Result<Stream> {
    UnixStream::connect(endpoint)
}

/// The user id of the program at the other end, as the kernel recorded it when the connection was made.
pub fn peer_uid(stream: &Stream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `credentials` and `length` are valid for writes of the sizes passed, and the descriptor is
    // open for as long as `stream` is borrowed.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::addr_of_mut!(credentials).cast(),
            &mut length,
        )
    };
    if result == 0 {
        Ok(credentials.uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> String {
        std::env::temp_dir()
            .join(format!(
                "anixops-net-unix-{name}-{}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn the_core_learns_who_connected() {
        let endpoint = path("uid");
        let listener = Listener::bind(&endpoint).unwrap();
        let _window = connect(&endpoint).unwrap();
        let stream = listener.accept().unwrap();
        // SAFETY: getuid has no preconditions.
        assert_eq!(peer_uid(&stream).unwrap(), unsafe { libc::getuid() });
    }

    #[test]
    fn the_socket_file_is_open_to_local_users_and_removed_afterwards() {
        let endpoint = path("mode");
        let listener = Listener::bind(&endpoint).unwrap();
        let mode = fs::metadata(listener.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, SOCKET_MODE);
        drop(listener);
        assert!(!Path::new(&endpoint).exists());
    }

    #[test]
    fn a_socket_file_nobody_listens_on_is_replaced() {
        let endpoint = path("stale");
        // What a run that was killed leaves behind.
        drop(UnixListener::bind(&endpoint).unwrap());
        assert!(Path::new(&endpoint).exists());
        let listener = Listener::bind(&endpoint).unwrap();
        assert!(connect(&endpoint).is_ok());
        drop(listener);
    }
}
