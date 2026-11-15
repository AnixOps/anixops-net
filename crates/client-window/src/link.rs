//! The window's standing link to the core.
//!
//! The core is a daemon that may not be running yet, and that is stopped and started again by an upgrade.
//! The link keeps trying: it connects, greets the core as the window, serves the channel until it closes,
//! and then tries again. The window is told each change and shows it; it asks the core through the link
//! and gets an error at once while there is no connection.

use std::io;
use std::net::Shutdown;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use client_ipc::transport::{connect, Stream, DEFAULT_ENDPOINT};
use client_ipc::{Connection, Handler, Peer, Role};
use serde_json::Value;

const NOT_CONNECTED: &str = "the core service is not connected";
const CLOSED: &str = "the core service closed the channel";

/// Whether the window can reach the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// Not tried yet.
    Connecting,
    /// The core answers; its version.
    Connected { core_version: String },
    /// The daemon is not running, or is not one this window can talk to. `problem` is for the log and
    /// diagnostics, not to be shown as it is.
    Unavailable { problem: String },
}

/// Where the core is and how patiently it is looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkOptions {
    /// The socket the core listens on.
    pub endpoint: String,
    /// This window's version, told to the core.
    pub version: String,
    /// How long between two tries while the core is not there.
    pub retry: Duration,
    /// How long the core gets to introduce itself. A program that accepts the connection and then says
    /// nothing is not the core.
    pub greeting: Duration,
}

impl LinkOptions {
    /// The core's usual socket, tried every two seconds.
    pub fn new(version: &str) -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            version: version.to_string(),
            retry: Duration::from_secs(2),
            greeting: Duration::from_secs(5),
        }
    }
}

/// An open channel: the half that asks, and the socket itself to close it with.
struct Open {
    peer: Arc<Peer>,
    stream: Stream,
}

struct Now {
    state: LinkState,
    open: Option<Open>,
    stopping: bool,
}

struct Shared {
    options: LinkOptions,
    handler: Arc<dyn Handler>,
    changed: Box<dyn Fn(&LinkState) + Send + Sync>,
    now: Mutex<Now>,
    wake: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The link. It works on a thread of its own from [`CoreLink::start`] until it is dropped.
pub struct CoreLink {
    shared: Arc<Shared>,
}

impl CoreLink {
    /// Starts looking for the core. `handler` answers what the core asks and takes its events (a
    /// [`WindowEnd`](crate::WindowEnd)). `changed` is told every change of the link, in order, on the link's
    /// thread: hand it to the interface's own thread and return.
    pub fn start(
        options: LinkOptions,
        handler: Arc<dyn Handler>,
        changed: impl Fn(&LinkState) + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            options,
            handler,
            changed: Box::new(changed),
            now: Mutex::new(Now {
                state: LinkState::Connecting,
                open: None,
                stopping: false,
            }),
            wake: Condvar::new(),
        });
        let running = Arc::clone(&shared);
        thread::spawn(move || running.run());
        Self { shared }
    }

    /// The link as it is now.
    pub fn state(&self) -> LinkState {
        lock(&self.shared.now).state.clone()
    }

    /// Asks the core to do `command` and waits up to `patience` for the answer. Without a connection this
    /// fails at once. Not for the interface's own thread: the answer can take as long as `patience`.
    pub fn ask(&self, command: &str, args: Value, patience: Duration) -> Result<Value, String> {
        let peer = lock(&self.shared.now)
            .open
            .as_ref()
            .map(|open| Arc::clone(&open.peer));
        match peer {
            Some(peer) => peer.ask(command, args, patience),
            None => Err(NOT_CONNECTED.to_string()),
        }
    }
}

impl Drop for CoreLink {
    /// Stops looking and closes the channel. Nothing more is told to `changed`.
    fn drop(&mut self) {
        let mut now = lock(&self.shared.now);
        now.stopping = true;
        if let Some(open) = now.open.take() {
            let _ = open.stream.shutdown(Shutdown::Both);
        }
        self.shared.wake.notify_all();
    }
}

impl Shared {
    fn run(&self) {
        loop {
            let problem = match self.serve() {
                Ok(()) => CLOSED.to_string(),
                Err(error) => error.to_string(),
            };
            self.set(LinkState::Unavailable { problem });
            let now = lock(&self.now);
            let (now, _) = self
                .wake
                .wait_timeout_while(now, self.options.retry, |now| !now.stopping)
                .unwrap_or_else(PoisonError::into_inner);
            if now.stopping {
                return;
            }
        }
    }

    /// One connection, from the greeting until the channel closes (`Ok`) or breaks.
    fn serve(&self) -> io::Result<()> {
        let stream = connect(&self.options.endpoint)?;
        // The limit is the socket's, so it also holds for the copies made below.
        stream.set_read_timeout(Some(self.options.greeting))?;
        let mut connection = Connection::new(stream.try_clone()?, stream.try_clone()?);
        let core = connection.greet(Role::Window, &self.options.version, Role::Core)?;
        stream.set_read_timeout(None)?;
        let (reader, writer) = connection.into_parts();
        let peer = Peer::new(writer);
        {
            let mut now = lock(&self.now);
            if now.stopping {
                return stream.shutdown(Shutdown::Both);
            }
            now.open = Some(Open {
                peer: Arc::clone(&peer),
                stream,
            });
        }
        self.set(LinkState::Connected {
            core_version: core.version,
        });
        let outcome = peer.run(reader, Arc::clone(&self.handler));
        lock(&self.now).open = None;
        outcome
    }

    /// Records the state and tells the window when it changed. While the core stays away the reason can
    /// differ from one try to the next; the window is told once that it is away, not every reason.
    fn set(&self, state: LinkState) {
        {
            let mut now = lock(&self.now);
            if now.stopping {
                return;
            }
            let known = match (&now.state, &state) {
                (LinkState::Unavailable { .. }, LinkState::Unavailable { .. }) => true,
                (old, new) => old == new,
            };
            now.state = state.clone();
            if known {
                return;
            }
        }
        (self.changed)(&state);
    }
}
