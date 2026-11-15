//! The service's side: the windows that are connected, and the core they talk to.

use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use app_notifications::Notice;
use client_core::backend::session::UserSession;
use client_core::backend::system_proxy::{Desktop, ProxyBackup};
use client_core::client::{Client, Platform};
use client_ipc::{Connection, Handler, Peer, Role};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ask, event};

/// How long a window gets to answer. Changing the proxy settings runs a few short programs on Linux and
/// writes the registry on Windows; a window that takes longer is not answering.
const PATIENCE: Duration = Duration::from_secs(15);

/// How long a window gets where the system asks the user to confirm (trusting a root certificate): as long
/// as a person may take to read a warning.
const USER_PATIENCE: Duration = Duration::from_secs(300);

const NO_WINDOW: &str =
    "the AnixOps Net window is not open, and only it can change this user's settings";

/// What answers a window's commands: the core, or a stand-in for it in tests.
pub trait Commands: Send + Sync + 'static {
    fn invoke(&self, command: &str, args: Value) -> Result<Value, String>;

    /// A window in a user's session has connected and can be asked from now on.
    fn session_arrived(&self) {}
}

impl Commands for Client {
    fn invoke(&self, command: &str, args: Value) -> Result<Value, String> {
        Client::invoke(self, command, args)
    }

    fn session_arrived(&self) {
        Client::session_arrived(self);
    }
}

/// A window that said it runs in a user's session.
struct Session {
    peer: Arc<Peer>,
    /// What keeps the proxy settings there.
    desktop: Option<Desktop>,
}

#[derive(Deserialize)]
struct Described {
    desktop: Option<Desktop>,
}

/// The windows connected to the core.
///
/// Hand [`Windows::session`] and [`Windows::platform`] to the core when it is opened, then call
/// [`Windows::serve`] for every window that connects.
#[derive(Default)]
pub struct Windows {
    /// In the order they joined. The last one is asked: it is the window the user opened most recently.
    sessions: Mutex<Vec<Session>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Windows {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The user's session as the core sees it: whatever window is connected at the moment of each call.
    pub fn session(self: &Arc<Self>) -> Box<dyn UserSession> {
        Box::new(WindowSession(Arc::clone(self)))
    }

    /// The platform as the core sees it: notifications go to the windows.
    pub fn platform(self: &Arc<Self>) -> Arc<dyn Platform> {
        Arc::new(WindowPlatform(Arc::clone(self)))
    }

    /// Talks to one window until it leaves: greets it, answers its commands with `commands`, and asks it
    /// for the user's settings while it is there. `version` is this program's, told to the window. Returns
    /// when the channel has closed (`Ok`) or broken.
    pub fn serve(
        self: &Arc<Self>,
        commands: Arc<dyn Commands>,
        reader: impl Read,
        writer: impl Write + Send + 'static,
        version: &str,
    ) -> io::Result<()> {
        let mut connection = Connection::new(reader, writer);
        connection.greet(Role::Core, version, Role::Window)?;
        let (reader, writer) = connection.into_parts();
        let peer = Peer::new(writer);
        // The window's answer is read by the loop below, so the question is asked from another thread.
        let joining = {
            let windows = Arc::clone(self);
            let peer = Arc::clone(&peer);
            let commands = Arc::clone(&commands);
            thread::spawn(move || {
                if windows.admit(peer) {
                    commands.session_arrived();
                }
            })
        };
        let outcome = peer.run(reader, Arc::new(CoreEnd { commands }));
        // With the channel closed the question fails at once, so this does not wait on a window that left.
        let _ = joining.join();
        self.leave(&peer);
        outcome
    }

    /// Asks the program what session it runs in and keeps it when it says. One that does not (a command
    /// line tool, which only sends commands) is never asked for anything.
    fn admit(&self, peer: Arc<Peer>) -> bool {
        let Ok(answer) = peer.ask(ask::DESCRIBE, json!({}), PATIENCE) else {
            return false;
        };
        let Ok(described) = serde_json::from_value::<Described>(answer) else {
            return false;
        };
        lock(&self.sessions).push(Session {
            peer,
            desktop: described.desktop,
        });
        true
    }

    fn leave(&self, peer: &Arc<Peer>) {
        lock(&self.sessions).retain(|session| !Arc::ptr_eq(&session.peer, peer));
    }

    fn current(&self) -> Option<(Arc<Peer>, Option<Desktop>)> {
        lock(&self.sessions)
            .last()
            .map(|session| (Arc::clone(&session.peer), session.desktop))
    }

    fn ask<T: DeserializeOwned>(&self, command: &str, args: Value) -> Result<T, String> {
        self.ask_within(command, args, PATIENCE)
    }

    fn ask_within<T: DeserializeOwned>(
        &self,
        command: &str,
        args: Value,
        patience: Duration,
    ) -> Result<T, String> {
        let (peer, _) = self.current().ok_or(NO_WINDOW)?;
        let answer = peer.ask(command, args, patience)?;
        serde_json::from_value(answer).map_err(|error| {
            format!("the window's answer to {command} was not understood: {error}")
        })
    }
}

/// Answers a window with the core.
struct CoreEnd {
    commands: Arc<dyn Commands>,
}

impl Handler for CoreEnd {
    fn request(&self, command: &str, args: Value) -> Result<Value, String> {
        if command.starts_with(ask::PREFIX) {
            // These go from the core to a window, never the other way.
            return Err(format!("{command} is not a command of the core"));
        }
        self.commands.invoke(command, args)
    }
}

struct WindowSession(Arc<Windows>);

impl UserSession for WindowSession {
    fn proxy_settings(&self) -> Option<&'static str> {
        let (_, desktop) = self.0.current()?;
        desktop.map(Desktop::name)
    }

    fn apply_proxy(&self, port: u16) -> Result<ProxyBackup, String> {
        self.0.ask(ask::APPLY_PROXY, json!({ "port": port }))
    }

    fn restore_proxy(&self, backup: &ProxyBackup) -> Result<(), String> {
        self.0
            .ask::<Value>(ask::RESTORE_PROXY, json!({ "backup": backup }))
            .map(|_| ())
    }

    fn starts_after_login(&self) -> bool {
        // Without a window to ask, the answer the user sees is "no" rather than an error on every page.
        self.0
            .ask(ask::STARTS_AFTER_LOGIN, json!({}))
            .unwrap_or(false)
    }

    fn set_start_after_login(&self, enabled: bool) -> Result<(), String> {
        self.0
            .ask::<Value>(ask::SET_START_AFTER_LOGIN, json!({ "enabled": enabled }))
            .map(|_| ())
    }

    fn trusts_root(&self, certificate_pem: &str) -> Option<bool> {
        // Without a window to ask, the user sees "not trusted" and is told why when trying to change it.
        self.0
            .ask::<Option<bool>>(ask::ROOT_TRUSTED, json!({ "certificate": certificate_pem }))
            .unwrap_or(Some(false))
    }

    fn trust_root(&self, certificate_pem: &str) -> Result<(), String> {
        self.0
            .ask_within::<Value>(
                ask::TRUST_ROOT,
                json!({ "certificate": certificate_pem }),
                USER_PATIENCE,
            )
            .map(|_| ())
    }

    fn untrust_root(&self, certificate_pem: &str) -> Result<(), String> {
        self.0
            .ask_within::<Value>(
                ask::UNTRUST_ROOT,
                json!({ "certificate": certificate_pem }),
                USER_PATIENCE,
            )
            .map(|_| ())
    }
}

struct WindowPlatform(Arc<Windows>);

impl Platform for WindowPlatform {
    fn post_notice(&self, notice: &Notice) {
        let peers: Vec<Arc<Peer>> = lock(&self.0.sessions)
            .iter()
            .map(|session| Arc::clone(&session.peer))
            .collect();
        let payload = json!({
            "title": notice.title,
            "body": notice.body,
            "openUrl": notice.open_url,
        });
        for peer in peers {
            // A window that has just left misses it; the service keeps no notifications for later.
            let _ = peer.notify(event::NOTICE, payload.clone());
        }
    }
}
