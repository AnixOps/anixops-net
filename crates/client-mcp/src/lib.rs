//! The client's Model Context Protocol (MCP) server, as `docs/mcp-desktop.md` designs it.
//!
//! An AI tool on the same computer starts the server and speaks MCP with it over standard input and output
//! ([`Server`]). The server holds no data of its own: every tool asks a [`Backend`], which is the client's
//! own local channel to the running app (the named pipe on Windows, the Unix socket on Linux, the command
//! line channel on macOS). The same program serves the three desktop platforms; only the backend differs.
//!
//! - [`server`]: JSON-RPC over standard input and output, one message per line, and the MCP methods
//!   (`initialize`, `ping`, `tools/list`, `tools/call`).
//! - [`tools`]: the tools. Their names, descriptions and input schemas are data, in `tools.json`, which the
//!   other implementations and the tests read too.
//! - [`backend`]: what a tool may ask the app, and the user's access settings the app reports.
//! - [`text`]: masking of URLs and log lines.
//!
//! What a tool returns is built field by field from what the backend answers: a field the tool does not
//! name is left out, so a field added to the engine later never reaches the AI tool by accident. Requests
//! and exchanges are asked for redacted (the engine masks them as it masks an export), and the app's own
//! gate checks the same again on its side; this crate never relies on the backend alone, and the app never
//! relies on this crate.

pub mod backend;
pub mod server;
pub mod text;
pub mod tools;

pub use backend::{
    Access, Backend, BackendError, ConnectionState, ContentLevel, LogLevel, LogLine, Node,
    NodeGroup, Status,
};
pub use server::Server;

/// The tool definitions, shared with the other implementations and the tests.
pub const TOOLS_JSON: &str = include_str!("../tools.json");

/// Largest answer text one tool call returns; a larger one is refused with a hint to narrow it.
pub const MAX_RESULT_BYTES: usize = 64 * 1024;
