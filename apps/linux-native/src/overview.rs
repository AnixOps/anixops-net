//! The overview page without GTK: what the core says, read into what the page shows.
//!
//! What the core's state means (connected or not, which node, whether connecting can work) is decided by
//! the core (its `overview` command, which is `client-view`'s `view.overview` over its own state), the same
//! answer every window gets. This module reads that answer ([`Overview::read`]) and turns it into the
//! page's sentences and numbers ([`shown`]).

use client_strings::Language;
use serde_json::Value;

/// What stands where a number is not known.
pub const UNKNOWN: &str = "—";

/// Whether the client is connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Connected,
    Disconnected,
    /// The last try to connect did not work.
    Failed,
    /// The engine stopped by itself.
    CoreError,
}

/// The engine's counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Traffic {
    /// Bytes per second.
    pub upload_rate: u64,
    pub download_rate: u64,
    /// Bytes since the engine started.
    pub upload_total: u64,
    pub download_total: u64,
    /// Open connections.
    pub connections: u64,
}

/// The core's state as the overview page needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overview {
    pub state: State,
    /// Traffic leaves directly, not through a node.
    pub direct: bool,
    /// The node in use: its name and protocol.
    pub node: Option<(String, String)>,
    /// Connecting can work: a node is chosen, or traffic leaves directly.
    pub can_connect: bool,
    /// Why the client is not connected, in the core's words.
    pub error: Option<String>,
    pub traffic: Option<Traffic>,
}

impl Overview {
    /// The core's command that answers with the overview. It takes no arguments.
    pub const COMMAND: &'static str = "overview";

    /// Reads the core's answer. What is missing reads as "not connected, nothing known".
    pub fn read(answer: &Value) -> Self {
        let text = |value: &Value, name: &str| {
            value
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let number = |value: &Value, name: &str| value.get(name).and_then(Value::as_u64);
        let traffic = answer.get("traffic").and_then(|traffic| {
            Some(Traffic {
                upload_rate: number(traffic, "uploadRate")?,
                download_rate: number(traffic, "downloadRate")?,
                upload_total: number(traffic, "uploadTotal")?,
                download_total: number(traffic, "downloadTotal")?,
                connections: number(traffic, "connections")?,
            })
        });
        Self {
            state: match answer.get("state").and_then(Value::as_str) {
                Some("connected") => State::Connected,
                Some("failed") => State::Failed,
                Some("core_error") => State::CoreError,
                _ => State::Disconnected,
            },
            direct: answer.get("egress").and_then(Value::as_str) == Some("direct"),
            node: answer
                .get("node")
                .filter(|node| node.is_object())
                .map(|node| (text(node, "label"), text(node, "protocol"))),
            can_connect: answer
                .get("canConnect")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            error: answer
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned),
            traffic,
        }
    }
}

/// What the user asked for that the core has not finished yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pending {
    #[default]
    Nothing,
    Connecting,
    Disconnecting,
}

/// What the button does when pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Connect,
    Disconnect,
}

impl Press {
    /// The core's command.
    pub fn command(self) -> &'static str {
        match self {
            Press::Connect => "connect",
            Press::Disconnect => "disconnect",
        }
    }

    pub fn pending(self) -> Pending {
        match self {
            Press::Connect => Pending::Connecting,
            Press::Disconnect => Pending::Disconnecting,
        }
    }
}

/// The page as it is drawn: every sentence and number, ready to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The state in large letters.
    pub headline: String,
    /// A sentence under it, when there is something to explain.
    pub detail: Option<String>,
    /// Why not, in the core's words.
    pub error: Option<String>,
    pub button: String,
    pub press: Press,
    /// Whether pressing can do anything now.
    pub enabled: bool,
    /// The node in use, or the sentence that says there is none.
    pub node: String,
    pub download_rate: String,
    pub upload_rate: String,
    /// "… in total", when known.
    pub download_total: Option<String>,
    pub upload_total: Option<String>,
    pub connections: String,
}

/// The page for what is known now. `overview` is `None` while the core does not answer (it is not running,
/// or the first answer has not come); `failure` is why the user's last press did not work.
pub fn shown(
    language: Language,
    core_answers: bool,
    overview: Option<&Overview>,
    pending: Pending,
    failure: Option<&str>,
) -> Shown {
    let text = |id: &str| client_strings::text(language, id).to_owned();
    let connected = overview.is_some_and(|overview| overview.state == State::Connected);
    let headline = match (pending, overview.map(|overview| overview.state)) {
        (Pending::Connecting, _) => text("overview.state.connecting"),
        (Pending::Disconnecting, _) => text("overview.state.disconnecting"),
        (_, Some(State::Connected)) => text("overview.state.connected"),
        (_, Some(State::Failed)) => text("overview.state.failed"),
        (_, Some(State::CoreError)) => text("overview.state.core_error"),
        (_, Some(State::Disconnected) | None) => text("overview.state.disconnected"),
    };
    let press = if connected {
        Press::Disconnect
    } else {
        Press::Connect
    };
    let node = match overview {
        Some(overview) if overview.direct => text("overview.node.direct"),
        Some(Overview {
            node: Some((label, protocol)),
            ..
        }) if protocol.is_empty() => label.clone(),
        Some(Overview {
            node: Some((label, protocol)),
            ..
        }) => format!("{label} · {protocol}"),
        Some(_) => text("overview.node.none"),
        None => UNKNOWN.to_owned(),
    };
    let traffic = overview.and_then(|overview| overview.traffic);
    let total =
        |bytes: u64| client_strings::format(language, "overview.total", &[size(bytes).as_str()]);
    Shown {
        headline,
        detail: (!core_answers).then(|| text("overview.no_core")),
        error: failure
            .map(str::to_owned)
            .or_else(|| overview.and_then(|overview| overview.error.clone())),
        button: match press {
            Press::Connect => text("overview.connect"),
            Press::Disconnect => text("overview.disconnect"),
        },
        press,
        enabled: core_answers
            && pending == Pending::Nothing
            && overview.is_some_and(|overview| connected || overview.can_connect),
        node,
        download_rate: traffic
            .map_or_else(|| UNKNOWN.to_owned(), |traffic| rate(traffic.download_rate)),
        upload_rate: traffic
            .map_or_else(|| UNKNOWN.to_owned(), |traffic| rate(traffic.upload_rate)),
        download_total: traffic.map(|traffic| total(traffic.download_total)),
        upload_total: traffic.map(|traffic| total(traffic.upload_total)),
        connections: traffic.map_or_else(
            || UNKNOWN.to_owned(),
            |traffic| traffic.connections.to_string(),
        ),
    }
}

/// A number of bytes for people: `980 B`, `1.5 KB`, `734.0 MB`. Units step by 1024.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    // 1023.95 and up would print as "1024.0" of the smaller unit.
    while value >= 1023.95 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Bytes per second for people: `1.5 MB/s`.
pub fn rate(bytes_per_second: u64) -> String {
    format!("{}/s", size(bytes_per_second))
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
            .filter(|case| case["command"] == "view.overview")
            .collect()
    }

    #[test]
    fn every_sample_answer_is_read() {
        let cases = cases();
        assert!(cases.len() >= 8, "the overview samples are missing");
        for case in &cases {
            let answer = &case["answer"];
            let read = Overview::read(answer);
            assert_eq!(
                read.state == State::Connected,
                answer["state"] == "connected",
                "{case}"
            );
            assert_eq!(read.direct, answer["egress"] == "direct", "{case}");
            assert_eq!(read.node.is_some(), answer["node"].is_object(), "{case}");
            assert_eq!(read.can_connect, answer["canConnect"] == true, "{case}");
            assert_eq!(read.error.as_deref(), answer["error"].as_str(), "{case}");
            assert_eq!(
                read.traffic.is_some(),
                answer["traffic"].is_object(),
                "{case}"
            );
        }
        let connected = Overview::read(&cases[0]["answer"]);
        assert_eq!(
            connected.node,
            Some(("Osaka".to_owned(), "vless".to_owned()))
        );
        assert_eq!(
            connected.traffic,
            Some(Traffic {
                upload_rate: 2048,
                download_rate: 1_536_000,
                upload_total: 1_048_576,
                download_total: 734_003_200,
                connections: 12,
            })
        );
        assert_eq!(Overview::read(&Value::Null).state, State::Disconnected);
    }

    fn overview(state: State) -> Overview {
        Overview {
            state,
            direct: false,
            node: Some(("Tokyo".to_owned(), "trojan".to_owned())),
            can_connect: true,
            error: None,
            traffic: None,
        }
    }

    #[test]
    fn a_core_that_does_not_answer_leaves_nothing_to_press() {
        let page = shown(Language::English, false, None, Pending::Nothing, None);
        assert_eq!(page.headline, "Not connected");
        assert_eq!(
            page.detail.as_deref(),
            Some("The core service is not running, so there is nothing to connect.")
        );
        assert!(!page.enabled);
        assert_eq!(page.press, Press::Connect);
        assert_eq!(page.node, UNKNOWN);
        assert_eq!(page.download_rate, UNKNOWN);
        assert_eq!(page.download_total, None);

        // Answering, but the first answer has not come yet.
        let page = shown(Language::English, true, None, Pending::Nothing, None);
        assert_eq!(page.detail, None);
        assert!(!page.enabled);
    }

    #[test]
    fn the_button_connects_or_disconnects_and_waits_while_the_core_works() {
        let off = overview(State::Disconnected);
        let page = shown(Language::English, true, Some(&off), Pending::Nothing, None);
        assert_eq!(
            (page.button.as_str(), page.press, page.enabled),
            ("Connect", Press::Connect, true)
        );
        assert_eq!(page.node, "Tokyo · trojan");

        let page = shown(
            Language::English,
            true,
            Some(&off),
            Pending::Connecting,
            None,
        );
        assert_eq!(page.headline, "Connecting");
        assert!(!page.enabled);

        let mut on = overview(State::Connected);
        on.traffic = Some(Traffic {
            upload_rate: 2048,
            download_rate: 1_536_000,
            upload_total: 1_048_576,
            download_total: 734_003_200,
            connections: 12,
        });
        let page = shown(Language::English, true, Some(&on), Pending::Nothing, None);
        assert_eq!(page.headline, "Connected");
        assert_eq!(
            (page.button.as_str(), page.press, page.enabled),
            ("Disconnect", Press::Disconnect, true)
        );
        assert_eq!(page.download_rate, "1.5 MB/s");
        assert_eq!(page.upload_rate, "2.0 KB/s");
        assert_eq!(page.download_total.as_deref(), Some("700.0 MB in total"));
        assert_eq!(page.upload_total.as_deref(), Some("1.0 MB in total"));
        assert_eq!(page.connections, "12");

        let page = shown(
            Language::Chinese,
            true,
            Some(&on),
            Pending::Disconnecting,
            None,
        );
        assert_eq!(page.headline, "正在断开");
        assert!(!page.enabled);
        assert_eq!(page.download_total.as_deref(), Some("累计 700.0 MB"));
    }

    #[test]
    fn without_a_node_there_is_nothing_to_connect_through_unless_traffic_leaves_directly() {
        let mut none = overview(State::Disconnected);
        none.node = None;
        none.can_connect = false;
        let page = shown(Language::English, true, Some(&none), Pending::Nothing, None);
        assert_eq!(page.node, "No node chosen");
        assert!(!page.enabled);

        none.direct = true;
        none.can_connect = true;
        let page = shown(Language::English, true, Some(&none), Pending::Nothing, None);
        assert_eq!(page.node, "Direct, not through a node");
        assert!(page.enabled);

        // Connected can always be disconnected.
        let mut on = overview(State::Connected);
        on.can_connect = false;
        assert!(shown(Language::English, true, Some(&on), Pending::Nothing, None).enabled);
    }

    #[test]
    fn the_reason_shown_is_the_last_press_first_and_the_cores_otherwise() {
        let mut failed = overview(State::Failed);
        failed.error = Some("the local port 7890 is in use".to_owned());
        let page = shown(
            Language::English,
            true,
            Some(&failed),
            Pending::Nothing,
            None,
        );
        assert_eq!(page.headline, "Could not connect");
        assert_eq!(page.error.as_deref(), Some("the local port 7890 is in use"));
        let page = shown(
            Language::English,
            true,
            Some(&failed),
            Pending::Nothing,
            Some("not for this user"),
        );
        assert_eq!(page.error.as_deref(), Some("not for this user"));
        let stopped = overview(State::CoreError);
        let page = shown(
            Language::English,
            true,
            Some(&stopped),
            Pending::Nothing,
            None,
        );
        assert_eq!(page.headline, "The core stopped unexpectedly");
        assert_eq!(page.error, None);
    }

    #[test]
    fn sizes_step_by_1024_with_one_decimal() {
        for (bytes, expected) in [
            (0, "0 B"),
            (1023, "1023 B"),
            (1024, "1.0 KB"),
            // Exactly half way between two tenths goes to the even one.
            (1280, "1.2 KB"),
            (1331, "1.3 KB"),
            (1536, "1.5 KB"),
            (1_048_575, "1.0 MB"),
            (1_048_576, "1.0 MB"),
            (734_003_200, "700.0 MB"),
            (1_073_741_824, "1.0 GB"),
            (5_497_558_138_880, "5.0 TB"),
            (u64::MAX, "16384.0 PB"),
        ] {
            assert_eq!(size(bytes), expected, "{bytes}");
        }
        assert_eq!(rate(0), "0 B/s");
        assert_eq!(rate(1_536_000), "1.5 MB/s");
    }
}
