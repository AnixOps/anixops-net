//! What every window decides the same way, written once.
//!
//! The windows are written in different languages (C#, Rust, Kotlin, and TypeScript while the web interface
//! lasts). Logic with behaviour of its own (how an `anixops://` link is read, how quick filters join a typed
//! filter, how the request and node lists are ordered) would otherwise be written and tested once per
//! language and drift apart. It lives here instead, and a window reaches it as commands whose names start
//! with `view.`, through the same door as the core's commands ([`invoke`]).
//!
//! The commands take and give JSON. Rows (requests, nodes) are passed through as they came, so fields this
//! crate does not know stay on them. Nothing here is written for one language's eyes: what a window shows
//! (a refusal's sentence, a number's format, the sort order of names in the user's language) is the
//! window's own, made from the ids and values given here.
//!
//! `fixtures/cases.json` lists every command with arguments and the expected answer. The tests here run
//! it, and a window in another language can run the same file against its end of the channel.

pub mod app_link;
pub mod capture;
pub mod nodes;

use serde_json::{json, Value};

/// The prefix of this crate's commands.
pub const PREFIX: &str = "view.";

fn text<'a>(args: &'a Value, name: &str) -> &'a str {
    args.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn rows<'a>(args: &'a Value, name: &str) -> &'a [Value] {
    args.get(name)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Answers a `view.` command; `None` when `command` is not one of this crate's.
pub fn invoke(command: &str, args: &Value) -> Option<Result<Value, String>> {
    let name = command.strip_prefix(PREFIX)?;
    let answer = match name {
        "parse_app_link" => app_link::parse(text(args, "link")),
        "find_link_node" => app_link::find_node(
            rows(args, "nodes"),
            text(args, "name"),
            args.get("group").and_then(Value::as_str),
        )
        .cloned()
        .unwrap_or(Value::Null),
        "find_link_member" => app_link::find_member(
            rows(args, "groups"),
            text(args, "policy"),
            text(args, "member"),
        ),
        "combine_filter" => {
            let quick: Vec<&str> = rows(args, "quick")
                .iter()
                .filter_map(Value::as_str)
                .collect();
            json!(capture::combine_filter(&quick, text(args, "typed")))
        }
        "next_since" => json!(capture::next_since(
            rows(args, "rows"),
            args.get("lastSeq").and_then(Value::as_u64).unwrap_or(0),
        )),
        "merge_requests" => Value::Array(capture::merge(
            rows(args, "rows"),
            args.get("answer").unwrap_or(&Value::Null),
        )),
        "sort_requests" => {
            let Some(by) = capture::Column::from_id(text(args, "by")) else {
                return Some(Err(format!(
                    "the request list has no column {}",
                    text(args, "by")
                )));
            };
            let descending = args
                .get("descending")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Value::Array(capture::sort(rows(args, "rows"), by, descending))
        }
        "describe_request" => {
            let request = args.get("request").unwrap_or(&Value::Null);
            json!({
                "path": capture::path(request),
                "tone": capture::status_tone(request),
            })
        }
        "filter_nodes" => Value::Array(nodes::filter(
            rows(args, "nodes"),
            text(args, "query"),
            text(args, "protocol"),
        )),
        "group_nodes" => nodes::group(rows(args, "nodes")),
        "sort_nodes" => {
            let Some(order) = nodes::Order::from_id(text(args, "sort")) else {
                return Some(Err(format!(
                    "the node list has no order {}",
                    text(args, "sort")
                )));
            };
            Value::Array(nodes::sort(
                rows(args, "nodes"),
                order,
                args.get("delays").unwrap_or(&Value::Null),
            ))
        }
        "delay_tone" => json!(nodes::delay_tone(args.get("delay").unwrap_or(&Value::Null))),
        _ => return Some(Err(format!("unknown command {command}"))),
    };
    Some(Ok(answer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sample_gets_the_answer_it_lists() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../fixtures/cases.json")).unwrap();
        assert!(cases.len() > 40, "the samples are missing");
        for case in &cases {
            let command = case["command"].as_str().unwrap();
            let answer = invoke(command, &case["args"])
                .unwrap_or_else(|| panic!("{command} is not a view command"));
            match case.get("refused") {
                Some(_) => assert!(answer.is_err(), "{case}"),
                None => assert_eq!(answer.as_ref(), Ok(&case["answer"]), "{case}"),
            }
        }
    }

    #[test]
    fn every_command_has_a_sample() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../fixtures/cases.json")).unwrap();
        for name in [
            "parse_app_link",
            "find_link_node",
            "find_link_member",
            "combine_filter",
            "next_since",
            "merge_requests",
            "sort_requests",
            "describe_request",
            "filter_nodes",
            "group_nodes",
            "sort_nodes",
            "delay_tone",
        ] {
            let command = format!("{PREFIX}{name}");
            assert!(
                cases.iter().any(|case| case["command"] == command.as_str()),
                "{command} has no sample"
            );
        }
    }

    #[test]
    fn other_commands_are_left_to_the_core() {
        assert_eq!(invoke("connect", &json!({})), None);
        assert!(invoke("view.reboot", &json!({})).unwrap().is_err());
    }
}
