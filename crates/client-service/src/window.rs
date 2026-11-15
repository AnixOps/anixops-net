//! The window's side, for a window written in Rust.

use client_core::backend::session::UserSession;
use client_core::backend::system_proxy::{Desktop, ProxyBackup};
use client_ipc::Handler;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::ask;

/// What takes the core's events: the event's name and what it carries.
type Events = Box<dyn Fn(&str, Value) + Send + Sync>;

/// Answers the core's questions about the session the window runs in, and passes the core's events on.
/// Give it to [`client_ipc::Peer::run`] after greeting the core as the window.
pub struct WindowEnd {
    desktop: Option<Desktop>,
    session: Box<dyn UserSession>,
    events: Events,
}

impl WindowEnd {
    /// `desktop` is what keeps the proxy settings here and `session` what changes them and the login entry
    /// (a `LocalSession` made with the same `desktop`). `events` takes what the core tells the window, one
    /// event at a time, on the thread that reads the channel: hand them to the interface's own thread and
    /// return.
    pub fn new(
        desktop: Option<Desktop>,
        session: Box<dyn UserSession>,
        events: impl Fn(&str, Value) + Send + Sync + 'static,
    ) -> Self {
        Self {
            desktop,
            session,
            events: Box::new(events),
        }
    }
}

fn argument<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    let value = args.get(name).cloned().unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|error| format!("the argument {name} is wrong: {error}"))
}

impl Handler for WindowEnd {
    fn request(&self, command: &str, args: Value) -> Result<Value, String> {
        match command {
            ask::DESCRIBE => Ok(json!({ "desktop": self.desktop })),
            ask::APPLY_PROXY => {
                let backup = self.session.apply_proxy(argument(&args, "port")?)?;
                serde_json::to_value(backup).map_err(|error| error.to_string())
            }
            ask::RESTORE_PROXY => {
                let backup: ProxyBackup = argument(&args, "backup")?;
                self.session.restore_proxy(&backup)?;
                Ok(Value::Null)
            }
            ask::STARTS_AFTER_LOGIN => Ok(json!(self.session.starts_after_login())),
            ask::SET_START_AFTER_LOGIN => {
                self.session
                    .set_start_after_login(argument(&args, "enabled")?)?;
                Ok(Value::Null)
            }
            ask::ROOT_TRUSTED => {
                let certificate: String = argument(&args, "certificate")?;
                Ok(json!(self.session.trusts_root(&certificate)))
            }
            ask::TRUST_ROOT => {
                let certificate: String = argument(&args, "certificate")?;
                self.session.trust_root(&certificate)?;
                Ok(Value::Null)
            }
            ask::UNTRUST_ROOT => {
                let certificate: String = argument(&args, "certificate")?;
                self.session.untrust_root(&certificate)?;
                Ok(Value::Null)
            }
            _ => Err(format!("{command} is not something a window does")),
        }
    }

    fn event(&self, name: &str, payload: Value) {
        (self.events)(name, payload);
    }
}
