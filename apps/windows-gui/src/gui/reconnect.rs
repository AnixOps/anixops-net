//! Reconnecting the window's engine after a changed setting, as one step that either takes the new setting or
//! leaves the earlier working one running.
//!
//! The engine has to stop before it starts again (the same ports, the same TUN adapter), so a new setting that
//! does not start (a Wintun adapter the security software blocks, a busy port, a broken plugin) used to leave
//! the computer disconnected, with the proxy given back and the earlier setting lost. [`reconnect`] puts the
//! earlier setting back and starts the engine on it in that case, and says which of the outcomes happened, so
//! the window tells the user the truth: the new setting runs, the earlier one runs again, or it is disconnected.
//!
//! Capture only lives as long as the engine does: what live capture holds is saved as a session before the
//! engine stops, and capture is turned on again, with its budget, in the engine that starts
//! (`desktop_mitm::sessions::preserve`).

use desktop_mitm::sessions::CaptureCarry;

/// The steps of a reconnect, so the order can be tested without an engine.
pub trait Steps {
    type Connected;
    /// Whether an engine runs now; nothing is done otherwise.
    fn running(&mut self) -> Result<bool, String>;
    /// Saves what capture holds (see the module) and stops the engine, giving the system proxy back.
    fn stop(&mut self) -> CaptureCarry;
    /// Starts the engine on the new setting.
    fn start_new(&mut self) -> Result<Self::Connected, String>;
    /// Puts the earlier setting back where the next start reads it; `None` when the change cannot be undone
    /// (the plugin library, the decryption list).
    fn restore(&mut self) -> Option<Result<(), String>>;
    /// Starts the engine on the earlier setting.
    fn start_previous(&mut self) -> Result<Self::Connected, String>;
    /// Turns capture on again in the engine that started.
    fn resume_capture(&mut self, carry: &CaptureCarry);
}

/// How a reconnect ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<C> {
    /// No engine was running: the change applies with the next connection.
    NotRunning,
    /// The engine runs the new setting.
    Applied(C),
    /// The new setting did not start (`error`); the earlier one was put back and runs.
    Restored { error: String, connected: C },
    /// The new setting did not start (`error`) and the earlier one could not run again (`restore_error`, or
    /// `None` when there was no earlier setting to go back to): the engine is stopped.
    Disconnected {
        error: String,
        restore_error: Option<String>,
        /// Whether the earlier setting was put back, so the next connection uses it.
        restored: bool,
    },
}

/// A reconnect's outcome and what happened to capture.
#[derive(Debug, PartialEq, Eq)]
pub struct Report<C> {
    pub outcome: Outcome<C>,
    pub capture: CaptureCarry,
}

/// Stops a running engine and starts it on the new setting; when that fails, puts the earlier setting back
/// and starts the engine on it. The caller holds the engine's lock for the whole call, so nothing observes the
/// engine stopped in between.
pub fn reconnect<S: Steps>(steps: &mut S) -> Result<Report<S::Connected>, String> {
    if !steps.running()? {
        return Ok(Report {
            outcome: Outcome::NotRunning,
            capture: CaptureCarry::default(),
        });
    }
    let capture = steps.stop();
    let outcome = match steps.start_new() {
        Ok(connected) => Outcome::Applied(connected),
        Err(error) => match steps.restore() {
            None => Outcome::Disconnected {
                error,
                restore_error: None,
                restored: false,
            },
            Some(Err(restore_error)) => Outcome::Disconnected {
                error,
                restore_error: Some(restore_error),
                restored: false,
            },
            Some(Ok(())) => match steps.start_previous() {
                Ok(connected) => Outcome::Restored { error, connected },
                Err(restore_error) => Outcome::Disconnected {
                    error,
                    restore_error: Some(restore_error),
                    restored: true,
                },
            },
        },
    };
    if matches!(outcome, Outcome::Applied(_) | Outcome::Restored { .. }) {
        steps.resume_capture(&capture);
    }
    Ok(Report { outcome, capture })
}

/// The window's language for the sentences below: `zh`, `zh-Hant` or `en`, as the notifications take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Simplified,
    Traditional,
    English,
}

impl Language {
    pub fn from_id(id: &str) -> Self {
        match id {
            "en" => Self::English,
            "zh-Hant" => Self::Traditional,
            _ => Self::Simplified,
        }
    }
}

/// The start of the message when a changed setting did not take effect.
pub fn not_applied(language: Language) -> &'static str {
    match language {
        Language::English => "The new settings could not be applied",
        Language::Traditional => "新設定未能生效",
        Language::Simplified => "新设置未能生效",
    }
}

/// The outcome for the app log (English, no error text: that is in the message the user saw).
pub fn log_label<C>(outcome: &Outcome<C>) -> &'static str {
    match outcome {
        Outcome::NotRunning => "not running",
        Outcome::Applied(_) => "applied",
        Outcome::Restored { .. } => "failed, earlier settings restored",
        Outcome::Disconnected { restored: true, .. } => {
            "failed, earlier settings put back but did not start; disconnected"
        }
        Outcome::Disconnected { .. } => "failed; disconnected",
    }
}

/// What the user is told after `failed` (the change that did not start, in the window's language) about where
/// the connection ended up, or `None` when the change applied.
pub fn outcome_text<C>(outcome: &Outcome<C>, failed: &str, language: Language) -> Option<String> {
    match outcome {
        Outcome::NotRunning | Outcome::Applied(_) => None,
        Outcome::Restored { error, .. } => Some(match language {
            Language::English => {
                format!("{failed}: {error}. The connection was restored with the earlier settings.")
            }
            Language::Traditional => format!("{failed}：{error}。已按原設定恢復連線。"),
            Language::Simplified => format!("{failed}：{error}。已按原设置恢复连接。"),
        }),
        Outcome::Disconnected {
            error,
            restore_error,
            restored,
        } => {
            let again = restore_error.as_deref().unwrap_or_default();
            Some(match (language, restore_error.is_some(), *restored) {
                (Language::English, true, true) => format!(
                    "{failed}: {error}. The earlier settings are back but could not connect either ({again}); you are disconnected."
                ),
                (Language::English, true, false) => format!(
                    "{failed}: {error}. The earlier settings could not be put back ({again}); you are disconnected."
                ),
                (Language::English, false, _) => format!("{failed}: {error}. You are disconnected."),
                (Language::Traditional, true, true) => {
                    format!("{failed}：{error}。已改回原設定，但仍無法連線（{again}），目前已斷開。")
                }
                (Language::Traditional, true, false) => {
                    format!("{failed}：{error}。無法改回原設定（{again}），目前已斷開。")
                }
                (Language::Traditional, false, _) => format!("{failed}：{error}。目前已斷開。"),
                (Language::Simplified, true, true) => {
                    format!("{failed}：{error}。已改回原设置，但仍无法连接（{again}），当前已断开。")
                }
                (Language::Simplified, true, false) => {
                    format!("{failed}：{error}。无法改回原设置（{again}），当前已断开。")
                }
                (Language::Simplified, false, _) => format!("{failed}：{error}。当前已断开。"),
            })
        }
    }
}

/// The sentences joined, the way the window's notices read.
pub fn joined<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    parts
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted engine: which starts succeed, whether the earlier setting can be put back, and the order
    /// of what was done.
    struct Script {
        running: bool,
        new_starts: bool,
        restore: Option<Result<(), String>>,
        previous_starts: bool,
        carry: CaptureCarry,
        done: Vec<&'static str>,
        resumed: Option<CaptureCarry>,
    }

    impl Script {
        fn new(
            new_starts: bool,
            restore: Option<Result<(), String>>,
            previous_starts: bool,
        ) -> Self {
            Self {
                running: true,
                new_starts,
                restore,
                previous_starts,
                carry: CaptureCarry {
                    was_on: true,
                    max_total_bytes: Some(16 * 1024 * 1024),
                    saved: Some("1-ab".to_string()),
                    save_error: None,
                },
                done: Vec::new(),
                resumed: None,
            }
        }
    }

    impl Steps for Script {
        type Connected = &'static str;
        fn running(&mut self) -> Result<bool, String> {
            Ok(self.running)
        }
        fn stop(&mut self) -> CaptureCarry {
            self.done.push("stop");
            self.carry.clone()
        }
        fn start_new(&mut self) -> Result<&'static str, String> {
            self.done.push("start new");
            if self.new_starts {
                Ok("new")
            } else {
                Err("TUN adapter could not be created".to_string())
            }
        }
        fn restore(&mut self) -> Option<Result<(), String>> {
            self.done.push("restore");
            self.restore.clone()
        }
        fn start_previous(&mut self) -> Result<&'static str, String> {
            self.done.push("start previous");
            if self.previous_starts {
                Ok("previous")
            } else {
                Err("port 7890 is in use".to_string())
            }
        }
        fn resume_capture(&mut self, carry: &CaptureCarry) {
            self.done.push("resume capture");
            self.resumed = Some(carry.clone());
        }
    }

    #[test]
    fn a_stopped_engine_is_left_alone() {
        let mut script = Script::new(true, None, true);
        script.running = false;
        let report = reconnect(&mut script).unwrap();
        assert_eq!(report.outcome, Outcome::NotRunning);
        assert!(script.done.is_empty());
    }

    #[test]
    fn the_new_setting_runs_and_capture_carries_on() {
        let mut script = Script::new(true, Some(Ok(())), true);
        let report = reconnect(&mut script).unwrap();
        assert_eq!(report.outcome, Outcome::Applied("new"));
        assert_eq!(script.done, ["stop", "start new", "resume capture"]);
        assert_eq!(script.resumed.as_ref(), Some(&report.capture));
    }

    /// The finding's case: TUN mode turned on while connected, and the adapter cannot be made. The earlier
    /// setting runs again instead of leaving the computer disconnected.
    #[test]
    fn a_failed_setting_restores_the_earlier_one() {
        let mut script = Script::new(false, Some(Ok(())), true);
        let report = reconnect(&mut script).unwrap();
        assert_eq!(
            report.outcome,
            Outcome::Restored {
                error: "TUN adapter could not be created".to_string(),
                connected: "previous",
            }
        );
        assert_eq!(
            script.done,
            [
                "stop",
                "start new",
                "restore",
                "start previous",
                "resume capture"
            ]
        );
    }

    #[test]
    fn when_the_earlier_setting_fails_too_it_says_disconnected() {
        let mut script = Script::new(false, Some(Ok(())), false);
        let report = reconnect(&mut script).unwrap();
        assert_eq!(
            report.outcome,
            Outcome::Disconnected {
                error: "TUN adapter could not be created".to_string(),
                restore_error: Some("port 7890 is in use".to_string()),
                restored: true,
            }
        );
        assert!(script.resumed.is_none());

        let mut unrestorable = Script::new(false, Some(Err("disk full".to_string())), true);
        let report = reconnect(&mut unrestorable).unwrap();
        assert!(matches!(
            report.outcome,
            Outcome::Disconnected { restored: false, ref restore_error, .. } if restore_error.as_deref() == Some("disk full")
        ));
        assert_eq!(unrestorable.done, ["stop", "start new", "restore"]);

        let mut no_way_back = Script::new(false, None, true);
        let report = reconnect(&mut no_way_back).unwrap();
        assert!(matches!(
            report.outcome,
            Outcome::Disconnected {
                restore_error: None,
                restored: false,
                ..
            }
        ));
    }

    #[test]
    fn the_user_is_told_where_the_connection_ended_up() {
        let restored = Outcome::Restored {
            error: "blocked".to_string(),
            connected: (),
        };
        assert_eq!(
            outcome_text(&restored, "TUN 模式未能开启", Language::Simplified).as_deref(),
            Some("TUN 模式未能开启：blocked。已按原设置恢复连接。")
        );
        let disconnected: Outcome<()> = Outcome::Disconnected {
            error: "blocked".to_string(),
            restore_error: Some("busy".to_string()),
            restored: true,
        };
        assert_eq!(
            outcome_text(&disconnected, "TUN mode could not be turned on", Language::English)
                .as_deref(),
            Some("TUN mode could not be turned on: blocked. The earlier settings are back but could not connect either (busy); you are disconnected.")
        );
        assert!(outcome_text(&Outcome::Applied(()), "x", Language::English).is_none());
        assert!(outcome_text(&Outcome::<()>::NotRunning, "x", Language::English).is_none());
        assert_eq!(not_applied(Language::Traditional), "新設定未能生效");
        assert_eq!(log_label(&restored), "failed, earlier settings restored");
    }

    #[test]
    fn sentences_join_like_the_notices() {
        assert_eq!(
            joined(["TUN mode is on.", "", " Saved. "]),
            "TUN mode is on. Saved."
        );
    }

    #[test]
    fn languages_follow_the_notification_ids() {
        assert_eq!(Language::from_id("en"), Language::English);
        assert_eq!(Language::from_id("zh-Hant"), Language::Traditional);
        assert_eq!(Language::from_id(""), Language::Simplified);
    }
}
