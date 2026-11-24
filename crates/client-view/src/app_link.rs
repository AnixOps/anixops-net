//! `anixops://` links: what one asks for, and which node or policy member it names.
//!
//! The same actions on every platform:
//!
//! - `anixops://connect`, `anixops://disconnect`, `anixops://toggle`
//! - `anixops://mode?set=rule|global|direct` (public builds have no preset list modes, docs/no-presets.md)
//! - `anixops://select?node=<name>[&group=<subscription>]`, or
//!   `anixops://select?policy=<group>&node=<member>`
//! - `anixops://import?url=<subscription or node link>` fills in the import form; nothing is saved until
//!   the user confirms
//!
//! `anixops:///connect` and `anixops://x-callback-url/connect` are accepted too.

use std::collections::HashMap;

use serde_json::{json, Value};
use url::Url;

pub const SCHEME: &str = "anixops";

/// Longer imports are refused, so a page cannot hand the form megabytes of text. Counted as the web
/// interface counts it, in UTF-16 units.
pub const MAX_IMPORT_LENGTH: usize = 8 * 1024;

const MODES: [&str; 3] = ["rule", "global", "direct"];

fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

fn refused(reason: &str) -> Value {
    json!({ "error": reason })
}

fn refused_for(reason: &str, detail: &str) -> Value {
    json!({ "error": reason, "detail": detail })
}

/// What `raw` asks for: `{ "action": … }` with the action's own fields, or `{ "error": <id> }` (with
/// `detail` where there is something to name) for a link that is refused. The ids: `invalid`,
/// `not_app_link`, `too_long`, `missing_action`, `unknown_action`, `missing_parameter`, `unknown_mode`.
pub fn parse(raw: &str) -> Value {
    let Ok(url) = Url::parse(raw.trim()) else {
        return refused("invalid");
    };
    if url.scheme() != SCHEME {
        return refused("not_app_link");
    }
    if length(raw) > MAX_IMPORT_LENGTH + 256 {
        return refused("too_long");
    }
    let host = url.host_str().unwrap_or_default().to_lowercase();
    let action = if host.is_empty() || host == "x-callback-url" {
        url.path()
            .split('/')
            .find(|segment| !segment.is_empty())
            .unwrap_or_default()
            .to_lowercase()
    } else {
        host
    };
    // The first value of a name counts; names are read without regard to case.
    let mut query: HashMap<String, String> = HashMap::new();
    for (name, value) in url.query_pairs() {
        query
            .entry(name.to_lowercase())
            .or_insert_with(|| value.trim().to_string());
    }
    let value = |name: &str| query.get(name).filter(|value| !value.is_empty());

    match action.as_str() {
        "connect" | "start" | "on" => json!({ "action": "connect" }),
        "disconnect" | "stop" | "off" => json!({ "action": "disconnect" }),
        "toggle" => json!({ "action": "toggle" }),
        "mode" => {
            let Some(asked) = value("set").or_else(|| value("mode")) else {
                return refused_for("missing_parameter", "set");
            };
            let mode = asked.to_lowercase();
            if MODES.contains(&mode.as_str()) {
                json!({ "action": "mode", "mode": mode })
            } else {
                refused_for("unknown_mode", asked)
            }
        }
        "select" => {
            let Some(node) = value("node") else {
                return refused_for("missing_parameter", "node");
            };
            if let Some(policy) = value("policy") {
                return json!({ "action": "select_member", "policy": policy, "member": node });
            }
            match value("group") {
                Some(group) => json!({ "action": "select", "node": node, "group": group }),
                None => json!({ "action": "select", "node": node }),
            }
        }
        "import" | "install-config" | "add" => {
            let Some(link) = value("url") else {
                return refused_for("missing_parameter", "url");
            };
            if length(link) > MAX_IMPORT_LENGTH {
                return refused("too_long");
            }
            json!({ "action": "import", "url": link })
        }
        "" => refused("missing_action"),
        other => refused_for("unknown_action", other),
    }
}

fn field<'a>(row: &'a Value, name: &str) -> &'a str {
    row.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// The node a `select` link names: in subscription `group` when given (else any), by its exact name, else
/// by its name without regard to case, else by id.
pub fn find_node<'a>(nodes: &'a [Value], name: &str, group: Option<&str>) -> Option<&'a Value> {
    let group = group.map(str::to_lowercase);
    let pool: Vec<&Value> = nodes
        .iter()
        .filter(|node| {
            group
                .as_ref()
                .is_none_or(|group| field(node, "group").to_lowercase() == *group)
        })
        .collect();
    let lower = name.to_lowercase();
    pool.iter()
        .find(|node| field(node, "label") == name)
        .or_else(|| {
            pool.iter()
                .find(|node| field(node, "label").to_lowercase() == lower)
        })
        .or_else(|| pool.iter().find(|node| field(node, "id") == name))
        .copied()
}

/// The policy group and member a `select_member` link names, the group by tag (exact, else without regard
/// to case) and the member by tag or by shown name: `{ "tag": …, "outbound": … }`, or `null`.
pub fn find_member(groups: &[Value], policy: &str, member: &str) -> Value {
    let lower = policy.to_lowercase();
    let Some(group) = groups
        .iter()
        .find(|group| field(group, "tag") == policy)
        .or_else(|| {
            groups
                .iter()
                .find(|group| field(group, "tag").to_lowercase() == lower)
        })
    else {
        return Value::Null;
    };
    let list = |name: &str| -> &[Value] {
        group
            .get(name)
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    };
    let outbounds = list("outbounds");
    let position = |values: &[Value]| {
        values
            .iter()
            .position(|value| value.as_str() == Some(member))
    };
    let index = position(outbounds).or_else(|| position(list("outboundLabels")));
    match index.and_then(|index| outbounds.get(index)) {
        Some(outbound) => json!({ "tag": field(group, "tag"), "outbound": outbound }),
        None => Value::Null,
    }
}
