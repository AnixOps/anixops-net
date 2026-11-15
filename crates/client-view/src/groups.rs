//! The policy groups of the proxies page: what each one's row says and what can be chosen in it.

use serde_json::{json, Value};

fn text<'a>(group: &'a Value, name: &str) -> Option<&'a str> {
    group
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn texts<'a>(group: &'a Value, name: &str) -> Vec<&'a str> {
    group
        .get(name)
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// One policy group as the page lists it, from the core's `list_native_groups` entry:
///
/// - `tag`: the group's name, as the subscription wrote it.
/// - `kind`: `select` (the user chooses the member), `auto` (the fastest member is used), `fallback`
///   (the first member that answers is used), or `other` for a kind this does not know.
/// - `choosable`: whether the user chooses the member; only for `select`.
/// - `members`: `{ id, label, selected }` in the subscription's order. `id` is what the core is told when
///   a member is chosen; `label` is the node's name, or the id itself for another group and for the
///   built-in exits. A member without a label is shown by its id.
/// - `selected`, `selectedLabel`: the member in use and its label, or `null` when the core does not say
///   (a group that chooses by itself and has not chosen yet) or names one that is not a member.
fn row(group: &Value) -> Value {
    let kind = match text(group, "groupType") {
        Some("selector") => "select",
        Some("urltest") => "auto",
        Some("fallback") => "fallback",
        _ => "other",
    };
    let ids = texts(group, "outbounds");
    let labels = texts(group, "outboundLabels");
    let label = |at: usize| labels.get(at).copied().filter(|label| !label.is_empty());
    let chosen =
        text(group, "selected").and_then(|selected| ids.iter().position(|id| *id == selected));
    let members: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(at, id)| {
            json!({
                "id": id,
                "label": label(at).unwrap_or(id),
                "selected": chosen == Some(at),
            })
        })
        .collect();
    json!({
        "tag": text(group, "tag").unwrap_or_default(),
        "kind": kind,
        "choosable": kind == "select",
        "members": members,
        "selected": chosen.map(|at| ids[at]),
        "selectedLabel": chosen.map(|at| label(at).unwrap_or(ids[at])),
    })
}

/// What the page's list of policy groups shows: `{ "groups": [row, …] }` in the core's order.
pub fn groups(groups: &[Value]) -> Value {
    json!({ "groups": groups.iter().map(row).collect::<Vec<_>>() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_says_its_kind_its_members_and_the_one_in_use() {
        let answer = groups(&[json!({
            "tag": "Proxy", "groupType": "selector", "selected": "n2",
            "outbounds": ["Auto", "n1", "n2", "DIRECT"],
            "outboundLabels": ["Auto", "Tokyo", "Osaka", "DIRECT"],
        })]);
        assert_eq!(
            answer,
            json!({ "groups": [{
                "tag": "Proxy", "kind": "select", "choosable": true,
                "members": [
                    { "id": "Auto", "label": "Auto", "selected": false },
                    { "id": "n1", "label": "Tokyo", "selected": false },
                    { "id": "n2", "label": "Osaka", "selected": true },
                    { "id": "DIRECT", "label": "DIRECT", "selected": false },
                ],
                "selected": "n2", "selectedLabel": "Osaka",
            }] })
        );
    }

    #[test]
    fn what_the_core_does_not_say_is_null_and_a_missing_label_is_the_id() {
        let answer = groups(&[
            json!({ "tag": "Auto", "groupType": "urltest", "outbounds": ["n1", "n2"], "outboundLabels": ["Tokyo"] }),
            json!({ "tag": "Old", "groupType": "relay", "selected": "gone", "outbounds": ["n1"] }),
            json!({}),
        ]);
        assert_eq!(
            answer,
            json!({ "groups": [
                {
                    "tag": "Auto", "kind": "auto", "choosable": false,
                    "members": [
                        { "id": "n1", "label": "Tokyo", "selected": false },
                        { "id": "n2", "label": "n2", "selected": false },
                    ],
                    "selected": null, "selectedLabel": null,
                },
                {
                    "tag": "Old", "kind": "other", "choosable": false,
                    "members": [{ "id": "n1", "label": "n1", "selected": false }],
                    "selected": null, "selectedLabel": null,
                },
                {
                    "tag": "", "kind": "other", "choosable": false, "members": [],
                    "selected": null, "selectedLabel": null,
                },
            ] })
        );
        assert_eq!(groups(&[]), json!({ "groups": [] }));
    }
}
