//! Wi-Fi rules of the desktop and Android clients, as in the iOS On Demand settings (`OnDemandSettings.swift`):
//! named Wi-Fi networks first (first match wins), then any other Wi-Fi, cellular data, and other networks
//! (Ethernet), each set to connect, disconnect or leave the connection as it is.
//!
//! The system applies the iOS rules itself; here the client samples the network every few seconds and acts when it
//! changes, so a connect or disconnect by hand holds until the next network. [`windows_wifi`] reads the Windows
//! Wi-Fi name; Android reads it in the VPN plugin.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// What the client does on one kind of network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Connect,
    Disconnect,
    Ignore,
}

/// One Wi-Fi network by its name (SSID).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub ssid: String,
    pub action: Action,
}

/// The rules, in the interface's `OnDemandSettings` shape. Off by default: reading the Wi-Fi name needs the
/// location permission on Android and recent Windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub wifi: Vec<Rule>,
    #[serde(default = "connect")]
    pub other_wifi: Action,
    #[serde(default = "connect")]
    pub cellular: Action,
    /// Ethernet and other networks.
    #[serde(default = "connect")]
    pub other: Action,
}

fn connect() -> Action {
    Action::Connect
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            wifi: Vec::new(),
            other_wifi: Action::Connect,
            cellular: Action::Connect,
            other: Action::Connect,
        }
    }
}

/// Longest SSID (IEEE 802.11: 32 bytes) and most named networks kept, as on iOS.
pub const MAX_SSID_BYTES: usize = 32;
pub const MAX_RULES: usize = 64;

/// The network the device is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Network {
    /// Wi-Fi; the name is unknown when the system withholds it (no location permission).
    Wifi(Option<String>),
    Cellular,
    /// Ethernet or another interface.
    Other,
    Offline,
}

impl Network {
    /// The network the Android plugin reports: `kind` is wifi, cellular, other or offline.
    pub fn from_kind(kind: &str, ssid: Option<String>) -> Self {
        match kind {
            "wifi" => Self::Wifi(ssid.filter(|ssid| !ssid.is_empty())),
            "cellular" => Self::Cellular,
            "offline" => Self::Offline,
            _ => Self::Other,
        }
    }

    /// What the window shows (`current_network`).
    pub fn current(&self) -> Current {
        let (kind, ssid) = match self {
            Self::Wifi(ssid) => ("wifi", ssid.clone()),
            Self::Cellular => ("cellular", None),
            Self::Other => ("other", None),
            Self::Offline => ("offline", None),
        };
        Current {
            kind: kind.to_string(),
            ssid,
        }
    }
}

/// The network as the window reads it: `kind` is wifi, cellular, other or offline; `ssid` the Wi-Fi name when
/// the system gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Current {
    pub kind: String,
    pub ssid: Option<String>,
}

impl Settings {
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

    /// The settings with names trimmed, empty, too long and repeated names dropped and at most [`MAX_RULES`] kept;
    /// `Err` names the first name refused.
    pub fn checked(mut self) -> Result<Self, String> {
        let mut rules: Vec<Rule> = Vec::new();
        for rule in self.wifi {
            let ssid = rule.ssid.trim().to_string();
            if ssid.is_empty() || ssid.len() > MAX_SSID_BYTES {
                return Err(ssid);
            }
            if !rules.iter().any(|kept| kept.ssid == ssid) {
                rules.push(Rule {
                    ssid,
                    action: rule.action,
                });
            }
        }
        rules.truncate(MAX_RULES);
        self.wifi = rules;
        Ok(self)
    }

    /// What to do on `network`; `Ignore` while the rules are off.
    pub fn action(&self, network: &Network) -> Action {
        if !self.enabled {
            return Action::Ignore;
        }
        match network {
            Network::Wifi(ssid) => ssid
                .as_ref()
                .and_then(|ssid| self.wifi.iter().find(|rule| &rule.ssid == ssid))
                .map_or(self.other_wifi, |rule| rule.action),
            Network::Cellular => self.cellular,
            Network::Other => self.other,
            Network::Offline => Action::Ignore,
        }
    }
}

/// Acts once per network: the first sample and each change return what the rules say, repeats return nothing.
#[derive(Debug, Default)]
pub struct Watch {
    last: Option<Network>,
}

impl Watch {
    pub fn observe(&mut self, settings: &Settings, network: Network) -> Option<Action> {
        if self.last.as_ref() == Some(&network) {
            return None;
        }
        let action = settings.action(&network);
        self.last = Some(network);
        (action != Action::Ignore).then_some(action)
    }

    /// Forgets the network, so changed rules apply to the current one at the next sample.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// The Wi-Fi network from `netsh wlan show interfaces`: `Some(Some(name))` when connected to a named network,
/// `Some(None)` when connected but the name is withheld (Windows 11 asks for the location permission), `None`
/// when no Wi-Fi is connected. The field names are the same in every Windows language; the state is read from
/// the presence of an SSID line.
pub fn parse_netsh(output: &str) -> Option<Option<String>> {
    // Windows 11 without the location permission prints only where to grant it.
    if output.contains("privacy-location") {
        return Some(None);
    }
    let mut connected = false;
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "SSID" {
            if value.is_empty() {
                connected = true;
                continue;
            }
            return Some(Some(value.to_string()));
        }
        if key == "BSSID" && !value.is_empty() {
            connected = true;
        }
    }
    connected.then_some(None)
}

/// The Windows Wi-Fi network, or none when Wi-Fi is not connected (or `netsh` is missing).
#[cfg(windows)]
pub fn windows_wifi() -> Option<Option<String>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_netsh(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Settings {
        Settings {
            enabled: true,
            wifi: vec![
                Rule {
                    ssid: "Home".to_string(),
                    action: Action::Disconnect,
                },
                Rule {
                    ssid: "Cafe".to_string(),
                    action: Action::Ignore,
                },
            ],
            other_wifi: Action::Connect,
            cellular: Action::Connect,
            other: Action::Ignore,
        }
    }

    #[test]
    fn picks_the_rule_for_the_network() {
        let settings = rules();
        assert_eq!(
            settings.action(&Network::Wifi(Some("Home".into()))),
            Action::Disconnect
        );
        assert_eq!(
            settings.action(&Network::Wifi(Some("Cafe".into()))),
            Action::Ignore
        );
        assert_eq!(
            settings.action(&Network::Wifi(Some("Library".into()))),
            Action::Connect
        );
        assert_eq!(settings.action(&Network::Wifi(None)), Action::Connect);
        assert_eq!(settings.action(&Network::Other), Action::Ignore);
        assert_eq!(settings.action(&Network::Offline), Action::Ignore);
        assert_eq!(
            Settings::default().action(&Network::Cellular),
            Action::Ignore
        );
    }

    #[test]
    fn acts_once_per_network() {
        let settings = rules();
        let mut watch = Watch::default();
        assert_eq!(
            watch.observe(&settings, Network::Wifi(Some("Home".into()))),
            Some(Action::Disconnect)
        );
        assert_eq!(
            watch.observe(&settings, Network::Wifi(Some("Home".into()))),
            None
        );
        assert_eq!(
            watch.observe(&settings, Network::Wifi(Some("Cafe".into()))),
            None
        );
        assert_eq!(
            watch.observe(&settings, Network::Cellular),
            Some(Action::Connect)
        );
        watch.reset();
        assert_eq!(
            watch.observe(&settings, Network::Cellular),
            Some(Action::Connect)
        );
    }

    #[test]
    fn checks_names_and_round_trips() {
        let mut settings = rules();
        settings.wifi.push(Rule {
            ssid: " Home ".into(),
            action: Action::Connect,
        });
        let checked = settings.clone().checked().unwrap();
        assert_eq!(checked.wifi.len(), 2);
        settings.wifi.push(Rule {
            ssid: "x".repeat(33),
            action: Action::Connect,
        });
        assert!(settings.checked().is_err());

        let path = std::env::temp_dir()
            .join(format!("on-demand-{}", std::process::id()))
            .join("rules.json");
        assert_eq!(Settings::load(&path), Settings::default());
        checked.save(&path).unwrap();
        assert_eq!(Settings::load(&path), checked);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reads_plugin_kinds() {
        assert_eq!(
            Network::from_kind("wifi", Some("Home".into())),
            Network::Wifi(Some("Home".into()))
        );
        assert_eq!(
            Network::from_kind("wifi", Some(String::new())),
            Network::Wifi(None)
        );
        assert_eq!(Network::from_kind("cellular", None), Network::Cellular);
        assert_eq!(Network::from_kind("ethernet", None), Network::Other);
        assert_eq!(Network::Offline.current().kind, "offline");
    }

    #[test]
    fn reads_netsh() {
        let connected = "There is 1 interface on the system:\n\n    Name                   : WLAN\n    State                  : connected\n    SSID                   : Home Net\n    BSSID                  : aa:bb:cc:dd:ee:ff\n";
        assert_eq!(parse_netsh(connected), Some(Some("Home Net".to_string())));
        let hidden = "    名称                   : WLAN\n    状态                   : 已连接\n    SSID                   : \n    BSSID                  : aa:bb:cc:dd:ee:ff\n";
        assert_eq!(parse_netsh(hidden), Some(None));
        let disconnected =
            "    Name                   : WLAN\n    State                  : disconnected\n";
        assert_eq!(parse_netsh(disconnected), None);
        assert_eq!(parse_netsh(""), None);
        let refused = "Network shell commands need location permission to access WLAN information. ms-settings:privacy-location\n";
        assert_eq!(parse_netsh(refused), Some(None));
    }
}
