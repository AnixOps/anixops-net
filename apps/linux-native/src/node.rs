//! One node's dialog without GTK: what the core says about the node, read into what the dialog shows, and
//! what the dialog asks of the core.
//!
//! Whether a node can be changed or removed, and its share link and QR code, are decided by the core (its
//! `node_details` command); whether what the user typed can be saved is decided by the core too
//! (`check_node`). This module reads those answers and turns them into the dialog's sentences ([`shown`]).

use client_strings::Language;
use serde_json::{json, Value};

/// The core's command that answers with one node's details.
pub const COMMAND: &str = "node_details";

/// The arguments of [`COMMAND`], of removing the node, and the start of those of saving it.
pub fn args(id: &str) -> Value {
    json!({ "nodeId": id })
}

/// The core's command that removes a node; its arguments are [`args`].
pub const REMOVE: &str = "remove_node";

/// Why a node cannot be removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// It is a node of a subscription that is not the one in use.
    Other,
    /// It is the only node left.
    Last,
    /// The subscription's policy groups or rules name it.
    Used,
}

/// One node as the core describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub server: String,
    pub port: u64,
    /// Whether its name, server and port can be changed.
    pub editable: bool,
    /// Why it cannot be removed; `None` when it can.
    pub keep: Option<Keep>,
    /// The node as a share link, if it has one.
    pub link: Option<String>,
    /// That link as a QR code: its rows from the top, `#` for a dark module.
    pub qr: Option<Vec<String>>,
}

/// Reads the core's answer. What is missing reads as empty; a reason this window does not know keeps the
/// node from being removed all the same.
pub fn read(answer: &Value) -> Details {
    let text = |name: &str| answer.get(name).and_then(Value::as_str).map(str::to_owned);
    Details {
        id: text("id").unwrap_or_default(),
        name: text("name").unwrap_or_default(),
        protocol: text("protocol").unwrap_or_default(),
        server: text("server").unwrap_or_default(),
        port: answer.get("port").and_then(Value::as_u64).unwrap_or(0),
        editable: answer.get("editable").and_then(Value::as_bool) == Some(true),
        keep: match answer.get("keep").and_then(Value::as_str) {
            None => None,
            Some("last") => Some(Keep::Last),
            Some("used") => Some(Keep::Used),
            Some(_) => Some(Keep::Other),
        },
        link: text("link").filter(|link| !link.is_empty()),
        qr: answer
            .get("qr")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|rows| !rows.is_empty()),
    }
}

/// What the user typed for a node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub name: String,
    pub server: String,
    pub port: String,
}

impl Draft {
    /// The core's command that says whether this can be saved, and its arguments.
    pub fn check(&self) -> (&'static str, Value) {
        (
            "check_node",
            json!({ "name": self.name, "server": self.server, "port": self.port }),
        )
    }

    /// The core's command that saves this for the node `id`, and its arguments.
    pub fn save(&self, id: &str) -> (&'static str, Value) {
        (
            "edit_node",
            json!({ "nodeId": id, "name": self.name, "server": self.server, "port": self.port }),
        )
    }
}

/// Why the core will not save a draft, in the window's language; `None` when it will. `answer` is what
/// the core said to [`Draft::check`].
pub fn problem(language: Language, answer: &Value) -> Option<String> {
    let id = match answer.get("problem").and_then(Value::as_str)? {
        "server.empty" => "node.problem.server.empty",
        "server.invalid" => "node.problem.server.invalid",
        _ => "node.problem.port",
    };
    Some(client_strings::text(language, id).to_owned())
}

/// The dialog as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub id: String,
    /// The node's name, over the dialog, and its protocol under it.
    pub title: String,
    pub protocol: String,
    /// What the three boxes hold when nothing was typed yet.
    pub draft: Draft,
    /// Whether the boxes can be typed in.
    pub editable: bool,
    /// Under the boxes: how long a change lasts, or why nothing can be changed.
    pub note: String,
    /// Why the last save did not work.
    pub problem: Option<String>,
    /// Whether "save" and "remove" can be pressed now.
    pub can_save: bool,
    pub can_remove: bool,
    /// Why the node cannot be removed, for a node that can be changed.
    pub keep: Option<String>,
    /// The share link and its QR code.
    pub link: Option<String>,
    pub qr: Option<Vec<String>>,
}

/// The dialog for what is known now. `busy` says that the core is saving or removing the node; `problem`
/// is why the last save or removal did not work.
pub fn shown(language: Language, details: &Details, busy: bool, problem: Option<&str>) -> Shown {
    let text = |id: &str| client_strings::text(language, id).to_owned();
    Shown {
        id: details.id.clone(),
        title: details.name.clone(),
        protocol: details.protocol.clone(),
        draft: Draft {
            name: details.name.clone(),
            server: details.server.clone(),
            port: details.port.to_string(),
        },
        editable: details.editable,
        note: text(if details.editable {
            "node.edit.note"
        } else {
            "node.readonly"
        }),
        problem: problem.map(str::to_owned),
        can_save: details.editable && !busy,
        can_remove: details.editable && details.keep.is_none() && !busy,
        keep: match details.keep {
            Some(Keep::Last) if details.editable => Some(text("node.keep.last")),
            Some(Keep::Used) if details.editable => Some(text("node.keep.used")),
            _ => None,
        },
        link: details.link.clone(),
        qr: details.qr.clone(),
    }
}

/// A QR code's picture: `side` pixels square, three bytes (red, green, blue) per pixel, dark modules black
/// on white with a quiet zone of four modules around them. `scale` is how many pixels a module is wide.
pub fn picture(rows: &[String], scale: usize) -> (usize, Vec<u8>) {
    const QUIET: usize = 4;
    let modules = rows.len() + 2 * QUIET;
    let side = modules * scale;
    let mut pixels = vec![0xff_u8; side * side * 3];
    for (y, row) in rows.iter().enumerate() {
        for (x, module) in row.chars().enumerate() {
            if module != '#' || x >= rows.len() {
                continue;
            }
            for dy in 0..scale {
                let line = ((y + QUIET) * scale + dy) * side;
                let from = (line + (x + QUIET) * scale) * 3;
                pixels[from..from + scale * 3].fill(0);
            }
        }
    }
    (side, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer() -> Value {
        json!({
            "id": "n1", "name": "HK", "protocol": "Shadowsocks", "server": "hk.example", "port": 8443,
            "editable": true, "keep": null,
            "link": "ss://abc@hk.example:8443#HK",
            "qr": ["#.#", ".#.", "#.#"],
        })
    }

    #[test]
    fn the_cores_answer_is_read() {
        let details = read(&answer());
        assert_eq!(
            details,
            Details {
                id: "n1".to_owned(),
                name: "HK".to_owned(),
                protocol: "Shadowsocks".to_owned(),
                server: "hk.example".to_owned(),
                port: 8443,
                editable: true,
                keep: None,
                link: Some("ss://abc@hk.example:8443#HK".to_owned()),
                qr: Some(vec!["#.#".to_owned(), ".#.".to_owned(), "#.#".to_owned()]),
            }
        );
        assert_eq!(read(&json!({ "keep": "last" })).keep, Some(Keep::Last));
        assert_eq!(read(&json!({ "keep": "used" })).keep, Some(Keep::Used));
        assert_eq!(read(&json!({ "keep": "other" })).keep, Some(Keep::Other));
        // A reason this window does not know still keeps the node.
        assert_eq!(read(&json!({ "keep": "new" })).keep, Some(Keep::Other));
        assert_eq!(read(&Value::Null), Details::default());
        assert_eq!(read(&json!({ "link": "", "qr": [] })), Details::default());
    }

    #[test]
    fn a_node_of_the_subscription_in_use_can_be_changed_and_removed() {
        let shown = shown(Language::English, &read(&answer()), false, None);
        assert_eq!(shown.title, "HK");
        assert_eq!(
            shown.draft,
            Draft {
                name: "HK".to_owned(),
                server: "hk.example".to_owned(),
                port: "8443".to_owned()
            }
        );
        assert!(shown.editable && shown.can_save && shown.can_remove);
        assert_eq!(
            shown.note,
            "Changes and removals last until the subscription is next updated."
        );
        assert_eq!(shown.keep, None);
        assert_eq!(shown.link.as_deref(), Some("ss://abc@hk.example:8443#HK"));
        // While the core works on the node nothing more is asked of it.
        let busy = super::shown(Language::English, &read(&answer()), true, None);
        assert!(busy.editable && !busy.can_save && !busy.can_remove);
    }

    #[test]
    fn a_node_that_must_stay_says_why() {
        let mut details = read(&answer());
        details.keep = Some(Keep::Last);
        let last = shown(Language::Chinese, &details, false, None);
        assert!(last.can_save && !last.can_remove);
        assert_eq!(last.keep.as_deref(), Some("只剩这一个节点，不能删除。"));
        details.keep = Some(Keep::Used);
        assert_eq!(
            shown(Language::English, &details, false, None)
                .keep
                .as_deref(),
            Some(
                "The subscription's policy groups or rules use this node, so it cannot be removed."
            )
        );
        // A node of another subscription: nothing to change, and one sentence says so.
        details.editable = false;
        details.keep = Some(Keep::Other);
        let other = shown(Language::English, &details, false, Some("no"));
        assert!(!other.editable && !other.can_save && !other.can_remove);
        assert_eq!(other.keep, None);
        assert_eq!(
            other.note,
            "Only nodes of the subscription in use can be changed or removed."
        );
        assert_eq!(other.problem.as_deref(), Some("no"));
    }

    #[test]
    fn what_was_typed_names_the_cores_commands() {
        let draft = Draft {
            name: "香港".to_owned(),
            server: "hk.example".to_owned(),
            port: "443".to_owned(),
        };
        assert_eq!(
            draft.check(),
            (
                "check_node",
                json!({ "name": "香港", "server": "hk.example", "port": "443" })
            )
        );
        assert_eq!(
            draft.save("n1"),
            (
                "edit_node",
                json!({ "nodeId": "n1", "name": "香港", "server": "hk.example", "port": "443" })
            )
        );
        assert_eq!(args("n1"), json!({ "nodeId": "n1" }));
    }

    #[test]
    fn the_cores_objection_is_said_in_the_windows_language() {
        let said = |code: &str| problem(Language::English, &json!({ "problem": code }));
        assert_eq!(
            said("server.empty").as_deref(),
            Some("Enter the server address.")
        );
        assert_eq!(
            said("server.invalid").as_deref(),
            Some("The server address cannot contain spaces or slashes.")
        );
        assert_eq!(
            said("port.invalid").as_deref(),
            Some("The port must be a number from 1 to 65535.")
        );
        assert_eq!(
            problem(Language::English, &json!({ "problem": null })),
            None
        );
        assert_eq!(problem(Language::English, &json!({})), None);
    }

    #[test]
    fn a_qr_code_is_drawn_black_on_white_with_a_quiet_zone() {
        let rows = vec!["#.".to_owned(), ".#".to_owned()];
        let (side, pixels) = picture(&rows, 2);
        // Two modules and four of quiet zone on each side, two pixels each.
        assert_eq!(side, 20);
        assert_eq!(pixels.len(), 20 * 20 * 3);
        let at = |x: usize, y: usize| pixels[(y * side + x) * 3];
        assert_eq!(at(0, 0), 0xff);
        assert_eq!(at(7, 7), 0xff);
        assert_eq!((at(8, 8), at(9, 9)), (0, 0));
        assert_eq!((at(10, 8), at(8, 10)), (0xff, 0xff));
        assert_eq!((at(10, 10), at(11, 11)), (0, 0));
        assert_eq!(at(12, 12), 0xff);
    }
}
