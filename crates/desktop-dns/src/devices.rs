//! Names the user gives the devices of the local network, for `DEVICE-NAME` rules, on Windows and Linux, as on the
//! Mac (`LocalRules.devices`). A device is known by the address its connections come from: through LAN sharing (or
//! with the tunnel as their gateway) the engine sees each device's own address and looks its name up here
//! (`routing.devices`). Names are matched without regard to case.

use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The most names kept; the engine takes 256.
pub const MAX_DEVICES: usize = 256;

/// The longest name, as the engine takes it.
const MAX_NAME_CHARS: usize = 64;

/// One named device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// The device's address on the local network, as written by `IpAddr` (an IPv4-mapped address as IPv4).
    pub address: String,
    /// What `DEVICE-NAME` rules call it.
    pub name: String,
}

/// `address` as stored: an IP address in its canonical form, `None` for anything else.
pub fn normalized_address(address: &str) -> Option<String> {
    let address = address.trim().trim_matches(['[', ']']);
    address
        .parse::<IpAddr>()
        .ok()
        .map(|address| address.to_canonical().to_string())
}

/// `name` as stored: trimmed, 1 to 64 characters, no comma (a rule could not name it) and no control character.
pub fn normalized_name(name: &str) -> Option<String> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.contains(',')
        && !name.chars().any(char::is_control);
    valid.then(|| name.to_string())
}

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Where the app keeps the names; set once at start. Until then there are none.
pub fn use_file(path: PathBuf) {
    let _ = FILE.set(path);
}

/// The saved names, by address; none when nothing was saved or the file cannot be read.
pub fn current() -> Vec<Device> {
    FILE.get().map(|path| load(path)).unwrap_or_default()
}

fn load(path: &Path) -> Vec<Device> {
    let mut devices: Vec<Device> = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    devices.retain(|device| {
        normalized_address(&device.address).as_deref() == Some(device.address.as_str())
            && normalized_name(&device.name).as_deref() == Some(device.name.as_str())
    });
    sort(&mut devices);
    devices.dedup_by(|a, b| a.address == b.address);
    devices.truncate(MAX_DEVICES);
    devices
}

fn sort(devices: &mut [Device]) {
    devices.sort_by_key(|device| device.address.parse::<IpAddr>().ok());
}

/// `devices` with the device at `address` called `name`, replacing its earlier name; checked.
pub fn setting(devices: &[Device], address: &str, name: &str) -> Result<Vec<Device>, String> {
    let address = normalized_address(address).ok_or("the device address must be an IP address")?;
    let name = normalized_name(name)
        .ok_or("the device name must be 1 to 64 characters without a comma")?;
    let mut next: Vec<Device> = devices
        .iter()
        .filter(|device| device.address != address)
        .cloned()
        .collect();
    if next.len() >= MAX_DEVICES {
        return Err(format!("at most {MAX_DEVICES} device names"));
    }
    next.push(Device { address, name });
    sort(&mut next);
    Ok(next)
}

/// `devices` without the one at `address`.
pub fn removing(devices: &[Device], address: &str) -> Vec<Device> {
    let address = normalized_address(address).unwrap_or_default();
    devices
        .iter()
        .filter(|device| device.address != address)
        .cloned()
        .collect()
}

/// Writes `devices` as the routing section's `devices` (address → name), replacing what it had; none leaves the
/// section without the key.
pub fn insert(devices: &[Device], routing: &mut Map<String, Value>) {
    if devices.is_empty() {
        routing.remove("devices");
        return;
    }
    let names: BTreeMap<&str, &str> = devices
        .iter()
        .map(|device| (device.address.as_str(), device.name.as_str()))
        .collect();
    routing.insert(
        "devices".to_string(),
        serde_json::to_value(names).unwrap_or(Value::Null),
    );
}

/// Saves `devices` once `check` (which builds envelopes with them, or restarts the engine) succeeds; when it fails,
/// the previous file is put back.
pub fn replace<T>(
    devices: &[Device],
    check: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let path = FILE.get().ok_or("the device names have no file yet")?;
    let previous = fs::read(path).ok();
    write(
        path,
        &serde_json::to_vec_pretty(devices).map_err(|error| error.to_string())?,
    )?;
    match check() {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = match previous {
                Some(bytes) => write(path, &bytes),
                None => fs::remove_file(path).map_err(|error| error.to_string()),
            };
            Err(error)
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("the device name file has no folder")?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let temporary = dir.join(".devices.tmp");
    fs::write(&temporary, bytes).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn addresses_and_names_are_checked() {
        assert_eq!(
            normalized_address(" 192.168.1.30 ").as_deref(),
            Some("192.168.1.30")
        );
        assert_eq!(
            normalized_address("::ffff:192.168.1.30").as_deref(),
            Some("192.168.1.30")
        );
        assert_eq!(normalized_address("[fd00::5]").as_deref(), Some("fd00::5"));
        assert_eq!(normalized_address("tv.local"), None);
        assert_eq!(
            normalized_name(" Living Room TV ").as_deref(),
            Some("Living Room TV")
        );
        for bad in ["", "  ", "a,b", "a\nb", "x".repeat(65).as_str()] {
            assert_eq!(normalized_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn setting_replaces_by_address_and_keeps_address_order() {
        let one = setting(&[], "192.168.1.30", "TV").unwrap();
        let two = setting(&one, "192.168.1.4", "NAS").unwrap();
        assert_eq!(
            two.iter()
                .map(|device| device.address.as_str())
                .collect::<Vec<_>>(),
            ["192.168.1.4", "192.168.1.30"]
        );
        let renamed = setting(&two, "::ffff:192.168.1.30", "Apple TV").unwrap();
        assert_eq!(renamed.len(), 2);
        assert_eq!(renamed[1].name, "Apple TV");
        assert!(setting(&two, "nas", "NAS").is_err());
        assert!(setting(&two, "192.168.1.9", "a,b").is_err());
        assert_eq!(removing(&renamed, "192.168.1.30").len(), 1);

        let full: Vec<Device> = (0..MAX_DEVICES)
            .map(|index| Device {
                address: format!("10.0.{}.{}", index / 256, index % 256),
                name: "x".to_string(),
            })
            .collect();
        assert!(setting(&full, "192.168.1.30", "TV").is_err());
        assert!(setting(&full, "10.0.0.0", "renamed").is_ok());
    }

    #[test]
    fn names_go_into_the_routing_section() {
        let devices = setting(&[], "192.168.1.30", "TV").unwrap();
        let mut routing = Map::new();
        insert(&devices, &mut routing);
        assert_eq!(routing["devices"], json!({ "192.168.1.30": "TV" }));
        insert(&[], &mut routing);
        assert!(!routing.contains_key("devices"));
    }

    #[test]
    fn a_damaged_or_hand_edited_file_keeps_only_valid_names() {
        let dir = std::env::temp_dir().join(format!("devices-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("devices.json");
        fs::write(
            &path,
            r#"[{"address":"192.168.1.30","name":"TV"},{"address":"tv","name":"TV"},{"address":"192.168.1.30","name":"Other"},{"address":"192.168.1.5","name":"a,b"}]"#,
        )
        .unwrap();
        let devices = load(&path);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "TV");
        fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
