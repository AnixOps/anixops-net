//! System notifications of the desktop and Android clients, as on iOS (`AppNotifications.swift`,
//! `NotificationSettings`): what plugin scripts post (`$notification.post`, read from the engine with
//! `{"cmd":"script_notifications"}`), an unexpected disconnect and a failed subscription update. Each kind has
//! its own switch, all on by default. The texts follow the interface language, which the window passes in.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Script notifications posted per read; the rest of a burst is dropped, as on iOS.
pub const SCRIPT_NOTICES_PER_READ: usize = 8;

fn on() -> bool {
    true
}

/// The three switches and the language of the texts, in the interface's `NotificationSettings` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "on")]
    pub scripts: bool,
    #[serde(default = "on")]
    pub connection: bool,
    #[serde(default = "on")]
    pub subscriptions: bool,
    /// `zh`, `zh-Hant` or `en`.
    #[serde(default)]
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scripts: true,
            connection: true,
            subscriptions: true,
            language: String::new(),
        }
    }
}

impl Settings {
    /// The saved settings; all on when the file is missing or unreadable.
    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        fs::write(path, bytes)
    }

    /// The connection notice when the engine stopped without the user disconnecting; `recovering` when the
    /// client starts it again by itself.
    pub fn disconnected(&self, recovering: bool) -> Option<Notice> {
        if !self.connection {
            return None;
        }
        let (title, body) = match (self.language(), recovering) {
            (Language::English, false) => (
                "Disconnected",
                "The connection stopped unexpectedly. Open AnixOps Net to connect again.",
            ),
            (Language::English, true) => (
                "Disconnected",
                "The connection stopped unexpectedly; reconnecting.",
            ),
            (Language::Traditional, false) => {
                ("連線已中斷", "連線意外中斷，打開 AnixOps Net 重新連線。")
            }
            (Language::Traditional, true) => ("連線已中斷", "連線意外中斷，正在重新連線。"),
            (Language::Simplified, false) => {
                ("连接已断开", "连接意外断开，打开 AnixOps Net 重新连接。")
            }
            (Language::Simplified, true) => ("连接已断开", "连接意外断开，正在重新连接。"),
        };
        Some(Notice::new(title, body))
    }

    /// The notice when updating subscription `name` failed with `error`.
    pub fn subscription_failed(&self, name: &str, error: &str) -> Option<Notice> {
        if !self.subscriptions {
            return None;
        }
        let title = match self.language() {
            Language::English => format!("Updating {name} failed"),
            Language::Traditional => format!("「{name}」更新失敗"),
            Language::Simplified => format!("“{name}”更新失败"),
        };
        Some(Notice {
            title,
            body: error.chars().take(200).collect(),
            open_url: None,
        })
    }

    /// What scripts posted, from the engine's answer to `{"cmd":"script_notifications"}`; none while script
    /// notifications are off (the answer is still read, so the engine's queue does not fill).
    pub fn script_notices(&self, answer: &Value) -> Vec<Notice> {
        if !self.scripts {
            return Vec::new();
        }
        answer
            .get("notifications")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(SCRIPT_NOTICES_PER_READ)
            .filter_map(|item| {
                let text = |key: &str| {
                    item.get(key)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                };
                let (title, subtitle, content) = (text("title"), text("subtitle"), text("content"));
                if title.is_empty() && subtitle.is_empty() && content.is_empty() {
                    return None;
                }
                let body = [subtitle, content]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                let open_url = Some(text("open_url")).filter(|url| {
                    let lower = url.to_ascii_lowercase();
                    lower.starts_with("https://") || lower.starts_with("http://")
                });
                Some(Notice {
                    title: if title.is_empty() {
                        "AnixOps Net".to_string()
                    } else {
                        title
                    },
                    body,
                    open_url,
                })
            })
            .collect()
    }

    fn language(&self) -> Language {
        match self.language.as_str() {
            "en" => Language::English,
            "zh-Hant" => Language::Traditional,
            _ => Language::Simplified,
        }
    }
}

enum Language {
    Simplified,
    Traditional,
    English,
}

/// One system notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// An http(s) link a script attached; opened when the notification is clicked where the platform allows.
    pub open_url: Option<String>,
}

impl Notice {
    fn new(title: &str, body: &str) -> Self {
        Self {
            title: title.to_string(),
            body: body.to_string(),
            open_url: None,
        }
    }
}

/// Tells an unexpected stop from a disconnect: the engine counts as expected to run from a successful connect
/// until the user disconnects, and a stop in between is reported once.
#[derive(Debug, Default)]
pub struct DisconnectWatch {
    expected: bool,
}

impl DisconnectWatch {
    pub fn connected(&mut self) {
        self.expected = true;
    }

    pub fn disconnected(&mut self) {
        self.expected = false;
    }

    /// `true` once when the engine is found stopped while it was expected to run.
    pub fn stopped_unexpectedly(&mut self, running: bool) -> bool {
        let stopped = self.expected && !running;
        if stopped {
            self.expected = false;
        }
        stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_default_on_and_round_trip() {
        let path = std::env::temp_dir()
            .join(format!("app-notifications-{}", std::process::id()))
            .join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let settings = Settings {
            scripts: false,
            language: "en".to_string(),
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        let partial: Settings = serde_json::from_str(r#"{"connection":false}"#).unwrap();
        assert!(partial.scripts && !partial.connection && partial.subscriptions);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn texts_follow_the_switches_and_language() {
        let mut settings = Settings::default();
        assert_eq!(settings.disconnected(false).unwrap().title, "连接已断开");
        settings.language = "en".to_string();
        assert_eq!(
            settings
                .subscription_failed("HK", "timed out")
                .unwrap()
                .title,
            "Updating HK failed"
        );
        settings.language = "zh-Hant".to_string();
        assert!(settings
            .disconnected(true)
            .unwrap()
            .body
            .contains("正在重新連線"));
        settings.connection = false;
        settings.subscriptions = false;
        assert!(settings.disconnected(false).is_none());
        assert!(settings.subscription_failed("HK", "x").is_none());
    }

    #[test]
    fn reads_script_notifications() {
        let answer = json!({"notifications": [
            {"title": "签到", "subtitle": "成功", "content": "+1", "open_url": "https://a.example"},
            {"title": "", "subtitle": "", "content": ""},
            {"title": "", "subtitle": "", "content": "only body", "open_url": "javascript:alert(1)"}
        ]});
        let mut settings = Settings::default();
        let notices = settings.script_notices(&answer);
        assert_eq!(notices.len(), 2);
        assert_eq!(notices[0].body, "成功\n+1");
        assert_eq!(notices[0].open_url.as_deref(), Some("https://a.example"));
        assert_eq!(notices[1].title, "AnixOps Net");
        assert_eq!(notices[1].open_url, None);
        settings.scripts = false;
        assert!(settings.script_notices(&answer).is_empty());
    }

    #[test]
    fn reports_an_unexpected_stop_once() {
        let mut watch = DisconnectWatch::default();
        assert!(!watch.stopped_unexpectedly(false));
        watch.connected();
        assert!(!watch.stopped_unexpectedly(true));
        assert!(watch.stopped_unexpectedly(false));
        assert!(!watch.stopped_unexpectedly(false));
        watch.connected();
        watch.disconnected();
        assert!(!watch.stopped_unexpectedly(false));
    }
}
