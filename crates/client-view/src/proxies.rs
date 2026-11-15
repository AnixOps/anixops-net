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

/// The rows of `nodes` split by subscription, in the core's order. Nodes belong together when they follow
/// each other and come from the same subscription (`source`, the subscription's id) under the same name:
/// two subscriptions that happen to share a name stay apart.
fn sections(nodes: &[Value], delays: &Value) -> Value {
    let mut sections: Vec<(&Value, Vec<Value>)> = Vec::new();
    for node in nodes {
        let same = |first: &Value| {
            first.get("source").and_then(Value::as_str)
                == node.get("source").and_then(Value::as_str)
                && first.get("group").and_then(Value::as_str)
                    == node.get("group").and_then(Value::as_str)
        };
        match sections.last_mut() {
            Some((first, members)) if same(first) => members.push(row(node, delays)),
            _ => sections.push((node, vec![row(node, delays)])),
        }
    }
    sections
        .into_iter()
        .map(|(first, nodes)| {
            json!({
                "group": first.get("group").and_then(Value::as_str),
                "source": first.get("source").and_then(Value::as_str),
                "pasted": first.get("pasted").and_then(Value::as_bool).unwrap_or(false),
                "nodes": nodes,
            })
        })
        .collect()
}

/// What the proxies page shows:
///
/// - `total`: how many nodes there are.
/// - `shown`: how many of them `query` leaves (name, protocol or subscription contains it, without regard
///   to case; an empty query leaves all).
/// - `sections`: those nodes split by subscription in the core's order,
///   `[{ "group": …, "source": …, "pasted": …, "nodes": [{ id, label, protocol, group, selected, delay }] }]`.
///   `source` is the subscription's id. `pasted` says that the nodes are share links pasted into the
///   client: `group` is then the core's own name for them, and a window writes its own wording instead.
///
/// `delays` maps node ids to the last delay test (`{ "delayMs": … }`, without `delayMs` when it failed).
pub fn proxies(nodes: &[Value], delays: &Value, query: &str) -> Value {
    let shown = nodes::filter(nodes, query, nodes::ALL_PROTOCOLS);
    json!({
        "total": nodes.len(),
        "shown": shown.len(),
        "sections": sections(&shown, delays),
    })
}

/// The ids of the nodes [`proxies`] shows for `query`, in the order it shows them.
pub fn shown_ids(nodes: &[Value], query: &str) -> Vec<String> {
    nodes::filter(nodes, query, nodes::ALL_PROTOCOLS)
        .iter()
        .map(|node| text(node, "id").to_owned())
        .collect()
}
