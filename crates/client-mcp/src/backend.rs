//! What a tool may ask the running app.
//!
//! Each platform implements [`Backend`] over its own local channel. The types here carry only what a tool
//! may show: a node is a name, a protocol and a latency, never an address or a credential, so a backend
//! cannot hand one over by mistake.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How much of a captured request the user lets AI tools see (设置 › 高级 › 外部工具访问 › 请求内容).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentLevel {
    /// Method, host, path, status, sizes and timing.
    Summary,
    /// The summary and the headers, masked.
    Headers,
    /// The summary, the headers and text bodies, masked.
    Bodies,
}

/// The user's access settings, as the app reports them. The app's gate enforces them too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Access {
    /// AI tools may read the client's state.
    pub read: bool,
    /// AI tools may ask for actions, each confirmed by the user in the app.
    pub act: bool,
    pub content: ContentLevel,
}

impl Access {
    /// Everything off, as a new installation starts.
    pub const OFF: Self = Self {
        read: false,
        act: false,
        content: ContentLevel::Summary,
    };
}

/// Why the backend gave no answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    /// The app is not running, or its channel refused or dropped this program.
    Unavailable,
    /// The user has not turned on access for AI tools; the app's gate refused.
    AccessOff,
    /// The command needs the engine, which runs only while the client is connected.
    NotConnected,
    /// Anything else: a sentence that may be shown as it is.
    Failed(String),
}

impl BackendError {
    /// The sentence the AI tool is given.
    pub fn message(&self) -> String {
        match self {
            Self::Unavailable => {
                "AnixOps Net is not running, or it refused this connection.".to_string()
            }
            Self::AccessOff => ACCESS_OFF.to_string(),
            Self::NotConnected => NOT_CONNECTED.to_string(),
            Self::Failed(message) => message.clone(),
        }
    }
}

/// What the desktop core answers on its channel when the engine is not running; [`BackendError::NotConnected`].
pub const NOT_CONNECTED: &str =
    "AnixOps Net is not connected. This information is only available while it is connected.";

/// What every tool answers while the user has not turned access on.
pub const ACCESS_OFF: &str =
    "AnixOps Net has not allowed access by AI tools. The user can turn it on in Settings, Advanced, External tool access.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

/// The client's state, as the main window shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub connection: ConnectionState,
    /// `rule`, `global` or `direct`.
    pub mode: Option<String>,
    /// The current subscription's name.
    pub subscription: Option<String>,
    /// The current node's name.
    pub node: Option<String>,
    /// How traffic reaches the client: `tun` or `system_proxy`.
    pub takeover: Option<String>,
}

/// One subscription and its nodes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeGroup {
    pub name: String,
    /// The subscription in use.
    pub current: bool,
    pub nodes: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub protocol: String,
    /// The last delay test's result; `None` when untested or failed.
    pub latency_ms: Option<u32>,
    pub selected: bool,
}

/// Most severe first, so `level <= wanted` keeps a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// Unix time in milliseconds.
    pub at_ms: u64,
    pub level: LogLevel,
    pub message: String,
}

/// The running app, seen through its local channel.
pub trait Backend {
    /// The user's access settings. Asked before every tool call, so a change in the app applies at once.
    fn access(&self) -> Result<Access, BackendError>;

    fn status(&self) -> Result<Status, BackendError>;

    fn nodes(&self) -> Result<Vec<NodeGroup>, BackendError>;

    /// The newest `limit` lines of the app's log, oldest first.
    fn logs(&self, limit: usize) -> Result<Vec<LogLine>, BackendError>;

    /// One engine runtime control request (`cmd` and its fields, as networkcore's
    /// `engine-embedded/src/control.rs` lists them) and its answer. [`BackendError::NotConnected`] while
    /// the engine is not running.
    fn engine(&self, request: &Value) -> Result<Value, BackendError>;
}
