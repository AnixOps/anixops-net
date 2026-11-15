//! The AnixOps Net core as a Linux daemon.
//!
//! The daemon runs as root from a systemd unit (`packaging/anixops-netd.service`), keeps the client's data
//! under `/var/lib/anixops-net`, runs the engine, and answers the windows of the users signed in at the
//! machine over a Unix socket (`docs/architecture/desktop-native.md`). What belongs to one user, the system
//! proxy settings and the login entry, it asks of that user's window (`client-service`).

#![cfg(target_os = "linux")]

pub mod access;

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

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

/// The running daemon.
pub struct Daemon {
    client: Arc<Client>,
    windows: Arc<Windows>,
    listener: Listener,
    access: Arc<dyn Access>,
}

impl Daemon {
    /// Opens the client's data, starts its background work and listens. Nothing is answered until
    /// [`Daemon::run`].
    pub fn start(options: &Options, access: Arc<dyn Access>) -> io::Result<Arc<Self>> {
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
        }))
    }

    /// Answers windows until the socket fails. Each window is served on a thread of its own.
    pub fn run(self: &Arc<Self>) -> io::Result<()> {
        loop {
            let stream = self.listener.accept()?;
            let daemon = Arc::clone(self);
            thread::spawn(move || {
                // A window that breaks the channel only loses its own connection.
                let _ = daemon.serve(stream);
            });
        }
    }

    fn serve(&self, stream: Stream) -> io::Result<()> {
        let reader = stream.try_clone()?;
        if self.access.allows(peer_uid(&stream)?) {
            return self
                .windows
                .serve(self.client.clone(), reader, stream, VERSION);
        }
        // Greeted and then told why, so the window can show the reason instead of a dropped connection.
        let mut connection = Connection::new(reader, stream);
        connection.greet(Role::Core, VERSION, Role::Window)?;
        let (reader, writer) = connection.into_parts();
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
