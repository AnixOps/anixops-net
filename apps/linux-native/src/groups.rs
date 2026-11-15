//! The policy groups of the proxies page without GTK: what the core says, read into what each row shows.
//!
//! Which groups there are, what kind each is and which member it uses is decided by the core (its `groups`
//! command, which is `client-view`'s `view.groups` over its own state), the same answer every window gets.
//! This module reads that answer ([`read`]) and turns it into the rows and sentences of the list
//! ([`shown`]).

use client_strings::Language;
use serde_json::{json, Value};

/// The core's command that answers with the policy groups. It takes no arguments.
pub const COMMAND: &str = "groups";

/// One member of a group.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Member {
    /// What the core is told when the member is chosen.
    pub id: String,
    pub label: String,
}

/// How a group comes to the member it uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// The user chooses.
    Select,
    /// The fastest member.
    Auto,
    /// The first member that answers.
    Fallback,
    /// A kind this window does not know.
    #[default]
    Other,
}

/// One policy group as the core describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub tag: String,
    pub kind: Kind,
    /// Whether the user chooses the member.
    pub choosable: bool,
    pub members: Vec<Member>,
    /// Which of the members is in use, if the core says.
    pub selected: Option<usize>,
}

/// Reads the core's answer. What is missing reads as "no groups".
pub fn read(answer: &Value) -> Vec<Group> {
    let text = |row: &Value, name: &str| {
        row.get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let flag = |row: &Value, name: &str| row.get(name).and_then(Value::as_bool) == Some(true);
    answer
        .get("groups")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let members = row
                        .get("members")
                        .and_then(Value::as_array)
                        .map(Vec::as_slice)
                        .unwrap_or_default();
                    Group {
                        tag: text(row, "tag"),
                        kind: match row.get("kind").and_then(Value::as_str) {
                            Some("select") => Kind::Select,
                            Some("auto") => Kind::Auto,
                            Some("fallback") => Kind::Fallback,
                            _ => Kind::Other,
                        },
                        choosable: flag(row, "choosable"),
                        selected: members.iter().position(|member| flag(member, "selected")),
                        members: members
                            .iter()
                            .map(|member| Member {
                                id: text(member, "id"),
                                label: text(member, "label"),
                            })
                            .collect(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The user chose a member for a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The group's name.
    pub group: String,
    /// The member's id.
    pub member: String,
}

impl Choice {
    /// The core's command and its arguments.
    pub fn ask(&self) -> (&'static str, Value) {
        (
            "select_native_group_outbound",
            json!({ "groupTag": self.group, "outboundTag": self.member }),
        )
    }
}

/// One group of the list as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The group's name, over the row.
    pub tag: String,
    /// How the group chooses and what it uses, in one line.
    pub details: String,
    /// What can be chosen, as (id, label), in the subscription's order; empty for a group the user does
    /// not choose in.
    pub options: Vec<Member>,
    /// Which of the options is in use.
    pub chosen: Option<usize>,
    /// Whether a member can be chosen now.
    pub enabled: bool,
}

/// The list for what is known now. `busy` holds the names of the groups the core is still changing.
pub fn shown(language: Language, groups: &[Group], busy: &[String]) -> Vec<Row> {
    let text = |id: &str| client_strings::text(language, id).to_owned();
    groups
        .iter()
        .map(|group| {
            let mut details = Vec::new();
            match group.kind {
                Kind::Select => details.push(text("groups.kind.select")),
                Kind::Auto => details.push(text("groups.kind.auto")),
                Kind::Fallback => details.push(text("groups.kind.fallback")),
                Kind::Other => {}
            }
            details.push(match group.selected.and_then(|at| group.members.get(at)) {
                Some(member) => {
                    client_strings::format(language, "groups.using", &[member.label.as_str()])
                }
                None => text("groups.undecided"),
            });
            Row {
                tag: group.tag.clone(),
                details: details.join(" · "),
                options: if group.choosable {
                    group.members.clone()
                } else {
                    Vec::new()
                },
                chosen: group.selected.filter(|_| group.choosable),
                enabled: group.choosable && !busy.contains(&group.tag),
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
            .filter(|case| case["command"] == "view.groups")
            .collect()
    }

    #[test]
    fn every_sample_answer_is_read() {
        let cases = cases();
        assert!(cases.len() >= 3, "the policy group samples are missing");
        for case in &cases {
            let rows = case["answer"]["groups"].as_array().unwrap();
            let read = read(&case["answer"]);
            assert_eq!(read.len(), rows.len(), "{case}");
            for (read, row) in read.iter().zip(rows) {
                assert_eq!(Some(read.tag.as_str()), row["tag"].as_str(), "{case}");
                assert_eq!(Some(read.choosable), row["choosable"].as_bool(), "{case}");
                let members = row["members"].as_array().unwrap();
                assert_eq!(read.members.len(), members.len(), "{case}");
                for (read, member) in read.members.iter().zip(members) {
                    assert_eq!(Some(read.id.as_str()), member["id"].as_str(), "{case}");
                    assert_eq!(
                        Some(read.label.as_str()),
                        member["label"].as_str(),
                        "{case}"
                    );
                }
                // The member the answer marks is the one it names.
                assert_eq!(
                    read.selected.map(|at| read.members[at].id.as_str()),
                    row["selected"].as_str(),
                    "{case}"
                );
            }
        }
        let first = read(&cases[0]["answer"]);
        assert_eq!(
            first.iter().map(|group| group.kind).collect::<Vec<_>>(),
            [Kind::Select, Kind::Auto, Kind::Fallback]
        );
        assert_eq!(read(&cases[1]["answer"])[0].kind, Kind::Other);
    }

    #[test]
    fn an_answer_that_says_nothing_reads_as_no_groups() {
        for answer in [
            Value::Null,
            json!({}),
            json!([]),
            json!({ "groups": "many" }),
        ] {
            assert_eq!(read(&answer), Vec::new(), "{answer}");
        }
        assert_eq!(read(&json!({ "groups": [{}] })), vec![Group::default()]);
    }

    #[test]
    fn a_row_says_how_the_group_chooses_and_what_it_uses() {
        let groups = read(&cases()[0]["answer"]);
        let rows = shown(Language::English, &groups, &[]);
        assert_eq!(rows[0].tag, "Proxy");
        assert_eq!(rows[0].details, "Manual · Using Osaka");
        assert_eq!(
            rows[0]
                .options
                .iter()
                .map(|member| (member.id.as_str(), member.label.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Auto", "Auto"),
                ("n1", "Tokyo"),
                ("n2", "Osaka"),
                ("DIRECT", "DIRECT")
            ]
        );
        assert_eq!(rows[0].chosen, Some(2));
        assert!(rows[0].enabled);
        // A group that chooses by itself offers nothing, and says so when it has not chosen yet.
        assert_eq!(rows[1].details, "Fastest · Not chosen yet");
        assert!(rows[1].options.is_empty());
        assert_eq!(rows[1].chosen, None);
        assert!(!rows[1].enabled);
        assert_eq!(rows[2].details, "Fallback · Using Tokyo");
        assert!(!rows[2].enabled);
        // A kind this window does not know is not named.
        let other = read(&cases()[1]["answer"]);
        assert_eq!(
            shown(Language::English, &other, &[])[0].details,
            "Not chosen yet"
        );
        assert_eq!(
            shown(Language::Chinese, &groups, &[])[0].details,
            "手动选择 · 正在用 Osaka"
        );
    }

    #[test]
    fn a_group_the_core_is_changing_cannot_be_asked_again() {
        let groups = read(&cases()[0]["answer"]);
        let rows = shown(Language::English, &groups, &["Proxy".to_owned()]);
        assert!(!rows[0].enabled);
        assert_eq!(rows[0].chosen, Some(2));
    }

    #[test]
    fn a_choice_names_the_cores_command() {
        let choice = Choice {
            group: "Proxy".to_owned(),
            member: "n1".to_owned(),
        };
        assert_eq!(
            choice.ask(),
            (
                "select_native_group_outbound",
                json!({ "groupTag": "Proxy", "outboundTag": "n1" })
            )
        );
    }
}
