//! The Mac app's command-line channel as a [`Backend`] (`apps/ios/Sources/NetworkCoreBridge/ControlChannel.swift`,
//! docs/macos-cli.md).
//!
//! The app listens on a Unix socket in its app group container and answers only processes of the same user. One
//! connection carries one request and one answer, each one line of JSON. Status and nodes are the commands the
//! `anixops` tool uses; the rest goes through the `agent` command, which the app answers only while the user allows
//! AI tools, and whose engine requests the app's gate (`AgentGate`) checks against `policy.json` again. An action
//! (`agent` `act`) waits for the user's answer to the app's confirmation, so its connection waits longer.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::backend::{
    Access, Action, Backend, BackendError, ConnectionState, ContentLevel, LogLevel, LogLine, Node,
    NodeGroup, Status,
};

/// `ControlProtocol.version` of the app.
pub const PROTOCOL_VERSION: u32 = 1;

/// How long an action may take: the app's confirmation waits 60 seconds for the user, and a connection that comes
/// up after that may take 40 more.
const ACT_TIMEOUT: Duration = Duration::from_secs(150);

/// Largest answer read: a capture exchange with its bodies, as the engine sends it.
const MAX_ANSWER_BYTES: u64 = 8 << 20;

pub struct ControlChannel {
    path: PathBuf,
    timeout: Duration,
}

impl ControlChannel {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn ask(&self, request: Value) -> Result<Answer, BackendError> {
        self.ask_within(request, self.timeout)
    }

    fn ask_within(&self, mut request: Value, timeout: Duration) -> Result<Answer, BackendError> {
        request["version"] = json!(PROTOCOL_VERSION);
        let mut stream = UnixStream::connect(&self.path).map_err(|_| BackendError::Unavailable)?;
        // Without timeouts a stuck app would hold the AI tool's call for ever.
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|_| BackendError::Unavailable)?;
        let mut line = request.to_string().into_bytes();
        line.push(b'\n');
        stream
            .write_all(&line)
            .map_err(|_| BackendError::Unavailable)?;
        let mut answer = Vec::new();
        BufReader::new(stream)
            .take(MAX_ANSWER_BYTES)
            .read_until(b'\n', &mut answer)
            .map_err(|error| match error.kind() {
                ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                    BackendError::Failed("AnixOps Net did not answer in time.".to_string())
                }
                _ => BackendError::Unavailable,
            })?;
        if answer.last() != Some(&b'\n') {
            return Err(if answer.is_empty() {
                BackendError::Unavailable
            } else {
                BackendError::Failed("The answer of AnixOps Net was cut off.".to_string())
            });
        }
        let answer: Answer = serde_json::from_slice(&answer).map_err(|_| {
            BackendError::Failed(
                "This program cannot read the answer of AnixOps Net. Update AnixOps Net."
                    .to_string(),
            )
        })?;
        if answer.ok {
            return Ok(answer);
        }
        Err(match answer.error.as_deref() {
            Some("accessOff") => BackendError::AccessOff,
            Some("notConnected") => BackendError::NotConnected,
            Some("actOff") => BackendError::ActOff,
            Some("denied") => BackendError::Denied,
            Some("noAnswer") => BackendError::NoAnswer,
            Some("unsupportedVersion") => BackendError::Failed(
                "This program and AnixOps Net are different versions. Use the program inside the app.".to_string(),
            ),
            _ => BackendError::Failed(
                answer
                    .message
                    .unwrap_or_else(|| "AnixOps Net refused the request.".to_string()),
            ),
        })
    }

    /// An `agent` request's JSON payload.
    fn payload(answer: Answer) -> Result<Value, BackendError> {
        let text = answer
            .payload
            .ok_or_else(|| BackendError::Failed("AnixOps Net gave no answer.".to_string()))?;
        serde_json::from_str(&text)
            .map_err(|_| BackendError::Failed("The engine's answer is not JSON.".to_string()))
    }
}

#[derive(Deserialize)]
struct Answer {
    ok: bool,
    error: Option<String>,
    message: Option<String>,
    status: Option<WireStatus>,
    groups: Option<Vec<WireGroup>>,
    access: Option<WireAccess>,
    payload: Option<String>,
}

#[derive(Deserialize)]
struct WireStatus {
    connection: String,
    node: Option<String>,
    group: Option<String>,
    mode: Option<String>,
}

#[derive(Deserialize)]
struct WireGroup {
    name: String,
    active: bool,
    nodes: Vec<WireNode>,
}

#[derive(Deserialize)]
struct WireNode {
    name: String,
    protocol: String,
    selected: bool,
    #[serde(rename = "latencyMs")]
    latency_ms: Option<u32>,
}

#[derive(Deserialize)]
struct WireAccess {
    read: bool,
    act: bool,
    content: String,
}

#[derive(Deserialize)]
struct WireLogLine {
    at_ms: u64,
    level: String,
    message: String,
}

impl Backend for ControlChannel {
    fn access(&self) -> Result<Access, BackendError> {
        let answer = self.ask(json!({ "command": "agent", "agent": "access" }))?;
        let access = answer.access.ok_or_else(|| {
            BackendError::Failed("AnixOps Net did not report its settings.".to_string())
        })?;
        Ok(Access {
            read: access.read,
            act: access.act,
            content: match access.content.as_str() {
                "headers" => ContentLevel::Headers,
                "bodies" => ContentLevel::Bodies,
                _ => ContentLevel::Summary,
            },
        })
    }

    fn status(&self) -> Result<Status, BackendError> {
        let answer = self.ask(json!({ "command": "status" }))?;
        let status = answer.status.ok_or_else(|| {
            BackendError::Failed("AnixOps Net did not report its status.".to_string())
        })?;
        Ok(Status {
            connection: match status.connection.as_str() {
                "connected" => ConnectionState::Connected,
                "connecting" | "reasserting" => ConnectionState::Connecting,
                "disconnecting" => ConnectionState::Disconnecting,
                _ => ConnectionState::Disconnected,
            },
            mode: status.mode,
            subscription: status.group,
            node: status.node,
            takeover: None,
        })
    }

    fn nodes(&self) -> Result<Vec<NodeGroup>, BackendError> {
        let answer = self.ask(json!({ "command": "nodes" }))?;
        Ok(answer
            .groups
            .unwrap_or_default()
            .into_iter()
            .map(|group| NodeGroup {
                name: group.name,
                current: group.active,
                nodes: group
                    .nodes
                    .into_iter()
                    .map(|node| Node {
                        name: node.name,
                        protocol: node.protocol,
                        latency_ms: node.latency_ms,
                        selected: node.selected,
                    })
                    .collect(),
            })
            .collect())
    }

    fn logs(&self, limit: usize) -> Result<Vec<LogLine>, BackendError> {
        let answer = self.ask(json!({ "command": "agent", "agent": "logs" }))?;
        let lines: Vec<WireLogLine> =
            serde_json::from_value(Self::payload(answer)?).map_err(|_| {
                BackendError::Failed("The log of AnixOps Net could not be read.".to_string())
            })?;
        let skip = lines.len().saturating_sub(limit);
        Ok(lines
            .into_iter()
            .skip(skip)
            .map(|line| LogLine {
                at_ms: line.at_ms,
                level: match line.level.as_str() {
                    "error" => LogLevel::Error,
                    "warn" => LogLevel::Warn,
                    "debug" => LogLevel::Debug,
                    _ => LogLevel::Info,
                },
                message: line.message,
            })
            .collect())
    }

    fn engine(&self, request: &Value) -> Result<Value, BackendError> {
        let answer = self.ask(json!({
            "command": "agent",
            "agent": "engine",
            "payload": request.to_string(),
        }))?;
        Self::payload(answer)
    }

    fn act(&self, action: &Action) -> Result<(), BackendError> {
        let action = serde_json::to_string(action)
            .map_err(|_| BackendError::Failed("The action could not be written.".to_string()))?;
        let timeout = self.timeout.max(ACT_TIMEOUT);
        self.ask_within(
            json!({ "command": "agent", "agent": "act", "payload": action }),
            timeout,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::thread;

    /// A stand-in for the app: answers each connection with the next line of `answers` and reports the requests.
    fn app(answers: Vec<Value>) -> (PathBuf, mpsc::Receiver<Value>) {
        let path = std::env::temp_dir().join(format!(
            "anixops-mcp-{}-{}.sock",
            std::process::id(),
            answers.len() * 1000 + rand_suffix()
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (sender, requests) = mpsc::channel();
        thread::spawn(move || {
            for answer in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                sender
                    .send(serde_json::from_str::<Value>(&line).unwrap())
                    .unwrap();
                let mut stream = stream;
                stream.write_all(format!("{answer}\n").as_bytes()).unwrap();
            }
        });
        (path, requests)
    }

    fn rand_suffix() -> usize {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn status_nodes_and_access_come_from_the_app() {
        let (path, requests) = app(vec![
            json!({ "ok": true, "status": { "connection": "reasserting", "node": "Node A", "group": "Provider",
                                            "mode": "rule", "appVersion": "1.0.3" } }),
            json!({ "ok": true, "groups": [{ "id": "g", "name": "Provider", "active": true, "nodes": [
                { "id": "n", "name": "Node A", "protocol": "trojan", "selected": true, "latencyMs": 42 },
                { "id": "m", "name": "Node B", "protocol": "vless", "selected": false, "latency": "timeout" },
            ]}]}),
            json!({ "ok": true, "access": { "read": true, "act": false, "content": "headers" } }),
        ]);
        let channel = ControlChannel::new(&path);

        let status = channel.status().unwrap();
        assert_eq!(status.connection, ConnectionState::Connecting);
        assert_eq!(status.node.as_deref(), Some("Node A"));
        assert_eq!(status.subscription.as_deref(), Some("Provider"));
        assert_eq!(status.mode.as_deref(), Some("rule"));
        assert_eq!(
            requests.recv().unwrap(),
            json!({ "version": 1, "command": "status" })
        );

        let groups = channel.nodes().unwrap();
        assert_eq!(groups[0].nodes[0].latency_ms, Some(42));
        assert_eq!(groups[0].nodes[1].latency_ms, None);
        assert!(groups[0].current);
        requests.recv().unwrap();

        let access = channel.access().unwrap();
        assert_eq!(
            access,
            Access {
                read: true,
                act: false,
                content: ContentLevel::Headers
            }
        );
        assert_eq!(
            requests.recv().unwrap(),
            json!({ "version": 1, "command": "agent", "agent": "access" })
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn engine_requests_travel_as_json_text() {
        let (path, requests) = app(vec![
            json!({ "ok": true, "payload": "{\"upload_rate\":1}" }),
            json!({ "ok": true, "payload": "[{\"at_ms\":1,\"level\":\"warn\",\"message\":\"a\"},{\"at_ms\":2,\"level\":\"info\",\"message\":\"b\"}]" }),
        ]);
        let channel = ControlChannel::new(&path);
        let answer = channel.engine(&json!({ "cmd": "traffic" })).unwrap();
        assert_eq!(answer, json!({ "upload_rate": 1 }));
        let request = requests.recv().unwrap();
        assert_eq!(request["command"], "agent");
        assert_eq!(request["agent"], "engine");
        assert_eq!(
            serde_json::from_str::<Value>(request["payload"].as_str().unwrap()).unwrap(),
            json!({ "cmd": "traffic" })
        );

        let lines = channel.logs(1).unwrap();
        assert_eq!(lines.len(), 1, "the newest line only");
        assert_eq!(lines[0].message, "b");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refusals_keep_their_meaning() {
        let (path, _requests) = app(vec![
            json!({ "ok": false, "error": "accessOff", "message": "off" }),
            json!({ "ok": false, "error": "notConnected", "message": "not connected" }),
            json!({ "ok": false, "error": "usage", "message": "AI tools may not use set_mode." }),
            json!({ "ok": false, "error": "unsupportedVersion" }),
        ]);
        let channel = ControlChannel::new(&path);
        assert_eq!(
            channel.engine(&json!({ "cmd": "traffic" })),
            Err(BackendError::AccessOff)
        );
        assert_eq!(
            channel.engine(&json!({ "cmd": "traffic" })),
            Err(BackendError::NotConnected)
        );
        assert_eq!(
            channel.engine(&json!({ "cmd": "set_mode" })),
            Err(BackendError::Failed(
                "AI tools may not use set_mode.".to_string()
            ))
        );
        assert!(matches!(channel.status(), Err(BackendError::Failed(_))));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn actions_travel_as_json_text_and_refusals_keep_their_meaning() {
        let (path, requests) = app(vec![
            json!({ "ok": true }),
            json!({ "ok": false, "error": "actOff", "message": "off" }),
            json!({ "ok": false, "error": "denied", "message": "denied" }),
            json!({ "ok": false, "error": "noAnswer", "message": "no answer" }),
        ]);
        let channel = ControlChannel::new(&path);
        let select = Action::SelectNode {
            node: "Node A".to_string(),
            group: None,
        };
        assert_eq!(channel.act(&select), Ok(()));
        let request = requests.recv().unwrap();
        assert_eq!(request["command"], "agent");
        assert_eq!(request["agent"], "act");
        assert_eq!(
            serde_json::from_str::<Value>(request["payload"].as_str().unwrap()).unwrap(),
            json!({ "action": "select_node", "node": "Node A" })
        );
        assert_eq!(channel.act(&Action::Connect), Err(BackendError::ActOff));
        assert_eq!(channel.act(&Action::Connect), Err(BackendError::Denied));
        assert_eq!(channel.act(&Action::Connect), Err(BackendError::NoAnswer));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn no_app_is_unavailable() {
        let channel =
            ControlChannel::new(std::env::temp_dir().join("anixops-mcp-nobody-listens.sock"))
                .with_timeout(Duration::from_secs(1));
        assert_eq!(channel.status(), Err(BackendError::Unavailable));
    }
}
