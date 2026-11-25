//! What the tools show of an app whose answers carry secrets everywhere a careless tool could pass them
//! on: no answer, at any content level, may contain a canary.
//!
//! The fake engine answers like networkcore's runtime control (`engine-embedded/src/control.rs`), with an
//! extra canary field in every object, credentials in URLs, headers, bodies, stream messages and log
//! lines, and health text that names a server address. It masks what the real engine masks for
//! `redact: true` (headers and query values named like credentials) but leaves the rest, so the tests
//! show what this crate removes on its own.

use std::cell::RefCell;

use base64::Engine as _;
use client_mcp::backend::ACCESS_OFF;
use client_mcp::{
    Access, Backend, BackendError, ConnectionState, ContentLevel, LogLevel, LogLine, Node,
    NodeGroup, Server, Status,
};
use serde_json::{json, Value};

const CANARY: &str = "CANARY";

struct Fake {
    access: Access,
    /// Answer `capture_exchange` as an engine that does not know `redact`.
    old_engine: bool,
    asked: RefCell<Vec<Value>>,
}

impl Fake {
    fn new(content: ContentLevel) -> Self {
        Self {
            access: Access {
                read: true,
                act: false,
                content,
            },
            old_engine: false,
            asked: RefCell::new(Vec::new()),
        }
    }
}

fn base64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

impl Backend for Fake {
    fn access(&self) -> Result<Access, BackendError> {
        Ok(self.access)
    }

    fn status(&self) -> Result<Status, BackendError> {
        Ok(Status {
            connection: ConnectionState::Connected,
            mode: Some("rule".to_string()),
            subscription: Some("Provider".to_string()),
            node: Some("Node A".to_string()),
            takeover: Some("tun".to_string()),
        })
    }

    fn nodes(&self) -> Result<Vec<NodeGroup>, BackendError> {
        Ok(vec![NodeGroup {
            name: "Provider".to_string(),
            current: true,
            nodes: vec![Node {
                name: "Node A".to_string(),
                protocol: "trojan".to_string(),
                latency_ms: Some(42),
                selected: true,
            }],
        }])
    }

    fn logs(&self, _limit: usize) -> Result<Vec<LogLine>, BackendError> {
        Ok(vec![
            LogLine {
                at_ms: 1,
                level: LogLevel::Debug,
                message: "debug detail".to_string(),
            },
            LogLine {
                at_ms: 2,
                level: LogLevel::Warn,
                message: format!(
                    "subscription update from https://user:{CANARY}@sub.test/api?token={CANARY} failed"
                ),
            },
            LogLine {
                at_ms: 3,
                level: LogLevel::Error,
                message: format!("see \"https://b.test/x#{CANARY}\""),
            },
        ])
    }

    fn engine(&self, request: &Value) -> Result<Value, BackendError> {
        self.asked.borrow_mut().push(request.clone());
        let secret = json!(CANARY);
        let answer = match request["cmd"].as_str().unwrap() {
            "traffic" => json!({
                "upload_rate": 1, "download_rate": 2, "upload_total": 3, "download_total": 4, "active": 1,
                "secret": secret,
            }),
            "groups" => json!({ "groups": [{
                "id": "Proxy", "type": "select", "now": "Node A", "secret": secret,
                "members": [{ "name": "Node A", "latency_ms": 42, "server": secret }],
            }]}),
            "connections" => json!({
                "traffic": { "upload_rate": 1, "download_rate": 2, "upload_total": 3, "download_total": 4, "active": 1, "secret": secret },
                "active": [{
                    "id": 1, "network": "tcp", "host": "api.test", "process": "curl", "destination": "api.test:443",
                    "rule": "DOMAIN-SUFFIX,api.test", "chain": ["Proxy", "Node A", { "server": secret }],
                    "outbound": "Node A", "started_at_ms": 10, "ended_at_ms": null, "upload": 5, "download": 6,
                    "error": null, "captured": true, "secret": secret,
                    "redirect": { "destination": "b.test:443", "rule": "redirect", "secret": secret },
                }],
                "closed": [{
                    "id": 2, "network": "udp", "host": "old.test", "destination": "old.test:443", "rule": "FINAL",
                    "chain": ["DIRECT"], "outbound": "DIRECT", "started_at_ms": 5, "ended_at_ms": 9,
                    "upload": 0, "download": 0, "error": "connect_failed", "secret": secret,
                }],
            }),
            "test_route" => json!({
                "mode": "rule",
                "test": { "rule_index": 0, "rule": "DOMAIN-SUFFIX,api.test", "chain": ["Proxy", "Node A"],
                          "outbound": "Node A", "secret": secret },
                "secret": secret,
            }),
            "rule_hits" => json!({
                "generation": 3,
                "rules": [
                    { "rule": "DOMAIN-SUFFIX,api.test", "target": "Proxy", "hits": 4, "last_hit_at_ms": 10, "secret": secret },
                    { "rule": "DOMAIN,never.test", "target": "DIRECT", "hits": 0 },
                ],
                "final": { "rule": "FINAL", "target": "DIRECT", "hits": 1, "secret": secret },
            }),
            "reload_rules" => {
                assert_eq!(request["dry_run"], true, "check_rules never applies rules");
                if request["rules"][0] == "BAD" {
                    json!({ "reloaded": false, "error": { "line": 0, "code": "syntax", "text": secret } })
                } else {
                    json!({ "reloaded": false, "rules": 2, "generation": 3, "secret": secret })
                }
            }
            "outbound_health" => json!({ "outbounds": [{
                "node": "Node A", "protocol": "wireguard", "server": secret,
                "health": { "running": true, "last_handshake_secs": 3,
                            "last_error": format!("handshake with {CANARY}:51820 timed out") },
            }]}),
            "rule_sets" => json!({
                "rule_sets": [{ "name": "geoip-cn", "source": "remote", "entries": 10, "url": secret }],
                "missing": ["asn-1", { "url": secret }],
            }),
            "mitm_hosts" => json!({ "enabled": true, "secret": secret, "hosts": [
                { "host": "api.test", "decrypted": 3, "failed": 0, "secret": secret },
                { "host": "pinned.test", "decrypted": 0, "failed": 2, "last_failure": "client_closed" },
            ]}),
            "capture" => json!({
                "enabled": true, "max_total_bytes": 100, "used_bytes": 10, "connections": 1,
                "dropped_exchanges": 0, "dropped_bodies": 0, "run_session": secret,
            }),
            "capture_requests" => {
                assert_eq!(
                    request["redact"], true,
                    "requests are always asked for redacted"
                );
                if request["filter"] == "(" {
                    json!({ "error": { "code": "invalid_filter", "at": 0 } })
                } else {
                    // The real engine masks `token`; a parameter it does not know stays, and the tool must
                    // still leave its value out.
                    json!({
                        "requests": [{
                            "seq": 1, "connection_id": 1, "index": 0, "method": "GET",
                            "url": format!("https://u:{CANARY}@api.test/v1/items?token=<redacted>&page={CANARY}#{CANARY}"),
                            "host": "api.test", "status": 200, "http_version": "h2", "started_at_ms": 10,
                            "ended_at_ms": 11, "request_size": 0, "response_size": 7,
                            "response_content_type": "application/json", "state": "done", "secret": secret,
                        }],
                        "last_seq": 1, "last_rev": 1, "dropped_before_seq": 0, "secret": secret,
                    })
                }
            }
            "capture_exchange" => {
                assert_eq!(
                    request["redact"], true,
                    "exchanges are always asked for redacted"
                );
                if request["index"] != 0 {
                    json!({ "exchange": null })
                } else {
                    json!({ "exchange": {
                        "connection_id": 1, "index": 0, "redacted": !self.old_engine, "secret": secret,
                        "method": "POST", "url": format!("https://api.test/login?page={CANARY}"), "status": 200,
                        "http_version": "h2", "started_at_ms": 10, "ended_at_ms": 11, "state": "done",
                        "messages": { "seen": 1, "dropped": 0, "kept": [{ "index": 0, "text": secret }] },
                        "request": {
                            "headers": [
                                ["Content-Type", "application/json"],
                                ["Authorization", format!("Bearer {CANARY}")],
                                ["X-Odd", { "v": secret }],
                            ],
                            "body": { "size": 17, "truncated": false, "data_base64": base64("{\"user\":\"alice\"}") },
                            "secret": secret,
                        },
                        "response": {
                            "headers": [["Content-Type", "image/png"], ["Set-Cookie", format!("sid={CANARY}")]],
                            "body": { "size": 6, "truncated": false, "data_base64": base64(CANARY) },
                        },
                    }})
                }
            }
            other => panic!("the tools asked the engine for {other}"),
        };
        Ok(answer)
    }
}

/// Every tool with arguments that reach every branch above.
fn calls() -> Vec<(&'static str, Value)> {
    vec![
        ("get_status", json!({})),
        ("list_nodes", json!({})),
        ("list_connections", json!({ "include_closed": true })),
        (
            "test_route",
            json!({ "host": "api.test", "url": "https://api.test/?q=1" }),
        ),
        ("get_rule_hits", json!({})),
        (
            "check_rules",
            json!({ "rules": ["DOMAIN,a.test,DIRECT", "FINAL,DIRECT"] }),
        ),
        ("check_rules", json!({ "rules": ["BAD"] })),
        ("get_health", json!({})),
        ("get_decrypt_status", json!({})),
        ("get_capture_status", json!({})),
        ("list_requests", json!({ "filter": "status:200" })),
        ("get_request", json!({ "connection_id": 1, "index": 0 })),
        ("get_logs", json!({ "level": "debug" })),
    ]
}

fn call(server: &Server<Fake>, name: &str, arguments: Value) -> Value {
    let message = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    server.handle(&message.to_string()).expect("an answer")["result"].clone()
}

fn content(result: &Value) -> &Value {
    assert_eq!(result["isError"], false, "{result}");
    &result["structuredContent"]
}

#[test]
fn no_answer_carries_a_canary_at_any_content_level() {
    for level in [
        ContentLevel::Summary,
        ContentLevel::Headers,
        ContentLevel::Bodies,
    ] {
        let server = Server::new(Fake::new(level));
        for (name, arguments) in calls() {
            let result = call(&server, name, arguments);
            let text = result.to_string();
            assert!(!text.contains(CANARY), "{name} at {level:?}: {text}");
            assert_eq!(result["isError"], false, "{name} at {level:?}: {text}");
        }
    }
}

#[test]
fn every_tool_is_refused_while_access_is_off() {
    let mut fake = Fake::new(ContentLevel::Bodies);
    fake.access = Access::OFF;
    let server = Server::new(fake);
    for (name, arguments) in calls() {
        let result = call(&server, name, arguments);
        assert_eq!(result["isError"], true, "{name}");
        assert_eq!(result["content"][0]["text"], ACCESS_OFF, "{name}");
    }
    assert!(
        server.backend().asked.borrow().is_empty(),
        "nothing reached the engine"
    );
}

#[test]
fn an_engine_that_cannot_redact_shows_no_request() {
    let mut fake = Fake::new(ContentLevel::Bodies);
    fake.old_engine = true;
    let server = Server::new(fake);
    let result = call(
        &server,
        "get_request",
        json!({ "connection_id": 1, "index": 0 }),
    );
    assert_eq!(result["isError"], true);
    assert!(!result.to_string().contains(CANARY));
}

#[test]
fn requests_show_host_path_and_query_names_only() {
    let server = Server::new(Fake::new(ContentLevel::Summary));
    let listed = call(&server, "list_requests", json!({}));
    let row = &content(&listed)["requests"][0];
    assert_eq!(row["host"], "api.test");
    assert_eq!(row["path"], "/v1/items");
    assert_eq!(row["query_names"], json!(["token", "page"]));
    assert_eq!(row["status"], 200);
    assert_eq!(content(&listed)["last_seq"], 1);

    let bad = call(&server, "list_requests", json!({ "filter": "(" }));
    assert_eq!(bad["isError"], true);
    assert!(bad["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("character 0"));
}

#[test]
fn request_detail_follows_the_content_level() {
    let detail = |level| {
        let server = Server::new(Fake::new(level));
        content(&call(
            &server,
            "get_request",
            json!({ "connection_id": 1, "index": 0 }),
        ))
        .clone()
    };

    let summary = detail(ContentLevel::Summary);
    assert_eq!(summary["method"], "POST");
    assert_eq!(summary["path"], "/login");
    assert_eq!(summary["request"], json!({ "body_size": 17 }));
    assert_eq!(summary["stream_messages"], json!({ "seen": 1 }));
    assert!(summary.get("notice").is_none());

    let headers = detail(ContentLevel::Headers);
    assert_eq!(
        headers["request"]["headers"],
        json!([
            ["Content-Type", "application/json"],
            ["Authorization", "<redacted>"]
        ])
    );
    assert_eq!(
        headers["response"]["headers"][1],
        json!(["Set-Cookie", "<redacted>"])
    );
    assert!(headers["request"].get("body").is_none());
    assert!(headers["notice"].is_string());

    let bodies = detail(ContentLevel::Bodies);
    assert_eq!(
        bodies["request"]["body"],
        json!({ "text": "{\"user\":\"alice\"}", "truncated": false })
    );
    assert_eq!(bodies["response"]["body"], json!({ "omitted": "binary" }));

    let server = Server::new(Fake::new(ContentLevel::Bodies));
    let missing = call(
        &server,
        "get_request",
        json!({ "connection_id": 1, "index": 5 }),
    );
    assert_eq!(missing["isError"], true);
}

#[test]
fn connections_rules_and_health_keep_what_a_diagnosis_needs() {
    let server = Server::new(Fake::new(ContentLevel::Summary));

    let all = call(
        &server,
        "list_connections",
        json!({ "include_closed": true }),
    );
    let rows = content(&all)["connections"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], 1, "newest first");
    assert_eq!(rows[0]["chain"], json!(["Proxy", "Node A"]));
    assert_eq!(
        rows[0]["redirect"],
        json!({ "destination": "b.test:443", "rule": "redirect" })
    );
    assert_eq!(rows[1]["closed"], true);
    let curl = call(&server, "list_connections", json!({ "process": "CURL" }));
    assert_eq!(content(&curl)["matched"], 1);
    let none = call(&server, "list_connections", json!({ "host": "nothing" }));
    assert_eq!(content(&none)["matched"], 0);
    let too_many = call(&server, "list_connections", json!({ "limit": 201 }));
    assert_eq!(too_many["isError"], true);

    let route = call(&server, "test_route", json!({ "host": "api.test" }));
    assert_eq!(content(&route)["outbound"], "Node A");
    assert_eq!(content(&route)["mode"], "rule");
    assert_eq!(
        call(&server, "test_route", json!({ "host": " " }))["isError"],
        true
    );
    assert_eq!(
        call(&server, "test_route", json!({ "host": "a", "port": 0 }))["isError"],
        true
    );
    assert_eq!(call(&server, "test_route", json!({}))["isError"], true);

    let hits = call(&server, "get_rule_hits", json!({ "only_hit": true }));
    assert_eq!(content(&hits)["rules"].as_array().unwrap().len(), 1);
    assert_eq!(content(&hits)["rules"][0]["index"], 0);
    assert_eq!(content(&hits)["total_rules"], 2);
    assert_eq!(content(&hits)["final"]["hits"], 1);

    let valid = call(&server, "check_rules", json!({ "rules": ["FINAL,DIRECT"] }));
    assert_eq!(content(&valid), &json!({ "valid": true, "rules": 2 }));
    let invalid = call(&server, "check_rules", json!({ "rules": ["BAD"] }));
    assert_eq!(
        content(&invalid),
        &json!({ "valid": false, "line": 0, "code": "syntax" })
    );

    let health = call(&server, "get_health", json!({}));
    assert_eq!(
        content(&health)["outbounds"][0]["health"],
        json!({ "running": true, "last_handshake_secs": 3 })
    );
    assert_eq!(content(&health)["missing_rule_sets"], json!(["asn-1"]));

    let failed = call(
        &server,
        "get_decrypt_status",
        json!({ "only_failed": true }),
    );
    assert_eq!(
        content(&failed)["hosts"],
        json!([{ "host": "pinned.test", "decrypted": 0, "failed": 2, "last_failure": "client_closed" }])
    );
}

#[test]
fn logs_are_filtered_by_level_and_masked() {
    let server = Server::new(Fake::new(ContentLevel::Summary));
    let default = call(&server, "get_logs", json!({}));
    let lines = content(&default)["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2, "debug lines are left out by default");
    assert_eq!(
        lines[0]["message"],
        "subscription update from https://sub.test/api?<redacted> failed"
    );
    let newest = call(&server, "get_logs", json!({ "level": "debug", "limit": 1 }));
    assert_eq!(content(&newest)["lines"][0]["at_ms"], 3);
    assert_eq!(
        call(&server, "get_logs", json!({ "level": "loud" }))["isError"],
        true
    );
}
