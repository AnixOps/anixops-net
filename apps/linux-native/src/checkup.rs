//! The node checkup (节点体检) without GTK: what the core is asked and what its answer reads as. The rows,
//! their order and which are marked come from the core (`crates/client-view/src/checkup.rs`); this words them
//! and writes the numbers. The same as the Windows window's (`AnixOps.Net.Presentation/Checkup.cs`).

use client_strings::Language;
use serde_json::{json, Value};

use crate::overview;

/// The core's command; it answers when the checkup is over, which takes seconds.
pub const COMMAND: &str = "node_checkup";

/// The download stops at this many bytes, so a checkup spends little of a metered node.
pub const MAX_DOWNLOAD_BYTES: u64 = 200_000_000;

const NONE: &str = "—";

/// The arguments of [`COMMAND`]: the kernel's defaults, and the download when asked.
pub fn args(id: &str, throughput: bool) -> Value {
    if throughput {
        json!({ "nodeId": id, "options": { "throughput": { "max_bytes": MAX_DOWNLOAD_BYTES } } })
    } else {
        json!({ "nodeId": id })
    }
}

/// What the checkup does, said before it is started.
pub fn note(language: Language, throughput: bool) -> String {
    let note = client_strings::text(language, "checkup.note");
    if throughput {
        format!(
            "{note} {}",
            client_strings::text(language, "checkup.note.throughput")
        )
    } else {
        note.to_owned()
    }
}

/// One measure as it is drawn: what it measures, the value with its unit (a dash when no sample
/// succeeded), and whether it is marked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub label: String,
    pub value: String,
    pub warn: bool,
}

/// A group of measures under a heading: latency, then the download when it ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub heading: String,
    pub lines: Vec<Line>,
    /// Said under the group, such as the download stopping at its limit.
    pub note: Option<String>,
}

/// The checkup part of a node's dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// Whether a checkup runs now.
    pub running: bool,
    /// What the last one found; `None` before one ran.
    pub sections: Option<Vec<Section>>,
    /// Why the last one did not work.
    pub problem: Option<String>,
}

/// Reads the core's answer to [`COMMAND`]. What is missing or not known is left out.
pub fn read(language: Language, answer: &Value) -> Vec<Section> {
    let Some(sections) = answer.get("sections").and_then(Value::as_array) else {
        return Vec::new();
    };
    sections
        .iter()
        .filter_map(|section| {
            let id = section.get("id").and_then(Value::as_str)?;
            if !matches!(id, "latency" | "download") {
                return None;
            }
            let lines = section
                .get("rows")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|row| {
                    let id = row.get("id").and_then(Value::as_str)?;
                    let label = client_strings::find(language, &format!("checkup.row.{id}"))?;
                    Some(Line {
                        label: label.to_owned(),
                        value: value(
                            language,
                            row.get("kind").and_then(Value::as_str).unwrap_or_default(),
                            row.get("value").unwrap_or(&Value::Null),
                        ),
                        warn: row.get("warn").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect();
            let capped = section
                .get("capped")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(Section {
                heading: client_strings::text(language, &format!("checkup.section.{id}"))
                    .to_owned(),
                lines,
                note: capped.then(|| client_strings::text(language, "checkup.capped").to_owned()),
            })
        })
        .collect()
}

/// A value of the given kind with its unit: `ms`, `pct`, `mbps`, `bytes` or `failure` (a fixed reason).
pub fn value(language: Language, kind: &str, value: &Value) -> String {
    if kind == "failure" {
        return match value.as_str() {
            None => NONE.to_owned(),
            Some(code) => client_strings::find(language, &format!("checkup.failure.{code}"))
                .map_or_else(|| code.to_owned(), str::to_owned),
        };
    }
    let Some(number) = value.as_f64() else {
        return NONE.to_owned();
    };
    match kind {
        "ms" if number < 10.0 => format!("{number:.1} ms"),
        "ms" => format!("{number:.0} ms"),
        "pct" => format!("{number:.0}%"),
        "mbps" if number < 10.0 => format!("{number:.2} Mbps"),
        "mbps" => format!("{number:.1} Mbps"),
        "bytes" => overview::size(value.as_u64().unwrap_or(number.max(0.0) as u64)),
        _ => number.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers() -> Vec<Value> {
        let cases: Vec<Value> = serde_json::from_str(include_str!(
            "../../../crates/client-view/fixtures/cases.json"
        ))
        .unwrap();
        cases
            .into_iter()
            .filter(|case| case["command"] == "view.checkup")
            .map(|case| case["answer"].clone())
            .collect()
    }

    fn line(label: &str, value: &str, warn: bool) -> Line {
        Line {
            label: label.to_owned(),
            value: value.to_owned(),
            warn,
        }
    }

    #[test]
    fn the_command_and_its_arguments() {
        assert_eq!(COMMAND, "node_checkup");
        assert_eq!(args("a", false), json!({ "nodeId": "a" }));
        assert_eq!(
            args("a", true),
            json!({ "nodeId": "a", "options": { "throughput": { "max_bytes": 200_000_000 } } })
        );
    }

    #[test]
    fn a_full_checkup_reads_as_latency_then_download() {
        let sections = read(Language::English, &answers()[0]);
        let headings: Vec<&str> = sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["Latency", "Download"]);
        let latency = &sections[0].lines;
        let labels: Vec<&str> = latency.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Handshake",
                "First handshake",
                "Later handshakes",
                "Request latency",
                "Jitter",
                "Loss",
                "Reason"
            ]
        );
        assert_eq!(latency[0], line("Handshake", "42 ms", false));
        assert_eq!(latency[4], line("Jitter", "3.5 ms", false));
        assert_eq!(latency[5], line("Loss", "20%", true));
        assert_eq!(latency[6], line("Reason", "Timed out", true));
        assert_eq!(sections[0].note, None);
        let download = &sections[1].lines;
        assert_eq!(download[0], line("Average speed", "94.5 Mbps", false));
        assert_eq!(download[1], line("Peak speed", "120.3 Mbps", false));
        assert_eq!(
            download[2],
            line("Downloaded", &overview::size(94_500_000), false)
        );
        assert_eq!(download[5], line("Added latency", "142 ms", true));
        assert_eq!(download[6], line("Loaded loss", "0%", false));
    }

    #[test]
    fn a_node_that_does_not_answer_shows_dashes_and_the_reason() {
        let sections = read(Language::Chinese, &answers()[1]);
        assert_eq!(sections.len(), 1);
        let latency = &sections[0];
        assert_eq!(latency.heading, "延迟");
        assert_eq!(latency.lines[0], line("握手延迟", "—", false));
        assert!(!latency.lines.iter().any(|l| l.label == "后续握手"));
        let n = latency.lines.len();
        assert_eq!(latency.lines[n - 2], line("丢包", "100%", true));
        assert_eq!(latency.lines[n - 1], line("失败原因", "连接失败", true));
    }

    #[test]
    fn what_is_not_understood_is_left_out() {
        assert!(read(Language::English, &Value::Null).is_empty());
        assert!(read(
            Language::English,
            &json!({ "sections": [{ "id": "upload", "rows": [] }] })
        )
        .is_empty());
        let odd = read(
            Language::English,
            &json!({ "sections": [{ "id": "latency", "capped": true, "rows": [
                { "id": "later", "kind": "ms", "value": 1 },
                { "id": "failure", "kind": "failure", "value": "new_code", "warn": true },
            ] }] }),
        );
        assert_eq!(odd[0].lines, [line("Reason", "new_code", true)]);
        assert_eq!(
            odd[0].note.as_deref(),
            Some(client_strings::text(Language::English, "checkup.capped"))
        );
    }

    #[test]
    fn the_note_says_what_is_sent() {
        assert!(note(Language::English, false).contains("1.1.1.1"));
        assert!(!note(Language::English, false).contains("speed.cloudflare.com"));
        assert!(note(Language::English, true).contains("speed.cloudflare.com"));
    }
}
