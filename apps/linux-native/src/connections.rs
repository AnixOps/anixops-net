//! The open connections without GTK: what the core says about them, read into what the list shows, and
//! what closing one or all of them asks of the core.
//!
//! Which connections are open, where each goes, by which rule and through which groups and exit, and which
//! ones a search leaves, are decided by the core (its `connections` command). This module reads that answer
//! ([`read`]) and turns it into the list's rows ([`shown`]).

use client_strings::Language;
use serde_json::{json, Value};

use crate::overview::size;

/// The core's command that answers with the open connections.
pub const COMMAND: &str = "connections";

/// The arguments of [`COMMAND`]: what the user typed into the search.
pub fn args(query: &str) -> Value {
    json!({ "query": query })
}

/// The core's command that closes every open connection. It takes no arguments.
pub const CLOSE_ALL: &str = "close_connections";

/// The core's command that closes one connection, and its arguments.
pub fn close(id: u64) -> (&'static str, Value) {
    ("close_connection", json!({ "id": id }))
}

/// One open connection as the core describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    /// What the core is told when the connection is closed.
    pub id: u64,
    /// Where it goes: a name and port, or an address and port.
    pub target: String,
    /// `tcp` or `udp`.
    pub network: String,
    /// The rule that matched.
    pub rule: String,
    /// The policy groups passed through and the exit, by name.
    pub route: Vec<String>,
    pub upload: u64,
    pub download: u64,
}

/// The core's answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct List {
    /// Whether there is an engine whose connections could be listed.
    pub running: bool,
    /// How many connections are open, whatever the search leaves.
    pub total: u64,
    /// The ones the search leaves, the newest first.
    pub connections: Vec<Connection>,
}

/// Reads the core's answer. What is missing reads as "no engine, no connections".
pub fn read(answer: &Value) -> List {
    let text = |row: &Value, name: &str| {
        row.get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let number = |row: &Value, name: &str| row.get(name).and_then(Value::as_u64).unwrap_or(0);
    List {
        running: answer.get("running").and_then(Value::as_bool) == Some(true),
        total: number(answer, "total"),
        connections: answer
            .get("connections")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|row| Connection {
                        id: number(row, "id"),
                        target: text(row, "target"),
                        network: text(row, "network"),
                        rule: text(row, "rule"),
                        route: row
                            .get("route")
                            .and_then(Value::as_array)
                            .map(|route| {
                                route
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default(),
                        upload: number(row, "upload"),
                        download: number(row, "download"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// One connection of the list as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: u64,
    /// Where the connection goes, over the row.
    pub title: String,
    /// The rule, the route and what was moved, in one line.
    pub details: String,
    /// Whether the connection can be closed now.
    pub enabled: bool,
}

/// The list as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// How many connections are open, or how many of them the search leaves.
    pub count: String,
    pub rows: Vec<Row>,
    /// What stands in place of the rows when there are none.
    pub notice: Option<String>,
    /// Whether there is something to search and something for "close all" to close.
    pub can_search: bool,
    pub can_close_all: bool,
}

/// The list for what is known now. `query` is what the search holds; `busy` holds the connections the core
/// is still closing, and `closing_all` says that it is closing all of them.
pub fn shown(
    language: Language,
    list: &List,
    query: &str,
    busy: &[u64],
    closing_all: bool,
) -> Shown {
    let text = |id: &str| client_strings::text(language, id).to_owned();
    let searched = !query.trim().is_empty();
    let count = if searched {
        client_strings::format(
            language,
            "connections.count.shown",
            &[
                list.connections.len().to_string().as_str(),
                list.total.to_string().as_str(),
            ],
        )
    } else if list.total == 1 {
        text("connections.count.one")
    } else {
        client_strings::format(
            language,
            "connections.count",
            &[list.total.to_string().as_str()],
        )
    };
    let rows = list
        .connections
        .iter()
        .map(|connection| {
            let mut details = Vec::new();
            // Most connections are TCP; only the other kind is named.
            if connection.network.eq_ignore_ascii_case("udp") {
                details.push("UDP".to_owned());
            }
            if !connection.rule.is_empty() {
                details.push(connection.rule.clone());
            }
            if !connection.route.is_empty() {
                details.push(connection.route.join(" → "));
            }
            details.push(client_strings::format(
                language,
                "connections.traffic",
                &[
                    size(connection.upload).as_str(),
                    size(connection.download).as_str(),
                ],
            ));
            Row {
                id: connection.id,
                title: connection.target.clone(),
                details: details.join(" · "),
                enabled: !closing_all && !busy.contains(&connection.id),
            }
        })
        .collect::<Vec<_>>();
    let notice = if !list.running {
        Some(text("connections.stopped"))
    } else if !rows.is_empty() {
        None
    } else if list.total > 0 {
        Some(text("connections.no_match"))
    } else {
        Some(text("connections.empty"))
    };
    Shown {
        count,
        can_search: list.running && list.total > 0,
        can_close_all: list.running && list.total > 0 && !closing_all,
        rows,
        notice,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases() -> Vec<Value> {
        let cases: Vec<Value> = serde_json::from_str(include_str!(
            "../../../crates/client-view/fixtures/cases.json"
        ))
        .unwrap();
        cases
            .into_iter()
            .filter(|case| case["command"] == "view.connections")
            .collect()
    }

    fn sample(index: usize) -> List {
        read(&cases()[index]["answer"])
    }

    #[test]
    fn every_sample_answer_is_read() {
        let cases = cases();
        assert!(cases.len() >= 3, "the connection samples are missing");
        for case in &cases {
            let answer = &case["answer"];
            let list = read(answer);
            assert_eq!(list.running, answer["running"].as_bool().unwrap());
            assert_eq!(list.total, answer["total"].as_u64().unwrap());
            let rows = answer["connections"].as_array().unwrap();
            assert_eq!(list.connections.len(), rows.len());
            for (row, connection) in rows.iter().zip(&list.connections) {
                assert_eq!(connection.id, row["id"].as_u64().unwrap());
                assert_eq!(connection.target, row["target"].as_str().unwrap());
                assert_eq!(connection.network, row["network"].as_str().unwrap());
                assert_eq!(connection.rule, row["rule"].as_str().unwrap());
                let route: Vec<&str> = row["route"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|name| name.as_str().unwrap())
                    .collect();
                assert_eq!(connection.route, route);
                assert_eq!(connection.upload, row["upload"].as_u64().unwrap());
                assert_eq!(connection.download, row["download"].as_u64().unwrap());
            }
        }
        assert_eq!(read(&Value::Null), List::default());
        assert_eq!(read(&json!({ "connections": "many" })), List::default());
    }

    #[test]
    fn a_row_says_where_the_connection_goes_and_how() {
        let shown = shown(Language::English, &sample(0), "", &[], false);
        assert_eq!(shown.count, "3 connections");
        assert_eq!(shown.notice, None);
        assert!(shown.can_search && shown.can_close_all);
        let said: Vec<(&str, &str)> = shown
            .rows
            .iter()
            .map(|row| (row.title.as_str(), row.details.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("example.org", "MODE,global · n2 · ↑ 0 B  ↓ 0 B"),
                ("[2001:db8::1]:53", "UDP · FINAL · DIRECT · ↑ 64 B  ↓ 128 B"),
                (
                    "www.google.com:443",
                    "DOMAIN-SUFFIX,google.com · Proxy → Tokyo · ↑ 1.5 KB  ↓ 1.0 MB"
                ),
            ]
        );
        assert!(shown.rows.iter().all(|row| row.enabled));
        assert_eq!(
            super::shown(Language::Chinese, &sample(0), "", &[], false).count,
            "3 个连接"
        );
    }

    #[test]
    fn a_search_says_how_many_of_how_many() {
        let shown = shown(Language::English, &sample(1), " Tokyo ", &[], false);
        assert_eq!(shown.count, "1 of 3 connections");
        assert_eq!(shown.rows.len(), 1);
        assert_eq!(shown.notice, None);
        // A search that leaves nothing says so, and can still be changed.
        let mut none = sample(1);
        none.connections.clear();
        let shown = super::shown(Language::English, &none, "x", &[], false);
        assert_eq!(shown.count, "0 of 3 connections");
        assert_eq!(
            shown.notice.as_deref(),
            Some("No connection matches the search.")
        );
        assert!(shown.can_search && shown.can_close_all);
    }

    #[test]
    fn nothing_open_and_no_engine_say_so() {
        let stopped = shown(Language::English, &sample(2), "", &[], false);
        assert_eq!(stopped.count, "0 connections");
        assert_eq!(
            stopped.notice.as_deref(),
            Some("Open connections are listed here once connected.")
        );
        assert!(!stopped.can_search && !stopped.can_close_all);
        let idle = List {
            running: true,
            ..List::default()
        };
        let shown = shown(Language::English, &idle, "", &[], false);
        assert_eq!(
            shown.notice.as_deref(),
            Some("No connections are open right now.")
        );
        assert!(!shown.can_close_all);
        let mut one = sample(1);
        one.total = 1;
        assert_eq!(
            super::shown(Language::English, &one, "", &[], false).count,
            "1 connection"
        );
    }

    #[test]
    fn a_connection_the_core_is_closing_cannot_be_closed_again() {
        let shown = shown(Language::English, &sample(0), "", &[2], false);
        let enabled: Vec<(u64, bool)> =
            shown.rows.iter().map(|row| (row.id, row.enabled)).collect();
        assert_eq!(enabled, [(3, true), (2, false), (1, true)]);
        assert!(shown.can_close_all);
        let all = super::shown(Language::English, &sample(0), "", &[], true);
        assert!(all.rows.iter().all(|row| !row.enabled));
        assert!(!all.can_close_all);
    }

    #[test]
    fn closing_names_the_cores_commands() {
        assert_eq!(args("google"), json!({ "query": "google" }));
        assert_eq!(close(7), ("close_connection", json!({ "id": 7 })));
        assert_eq!(CLOSE_ALL, "close_connections");
    }
}
