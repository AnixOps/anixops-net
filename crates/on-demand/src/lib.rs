//! Wi-Fi rules of the desktop and Android clients, as in the iOS On Demand settings (`OnDemandSettings.swift`):
//! named Wi-Fi networks first (first match wins), then any other Wi-Fi, cellular data, and other networks
//! (Ethernet), each set to connect, disconnect or leave the connection as it is.
//!
//! The system applies the iOS rules itself; here the client samples the network every few seconds and acts when it
//! changes, so a connect or disconnect by hand holds until the next network. [`windows_wifi`] reads the Windows
//! Wi-Fi name from the WLAN service; Android reads it in the VPN plugin.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// How the Wi-Fi rules try again a connect they started that failed: later each time, and not after
/// [`retry::MAX_FAILURES`] failures in a row on the same network. The same limit as the iOS and macOS tunnel
/// (`AutoReconnect.swift`) and Android's restarts (`MAX_RESTARTS`); see docs/auto-reconnect.md.
pub mod retry {
    use std::time::Duration;

    /// Failed connects in a row on one network before the rules stop trying there. A new network starts over.
    pub const MAX_FAILURES: u32 = 5;

    /// What follows the `failures`-th failed connect in a row on the same network: the wait before the next try
    /// (half a minute, doubling), or `None` once the rules stop trying.
    pub fn after(failures: u32) -> Option<Duration> {
        if failures >= MAX_FAILURES {
            return None;
        }
        let doublings = failures.saturating_sub(1);
        Some(Duration::from_secs(30 << doublings))
    }
}

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
    /// Wi-Fi; the name is unknown when the app may not read it (no location permission), and such a network
    /// counts as "other Wi-Fi".
    Wifi(Option<String>),
    /// Wi-Fi whose name the app may read but the system hides for now: Android 10 and later hide it while the app
    /// is in the background without the background location permission, or while location is off. It may well be
    /// a named network, so the named rules cannot tell; see [`Watch::observe`].
    WifiWithheld,
    Cellular,
    /// Ethernet or another interface.
    Other,
    Offline,
}

impl Network {
    /// The network the Android plugin reports: `kind` is wifi, wifi-withheld (the name is hidden for now),
    /// cellular, other or offline.
    pub fn from_kind(kind: &str, ssid: Option<String>) -> Self {
        match kind {
            "wifi" => Self::Wifi(ssid.filter(|ssid| !ssid.is_empty())),
            "wifi-withheld" => Self::WifiWithheld,
            "cellular" => Self::Cellular,
            "offline" => Self::Offline,
            _ => Self::Other,
        }
    }

    /// What the window shows (`current_network`).
    pub fn current(&self) -> Current {
        let (kind, ssid) = match self {
            Self::Wifi(ssid) => ("wifi", ssid.clone()),
            Self::WifiWithheld => ("wifi", None),
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
            // Any named rule could be the one for this network: nothing until the name can be read.
            Network::WifiWithheld if !self.wifi.is_empty() => Action::Ignore,
            Network::WifiWithheld => self.other_wifi,
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
        // A hidden name is not a new network: with named rules it neither acts nor replaces the network seen
        // last, so the same Wi-Fi read with its name again (the app back in front) is no change either.
        if network == Network::WifiWithheld && !settings.wifi.is_empty() {
            return None;
        }
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

/// The name of a Wi-Fi network from the raw bytes of its SSID (at most 32): UTF-8, which every current system
/// writes, else `legacy` (the system's ANSI code page on Windows, for an old router named in GBK or Big5), else UTF-8
/// with the broken bytes replaced. `None` for a hidden network's empty SSID.
pub fn ssid_name(bytes: &[u8], legacy: impl FnOnce(&[u8]) -> Option<String>) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(name) => Some(name.to_string()),
        Err(_) => legacy(bytes)
            .filter(|name| !name.is_empty())
            .or_else(|| Some(String::from_utf8_lossy(bytes).into_owned())),
    }
}

/// The Windows Wi-Fi network, read from the WLAN service as the SSID's raw bytes (`netsh` prints it in the console's
/// code page, which turns a Chinese name into replacement characters): `Some(Some(name))` when connected to a named
/// network, `Some(None)` when connected but the name is hidden or withheld (Windows 11 asks for the location
/// permission), `None` when no Wi-Fi is connected or the WLAN service is not running.
#[cfg(windows)]
pub fn windows_wifi() -> Option<Option<String>> {
    wlan::current()
}

#[cfg(windows)]
mod wlan {
    use std::ffi::c_void;
    use std::ptr;

    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_ACP, MB_ERR_INVALID_CHARS};
    use windows_sys::Win32::NetworkManagement::WiFi::{
        wlan_interface_state_connected, wlan_intf_opcode_current_connection, WlanCloseHandle,
        WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
        WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST,
    };

    /// The client version of the Vista and later WLAN API.
    const CLIENT_VERSION: u32 = 2;

    struct Client(HANDLE);

    impl Drop for Client {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful WlanOpenHandle and is closed once.
            unsafe {
                WlanCloseHandle(self.0, ptr::null());
            }
        }
    }

    /// Memory the WLAN API allocated.
    struct Owned<T>(*mut T);

    impl<T> Drop for Owned<T> {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the pointer came from the WLAN API and is freed once.
                unsafe { WlanFreeMemory(self.0 as *const c_void) }
            }
        }
    }

    pub fn current() -> Option<Option<String>> {
        let mut negotiated = 0u32;
        let mut handle: HANDLE = ptr::null_mut();
        // SAFETY: both out pointers are valid for the call.
        if unsafe { WlanOpenHandle(CLIENT_VERSION, ptr::null(), &mut negotiated, &mut handle) }
            != ERROR_SUCCESS
        {
            // No WLAN service (a desktop without Wi-Fi): no Wi-Fi network.
            return None;
        }
        let client = Client(handle);
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
        // SAFETY: the handle is open and `list` is a valid out pointer.
        let status = unsafe { WlanEnumInterfaces(client.0, ptr::null(), &mut list) };
        let list = Owned(list);
        if status != ERROR_SUCCESS || list.0.is_null() {
            return None;
        }
        // SAFETY: the list holds `dwNumberOfItems` entries from `InterfaceInfo` on, in memory the API allocated.
        let interfaces = unsafe {
            let count = (*list.0).dwNumberOfItems as usize;
            let first = ptr::addr_of!((*list.0).InterfaceInfo).cast::<WLAN_INTERFACE_INFO>();
            std::slice::from_raw_parts(first, count)
        };
        let mut connected = false;
        for interface in interfaces
            .iter()
            .filter(|interface| interface.isState == wlan_interface_state_connected)
        {
            connected = true;
            let mut size = 0u32;
            let mut data: *mut c_void = ptr::null_mut();
            // SAFETY: the handle is open, the GUID lives in the list, and the out pointers are valid.
            let status = unsafe {
                WlanQueryInterface(
                    client.0,
                    &interface.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    ptr::null(),
                    &mut size,
                    &mut data,
                    ptr::null_mut(),
                )
            };
            let data = Owned(data.cast::<WLAN_CONNECTION_ATTRIBUTES>());
            if status == ERROR_ACCESS_DENIED {
                // Windows 11 withholds the network without the location permission.
                continue;
            }
            if status != ERROR_SUCCESS
                || data.0.is_null()
                || (size as usize) < std::mem::size_of::<WLAN_CONNECTION_ATTRIBUTES>()
            {
                continue;
            }
            // SAFETY: the API returned a whole WLAN_CONNECTION_ATTRIBUTES.
            let ssid = unsafe { (*data.0).wlanAssociationAttributes.dot11Ssid };
            let length = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
            if let Some(name) = super::ssid_name(&ssid.ucSSID[..length], ansi) {
                return Some(Some(name));
            }
        }
        connected.then_some(None)
    }

    /// `bytes` in the system's ANSI code page (GBK on Simplified Chinese Windows, Big5 on Traditional).
    fn ansi(bytes: &[u8]) -> Option<String> {
        let length = i32::try_from(bytes.len()).ok()?;
        let mut wide = [0u16; 64];
        // SAFETY: the input is `length` bytes and the output buffer holds `wide.len()` units; an SSID of at most 32
        // bytes never needs more than 32.
        let written = unsafe {
            MultiByteToWideChar(
                CP_ACP,
                MB_ERR_INVALID_CHARS,
                bytes.as_ptr(),
                length,
                wide.as_mut_ptr(),
                wide.len() as i32,
            )
        };
        let written = usize::try_from(written)
            .ok()
            .filter(|&written| written > 0)?;
        String::from_utf16(&wide[..written]).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_slow_down_and_stop_after_five_failures() {
        let waits: Vec<_> = (1..=4)
            .map(|failures| retry::after(failures).unwrap())
            .collect();
        assert_eq!(
            waits,
            [30, 60, 120, 240].map(std::time::Duration::from_secs)
        );
        assert_eq!(retry::after(retry::MAX_FAILURES), None);
        assert_eq!(retry::after(u32::MAX), None);
        // The first failure waits like any other.
        assert_eq!(retry::after(0), Some(std::time::Duration::from_secs(30)));
    }

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
    fn a_withheld_name_is_no_change() {
        let settings = rules();
        assert_eq!(settings.action(&Network::WifiWithheld), Action::Ignore);
        let mut watch = Watch::default();
        assert_eq!(
            watch.observe(&settings, Network::Wifi(Some("Home".into()))),
            Some(Action::Disconnect)
        );
        // The app went to the background: the same network without its name does nothing ...
        assert_eq!(watch.observe(&settings, Network::WifiWithheld), None);
        // ... and reading the name again is not a new network either.
        assert_eq!(
            watch.observe(&settings, Network::Wifi(Some("Home".into()))),
            None
        );
        // A new Wi-Fi first seen in the background waits for its name.
        assert_eq!(
            watch.observe(&settings, Network::Cellular),
            Some(Action::Connect)
        );
        assert_eq!(watch.observe(&settings, Network::WifiWithheld), None);
        assert_eq!(
            watch.observe(&settings, Network::Wifi(Some("Home".into()))),
            Some(Action::Disconnect)
        );
        // Without the permission the name is unknown, not withheld: that is still "other Wi-Fi".
        assert_eq!(
            watch.observe(&settings, Network::Wifi(None)),
            Some(Action::Connect)
        );
        // Without named rules a hidden name cannot matter: it is other Wi-Fi, once.
        let unnamed = Settings {
            wifi: Vec::new(),
            ..rules()
        };
        let mut watch = Watch::default();
        assert_eq!(
            watch.observe(&unnamed, Network::WifiWithheld),
            Some(Action::Connect)
        );
        assert_eq!(watch.observe(&unnamed, Network::WifiWithheld), None);
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
        assert_eq!(
            Network::from_kind("wifi-withheld", None),
            Network::WifiWithheld
        );
        assert_eq!(Network::WifiWithheld.current().kind, "wifi");
        assert_eq!(Network::from_kind("cellular", None), Network::Cellular);
        assert_eq!(Network::from_kind("ethernet", None), Network::Other);
        assert_eq!(Network::Offline.current().kind, "offline");
    }

    #[test]
    fn reads_the_ssid_bytes() {
        let none = |_: &[u8]| -> Option<String> { None };
        // UTF-8, as current routers and phones name networks.
        assert_eq!(
            ssid_name("我的家".as_bytes(), none),
            Some("我的家".to_string())
        );
        assert_eq!(ssid_name(b"Home Net", none), Some("Home Net".to_string()));
        // A hidden network.
        assert_eq!(ssid_name(b"", none), None);
        // GBK bytes of 我的家 go to the system's code page.
        let gbk = [0xCE, 0xD2, 0xB5, 0xC4, 0xBC, 0xD2];
        assert_eq!(
            ssid_name(&gbk, |bytes| {
                assert_eq!(bytes, gbk);
                Some("我的家".to_string())
            }),
            Some("我的家".to_string())
        );
        // Without a code page that reads them, the name is still something to match on.
        assert!(ssid_name(&gbk, none).is_some_and(|name| !name.is_empty()));
        // A 32-byte UTF-8 name stays whole.
        let long = "家里的无线网络名称十".as_bytes();
        assert!(long.len() <= MAX_SSID_BYTES);
        assert_eq!(
            ssid_name(long, none).as_deref(),
            Some("家里的无线网络名称十")
        );
    }

    #[cfg(windows)]
    #[test]
    fn reads_the_windows_wifi_without_failing() {
        // CI runners have no Wi-Fi; the call must answer rather than fail.
        let _ = windows_wifi();
    }
}
