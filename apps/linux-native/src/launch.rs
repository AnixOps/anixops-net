//! How the window is started. The options fix the surroundings for tests and screenshots; a user starts the
//! window without any: `--language zh|zh-Hant|en`, `--theme light|dark`, `--page <id>`, `--size <w>x<h>`,
//! `--screenshot <file.png>`, `--socket <path>`, `--show subscriptions|groups`. What is not understood is ignored.

use std::path::PathBuf;

use client_strings::Language;

use crate::sections;

/// Light, dark, or whatever the desktop says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// The smallest and largest window size `--size` takes, in logical pixels.
pub const SIZE_RANGE: std::ops::RangeInclusive<i32> = 200..=8192;

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    /// The language to show; `None` follows the desktop.
    pub language: Option<Language>,
    pub appearance: Appearance,
    /// The id of the page to open on.
    pub page: &'static str,
    /// The size to open at; `None` is the usual size.
    pub size: Option<(i32, i32)>,
    /// Draw the window into this PNG once it is shown, then quit.
    pub screenshot: Option<PathBuf>,
    /// The socket the core daemon listens on; `None` is the daemon's usual one.
    pub socket: Option<String>,
    /// Open the proxies page's list of subscriptions over the window (`--show subscriptions`).
    pub subscriptions: bool,
    /// Open the proxies page's list of policy groups over the window (`--show groups`).
    pub groups: bool,
}

impl Default for Launch {
    fn default() -> Self {
        Launch {
            language: None,
            appearance: Appearance::System,
            page: sections::OVERVIEW,
            size: None,
            screenshot: None,
            socket: None,
            subscriptions: false,
            groups: false,
        }
    }
}

impl Launch {
    /// Reads the arguments after the program's name. An option takes the next argument or what follows `=`;
    /// the last one of a kind wins; an option that is not known does not take the next argument with it.
    pub fn parse<I, S>(arguments: I) -> Launch
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut launch = Launch::default();
        let mut rest = arguments.into_iter();
        while let Some(argument) = rest.next() {
            let argument = argument.as_ref();
            let (name, value) = match argument.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (argument, None),
            };
            if !matches!(
                name,
                "--language"
                    | "--theme"
                    | "--page"
                    | "--size"
                    | "--screenshot"
                    | "--socket"
                    | "--show"
            ) {
                continue;
            }
            let Some(value) = value.or_else(|| rest.next().map(|next| next.as_ref().to_owned()))
            else {
                break;
            };
            match name {
                "--language" => {
                    if let Some(language) = Language::from_id(&value) {
                        launch.language = Some(language);
                    }
                }
                "--theme" => match value.as_str() {
                    "light" => launch.appearance = Appearance::Light,
                    "dark" => launch.appearance = Appearance::Dark,
                    _ => {}
                },
                "--page" => {
                    if let Some(section) = sections::find(&value) {
                        launch.page = section.id;
                    }
                }
                "--size" => {
                    if let Some(size) = size(&value) {
                        launch.size = Some(size);
                    }
                }
                "--screenshot" if !value.is_empty() => {
                    launch.screenshot = Some(PathBuf::from(value));
                }
                "--socket" if !value.is_empty() => {
                    launch.socket = Some(value);
                }
                "--show" if value == "subscriptions" => launch.subscriptions = true,
                "--show" if value == "groups" => launch.groups = true,
                _ => {}
            }
        }
        launch
    }
}

fn size(value: &str) -> Option<(i32, i32)> {
    let (width, height) = value.split_once('x')?;
    let side = |text: &str| {
        // Plain digits only: no sign, no spaces.
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse::<i32>()
            .ok()
            .filter(|side| SIZE_RANGE.contains(side))
    };
    Some((side(width)?, side(height)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_asked_follows_the_desktop() {
        assert_eq!(Launch::parse(Vec::<String>::new()), Launch::default());
        assert_eq!(Launch::default().page, sections::OVERVIEW);
    }

    #[test]
    fn options_take_the_next_argument() {
        let launch = Launch::parse(["--language", "en", "--theme", "dark", "--page", "rules"]);
        assert_eq!(launch.language, Some(Language::English));
        assert_eq!(launch.appearance, Appearance::Dark);
        assert_eq!(launch.page, sections::RULES);
    }

    #[test]
    fn the_list_of_subscriptions_is_opened_when_asked_for() {
        assert!(!Launch::default().subscriptions);
        assert!(Launch::parse(["--show", "subscriptions"]).subscriptions);
        assert!(Launch::parse(["--show=subscriptions"]).subscriptions);
        assert!(!Launch::parse(["--show", "everything"]).subscriptions);
        assert!(!Launch::default().groups);
        assert!(Launch::parse(["--show", "groups"]).groups);
        assert!(!Launch::parse(["--show", "groups"]).subscriptions);
    }

    #[test]
    fn options_take_an_equals_sign() {
        let launch = Launch::parse([
            "--language=zh-Hant",
            "--theme=light",
            "--page=settings",
            "--size=420x700",
        ]);
        assert_eq!(launch.language, Some(Language::TraditionalChinese));
        assert_eq!(launch.appearance, Appearance::Light);
        assert_eq!(launch.page, sections::SETTINGS);
        assert_eq!(launch.size, Some((420, 700)));
    }

    #[test]
    fn what_is_not_understood_is_ignored() {
        for (name, value) in [
            ("--language", "fr"),
            ("--theme", "purple"),
            ("--page", "nowhere"),
            ("--size", "big"),
            ("--size", "100x100"),
            ("--size", "-800x600"),
            ("--size", "800x"),
            ("--size", "800x600x2"),
            ("--screenshot", ""),
            ("--socket", ""),
            ("--unknown", "value"),
        ] {
            assert_eq!(
                Launch::parse([name, value]),
                Launch::default(),
                "{name} {value}"
            );
            assert_eq!(
                Launch::parse([format!("{name}={value}")]),
                Launch::default(),
                "{name}={value}"
            );
        }
    }

    #[test]
    fn an_option_without_a_value_is_ignored() {
        assert_eq!(Launch::parse(["--theme"]), Launch::default());
        let launch = Launch::parse(["--theme", "dark", "--page"]);
        assert_eq!(launch.appearance, Appearance::Dark);
        assert_eq!(launch.page, sections::OVERVIEW);
    }

    #[test]
    fn the_last_value_wins() {
        assert_eq!(
            Launch::parse(["--theme", "dark", "--theme", "light"]).appearance,
            Appearance::Light
        );
    }

    #[test]
    fn an_unknown_option_does_not_swallow_the_next_one() {
        assert_eq!(
            Launch::parse(["--verbose", "--theme", "dark"]).appearance,
            Appearance::Dark
        );
    }

    #[test]
    fn a_screenshot_names_a_file() {
        let launch = Launch::parse(["--screenshot", "/tmp/shots/overview.png"]);
        assert_eq!(
            launch.screenshot,
            Some(PathBuf::from("/tmp/shots/overview.png"))
        );
        // A path may hold an equals sign after the first one.
        let launch = Launch::parse(["--screenshot=/tmp/a=b.png"]);
        assert_eq!(launch.screenshot, Some(PathBuf::from("/tmp/a=b.png")));
    }

    #[test]
    fn a_socket_names_where_the_daemon_listens() {
        assert_eq!(Launch::default().socket, None);
        let launch = Launch::parse(["--socket", "/tmp/anixops/core.sock"]);
        assert_eq!(launch.socket.as_deref(), Some("/tmp/anixops/core.sock"));
        let launch = Launch::parse(["--socket=/tmp/core.sock", "--theme", "dark"]);
        assert_eq!(launch.socket.as_deref(), Some("/tmp/core.sock"));
        assert_eq!(launch.appearance, Appearance::Dark);
    }
}
