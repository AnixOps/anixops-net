//! The AnixOps Net core as a Linux daemon.
//!
//! The daemon runs as root from a systemd unit (`packaging/anixops-netd.service`), keeps the client's data
//! under `/var/lib/anixops-net`, runs the engine, and answers the windows of the users signed in at the
//! machine over a Unix socket (`docs/architecture/desktop-native.md`). What belongs to one user, the system
//! proxy settings and the login entry, it asks of that user's window (`client-service`).

#![cfg(target_os = "linux")]

pub mod access;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use client_core::client::{Client, Config};
use client_ipc::transport::{peer_uid, Listener, Stream, DEFAULT_ENDPOINT};
use client_ipc::{Connection, Handler, Peer, Role};
use client_service::Windows;
use serde_json::Value;

pub use access::{Access, SignedIn};

/// Where the daemon keeps the client's data; the unit's `StateDirectory`.
pub const DEFAULT_DIR: &str = "/var/lib/anixops-net";

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "usage: anixops-netd [--dir <folder>] [--socket <path>]";

/// Where the daemon keeps its data and where it listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub dir: PathBuf,
    pub endpoint: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dir: PathBuf::from(DEFAULT_DIR),
            endpoint: DEFAULT_ENDPOINT.to_string(),
        }
    }
}

impl Options {
    /// From the command line, without the program's name. An argument that is not understood is an error:
    /// a daemon that guessed would run with the wrong folder.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let mut value = || {
                arguments
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| format!("{argument} needs a value\n{USAGE}"))
            };
            match argument.as_str() {
                "--dir" => options.dir = PathBuf::from(value()?),
                "--socket" => options.endpoint = value()?,
                _ => return Err(format!("{argument} is not understood\n{USAGE}")),
            }
        }
        Ok(options)
    }
}

/// A folder only its owner can enter, made when missing. What is in it (subscriptions, keys, captured
/// traffic) is not for other users of the machine.
fn private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

/// The folder the socket is in: any user may look in it, only the daemon may change it.
fn socket_dir(endpoint: &str) -> io::Result<()> {
    let Some(parent) = Path::new(endpoint).parent() else {
        return Ok(());
    };
    if parent.exists() {
        return Ok(());
    }
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755))
}

/// How many connections the daemon keeps at once and how long it waits on one. Every connection is a
/// thread and two descriptors in the process that also runs the engine; without limits anyone who can reach
/// the socket (it is open to every local user) could use them all up and stop the core for everybody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Connections of one user other than root at once. A user runs one window, a tray and a command line
    /// tool or two.
    pub per_user: usize,
    /// Connections at once, root's included.
    pub total: usize,
    /// How long a connection has to introduce itself.
    pub greeting: Duration,
    /// How long the connection of a user who may not control the daemon stays without a request.
    pub refused_idle: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_user: 16,
            total: 128,
            greeting: Duration::from_secs(5),
            refused_idle: Duration::from_secs(30),
        }
    }
}

/// The connections open now, by user.
#[derive(Default)]
struct Connections {
    by_user: HashMap<u32, usize>,
    total: usize,
}

/// One connection counted in [`Connections`]; dropping it frees its place.
struct Slot {
    connections: Arc<Mutex<Connections>>,
    uid: u32,
}

impl Slot {
    /// A place for a connection of `uid`, or none when the limits are reached.
    fn take(connections: &Arc<Mutex<Connections>>, uid: u32, limits: &Limits) -> Option<Self> {
        let mut open = connections.lock().unwrap_or_else(PoisonError::into_inner);
        let mine = open.by_user.get(&uid).copied().unwrap_or(0);
        if open.total >= limits.total || (uid != 0 && mine >= limits.per_user) {
            return None;
        }
        open.total += 1;
        open.by_user.insert(uid, mine + 1);
        Some(Self {
            connections: Arc::clone(connections),
            uid,
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut open = self
            .connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        open.total = open.total.saturating_sub(1);
        if let Some(mine) = open.by_user.get_mut(&self.uid) {
            *mine = mine.saturating_sub(1);
            if *mine == 0 {
                open.by_user.remove(&self.uid);
            }
        }
    }
}

/// Whether an error of `accept` means the listening socket itself is broken, rather than that this one
/// connection failed or the process is short of descriptors or memory for a moment (`EMFILE`, `ENFILE`,
/// `ENOBUFS`, `ENOMEM`, `ECONNABORTED`, `EPROTO`, ...), which is waited out.
pub fn listener_broken(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EBADF | libc::EINVAL | libc::ENOTSOCK | libc::EOPNOTSUPP | libc::EFAULT)
    )
}

/// How long to wait after the `failures`-th failed `accept` in a row: 50 ms, doubling up to 1 s. Without a
/// wait a full descriptor table makes `accept` fail at once, again and again, with the CPU spinning.
pub fn accept_backoff(failures: u32) -> Duration {
    let millis = 50_u64.saturating_mul(1 << failures.saturating_sub(1).min(5));
    Duration::from_millis(millis.min(1000))
}

/// Raises this process's soft limit of open descriptors to its hard limit. The engine holds two for every
/// proxied connection besides the windows' and the listeners'; the usual soft limit of 1024 runs out with a
/// few hundred connections. Call before any thread starts. Answers the soft limit now in force.
pub fn raise_descriptor_limit() -> io::Result<libc::rlim_t> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is valid for the structure written.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if limit.rlim_cur < limit.rlim_max {
        let raised = libc::rlimit {
            rlim_cur: limit.rlim_max,
            rlim_max: limit.rlim_max,
        };
        // SAFETY: `raised` is a valid structure; only the soft limit changes, up to the hard one.
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } != 0 {
            return Err(io::Error::last_os_error());
        }
        limit = raised;
    }
    Ok(limit.rlim_cur)
}

/// The running daemon.
pub struct Daemon {
    client: Arc<Client>,
    windows: Arc<Windows>,
    listener: Listener,
    access: Arc<dyn Access>,
    limits: Limits,
    connections: Arc<Mutex<Connections>>,
}

impl Daemon {
    /// Opens the client's data, starts its background work and listens. Nothing is answered until
    /// [`Daemon::run`].
    pub fn start(options: &Options, access: Arc<dyn Access>) -> io::Result<Arc<Self>> {
        Self::start_with(options, access, Limits::default())
    }

    /// [`Daemon::start`] with other [`Limits`], for tests.
    pub fn start_with(
        options: &Options,
        access: Arc<dyn Access>,
        limits: Limits,
    ) -> io::Result<Arc<Self>> {
        private_dir(&options.dir)?;
        socket_dir(&options.endpoint)?;
        // Listening comes first: a second daemon on the same socket stops here, before it touches the data.
        let listener = Listener::bind(&options.endpoint)?;
        let windows = Windows::new();
        let client = Client::open_with_session(
            Config {
                dir: options.dir.clone(),
                // Only used for the login entry, which belongs to the window, not to the daemon.
                executable: std::env::current_exe()
                    .unwrap_or_else(|_| PathBuf::from("anixops-netd")),
                // Root, driven by the windows of every user signed in.
                shared_service: true,
            },
            windows.platform(),
            windows.session(),
        );
        client.start_background();
        Ok(Arc::new(Self {
            client,
            windows,
            listener,
            access,
            limits,
            connections: Arc::default(),
        }))
    }

    /// Answers windows until the socket itself breaks. Each window is served on a thread of its own, within
    /// the [`Limits`]. A failed `accept` that is not the socket's fault (the descriptors used up for a
    /// moment, a connection aborted before it was taken) is waited out, never the end of the daemon.
    pub fn run(self: &Arc<Self>) -> io::Result<()> {
        let mut failures = 0_u32;
        loop {
            let stream = match self.listener.accept() {
                Ok(stream) => stream,
                Err(error) if listener_broken(&error) => return Err(error),
                Err(_) => {
                    failures = failures.saturating_add(1);
                    thread::sleep(accept_backoff(failures));
                    continue;
                }
            };
            failures = 0;
            // Who connected is read at once; a connection over the limits is closed before it costs a thread.
            let Ok(uid) = peer_uid(&stream) else {
                continue;
            };
            let Some(slot) = Slot::take(&self.connections, uid, &self.limits) else {
                continue;
            };
            let daemon = Arc::clone(self);
            // A thread that cannot start drops the connection and its place; the window connects again.
            let spawned = thread::Builder::new()
                .name("window".to_string())
                .spawn(move || {
                    let _slot = slot;
                    // A window that breaks the channel only loses its own connection.
                    let _ = daemon.serve(stream, uid);
                });
            if spawned.is_err() {
                thread::sleep(accept_backoff(1));
            }
        }
    }

    fn serve(&self, stream: Stream, uid: u32) -> io::Result<()> {
        // A connection that does not introduce itself in time is closed. The timeout belongs to the socket,
        // so it holds for the reading clone too.
        stream.set_read_timeout(Some(self.limits.greeting))?;
        let reader = stream.try_clone()?;
        let mut connection = Connection::new(reader, stream);
        connection.greet(Role::Core, VERSION, Role::Window)?;
        let (reader, writer) = connection.into_parts();
        if self.access.allows(uid) {
            // A window may stay quiet for as long as it is open.
            writer.set_read_timeout(None)?;
            return self
                .windows
                .serve_greeted(self.client.clone(), reader, writer);
        }
        // Greeted and then told why, so the window can show the reason instead of a dropped connection;
        // one that asks nothing for a while is closed, and it counts in the user's limit meanwhile.
        writer.set_read_timeout(Some(self.limits.refused_idle))?;
        Peer::new(writer).run(reader, Arc::new(Refused))
    }

    /// Stops the engine, puts the user's proxy settings back while a window is there to do it, and removes
    /// the socket. Call before the process ends.
    pub fn shutdown(&self) {
        self.client.shutdown();
        let _ = fs::remove_file(self.listener.path());
    }
}

/// Answers a user who may not control the daemon.
struct Refused;

impl Handler for Refused {
    fn request(&self, _command: &str, _args: Value) -> Result<Value, String> {
        Err(access::REFUSED.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Options, String> {
        Options::parse(arguments.iter().map(|argument| argument.to_string()))
    }

    #[test]
    fn without_arguments_the_system_places_are_used() {
        assert_eq!(
            parse(&[]),
            Ok(Options {
                dir: PathBuf::from("/var/lib/anixops-net"),
                endpoint: "/run/anixops-net/core.sock".to_string(),
            })
        );
    }

    #[test]
    fn folder_and_socket_can_be_given() {
        assert_eq!(
            parse(&["--socket", "/tmp/core.sock", "--dir", "/tmp/data"]),
            Ok(Options {
                dir: PathBuf::from("/tmp/data"),
                endpoint: "/tmp/core.sock".to_string(),
            })
        );
    }

    #[test]
    fn connections_are_limited_per_user_and_in_all() {
        let limits = Limits {
            per_user: 2,
            total: 3,
            ..Limits::default()
        };
        let open = Arc::new(Mutex::new(Connections::default()));
        let first = Slot::take(&open, 1000, &limits).unwrap();
        let second = Slot::take(&open, 1000, &limits).unwrap();
        // A third of the same user is refused; another user still gets one.
        assert!(Slot::take(&open, 1000, &limits).is_none());
        let other = Slot::take(&open, 1001, &limits).unwrap();
        // The total is reached: not even root gets one.
        assert!(Slot::take(&open, 0, &limits).is_none());
        // A connection that ends frees its place.
        drop(first);
        assert!(Slot::take(&open, 1000, &limits).is_some());
        drop((second, other));
        let open = open.lock().unwrap();
        assert_eq!(open.total, 0);
        assert!(open.by_user.is_empty());
    }

    #[test]
    fn root_is_held_to_the_total_only() {
        let limits = Limits {
            per_user: 1,
            total: 3,
            ..Limits::default()
        };
        let open = Arc::new(Mutex::new(Connections::default()));
        let held: Vec<Slot> = (0..3)
            .map(|_| Slot::take(&open, 0, &limits).unwrap())
            .collect();
        assert!(Slot::take(&open, 0, &limits).is_none());
        drop(held);
    }

    #[test]
    fn running_out_of_descriptors_is_waited_out() {
        for code in [
            libc::EMFILE,
            libc::ENFILE,
            libc::ENOBUFS,
            libc::ENOMEM,
            libc::ECONNABORTED,
            libc::EPROTO,
            libc::EINTR,
        ] {
            assert!(
                !listener_broken(&io::Error::from_raw_os_error(code)),
                "{code}"
            );
        }
        for code in [libc::EBADF, libc::EINVAL, libc::ENOTSOCK] {
            assert!(
                listener_broken(&io::Error::from_raw_os_error(code)),
                "{code}"
            );
        }
        assert_eq!(accept_backoff(1), Duration::from_millis(50));
        assert_eq!(accept_backoff(3), Duration::from_millis(200));
        assert_eq!(accept_backoff(40), Duration::from_secs(1));
    }

    #[test]
    fn the_descriptor_limit_is_raised_to_the_hard_one() {
        let soft = raise_descriptor_limit().unwrap();
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `limit` is valid for the structure written.
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
            0
        );
        assert_eq!(limit.rlim_cur, limit.rlim_max);
        assert_eq!(soft, limit.rlim_cur);
    }

    #[test]
    fn what_is_not_understood_is_refused() {
        assert!(parse(&["--verbose"]).is_err());
        assert!(parse(&["--dir"]).is_err());
        assert!(parse(&["--socket", ""]).is_err());
        assert!(parse(&["/tmp/data"]).is_err());
    }

    #[test]
    fn the_data_folder_is_closed_to_other_users() {
        let dir = std::env::temp_dir().join(format!("anixops-netd-dir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        private_dir(&dir).unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        // One left open by an earlier version is closed again.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        private_dir(&dir).unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
