//! What an AI tool asks the desktop core over `client-ipc` (Windows and Linux, docs/mcp-desktop.md §3.2).
//!
//! The program greets the core as [`client_ipc::Role::Agent`]. The core then answers only the requests named
//! here, never a window's commands, and checks each against the user's access settings and, for engine
//! requests, against `policy.json` ([`crate::policy`]). Answers are the [`crate::backend`] types as JSON; a
//! refusal is one of the sentences of [`BackendError::message`], so this side can tell them apart.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::backend::{Action, BackendError, ACCESS_OFF, ACT_OFF, DENIED, NOT_CONNECTED, NO_ANSWER};

/// The user's access settings ([`crate::Access`]); answered whatever they are.
pub const ACCESS: &str = "agent.access";
/// The client's state ([`crate::Status`]).
pub const STATUS: &str = "agent.status";
/// Subscriptions and their nodes (a list of [`crate::NodeGroup`]).
pub const NODES: &str = "agent.nodes";
/// The newest lines of the log ([`LogsArgs`] in, a list of [`crate::LogLine`] out, oldest first).
pub const LOGS: &str = "agent.logs";
/// One engine request ([`EngineArgs`] in, the engine's answer out).
pub const ENGINE: &str = "agent.engine";
/// One action ([`ActArgs`] in, an empty object out once it is done). The core asks a window to confirm it first
/// ([`Confirmation`]), unless the user allowed actions of its kind for 15 minutes.
pub const ACT: &str = "agent.act";

/// Every request of an AI tool.
pub const CALLS: [&str; 6] = [ACCESS, STATUS, NODES, LOGS, ENGINE, ACT];

/// How long a grant of 「15 分钟内允许此类操作」 lasts.
pub const GRANT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// How long a window's confirmation waits for the user before it counts as no answer.
pub const CONFIRMATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogsArgs {
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActArgs {
    pub action: Action,
}

/// What the user answered in a window's confirmation of an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Allowed,
    Denied,
    /// The confirmation closed itself after [`CONFIRMATION_TIMEOUT`].
    NoAnswer,
}

/// A window's answer to the core's `session.confirm_action` (`{ "action": … }`, an [`Action`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confirmation {
    pub answer: Answer,
    /// With [`Answer::Allowed`]: the user also allowed actions of this kind ([`Action::kind`]) for [`GRANT`].
    #[serde(default)]
    pub for_kind: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineArgs {
    /// The engine request: `cmd` and its fields.
    pub request: Value,
}

/// The core's answer to a refused or failed request, as this program reads it.
pub fn error_from_core(message: String) -> BackendError {
    if message == ACCESS_OFF {
        BackendError::AccessOff
    } else if message == NOT_CONNECTED {
        BackendError::NotConnected
    } else if message == ACT_OFF {
        BackendError::ActOff
    } else if message == DENIED {
        BackendError::Denied
    } else if message == NO_ANSWER {
        BackendError::NoAnswer
    } else {
        BackendError::Failed(message)
    }
}

#[cfg(any(target_os = "linux", windows))]
pub use channel::IpcChannel;

#[cfg(any(target_os = "linux", windows))]
mod channel {
    use std::sync::{Arc, Mutex, PoisonError};
    use std::thread;
    use std::time::Duration;

    use client_ipc::{transport, Connection, Handler, Peer, Role};
    use serde::de::DeserializeOwned;
    use serde_json::{json, Value};

    use super::{
        error_from_core, ActArgs, EngineArgs, LogsArgs, ACCESS, ACT, ENGINE, LOGS, NODES, STATUS,
    };
    use crate::backend::{Access, Action, Backend, BackendError, LogLine, NodeGroup, Status};

    /// How long the core gets to answer: an engine request that waits on a busy engine still answers in
    /// seconds, and a core that takes longer is not answering.
    const PATIENCE: Duration = Duration::from_secs(30);

    /// How long an action may take: the window's confirmation waits a minute for the user, and connecting may
    /// take a while after.
    const ACT_PATIENCE: Duration = Duration::from_secs(150);

    /// The desktop core's channel as a [`Backend`]. One connection is kept open and made again when the core
    /// has gone away (a restart, an update).
    pub struct IpcChannel {
        endpoint: String,
        version: String,
        /// The open connection; its reader runs on a thread of its own until the core closes it.
        peer: Mutex<Option<Arc<Peer>>>,
    }

    /// The core never asks an AI tool anything.
    struct Nothing;

    impl Handler for Nothing {
        fn request(&self, command: &str, _args: Value) -> Result<Value, String> {
            Err(format!("{command} is not a command of an AI tool"))
        }
    }

    impl IpcChannel {
        /// The core at `endpoint` (`client_ipc::transport::DEFAULT_ENDPOINT` for the installed one);
        /// `version` is this program's, told to the core.
        pub fn new(endpoint: impl Into<String>, version: impl Into<String>) -> Self {
            Self {
                endpoint: endpoint.into(),
                version: version.into(),
                peer: Mutex::new(None),
            }
        }

        fn connect(&self) -> Result<Arc<Peer>, BackendError> {
            let stream =
                transport::connect(&self.endpoint).map_err(|_| BackendError::Unavailable)?;
            let reader = stream.try_clone().map_err(|_| BackendError::Unavailable)?;
            let mut connection = Connection::new(reader, stream);
            connection
                .greet(Role::Agent, &self.version, Role::Core)
                .map_err(|_| BackendError::Unavailable)?;
            let (reader, writer) = connection.into_parts();
            let peer = Peer::new(writer);
            let reading = Arc::clone(&peer);
            thread::Builder::new()
                .name("core".to_string())
                .spawn(move || {
                    let _ = reading.run(reader, Arc::new(Nothing));
                })
                .map_err(|_| BackendError::Unavailable)?;
            Ok(peer)
        }

        /// The open connection, made first when there is none or the last one closed.
        fn peer(&self) -> Result<Arc<Peer>, BackendError> {
            let mut kept = self.peer.lock().unwrap_or_else(PoisonError::into_inner);
            match kept.as_ref() {
                Some(peer) if !peer.is_closed() => Ok(Arc::clone(peer)),
                _ => {
                    let peer = self.connect()?;
                    *kept = Some(Arc::clone(&peer));
                    Ok(peer)
                }
            }
        }

        /// Drops `peer` as the connection to use, once it has closed.
        fn forget(&self, peer: &Arc<Peer>) {
            let mut kept = self.peer.lock().unwrap_or_else(PoisonError::into_inner);
            if kept.as_ref().is_some_and(|kept| Arc::ptr_eq(kept, peer)) {
                *kept = None;
            }
        }

        fn ask<T: DeserializeOwned>(&self, call: &str, args: Value) -> Result<T, BackendError> {
            self.ask_with(call, args, PATIENCE, 2)
        }

        /// `call` on the open connection, waiting `patience` for the answer. A connection the core closed since
        /// the last call is only noticed when asking on it: then the question is asked again on a new one, up to
        /// `tries` times in all.
        fn ask_with<T: DeserializeOwned>(
            &self,
            call: &str,
            args: Value,
            patience: Duration,
            tries: u32,
        ) -> Result<T, BackendError> {
            let mut tried = 0;
            let answer = loop {
                tried += 1;
                let peer = self.peer()?;
                match peer.ask(call, args.clone(), patience) {
                    Ok(answer) => break answer,
                    Err(message)
                        if peer.is_closed() || message.starts_with(client_ipc::peer::CLOSED) =>
                    {
                        self.forget(&peer);
                        if tried >= tries {
                            return Err(BackendError::Unavailable);
                        }
                    }
                    Err(message) if message == client_ipc::peer::NO_ANSWER => {
                        return Err(BackendError::Failed(
                            "AnixOps Net did not answer in time.".to_string(),
                        ))
                    }
                    Err(message) => return Err(error_from_core(message)),
                }
            };
            serde_json::from_value(answer).map_err(|_| {
                BackendError::Failed(
                    "This program cannot read the answer of AnixOps Net. Update AnixOps Net."
                        .to_string(),
                )
            })
        }
    }

    impl Backend for IpcChannel {
        fn access(&self) -> Result<Access, BackendError> {
            self.ask(ACCESS, json!({}))
        }

        fn status(&self) -> Result<Status, BackendError> {
            self.ask(STATUS, json!({}))
        }

        fn nodes(&self) -> Result<Vec<NodeGroup>, BackendError> {
            self.ask(NODES, json!({}))
        }

        fn logs(&self, limit: usize) -> Result<Vec<LogLine>, BackendError> {
            self.ask(LOGS, json!(LogsArgs { limit }))
        }

        fn engine(&self, request: &Value) -> Result<Value, BackendError> {
            self.ask(
                ENGINE,
                json!(EngineArgs {
                    request: request.clone()
                }),
            )
        }

        fn act(&self, action: &Action) -> Result<(), BackendError> {
            // Asked once: on a connection that closed while the core worked on it, the action may have been
            // taken, and asking again would ask the user, or take it, a second time.
            let _: Value = self.ask_with(
                ACT,
                json!(ActArgs {
                    action: action.clone()
                }),
                ACT_PATIENCE,
                1,
            )?;
            Ok(())
        }
    }
}
