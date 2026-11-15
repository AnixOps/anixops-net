//! The proxies page without GTK: what the core says, read into what the page shows.
//!
//! Which nodes a search leaves, how they are split by subscription and what each node's last delay test
//! said is decided by the core (its `proxies` command, which is `client-view`'s `view.proxies` over its own
//! state), the same answer every window gets. This module reads that answer ([`Proxies::read`]) and turns
//! it into the page's rows and sentences ([`shown`]).

use client_strings::Language;
use serde_json::{json, Value};

/// What a node's last delay test said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delay {
    /// Never tested while this core ran.
    Untested,
    /// The test got no answer.
    Failed,
    /// Milliseconds, and whether that is fast.
    Answered { ms: u64, fast: bool },
}

/// One node of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub protocol: String,
    /// The node in use.
    pub selected: bool,
    pub delay: Delay,
}

/// The nodes of one subscription, or (`group` `None`) those that belong to none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub group: Option<String>,
    /// Share links pasted into the client: the heading is this window's wording, not `group`, which is
    /// the core's own name for them.
    pub pasted: bool,
    pub nodes: Vec<Node>,
}

/// The node list as the proxies page needs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proxies {
    /// How many nodes there are.
    pub total: u64,
    /// How many of them the search leaves.
    pub shown: u64,
    pub sections: Vec<Section>,
}

impl Proxies {
    /// The core's command that answers with the node list.
    pub const COMMAND: &'static str = "proxies";

    /// The arguments of [`Proxies::COMMAND`] and of [`Action::Test`] for a search.
    pub fn args(query: &str) -> Value {
        json!({ "query": query })
    }

    /// Reads the core's answer. What is missing reads as "no nodes".
    pub fn read(answer: &Value) -> Self {
        let text = |value: &Value, name: &str| {
            value
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let number = |name: &str| answer.get(name).and_then(Value::as_u64).unwrap_or(0);
        let node = |node: &Value| Node {
            id: text(node, "id"),
            label: text(node, "label"),
            protocol: text(node, "protocol"),
            selected: node
                .get("selected")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            delay: match node.get("delay").filter(|delay| delay.is_object()) {
                None => Delay::Untested,
                Some(delay) => match delay.get("ms").and_then(Value::as_u64) {
                    None => Delay::Failed,
                    Some(ms) => Delay::Answered {
                        ms,
                        fast: delay.get("tone").and_then(Value::as_str) == Some("fast"),
                    },
                },
            },
        };
        let sections = answer
            .get("sections")
            .and_then(Value::as_array)
            .map(|sections| {
                sections
                    .iter()
                    .map(|section| Section {
                        group: section
                            .get("group")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        pasted: section.get("pasted").and_then(Value::as_bool) == Some(true),
                        nodes: section
                            .get("nodes")
                            .and_then(Value::as_array)
                            .map(|nodes| nodes.iter().map(node).collect())
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            total: number("total"),
            shown: number("shown"),
            sections,
        }
    }
}

/// What the user asked for on this page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Import the subscription or share links at this location.
    Import(String),
    /// Test the delay of the nodes this search leaves.
    Test(String),
    /// Use the node with this id.
    Switch(String),
}

impl Action {
    /// The core's command and its arguments.
    pub fn ask(&self) -> (&'static str, Value) {
        match self {
            Action::Import(location) => ("import_subscription", json!({ "location": location })),
            Action::Test(query) => ("test_proxies", Proxies::args(query)),
            Action::Switch(id) => ("switch_node", json!({ "nodeId": id })),
        }
    }
}

/// What the core has not finished yet. The three do not wait for each other: a node can be chosen while a
/// long list is still being tested.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Busy {
    pub importing: bool,
    pub testing: bool,
    pub switching: bool,
}

impl Busy {
    /// Notes that `action` was sent (`true`) or answered (`false`).
    pub fn set(&mut self, action: &Action, busy: bool) {
        match action {
            Action::Import(_) => self.importing = busy,
            Action::Test(_) => self.testing = busy,
            Action::Switch(_) => self.switching = busy,
        }
    }
}

/// How a delay is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Nothing to colour.
    Plain,
    Fast,
    Slow,
    Failed,
}

/// One node of the list as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Which run of nodes the row belongs to, counted from 0, and the heading over that run: the
    /// subscription's name.
    pub section: u32,
    pub heading: String,
    pub id: String,
    pub label: String,
    pub protocol: String,
    /// The delay for people; empty for a node that was never tested.
    pub delay: String,
    pub tone: Tone,
    /// The node in use.
    pub selected: bool,
}

impl Row {
    /// The row said in one breath, for a screen reader: the protocol, the delay, and whether it is in use.
    pub fn description(&self, language: Language) -> String {
        let mut parts = Vec::new();
        if !self.protocol.is_empty() {
            parts.push(self.protocol.as_str());
        }
        if !self.delay.is_empty() {
            parts.push(self.delay.as_str());
        }
        if self.selected {
            parts.push(client_strings::text(language, "proxies.selected"));
        }
        parts.join(", ")
    }
}

/// The page as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub rows: Vec<Row>,
    /// How many nodes there are, or how many of them the search leaves.
    pub count: String,
    /// What stands in place of the list when it has no rows.
    pub notice: Option<String>,
    /// Why the last thing the user asked for did not work, in the core's words.
    pub error: Option<String>,
    pub import: String,
    pub can_import: bool,
    pub test: String,
    pub can_test: bool,
    /// Whether a node can be chosen now.
    pub can_switch: bool,
}

impl Shown {
    /// The place of the node in use among the rows.
    pub fn selected(&self) -> Option<usize> {
        self.rows.iter().position(|row| row.selected)
    }
}

/// The page for what is known now. `proxies` is `None` while the core does not answer.
pub fn shown(
    language: Language,
    core_answers: bool,
    proxies: Option<&Proxies>,
    busy: Busy,
    failure: Option<&str>,
) -> Shown {
    let text = |id: &str| client_strings::text(language, id).to_owned();
    let mut rows = Vec::new();
    let sections = proxies
        .map(|proxies| proxies.sections.as_slice())
        .unwrap_or_default();
    // A subscription without nodes has no heading either.
    for (number, section) in sections
        .iter()
        .filter(|section| !section.nodes.is_empty())
        .enumerate()
    {
        let heading = if section.pasted {
            text("proxies.group.pasted")
        } else {
            section
                .group
                .clone()
                .unwrap_or_else(|| text("proxies.group.none"))
        };
        for node in &section.nodes {
            let (delay, tone) = match node.delay {
                Delay::Untested => (String::new(), Tone::Plain),
                Delay::Failed => (text("proxies.delay.failed"), Tone::Failed),
                Delay::Answered { ms, fast } => (
                    client_strings::format(
                        language,
                        "proxies.delay.ms",
                        &[ms.to_string().as_str()],
                    ),
                    if fast { Tone::Fast } else { Tone::Slow },
                ),
            };
            rows.push(Row {
                section: u32::try_from(number).unwrap_or(u32::MAX),
                heading: heading.clone(),
                id: node.id.clone(),
                label: node.label.clone(),
                protocol: node.protocol.clone(),
                delay,
                tone,
                selected: node.selected,
            });
        }
    }
    let count = match proxies {
        None => String::new(),
        Some(proxies) if proxies.shown == proxies.total => node_count(language, proxies.total),
        Some(proxies) => client_strings::format(
            language,
            "proxies.count.shown",
            &[
                proxies.shown.to_string().as_str(),
                proxies.total.to_string().as_str(),
            ],
        ),
    };
    let notice = match proxies {
        _ if !core_answers => Some(text("overview.no_core")),
        Some(proxies) if proxies.total == 0 => Some(text("proxies.empty")),
        Some(_) if rows.is_empty() => Some(text("proxies.no_match")),
        _ => None,
    };
    Shown {
        can_test: core_answers && !busy.testing && !rows.is_empty(),
        can_switch: core_answers && !busy.switching,
        rows,
        count,
        notice,
        error: failure.map(str::to_owned),
        import: text(if busy.importing {
            "proxies.importing"
        } else {
            "proxies.import"
        }),
        can_import: core_answers && !busy.importing,
        test: text(if busy.testing {
            "proxies.testing"
        } else {
            "proxies.test"
        }),
    }
}

/// "12 nodes", and "1 node" where the language tells one from many.
pub fn node_count(language: Language, nodes: u64) -> String {
    if nodes == 1 {
        client_strings::text(language, "proxies.count.one").to_owned()
    } else {
        client_strings::format(language, "proxies.count", &[nodes.to_string().as_str()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: Busy = Busy {
        importing: false,
        testing: false,
        switching: false,
    };

    fn cases() -> Vec<Value> {
        let cases: Vec<Value> = serde_json::from_str(include_str!(
            "../../../crates/client-view/fixtures/cases.json"
        ))
        .unwrap();
        cases
            .into_iter()
            .filter(|case| case["command"] == "view.proxies")
            .collect()
    }

    #[test]
    fn one_node_is_not_many() {
        assert_eq!(node_count(Language::English, 1), "1 node");
        assert_eq!(node_count(Language::English, 0), "0 nodes");
        assert_eq!(node_count(Language::English, 12), "12 nodes");
        assert_eq!(node_count(Language::Chinese, 1), "1 个节点");
    }

    #[test]
    fn every_sample_answer_is_read() {
        let cases = cases();
        assert!(cases.len() >= 6, "the proxies samples are missing");
        for case in &cases {
            let answer = &case["answer"];
            let read = Proxies::read(answer);
            assert_eq!(Some(read.total), answer["total"].as_u64(), "{case}");
            assert_eq!(Some(read.shown), answer["shown"].as_u64(), "{case}");
            let sections = answer["sections"].as_array().unwrap();
            assert_eq!(read.sections.len(), sections.len(), "{case}");
            for (read, section) in read.sections.iter().zip(sections) {
                assert_eq!(read.group.as_deref(), section["group"].as_str(), "{case}");
                assert_eq!(Some(read.pasted), section["pasted"].as_bool(), "{case}");
                let nodes = section["nodes"].as_array().unwrap();
                assert_eq!(read.nodes.len(), nodes.len(), "{case}");
                for (read, node) in read.nodes.iter().zip(nodes) {
                    assert_eq!(Some(read.id.as_str()), node["id"].as_str(), "{case}");
                    assert_eq!(Some(read.label.as_str()), node["label"].as_str(), "{case}");
                    assert_eq!(Some(read.selected), node["selected"].as_bool(), "{case}");
                    let expected = match (&node["delay"], node["delay"]["ms"].as_u64()) {
                        (Value::Null, _) => Delay::Untested,
                        (_, None) => Delay::Failed,
                        (delay, Some(ms)) => Delay::Answered {
                            ms,
                            fast: delay["tone"] == "fast",
                        },
                    };
                    assert_eq!(read.delay, expected, "{case}");
                }
            }
        }
        // The first sample: two subscriptions and a node outside them, tested fast, slow and without answer.
        let all = Proxies::read(&cases[0]["answer"]);
        assert_eq!((all.total, all.shown), (4, 4));
        assert_eq!(
            all.sections
                .iter()
                .map(|section| (section.group.as_deref(), section.nodes.len()))
                .collect::<Vec<_>>(),
            [(Some("One"), 2), (Some("Two"), 1), (None, 1)]
        );
        assert_eq!(
            all.sections[0].nodes[0],
            Node {
                id: "a".to_owned(),
                label: "Tokyo".to_owned(),
                protocol: "trojan".to_owned(),
                selected: true,
                delay: Delay::Answered {
                    ms: 120,
                    fast: true
                },
            }
        );
        assert_eq!(
            all.sections[0].nodes[1].delay,
            Delay::Answered {
                ms: 800,
                fast: false
            }
        );
        assert_eq!(all.sections[1].nodes[0].delay, Delay::Failed);
        assert_eq!(all.sections[2].nodes[0].delay, Delay::Untested);
        assert_eq!(Proxies::read(&Value::Null), Proxies::default());
    }

    #[test]
    fn the_list_is_drawn_as_nodes_under_their_subscriptions() {
        let all = Proxies::read(&cases()[0]["answer"]);
        let page = shown(Language::English, true, Some(&all), IDLE, None);
        assert_eq!(page.count, "4 nodes");
        assert_eq!(page.notice, None);
        assert_eq!(
            page.rows[0],
            Row {
                section: 0,
                heading: "One".to_owned(),
                id: "a".to_owned(),
                label: "Tokyo".to_owned(),
                protocol: "trojan".to_owned(),
                delay: "120 ms".to_owned(),
                tone: Tone::Fast,
                selected: true,
            }
        );
        let drawn: Vec<(u32, &str, &str, Tone)> = page
            .rows
            .iter()
            .map(|row| {
                (
                    row.section,
                    row.heading.as_str(),
                    row.delay.as_str(),
                    row.tone,
                )
            })
            .collect();
        assert_eq!(
            drawn,
            [
                (0, "One", "120 ms", Tone::Fast),
                (0, "One", "800 ms", Tone::Slow),
                (1, "Two", "No answer", Tone::Failed),
                (2, "Nodes outside subscriptions", "", Tone::Plain),
            ]
        );
        assert_eq!(page.selected(), Some(0));
        assert!(page.can_import && page.can_test && page.can_switch);
        assert_eq!(
            (page.import.as_str(), page.test.as_str()),
            ("Import", "Test latency")
        );
        assert_eq!(
            page.rows[0].description(Language::English),
            "trojan, 120 ms, In use"
        );
        assert_eq!(page.rows[3].description(Language::English), "vless");

        let page = shown(Language::Chinese, true, Some(&all), IDLE, None);
        assert_eq!(page.count, "4 个节点");
        assert_eq!(page.rows[3].heading, "未归入订阅的节点");
        assert_eq!(
            page.rows[2].description(Language::Chinese),
            "shadowsocks, 超时"
        );
    }

    #[test]
    fn subscriptions_of_one_name_stay_apart_and_pasted_links_are_named_in_the_windows_language() {
        // Two subscriptions called "One", then links pasted into the client.
        let all = Proxies::read(&cases()[6]["answer"]);
        let headings = |language| -> Vec<(u32, String)> {
            shown(language, true, Some(&all), IDLE, None)
                .rows
                .iter()
                .map(|row| (row.section, row.heading.clone()))
                .collect()
        };
        assert_eq!(
            headings(Language::English),
            [
                (0, "One".to_owned()),
                (1, "One".to_owned()),
                (2, "Pasted links".to_owned()),
                (2, "Pasted links".to_owned()),
            ]
        );
        assert_eq!(headings(Language::Chinese)[3], (2, "粘贴的链接".to_owned()));
        assert_eq!(
            headings(Language::TraditionalChinese)[3],
            (2, "貼上的連結".to_owned())
        );
    }

    #[test]
    fn a_search_says_how_many_it_leaves_and_when_it_leaves_none() {
        let cases = cases();
        let some = Proxies::read(&cases[1]["answer"]);
        let page = shown(Language::English, true, Some(&some), IDLE, None);
        assert_eq!(page.count, "2 of 4 nodes");
        assert_eq!(page.notice, None);
        // The node in use is not among the ones the search leaves.
        assert_eq!(page.selected(), None);
        assert_eq!(
            page.rows.iter().map(|row| row.section).collect::<Vec<_>>(),
            [0, 1]
        );

        let none = Proxies::read(&cases[3]["answer"]);
        let page = shown(Language::English, true, Some(&none), IDLE, None);
        assert_eq!(page.count, "0 of 4 nodes");
        assert_eq!(page.notice.as_deref(), Some("No node matches the search."));
        assert!(page.rows.is_empty());
        assert!(!page.can_test);
        assert!(page.can_import);
    }

    #[test]
    fn without_nodes_or_without_the_core_the_page_says_so() {
        let empty = Proxies::default();
        let page = shown(Language::English, true, Some(&empty), IDLE, None);
        assert_eq!(page.count, "0 nodes");
        assert_eq!(
            page.notice.as_deref(),
            Some("No nodes yet. Import a subscription and its nodes appear here.")
        );
        assert!(page.can_import);
        assert!(!page.can_test);

        let page = shown(Language::English, false, None, IDLE, None);
        assert_eq!(page.count, "");
        assert_eq!(
            page.notice.as_deref(),
            Some("The core service is not running, so there is nothing to connect.")
        );
        assert!(!page.can_import && !page.can_test && !page.can_switch);

        // Answering, but the first answer has not come yet: nothing to say, nothing to test.
        let page = shown(Language::English, true, None, IDLE, None);
        assert_eq!(page.notice, None);
        assert!(page.can_import);
        assert!(!page.can_test);
    }

    #[test]
    fn what_the_core_is_busy_with_cannot_be_asked_again_and_the_rest_can() {
        let all = Proxies::read(&cases()[0]["answer"]);
        let mut busy = IDLE;
        busy.set(&Action::Import(String::new()), true);
        let page = shown(Language::English, true, Some(&all), busy, None);
        assert_eq!(
            (page.can_import, page.can_test, page.can_switch),
            (false, true, true)
        );
        assert_eq!(page.import, "Importing");

        busy.set(&Action::Test(String::new()), true);
        busy.set(&Action::Import(String::new()), false);
        let page = shown(Language::English, true, Some(&all), busy, None);
        assert_eq!(
            (page.can_import, page.can_test, page.can_switch),
            (true, false, true)
        );
        assert_eq!(
            (page.import.as_str(), page.test.as_str()),
            ("Import", "Testing")
        );

        busy.set(&Action::Switch(String::new()), true);
        let page = shown(Language::English, true, Some(&all), busy, None);
        assert_eq!(
            (page.can_import, page.can_test, page.can_switch),
            (true, false, false)
        );
        busy.set(&Action::Test(String::new()), false);
        busy.set(&Action::Switch(String::new()), false);
        assert_eq!(busy, IDLE);

        let page = shown(
            Language::English,
            true,
            Some(&all),
            IDLE,
            Some("the link gave no nodes"),
        );
        assert_eq!(page.error.as_deref(), Some("the link gave no nodes"));
    }

    #[test]
    fn actions_name_the_cores_commands() {
        assert_eq!(
            Action::Import("ss://x".to_owned()).ask(),
            ("import_subscription", json!({ "location": "ss://x" }))
        );
        assert_eq!(
            Action::Test("tokyo".to_owned()).ask(),
            ("test_proxies", json!({ "query": "tokyo" }))
        );
        assert_eq!(
            Action::Switch("a".to_owned()).ask(),
            ("switch_node", json!({ "nodeId": "a" }))
        );
    }
}
