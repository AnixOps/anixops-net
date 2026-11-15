//! The overview page: what the core's snapshot, node list and traffic counters mean for the page that shows
//! whether the client is connected, through what, and how much is moving.

use serde_json::{json, Value};

fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
}

fn flag(value: &Value, name: &str) -> bool {
    value.get(name).and_then(Value::as_bool).unwrap_or(false)
}

/// The connection's state as an id: `connected`, `disconnected`, `failed` (the last try to connect did not
/// work) or `core_error` (the engine stopped by itself).
pub fn state(snapshot: &Value) -> &'static str {
    match text(snapshot, "connection") {
        Some("Connected") => "connected",
        Some("Core error") => "core_error",
        Some("Connection failed") => "failed",
        _ => "disconnected",
    }
}

/// Where traffic leaves: `direct` when the client is set to send everything directly (the routing mode, or
/// debug mode's direct egress), else `node`.
pub fn egress(snapshot: &Value) -> &'static str {
    if flag(snapshot, "directEgress") || text(snapshot, "routingMode") == Some("direct") {
        "direct"
    } else {
        "node"
    }
}

/// The node in use: the one the snapshot names, else the one the list marks as selected.
pub fn node<'a>(snapshot: &Value, nodes: &'a [Value]) -> Option<&'a Value> {
    text(snapshot, "selectedNode")
        .and_then(|id| nodes.iter().find(|node| text(node, "id") == Some(id)))
        .or_else(|| nodes.iter().find(|node| flag(node, "selected")))
}

/// The engine's traffic counters under the names the windows use; `None` when they are not all there.
fn traffic(counters: &Value) -> Option<Value> {
    let number = |name: &str| {
        let value = counters.get(name)?;
        // Whole numbers as the engine sends them; a rate sent with a fraction is rounded down.
        value.as_u64().or_else(|| {
            value
                .as_f64()
                .filter(|rate| *rate >= 0.0)
                .map(|rate| rate as u64)
        })
    };
    Some(json!({
        "uploadRate": number("upload_rate")?,
        "downloadRate": number("download_rate")?,
        "uploadTotal": number("upload_total")?,
        "downloadTotal": number("download_total")?,
        "connections": number("active")?,
    }))
}

/// What the overview page shows:
///
/// - `state`: [`state`].
/// - `egress`: [`egress`].
/// - `node`: the node in use as the core listed it, or `null`.
/// - `canConnect`: whether connecting can work at all: a node is chosen, or traffic leaves directly.
/// - `error`: why the client is not connected, in the core's words, or `null`. Never set while connected.
/// - `traffic`: rates in bytes per second, totals in bytes and the number of open connections, or `null`
///   while not connected or before the engine has answered.
pub fn overview(snapshot: &Value, nodes: &[Value], counters: &Value) -> Value {
    let state = state(snapshot);
    let egress = egress(snapshot);
    let node = node(snapshot, nodes);
    let connected = state == "connected";
    let error = if connected {
        None
    } else {
        text(snapshot, "lastError").or_else(|| text(snapshot, "configurationError"))
    };
    let traffic = connected.then(|| traffic(counters)).flatten();
    json!({
        "state": state,
        "egress": egress,
        "node": node,
        "canConnect": node.is_some() || egress == "direct",
        "error": error,
        "traffic": traffic,
    })
}
