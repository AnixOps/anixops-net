//! `anixops-mcp`'s backend for Windows and Linux (`client_mcp::agent::IpcChannel`) against the service over the
//! real channel: the named pipe on Windows, a Unix socket on Linux (docs/mcp-desktop.md §9, item 5).
#![cfg(any(target_os = "linux", windows))]

use std::sync::{Arc, Mutex};
use std::thread;

use client_ipc::transport::Listener;
use client_mcp::agent::IpcChannel;
use client_mcp::{Backend, BackendError, Server};
use client_service::{Commands, Windows};
use serde_json::{json, Value};

/// An endpoint no other test or program uses.
fn endpoint(name: &str) -> String {
    let id = std::process::id();
    if cfg!(windows) {
        format!(r"\\.\pipe\anixops-net-mcp-test-{name}-{id}")
    } else {
        std::env::temp_dir()
            .join(format!("anixops-net-mcp-test-{name}-{id}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

/// Stands in for the core's gate: reading on, one subscription, the engine running; writes down what it was asked.
#[derive(Default)]
struct Core {
    asked: Mutex<Vec<(String, Value)>>,
}

impl Commands for Core {
    fn invoke(&self, command: &str, _args: Value) -> Result<Value, String> {
        Err(format!("a window's command {command} reached the core"))
    }

    fn agent(&self, call: &str, args: Value) -> Result<Value, String> {
        self.asked
            .lock()
            .unwrap()
            .push((call.to_string(), args.clone()));
        match call {
            "agent.access" => Ok(json!({ "read": true, "act": false, "content": "summary" })),
            "agent.status" => Ok(json!({
                "connection": "connected",
                "mode": "rule",
                "subscription": "Home",
                "node": "HK",
                "takeover": "system_proxy",
            })),
            "agent.nodes" => Ok(json!([{
                "name": "Home",
                "current": true,
                "nodes": [{ "name": "HK", "protocol": "shadowsocks", "latency_ms": 42, "selected": true }],
            }])),
            "agent.logs" => Ok(json!([
                { "at_ms": 1, "level": "info", "message": "Connected" },
            ])),
            "agent.engine" if args["request"]["cmd"] == "traffic" => Ok(json!({
                "upload_rate": 1, "download_rate": 2, "upload_total": 3, "download_total": 4, "active": 5,
            })),
            "agent.engine" => Err(client_mcp::NOT_CONNECTED.to_string()),
            _ => Err(format!("{call} is not a request of an AI tool")),
        }
    }
}

/// The service on `endpoint`, serving every connection as the Windows service and the Linux daemon do.
fn service(endpoint: &str, core: Arc<Core>) {
    let listener = Listener::bind(endpoint).unwrap();
    let windows = Windows::new();
    thread::spawn(move || loop {
        let Ok(stream) = listener.accept() else {
            return;
        };
        let windows = Arc::clone(&windows);
        let commands: Arc<dyn Commands> = core.clone();
        thread::spawn(move || {
            let _ = stream
                .try_clone()
                .and_then(|reader| windows.serve(commands, reader, stream, "1.0.4"));
        });
    });
}

#[test]
fn an_ai_tool_reads_the_core_over_its_channel() {
    let endpoint = endpoint("read");
    let core = Arc::new(Core::default());
    service(&endpoint, Arc::clone(&core));
    let channel = IpcChannel::new(&endpoint, "1.0.4");

    let access = channel.access().unwrap();
    assert!(access.read && !access.act);
    let status = client_mcp::tools::call(&channel, "get_status", &json!({}))
        .unwrap()
        .unwrap();
    assert_eq!(status["connection"], "connected");
    assert_eq!(status["node"], "HK");
    assert_eq!(status["traffic"]["active"], 5);
    let nodes = client_mcp::tools::call(&channel, "list_nodes", &json!({}))
        .unwrap()
        .unwrap();
    assert!(nodes.to_string().contains("\"HK\""), "{nodes}");
    let logs = client_mcp::tools::call(&channel, "get_logs", &json!({ "limit": 10 }))
        .unwrap()
        .unwrap();
    assert!(logs.to_string().contains("Connected"), "{logs}");
    // What the core answers it does not have is the tool's error, word for word.
    assert_eq!(
        channel.engine(&json!({ "cmd": "rule_hits" })),
        Err(BackendError::NotConnected)
    );

    // One connection carried every question, each one of the AI tool's own requests.
    let asked = core.asked.lock().unwrap().clone();
    assert!(asked
        .iter()
        .all(|(call, _)| client_mcp::agent::CALLS.contains(&call.as_str())));
    assert!(asked
        .iter()
        .any(|(call, args)| call == "agent.logs" && args["limit"] == 10));

    // Through the MCP server, as the AI tool sees it.
    let server = Server::new(channel);
    let answer = server
        .handle(
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "get_status", "arguments": {} },
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(answer["result"]["isError"], false, "{answer}");
}

#[test]
fn without_the_core_every_tool_says_it_is_not_running() {
    let channel = IpcChannel::new(endpoint("none"), "1.0.4");
    assert_eq!(channel.access(), Err(BackendError::Unavailable));
    let answer = client_mcp::tools::call(&channel, "get_status", &json!({})).unwrap();
    assert_eq!(answer, Err(BackendError::Unavailable.message()));
}

#[test]
fn a_core_that_comes_back_is_reached_again() {
    let endpoint = endpoint("again");
    let channel = IpcChannel::new(&endpoint, "1.0.4");
    assert_eq!(channel.access(), Err(BackendError::Unavailable));
    service(&endpoint, Arc::new(Core::default()));
    assert!(channel.access().unwrap().read);
}
