//! The AnixOps Net core as a Windows service.
//!
//! The service (`AnixOpsNetCore`) runs as LocalSystem, keeps the client's data under
//! `%ProgramData%\AnixOps\Net`, runs the engine, and answers the windows of the users signed in at the
//! machine over a named pipe (`docs/architecture/desktop-native.md`). The window itself never needs
//! administrator rights. What belongs to one user, the system proxy settings and the login entry, the
//! service asks of that user's window (`client-service`).
//!
//! Who may connect is the pipe's access list (`client_ipc::transport`): the system, administrators and
//! users signed in at the machine.

#![cfg(windows)]

pub mod cleanup;
pub mod data_dir;
pub mod scm;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use client_core::backend::legacy_windows;
use client_core::backend::state::Store;
use client_core::client::{Client, Config};
use client_ipc::transport::{Listener, DEFAULT_ENDPOINT};
use client_service::Windows;

/// The service's name in the service manager.
pub const SERVICE_NAME: &str = "AnixOpsNetCore";

/// Failed waits for a window in a row after which the service gives up (about a minute of failing).
pub const MAX_ACCEPT_FAILURES: u32 = 64;

/// How long to wait after the `failures`-th failed wait for a window in a row: 50 ms, doubling up to 1 s.
pub fn accept_backoff(failures: u32) -> std::time::Duration {
    let millis = 50_u64.saturating_mul(1 << failures.saturating_sub(1).min(5));
    std::time::Duration::from_millis(millis.min(1000))
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str =
    "usage: anixops-net-service [--console] [--dir <folder>] [--pipe <name>] | --close-windows | --purge";

/// `%ProgramData%`, where programs keep data that belongs to the machine.
fn program_data() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

/// Where the service keeps its data, where it listens and where the client before it kept its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub dir: PathBuf,
    pub endpoint: String,
    /// The folder of the client this one replaces; what it saved is carried over on the first start.
    pub legacy_dir: Option<PathBuf>,
    /// Run in the foreground instead of under the service manager, for looking at it by hand.
    pub console: bool,
    /// Do what the installer asks at an upgrade or a removal instead of running the service.
    pub cleanup: Option<cleanup::Cleanup>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dir: program_data().join("AnixOps").join("Net"),
            endpoint: DEFAULT_ENDPOINT.to_string(),
            legacy_dir: Some(platform_windows::managed::windows_managed_data_directory()),
            console: false,
            cleanup: None,
        }
    }
}

impl Options {
    /// From the command line, without the program's name. An argument that is not understood is an error:
    /// a service that guessed would run with the wrong folder.
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
                "--console" => options.console = true,
                "--close-windows" => options.cleanup = Some(cleanup::Cleanup::CloseWindows),
                "--purge" => options.cleanup = Some(cleanup::Cleanup::Purge),
                "--dir" => options.dir = PathBuf::from(value()?),
                "--pipe" => options.endpoint = value()?,
                _ => return Err(format!("{argument} is not understood\n{USAGE}")),
            }
        }
        Ok(options)
    }
}

/// The running service.
pub struct Service {
    client: Arc<Client>,
    windows: Arc<Windows>,
    listener: Listener,
}

impl Service {
    /// Closes the data folder to other users, carries over what the earlier client saved, opens the
    /// client's data, starts its background work and listens. Nothing is answered until [`Service::run`].
    pub fn start(options: &Options) -> io::Result<Arc<Self>> {
        // Listening comes first: a second service on the same pipe stops here, before it touches the data.
        let listener = Listener::bind(&options.endpoint)?;
        data_dir::secure(&options.dir)?;
        if let Some(old) = &options.legacy_dir {
            // Only once the folder is closed: what is carried over includes subscriptions and keys. A
            // state that cannot be carried over is not a reason to stay down; the client starts empty and
            // the old folder is still there to try again from.
            let _ = legacy_windows::migrate(old, &Store::new(options.dir.clone()));
        }
        let windows = Windows::new();
        let client = Client::open_with_session(
            Config {
                dir: options.dir.clone(),
                // Only used for the login entry, which belongs to the window, not to the service.
                executable: std::env::current_exe()
                    .unwrap_or_else(|_| PathBuf::from("anixops-net-service.exe")),
                // LocalSystem, driven by the windows of every user signed in.
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
        }))
    }

    /// Answers windows until the pipe keeps failing. Each window is served on a thread of its own.
    ///
    /// One failed wait for a window (an instance that could not be made, a thread that could not start) is
    /// waited out and tried again: one window, or anyone opening the pipe, must not stop the core for every
    /// user. Only [`MAX_ACCEPT_FAILURES`] failures in a row end it, with the last error, which the service
    /// manager then sees as a failure and restarts the service for.
    pub fn run(self: &Arc<Self>) -> io::Result<()> {
        let mut failures = 0_u32;
        loop {
            let stream = match self.listener.accept() {
                Ok(stream) => stream,
                Err(error) => {
                    failures += 1;
                    if failures >= MAX_ACCEPT_FAILURES {
                        return Err(error);
                    }
                    thread::sleep(accept_backoff(failures));
                    continue;
                }
            };
            failures = 0;
            let service = Arc::clone(self);
            // A thread that cannot start drops this window, which connects again; the service goes on.
            let _ = thread::Builder::new()
                .name("window".to_string())
                .spawn(move || {
                    // A window that breaks the channel only loses its own connection.
                    let _ = stream.try_clone().and_then(|reader| {
                        service
                            .windows
                            .serve(service.client.clone(), reader, stream, VERSION)
                    });
                });
        }
    }

    /// Stops the engine and puts the user's proxy settings back while a window is there to do it. Call
    /// before the process ends.
    pub fn shutdown(&self) {
        self.client.shutdown();
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
        let options = parse(&[]).unwrap();
        assert!(options.dir.ends_with(r"AnixOps\Net"));
        assert_eq!(options.endpoint, r"\\.\pipe\AnixOpsNet.Core");
        assert!(!options.console);
        // The new folder is not the old client's: both can be installed while the new one is tried.
        assert_ne!(options.legacy_dir.as_ref(), Some(&options.dir));
        assert!(options.legacy_dir.is_some());
    }

    #[test]
    fn folder_pipe_and_console_can_be_given() {
        let options =
            parse(&["--console", "--pipe", r"\\.\pipe\test", "--dir", r"C:\data"]).unwrap();
        assert!(options.console);
        assert_eq!(options.endpoint, r"\\.\pipe\test");
        assert_eq!(options.dir, PathBuf::from(r"C:\data"));
    }

    #[test]
    fn the_installers_requests_are_understood() {
        assert_eq!(parse(&[]).unwrap().cleanup, None);
        assert_eq!(
            parse(&["--close-windows"]).unwrap().cleanup,
            Some(cleanup::Cleanup::CloseWindows)
        );
        assert_eq!(
            parse(&["--purge"]).unwrap().cleanup,
            Some(cleanup::Cleanup::Purge)
        );
    }

    #[test]
    fn failed_waits_back_off_up_to_a_second() {
        assert_eq!(accept_backoff(1).as_millis(), 50);
        assert_eq!(accept_backoff(2).as_millis(), 100);
        assert_eq!(accept_backoff(5).as_millis(), 800);
        assert_eq!(accept_backoff(6).as_millis(), 1000);
        assert_eq!(accept_backoff(MAX_ACCEPT_FAILURES).as_millis(), 1000);
    }

    #[test]
    fn what_is_not_understood_is_refused() {
        assert!(parse(&["--verbose"]).is_err());
        assert!(parse(&["--dir"]).is_err());
        assert!(parse(&["--pipe", ""]).is_err());
        assert!(parse(&[r"C:\data"]).is_err());
    }
}
