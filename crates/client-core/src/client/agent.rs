//! The gate for AI tools on Windows and Linux (docs/mcp-desktop.md §3.2, §4): what `anixops-mcp` may ask the core
//! over its channel, as `client_mcp::agent` names it.
//!
//! The program at the other end is not trusted: any program of a signed-in user can connect and call itself an
//! AI tool. So every request is checked here, not there. Nothing is answered while the user has not turned
//! reading on, except the settings themselves; an engine request must fit `policy.json`
//! (`client_mcp::policy::engine_request`), which forces redaction and dry runs; and only the requests of
//! `client_mcp::agent` exist, never a window's commands. The settings are only changed by a window.

use client_mcp::agent::{self as wire, EngineArgs, LogsArgs};
use client_mcp::{policy, LogLevel, LogLine, ACCESS_OFF, NOT_CONNECTED};
use serde_json::Value;

use super::{json, lock, Client};

/// The most log lines one request returns (`get_logs` asks for at most this many).
const MAX_LOG_LINES: usize = 500;

impl Client {
    /// Answers one request of an AI tool: `call` is one of `client_mcp::agent::CALLS`, `args` its arguments.
    pub fn agent(&self, call: &str, args: Value) -> Result<Value, String> {
        let access = self.backend().agent_access();
        if call == wire::ACCESS {
            return json(access);
        }
        if !wire::CALLS.contains(&call) {
            return Err(format!("{call} is not a request of an AI tool"));
        }
        if !access.read {
            return Err(ACCESS_OFF.to_string());
        }
        match call {
            wire::STATUS => json(self.backend().agent_status()),
            wire::NODES => {
                let delays = lock(&self.delays).clone();
                json(self.backend().agent_nodes(&delays))
            }
            wire::LOGS => {
                let LogsArgs { limit } = agent_args(args)?;
                let entries = self.backend().logs(false)?;
                let skip = entries.len().saturating_sub(limit.min(MAX_LOG_LINES));
                let lines: Vec<LogLine> = entries
                    .into_iter()
                    .skip(skip)
                    .map(|entry| LogLine {
                        at_ms: entry.time_ms,
                        level: match entry.level {
                            app_log::Level::Error => LogLevel::Error,
                            app_log::Level::Warn => LogLevel::Warn,
                            app_log::Level::Info => LogLevel::Info,
                        },
                        message: entry.message,
                    })
                    .collect();
                json(lines)
            }
            wire::ENGINE => {
                let EngineArgs { request } = agent_args(args)?;
                let request =
                    policy::engine_request(&request).map_err(|refusal| refusal.message())?;
                let backend = self.backend();
                if !backend.engine_running() {
                    return Err(NOT_CONNECTED.to_string());
                }
                backend.agent_engine_request(&request)
            }
            _ => Err(format!("{call} is not a request of an AI tool")),
        }
    }
}

fn agent_args<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|_| "The request's arguments are not valid.".to_string())
}
