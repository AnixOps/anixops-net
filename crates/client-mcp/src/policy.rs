//! The gate of the desktop core: which engine requests an AI tool may make, as `policy.json` lists them.
//!
//! The tools of this crate only make requests that fit the policy (`tests/privacy.rs` checks it), but the core
//! never trusts the program at the other end of its channel: any program of the same user can connect and call
//! itself an AI tool. So the core checks every engine request again with [`engine_request`] before the engine
//! sees it. The macOS app does the same in Swift (`AgentGate.swift`), from the same file.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Map, Value};

/// The engine commands an AI tool may send, the fields each may carry and the values forced on it.
pub const POLICY_JSON: &str = include_str!("../policy.json");

#[derive(Debug, Deserialize)]
struct Policy {
    engine: BTreeMap<String, Rule>,
}

/// What one engine command may carry.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Rule {
    /// Fields the caller may set.
    pub fields: BTreeSet<String>,
    /// Fields set to these values whatever the caller sent: `redact` and `dry_run` are always `true`.
    pub forced: BTreeMap<String, bool>,
}

/// The engine commands of the policy, by name.
pub fn engine_rules() -> &'static BTreeMap<String, Rule> {
    static RULES: OnceLock<BTreeMap<String, Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        serde_json::from_str::<Policy>(POLICY_JSON)
            .expect("policy.json is valid")
            .engine
    })
}

/// Why a request was not let through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not an object with a `cmd` string, or a field of a type no command takes.
    Malformed,
    /// A command or a field the policy does not list.
    NotAllowed(String),
}

impl Refusal {
    /// The sentence the AI tool is given.
    pub fn message(&self) -> String {
        match self {
            Self::Malformed => "The request is not a valid engine request.".to_string(),
            Self::NotAllowed(name) => format!("{name} is not available to AI tools."),
        }
    }
}

/// The request the engine is sent for `request`: the command and the fields it may carry, with the forced
/// values set, or why it is refused. A field value is a string, a number, a boolean or a list of strings.
pub fn engine_request(request: &Value) -> Result<Value, Refusal> {
    let object = request.as_object().ok_or(Refusal::Malformed)?;
    let command = object
        .get("cmd")
        .and_then(Value::as_str)
        .ok_or(Refusal::Malformed)?;
    let rule = engine_rules()
        .get(command)
        .ok_or_else(|| Refusal::NotAllowed(command.to_string()))?;
    let mut gated = Map::new();
    gated.insert("cmd".to_string(), Value::String(command.to_string()));
    for (name, value) in object {
        if name == "cmd" {
            continue;
        }
        if !rule.fields.contains(name) && !rule.forced.contains_key(name) {
            return Err(Refusal::NotAllowed(name.clone()));
        }
        let plain = match value {
            Value::String(_) | Value::Number(_) | Value::Bool(_) => true,
            Value::Array(items) => items.iter().all(Value::is_string),
            Value::Null | Value::Object(_) => false,
        };
        if !plain {
            return Err(Refusal::Malformed);
        }
        gated.insert(name.clone(), value.clone());
    }
    for (name, value) in &rule.forced {
        gated.insert(name.clone(), Value::Bool(*value));
    }
    Ok(Value::Object(gated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redaction_and_dry_runs_are_forced() {
        let requests = engine_request(
            &json!({ "cmd": "capture_requests", "filter": "status:200", "limit": 5, "redact": false }),
        )
        .unwrap();
        assert_eq!(
            requests,
            json!({ "cmd": "capture_requests", "filter": "status:200", "limit": 5, "redact": true })
        );
        let exchange =
            engine_request(&json!({ "cmd": "capture_exchange", "id": 3, "index": 0 })).unwrap();
        assert_eq!(exchange["redact"], true);
        let rules = engine_request(
            &json!({ "cmd": "reload_rules", "rules": ["FINAL,DIRECT"], "dry_run": false }),
        )
        .unwrap();
        assert_eq!(
            rules,
            json!({ "cmd": "reload_rules", "rules": ["FINAL,DIRECT"], "dry_run": true })
        );
        assert_eq!(
            engine_request(&json!({ "cmd": "traffic" })).unwrap(),
            json!({ "cmd": "traffic" })
        );
    }

    #[test]
    fn anything_else_is_refused() {
        for command in [
            "set_mode",
            "set_capture",
            "export_capture",
            "start_lan_share",
            "close_all",
        ] {
            assert_eq!(
                engine_request(&json!({ "cmd": command })),
                Err(Refusal::NotAllowed(command.to_string()))
            );
        }
        assert_eq!(
            engine_request(&json!({ "cmd": "traffic", "extra": 1 })),
            Err(Refusal::NotAllowed("extra".to_string()))
        );
        assert_eq!(
            engine_request(&json!({ "cmd": "capture_requests", "since_rev": 1 })),
            Err(Refusal::NotAllowed("since_rev".to_string()))
        );
        assert_eq!(
            engine_request(&json!({ "cmd": "test_route", "host": { "a": 1 } })),
            Err(Refusal::Malformed)
        );
        assert_eq!(
            engine_request(&json!({ "cmd": "test_route", "host": null })),
            Err(Refusal::Malformed)
        );
        assert_eq!(
            engine_request(&json!({ "cmd": "reload_rules", "rules": [1] })),
            Err(Refusal::Malformed)
        );
        assert_eq!(engine_request(&json!([])), Err(Refusal::Malformed));
        assert_eq!(
            engine_request(&json!({ "host": "a" })),
            Err(Refusal::Malformed)
        );
        assert_eq!(
            engine_request(&json!({ "cmd": 1 })),
            Err(Refusal::Malformed)
        );
    }

    #[test]
    fn only_safe_values_are_forced() {
        for (command, rule) in engine_rules() {
            for (name, value) in &rule.forced {
                assert!(
                    matches!(name.as_str(), "redact" | "dry_run") && *value,
                    "{command}.{name}"
                );
            }
        }
    }
}
