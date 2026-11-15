//! The subscription list of the proxies page without GTK: what the core says, read into what each row
//! shows.
//!
//! What a subscription's row says (how much was used, when it expires and whether that has passed) is
//! decided by the core (its `subscriptions` command, which is `client-view`'s `view.subscriptions` over its
//! own state), the same answer every window gets. This module reads that answer ([`read`]) and turns it into
//! the rows and sentences of the list ([`shown`]).

use client_strings::Language;
use serde_json::{json, Value};

use crate::overview::size;

/// The core's command that answers with the subscriptions. It takes no arguments.
pub const COMMAND: &str = "subscriptions";

/// One subscription as the core describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    /// Share links pasted into the client: the row is named in this window's wording, not by `name`,
    /// which is the core's own name for them.
    pub pasted: bool,
    /// Whether it is a link that can be downloaded again.
    pub refreshable: bool,
    pub nodes: Option<u64>,
    /// When it was last downloaded, in the core's words.
    pub updated: Option<String>,
    /// Why the last update did not work, in the core's words.
    pub error: Option<String>,
    /// Bytes used and bytes allowed.
    pub used: Option<u64>,
    pub total: Option<u64>,
    /// The day it expires, and whether that has passed.
    pub expire_date: Option<String>,
    pub expired: bool,
}

/// Reads the core's answer. What is missing reads as "no subscriptions".
pub fn read(answer: &Value) -> Vec<Subscription> {
    let text = |row: &Value, name: &str| row.get(name).and_then(Value::as_str).map(str::to_owned);
    let number = |row: &Value, name: &str| row.get(name).and_then(Value::as_u64);
    let flag = |row: &Value, name: &str| row.get(name).and_then(Value::as_bool) == Some(true);
    answer
        .get("subscriptions")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| Subscription {
                    id: text(row, "id").unwrap_or_default(),
                    name: text(row, "name").unwrap_or_default(),
                    pasted: flag(row, "pasted"),
                    refreshable: flag(row, "refreshable"),
                    nodes: number(row, "nodes"),
                    updated: text(row, "updated"),
                    error: text(row, "error"),
                    used: number(row, "used"),
                    total: number(row, "total"),
                    expire_date: text(row, "expireDate"),
                    expired: flag(row, "expired"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What the user asked for on one subscription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Download the subscription with this id again.
    Update(String),
    /// Call the subscription with this id by this name.
    Rename(String, String),
    /// Remove the subscription with this id and its nodes.
    Remove(String),
}

impl Change {
    /// The core's command and its arguments.
    pub fn ask(&self) -> (&'static str, Value) {
        match self {
            Change::Update(id) => ("update_subscription_source", json!({ "id": id })),
            Change::Rename(id, name) => ("rename_subscription", json!({ "id": id, "name": name })),
            Change::Remove(id) => ("remove_subscription", json!({ "id": id })),
        }
    }

    /// The subscription the change is about.
    pub fn id(&self) -> &str {
        match self {
            Change::Update(id) | Change::Rename(id, _) | Change::Remove(id) => id,
        }
    }
}

/// One subscription of the list as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    /// The name over the row.
    pub title: String,
    /// What is known about it, in one line: nodes, last update, use, expiry.
    pub details: String,
    /// Why the last update did not work.
    pub error: Option<String>,
    /// What the update button says; `None` for a subscription that cannot be downloaded again.
    pub update: Option<String>,
    /// Whether anything can be asked of this subscription now.
    pub enabled: bool,
    /// What the box for a new name starts with: the name as it is, empty for pasted links, which have
    /// none of their own.
    pub name: String,
}

/// The list for what is known now. `busy` holds the ids of the subscriptions the core is still working on.
pub fn shown(language: Language, subscriptions: &[Subscription], busy: &[String]) -> Vec<Row> {
    let text = |id: &str| client_strings::text(language, id).to_owned();
    let format = |id: &str, values: &[&str]| client_strings::format(language, id, values);
    subscriptions
        .iter()
        .map(|subscription| {
            let working = busy.contains(&subscription.id);
            let mut details = Vec::new();
            if let Some(nodes) = subscription.nodes {
                details.push(format("proxies.count", &[nodes.to_string().as_str()]));
            }
            if let Some(updated) = &subscription.updated {
                details.push(format("subscriptions.updated", &[updated.as_str()]));
            }
            match (subscription.used, subscription.total) {
                (Some(used), Some(total)) => details.push(format(
                    "subscriptions.usage",
                    &[size(used).as_str(), size(total).as_str()],
                )),
                (Some(used), None) => {
                    details.push(format("subscriptions.usage.used", &[size(used).as_str()]));
                }
                _ => {}
            }
            if subscription.expired {
                details.push(text("subscriptions.expired"));
            } else if let Some(day) = &subscription.expire_date {
                details.push(format("subscriptions.expire", &[day.as_str()]));
            }
            Row {
                id: subscription.id.clone(),
                title: if subscription.pasted {
                    text("proxies.group.pasted")
                } else {
                    subscription.name.clone()
                },
                details: details.join(" · "),
                error: subscription
                    .error
                    .as_deref()
                    .map(|error| format("subscriptions.error", &[error])),
                update: subscription.refreshable.then(|| {
                    text(if working {
                        "subscriptions.updating"
                    } else {
                        "subscriptions.update"
                    })
                }),
                enabled: !working,
                name: if subscription.pasted {
                    String::new()
                } else {
                    subscription.name.clone()
                },
            }
        })
        .collect()
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
            .filter(|case| case["command"] == "view.subscriptions")
            .collect()
    }

    #[test]
    fn every_sample_answer_is_read() {
        let cases = cases();
        assert!(cases.len() >= 3, "the subscription samples are missing");
        for case in &cases {
            let rows = case["answer"]["subscriptions"].as_array().unwrap();
            let read = read(&case["answer"]);
            assert_eq!(read.len(), rows.len(), "{case}");
            for (read, row) in read.iter().zip(rows) {
                assert_eq!(Some(read.id.as_str()), row["id"].as_str(), "{case}");
                assert_eq!(Some(read.name.as_str()), row["name"].as_str(), "{case}");
                assert_eq!(Some(read.pasted), row["pasted"].as_bool(), "{case}");
                assert_eq!(
                    Some(read.refreshable),
                    row["refreshable"].as_bool(),
                    "{case}"
                );
                assert_eq!(read.nodes, row["nodes"].as_u64(), "{case}");
                assert_eq!(read.updated.as_deref(), row["updated"].as_str(), "{case}");
                assert_eq!(read.error.as_deref(), row["error"].as_str(), "{case}");
                assert_eq!(read.used, row["used"].as_u64(), "{case}");
                assert_eq!(read.total, row["total"].as_u64(), "{case}");
                assert_eq!(
                    read.expire_date.as_deref(),
                    row["expireDate"].as_str(),
                    "{case}"
                );
                assert_eq!(Some(read.expired), row["expired"].as_bool(), "{case}");
            }
        }
    }

    #[test]
    fn an_answer_that_says_nothing_reads_as_no_subscriptions() {
        for answer in [
            Value::Null,
            json!({}),
            json!([]),
            json!({ "subscriptions": "many" }),
        ] {
            assert_eq!(read(&answer), Vec::new(), "{answer}");
        }
        // A row that says nothing is still a row, with nothing known about it.
        assert_eq!(
            read(&json!({ "subscriptions": [{}] })),
            vec![Subscription::default()]
        );
    }

    #[test]
    fn a_row_says_what_is_known_in_one_line() {
        let before = read(&cases()[0]["answer"]);
        let rows = shown(Language::English, &before, &[]);
        assert_eq!(
            rows[0],
            Row {
                id: "s1".to_owned(),
                title: "One".to_owned(),
                details: "12 nodes · Updated 2026-11-15 12:00 · 1000 B of 4.9 KB used · Expires 2033-05-18"
                    .to_owned(),
                error: None,
                update: Some("Update".to_owned()),
                enabled: true,
                name: "One".to_owned(),
            }
        );
        // Nothing known but why the last update did not work.
        assert_eq!(rows[1].title, "other.example");
        assert_eq!(rows[1].details, "");
        assert_eq!(
            rows[1].error.as_deref(),
            Some("The last update did not work: the server answered 403")
        );
        // Pasted links: this window's wording for the name, nothing to download again, and no name of
        // their own to start a new one from.
        assert_eq!(rows[2].title, "Pasted links");
        assert_eq!(
            rows[2].details,
            "2 nodes · Updated 2026-11-15 12:02 · 7 B used"
        );
        assert_eq!(rows[2].update, None);
        assert_eq!(rows[2].name, "");
        assert_eq!(
            shown(Language::Chinese, &before, &[])[2].title,
            "粘贴的链接"
        );

        // The same subscription one second later: the day has come.
        let after = read(&cases()[1]["answer"]);
        assert_eq!(
            shown(Language::Chinese, &after, &[])[0].details,
            "12 个节点 · 更新于 2026-11-15 12:00 · 已用 1000 B / 4.9 KB · 已到期"
        );
    }

    #[test]
    fn a_subscription_the_core_is_working_on_cannot_be_asked_again() {
        let subscriptions = read(&cases()[0]["answer"]);
        let rows = shown(Language::English, &subscriptions, &["s1".to_owned()]);
        assert_eq!(rows[0].update.as_deref(), Some("Updating"));
        assert!(!rows[0].enabled);
        // The others do not wait for it.
        assert_eq!(rows[1].update.as_deref(), Some("Update"));
        assert!(rows[1].enabled);
        assert!(rows[2].enabled);
    }

    #[test]
    fn changes_name_the_cores_commands() {
        assert_eq!(
            Change::Update("s1".to_owned()).ask(),
            ("update_subscription_source", json!({ "id": "s1" }))
        );
        assert_eq!(
            Change::Rename("s1".to_owned(), "Work".to_owned()).ask(),
            ("rename_subscription", json!({ "id": "s1", "name": "Work" }))
        );
        assert_eq!(
            Change::Remove("s1".to_owned()).ask(),
            ("remove_subscription", json!({ "id": "s1" }))
        );
        assert_eq!(
            Change::Rename("s1".to_owned(), "Work".to_owned()).id(),
            "s1"
        );
    }
}
