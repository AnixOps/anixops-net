//! The node list: searching it, splitting it by subscription and ordering it.

use serde_json::{json, Value};

use crate::capture::compare_text;

/// The protocol filter's value for "any protocol".
pub const ALL_PROTOCOLS: &str = "All";

fn text<'a>(node: &'a Value, name: &str) -> &'a str {
    node.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// The nodes whose name, protocol or subscription contains `query` (without regard to case) and whose
/// protocol is `protocol`, or any with [`ALL_PROTOCOLS`].
pub fn filter(nodes: &[Value], query: &str, protocol: &str) -> Vec<Value> {
    let query = query.trim().to_lowercase();
    nodes
        .iter()
        .filter(|node| {
            let searchable = format!(
                "{} {} {}",
                text(node, "label"),
                text(node, "protocol"),
                text(node, "group")
            )
            .to_lowercase();
            (query.is_empty() || searchable.contains(&query))
                && (protocol == ALL_PROTOCOLS || text(node, "protocol") == protocol)
        })
        .cloned()
        .collect()
}

/// Nodes split by subscription, in the order the core lists them: `[{ "group": …, "nodes": […] }]`, with
/// `group` `null` for nodes that name none.
pub fn group(nodes: &[Value]) -> Value {
    let mut sections: Vec<(Option<&str>, Vec<Value>)> = Vec::new();
    for node in nodes {
        let group = node.get("group").and_then(Value::as_str);
        match sections.last_mut() {
            Some((last, members)) if *last == group => members.push(node.clone()),
            _ => sections.push((group, vec![node.clone()])),
        }
    }
    sections
        .into_iter()
        .map(|(group, nodes)| json!({ "group": group, "nodes": nodes }))
        .collect()
}

/// An order of the node list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// As the core lists them.
    Default,
    Name,
    Delay,
}

impl Order {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "default" => Self::Default,
            "name" => Self::Name,
            "delay" => Self::Delay,
            _ => return None,
        })
    }
}

/// Nodes in the chosen order. `delays` maps node ids to the last delay test (`{ "delayMs": … }`, without
/// `delayMs` when it failed). By delay, answered nodes come first, fastest first, then failed ones, then
/// untested ones; ties keep the core's order.
pub fn sort(nodes: &[Value], order: Order, delays: &Value) -> Vec<Value> {
    let mut sorted: Vec<&Value> = nodes.iter().collect();
    match order {
        Order::Default => {}
        Order::Name => sorted.sort_by(|a, b| compare_text(text(a, "label"), text(b, "label"))),
        Order::Delay => sorted.sort_by_key(|node| match delays.get(text(node, "id")) {
            None => u64::MAX,
            Some(delay) => delay
                .get("delayMs")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX - 1),
        }),
    }
    sorted.into_iter().cloned().collect()
}

/// `fast`, `slow` or `failed`, for the delay label's colour.
pub fn delay_tone(delay: &Value) -> &'static str {
    match delay.get("delayMs").and_then(Value::as_u64) {
        None => "failed",
        Some(delay) if delay < 500 => "fast",
        Some(_) => "slow",
    }
}
