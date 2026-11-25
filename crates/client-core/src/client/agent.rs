//! The gate for AI tools on Windows and Linux (docs/mcp-desktop.md §3.2, §4): what `anixops-mcp` may ask the core
//! over its channel, as `client_mcp::agent` names it.
//!
//! The program at the other end is not trusted: any program of a signed-in user can connect and call itself an
//! AI tool. So every request is checked here, not there. Nothing is answered while the user has not turned
//! reading on, except the settings themselves; an engine request must fit `policy.json`
//! (`client_mcp::policy::engine_request`), which forces redaction and dry runs; and only the requests of
//! `client_mcp::agent` exist, never a window's commands. The settings are only changed by a window.
//!
//! An action (`agent.act`) needs the user's switch for actions as well, and the user's confirmation in a window
//! (`Platform::confirm_action`, `session.confirm_action`), unless the user allowed actions of its kind for 15
//! minutes. Without a window there is no one to ask, and the action is refused. The confirmation is asked with
//! the backend unlocked, so the window and the core go on working while the user decides.

use std::collections::{HashMap, VecDeque};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use client_mcp::agent::{self as wire, ActArgs, Answer, EngineArgs, LogsArgs};
use client_mcp::{
    policy, Action, LogLevel, LogLine, Mode, ACCESS_OFF, ACT_OFF, DENIED, NOT_CONNECTED, NO_ANSWER,
};
use serde::Serialize;
use serde_json::Value;

use super::{engine, json, lock, node_delays, Client, RoutingMode, DELAY_URL};

/// The most log lines one request returns (`get_logs` asks for at most this many).
const MAX_LOG_LINES: usize = 500;

/// The most entries the access log keeps; older ones are dropped.
pub const ACCESS_LOG_LIMIT: usize = 200;

/// What the core keeps about AI tools while it runs: grants and the access log.
#[derive(Default)]
pub struct AgentMemory {
    /// Until when each kind of action (`Action::kind`) is allowed without asking.
    grants: HashMap<&'static str, Instant>,
    log: VecDeque<AccessEntry>,
}

/// One request of an AI tool and what came of it, for the window's access log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccessEntry {
    /// Unix time in milliseconds.
    pub at_ms: u64,
    /// The request: `status`, `nodes`, `logs`, an engine command, or an action's kind.
    pub call: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Answered, or the action was taken after the user allowed it.
    Allowed,
    /// The action was taken under a 15-minute grant, without asking.
    Granted,
    /// The user refused the action.
    Denied,
    /// The user did not answer in time.
    NoAnswer,
    /// The user's settings do not allow it, the gate refused it, or no window could ask the user.
    Refused,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

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
        let name = log_name(call, &args);
        if !access.read {
            self.record(name, Outcome::Refused);
            return Err(ACCESS_OFF.to_string());
        }
        if call == wire::ACT {
            return self.act(args, name);
        }
        let answer = self.read(call, args);
        self.record(
            name,
            match &answer {
                Err(message) if refused(message) => Outcome::Refused,
                _ => Outcome::Allowed,
            },
        );
        answer
    }

    /// The access log, oldest first (window command `agent_access_log`).
    pub fn agent_access_log(&self) -> Vec<AccessEntry> {
        lock(&self.agent).log.iter().cloned().collect()
    }

    pub fn clear_agent_access_log(&self) {
        lock(&self.agent).log.clear();
    }

    /// Ends every 15-minute grant: the user turned actions off.
    pub fn end_agent_grants(&self) {
        lock(&self.agent).grants.clear();
    }

    fn record(&self, call: String, outcome: Outcome) {
        let mut memory = lock(&self.agent);
        memory.log.push_back(AccessEntry {
            at_ms: now_ms(),
            call,
            outcome,
        });
        while memory.log.len() > ACCESS_LOG_LIMIT {
            memory.log.pop_front();
        }
    }

    /// An action: the switch, the action itself, the user's confirmation, and then the same code as the
    /// window's command.
    fn act(&self, args: Value, name: String) -> Result<Value, String> {
        if !self.backend().agent_access().act {
            self.record(name, Outcome::Refused);
            return Err(ACT_OFF.to_string());
        }
        let action = match agent_args::<ActArgs>(args) {
            Ok(ActArgs { action }) => action,
            Err(error) => {
                self.record(name, Outcome::Refused);
                return Err(error);
            }
        };
        // A node that does not exist is an error before the user is asked about it.
        if let Action::SelectNode { node, group } = &action {
            if node.trim().is_empty() {
                self.record(name, Outcome::Refused);
                return Err(BAD_ARGUMENTS.to_string());
            }
            if let Err(error) = self.backend().agent_find_node(node, group.as_deref()) {
                self.record(name, Outcome::Refused);
                return Err(error);
            }
        }
        // What needs the engine is refused before the user is asked about it.
        let needs_engine = matches!(
            action,
            Action::SetCapture { .. } | Action::ClearCapture | Action::CloseConnection { .. }
        );
        if needs_engine && !self.backend().engine_running() {
            return Err(NOT_CONNECTED.to_string());
        }
        let kind = action.kind();
        let granted = lock(&self.agent)
            .grants
            .get(kind)
            .is_some_and(|until| Instant::now() < *until);
        if granted {
            self.record(name, Outcome::Granted);
        } else {
            match self.platform.confirm_action(&action) {
                Err(error) => {
                    self.record(name, Outcome::Refused);
                    return Err(format!(
                        "AnixOps Net could not ask the user to confirm this action: {error}. It was not taken."
                    ));
                }
                Ok(confirmation) => match confirmation.answer {
                    Answer::Denied => {
                        self.record(name, Outcome::Denied);
                        return Err(DENIED.to_string());
                    }
                    Answer::NoAnswer => {
                        self.record(name, Outcome::NoAnswer);
                        return Err(NO_ANSWER.to_string());
                    }
                    Answer::Allowed => {
                        if confirmation.for_kind {
                            lock(&self.agent)
                                .grants
                                .insert(kind, Instant::now() + wire::GRANT);
                        }
                        self.record(name, Outcome::Allowed);
                    }
                },
            }
        }
        // The user may have turned actions off while the confirmation was open.
        if !self.backend().agent_access().act {
            return Err(ACT_OFF.to_string());
        }
        self.take(&action)
            .map(|()| Value::Object(serde_json::Map::new()))
            .map_err(|error| format!("AnixOps Net could not take the action: {error}"))
    }

    /// The action, as the window's command of the same name does it.
    fn take(&self, action: &Action) -> Result<(), String> {
        match action {
            Action::Connect => self.connect_through_tunnel().map(drop),
            Action::Disconnect => {
                let result = self.backend().disconnect();
                self.close_tunnel();
                result.map(drop)
            }
            Action::SetMode { mode } => {
                let mode = match mode {
                    Mode::Rule => RoutingMode::Rule,
                    Mode::Global => RoutingMode::Global,
                    Mode::Direct => RoutingMode::Direct,
                };
                self.backend().set_routing_mode(mode).map(drop)
            }
            Action::SelectNode { node, group } => {
                let mut backend = self.backend();
                let id = backend.agent_find_node(node, group.as_deref())?;
                backend.switch_node(&id).map(drop)
            }
            Action::TestNodeDelays => {
                // The tests run with the backend unlocked, as the window's test does.
                let nodes = {
                    let backend = self.backend();
                    let ids: Vec<String> =
                        backend.nodes().into_iter().map(|node| node.id).collect();
                    backend.delay_nodes(&ids)
                };
                let results = node_delays(engine::node_delays(&nodes, DELAY_URL));
                self.remember_delays(&results);
                Ok(())
            }
            Action::SetCapture { enabled } => self
                .runtime_control(&serde_json::json!({ "cmd": "set_capture", "enabled": enabled }))
                .map(drop),
            Action::ClearCapture => self
                .runtime_control(&serde_json::json!({ "cmd": "clear_capture" }))
                .map(drop),
            Action::CloseConnection { id } => self
                .backend()
                .runtime_control(&serde_json::json!({ "cmd": "close_connection", "id": id }))
                .map(drop),
        }
    }

    /// A request that only reads.
    fn read(&self, call: &str, args: Value) -> Result<Value, String> {
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

/// Whether a read was refused by the gate (a request the policy does not allow, or malformed), not by the engine.
fn refused(message: &str) -> bool {
    message.ends_with("is not available to AI tools.")
        || message == policy::Refusal::Malformed.message()
        || message == BAD_ARGUMENTS
}

const BAD_ARGUMENTS: &str = "The request's arguments are not valid.";

/// The request as the access log shows it.
fn log_name(call: &str, args: &Value) -> String {
    let named = |value: Option<&Value>, fallback: &str| {
        value
            .and_then(Value::as_str)
            .map_or(fallback.to_string(), |name| name.chars().take(40).collect())
    };
    match call {
        wire::ENGINE => named(args.pointer("/request/cmd"), "engine"),
        wire::ACT => named(args.pointer("/action/action"), "act"),
        _ => call.trim_start_matches("agent.").to_string(),
    }
}

fn agent_args<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|_| BAD_ARGUMENTS.to_string())
}
