//! The proxies page: the node list as the page shows it, searched, split by subscription, each node with
//! what its last delay test said.

use serde_json::{json, Value};

use crate::nodes;

fn text<'a>(node: &'a Value, name: &str) -> &'a str {
    node.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// One node as the page lists it. `delay` is `null` for a node that was never tested, else
/// `{ "ms": …, "tone": … }` with `ms` `null` when the test got no answer and `tone` as [`nodes::delay_tone`]
/// says.
fn row(node: &Value, delays: &Value) -> Value {
    let delay = delays
        .get(text(node, "id"))
        .filter(|delay| delay.is_object())
        .map(|delay| {
            json!({
                "ms": delay.get("delayMs").and_then(Value::as_u64),
                "tone": nodes::delay_tone(delay),
            })
        });
    json!({
        "id": text(node, "id"),
        "label": text(node, "label"),
        "protocol": text(node, "protocol"),
        "group": node.get("group").and_then(Value::as_str),
        "selected": node.get("selected").and_then(Value::as_bool).unwrap_or(false),
        "delay": delay,
    })
}

/// What the proxies page shows:
///
/// - `total`: how many nodes there are.
/// - `shown`: how many of them `query` leaves (name, protocol or subscription contains it, without regard
///   to case; an empty query leaves all).
/// - `sections`: those nodes split by subscription in the core's order,
///   `[{ "group": …, "nodes": [{ id, label, protocol, group, selected, delay }] }]`.
///
/// `delays` maps node ids to the last delay test (`{ "delayMs": … }`, without `delayMs` when it failed).
pub fn proxies(nodes: &[Value], delays: &Value, query: &str) -> Value {
    let rows: Vec<Value> = nodes::filter(nodes, query, nodes::ALL_PROTOCOLS)
        .iter()
        .map(|node| row(node, delays))
        .collect();
    json!({
        "total": nodes.len(),
        "shown": rows.len(),
        "sections": nodes::group(&rows),
    })
}

/// The ids of the nodes [`proxies`] shows for `query`, in the order it shows them.
pub fn shown_ids(nodes: &[Value], query: &str) -> Vec<String> {
    nodes::filter(nodes, query, nodes::ALL_PROTOCOLS)
        .iter()
        .map(|node| text(node, "id").to_owned())
        .collect()
}
