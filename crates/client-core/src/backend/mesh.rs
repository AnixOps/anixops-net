//! The 组网 (mesh) networks this client saved (docs/architecture/mesh-client.md; networkcore_anixops
//! docs/architecture/mesh-module.md, 配置): each the kernel's `config_version: 1` configuration with its secret, in an
//! owner-only file beside the state. Every saved network that is joined goes into the engine's profile as a
//! `{"type":"mesh"}` outbound; the engine sends the network's routes to it ahead of the rules (R23), so no rule names
//! it. Joining, leaving, saving and removing also go to a running engine directly, without a restart. Windows never
//! see a secret: [`summaries`] leaves it out.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The file under the client's data folder.
pub const FILE: &str = "mesh-networks.json";

/// The key of the selected node in the routing section [`with_outbounds`] builds for a profile without one.
pub const NODE_KEY: &str = "node";

/// One saved network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedNetwork {
    /// `network.name`: the network's id in the status snapshot.
    pub name: String,
    /// Joined by this device: the network goes into the profile.
    pub joined: bool,
    /// The configuration, `network.secret` included.
    pub config: Value,
}

/// A saved network as a window sees it: no secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSummary {
    pub name: String,
    pub joined: bool,
    /// `forward_secrecy`, `standard` or `none`, as the status snapshot words it.
    pub security: &'static str,
    pub peers: usize,
    /// `node.address`, when the configuration fixes one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// The saved networks; none when nothing was saved yet. A file that cannot be read or understood is an error, so
/// that the next save does not replace it with fewer networks.
pub fn load(dir: &Path) -> Result<Vec<SavedNetwork>, String> {
    match fs::read(path(dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("the mesh networks file is not valid: {error}")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("the mesh networks file could not be read: {error}")),
    }
}

/// The network name `config` saves under, or why the configuration cannot be saved: it must be `config_version`
/// 1 with a `network.name` of 1–64 characters (no `/`, no line break) and a `network.secret`.
pub fn validate(config: &Value) -> Result<String, String> {
    if config.get("config_version").and_then(Value::as_u64) != Some(1) {
        return Err(crate::refusal::coded(
            "mesh.config_version",
            "the network configuration is not config_version 1",
        ));
    }
    let network = config.get("network").and_then(Value::as_object);
    let name = network
        .and_then(|network| network.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let count = name.chars().count();
    if count == 0 || count > 64 || name.contains('/') || name.contains(['\n', '\r']) {
        return Err(crate::refusal::coded(
            "mesh.name",
            "the network name must be 1 to 64 characters without a slash",
        ));
    }
    let secret = network
        .and_then(|network| network.get("secret"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if secret.is_empty() {
        return Err(crate::refusal::coded(
            "mesh.secret",
            "the network configuration has no secret",
        ));
    }
    Ok(name.to_string())
}

/// `networks` with `config` saved under `name` in place of a network of the same name, joined: a device joins one
/// network at a time (docs/mesh-ui.md, 入口与导航), so the others are no longer joined.
pub fn saved(networks: &[SavedNetwork], name: &str, config: Value) -> Vec<SavedNetwork> {
    let mut next: Vec<SavedNetwork> = networks
        .iter()
        .filter(|network| network.name != name)
        .map(|network| SavedNetwork {
            joined: false,
            ..network.clone()
        })
        .collect();
    next.push(SavedNetwork {
        name: name.to_string(),
        joined: true,
        config,
    });
    next
}

/// `networks` with `name` joined (and the others not) or not joined; `None` when no network has that name.
pub fn joined(networks: &[SavedNetwork], name: &str, joined: bool) -> Option<Vec<SavedNetwork>> {
    if !networks.iter().any(|network| network.name == name) {
        return None;
    }
    Some(
        networks
            .iter()
            .map(|network| SavedNetwork {
                joined: if network.name == name {
                    joined
                } else {
                    network.joined && !joined
                },
                ..network.clone()
            })
            .collect(),
    )
}

/// What a window lists, without secrets.
pub fn summaries(networks: &[SavedNetwork]) -> Vec<NetworkSummary> {
    networks
        .iter()
        .map(|network| {
            let settings = network.config.get("network");
            let secure = settings
                .and_then(|settings| settings.get("secure_mode"))
                .and_then(Value::as_bool);
            let encrypted = settings
                .and_then(|settings| settings.get("cipher"))
                .and_then(Value::as_str)
                != Some("none");
            NetworkSummary {
                name: network.name.clone(),
                joined: network.joined,
                security: match (encrypted, secure) {
                    (false, _) => "none",
                    (true, Some(false)) => "standard",
                    (true, _) => "forward_secrecy",
                },
                peers: network
                    .config
                    .get("peers")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
                address: network
                    .config
                    .get("node")
                    .and_then(|node| node.get("address"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }
        })
        .collect()
}

/// A network's key in the profile's `routing.outbounds` (`[A-Za-z0-9._-]` only): `mesh-` and the name, or `mesh-x`
/// and the name's UTF-8 bytes in hex when the name has other characters, so two names never share a key. The
/// Apple clients use the same keys (`MeshNetwork.outboundKey`).
pub fn outbound_key(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if plain {
        format!("mesh-{name}")
    } else {
        let hex: String = name.bytes().map(|byte| format!("{byte:02x}")).collect();
        format!("mesh-x{hex}")
    }
}

/// The `routing.outbounds` entries of the joined networks.
pub fn outbounds(networks: &[SavedNetwork]) -> Map<String, Value> {
    networks
        .iter()
        .filter(|network| network.joined)
        .filter_map(|network| {
            let mut entry = network.config.as_object()?.clone();
            entry.insert("type".to_string(), Value::String("mesh".to_string()));
            Some((outbound_key(&network.name), Value::Object(entry)))
        })
        .collect()
}

/// `profile` with `outbounds` beside its nodes: in its routing section (a node of the same key stays), or, for a
/// profile with one top-level outbound, in a routing section with that outbound as [`NODE_KEY`] and everything else
/// through it, as when it runs alone. `None` when the profile has neither or there are no outbounds.
pub fn with_outbounds(profile: &Value, outbounds: &Map<String, Value>) -> Option<Value> {
    if outbounds.is_empty() {
        return None;
    }
    let mut profile = profile.clone();
    let object = profile.as_object_mut()?;
    if let Some(routing) = object.get_mut("routing") {
        let existing = routing.get_mut("outbounds")?.as_object_mut()?;
        for (key, entry) in outbounds {
            existing.entry(key.clone()).or_insert_with(|| entry.clone());
        }
        return Some(profile);
    }
    let node = object.remove("outbound")?;
    let mut all = outbounds.clone();
    all.insert(NODE_KEY.to_string(), node);
    object.insert(
        "routing".to_string(),
        json!({
            "outbounds": all,
            "rules": [format!("FINAL,{NODE_KEY}")],
            "groups": [],
            "global": NODE_KEY,
        }),
    );
    Some(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(name: &str, secret: &str) -> Value {
        json!({
            "config_version": 1,
            "network": { "name": name, "secret": secret, "kind": "managed", "secure_mode": true },
            "node": { "role": "access" },
            "peers": [{ "server": "203.0.113.10", "port": 11010, "transport": "tcp" }],
        })
    }

    #[test]
    fn validates_version_name_and_secret() {
        assert_eq!(validate(&config(" office ", "k")).unwrap(), "office");
        let mut old = config("office", "k");
        old["config_version"] = json!(2);
        assert!(validate(&old)
            .unwrap_err()
            .starts_with("mesh.config_version:"));
        assert!(validate(&config("", "k"))
            .unwrap_err()
            .starts_with("mesh.name:"));
        assert!(validate(&config("a/b", "k"))
            .unwrap_err()
            .starts_with("mesh.name:"));
        assert!(validate(&config(&"a".repeat(65), "k")).is_err());
        assert!(validate(&config("办公室", "k")).is_ok());
        assert!(validate(&config("office", ""))
            .unwrap_err()
            .starts_with("mesh.secret:"));
    }

    #[test]
    fn one_network_is_joined_at_a_time() {
        let networks = saved(&[], "a", config("a", "k"));
        let networks = saved(&networks, "b", config("b", "k"));
        let joined_names = |networks: &[SavedNetwork]| {
            networks
                .iter()
                .filter(|network| network.joined)
                .map(|network| network.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(joined_names(&networks), ["b"]);
        let networks = joined(&networks, "a", true).unwrap();
        assert_eq!(joined_names(&networks), ["a"]);
        let networks = joined(&networks, "a", false).unwrap();
        assert!(joined_names(&networks).is_empty());
        assert!(joined(&networks, "c", true).is_none());
        // Saving again under the same name replaces it.
        assert_eq!(saved(&networks, "a", config("a", "k2")).len(), 2);
    }

    #[test]
    fn summaries_leave_the_secret_out() {
        let mut standard = config("b", "k");
        standard["network"]["secure_mode"] = json!(false);
        standard["node"]["address"] = json!("10.144.144.5/24");
        let mut open = config("c", "k");
        open["network"]["cipher"] = json!("none");
        let networks = [
            SavedNetwork {
                name: "a".into(),
                joined: true,
                config: config("a", "s3cret"),
            },
            SavedNetwork {
                name: "b".into(),
                joined: false,
                config: standard,
            },
            SavedNetwork {
                name: "c".into(),
                joined: false,
                config: open,
            },
        ];
        let summaries = summaries(&networks);
        assert!(!serde_json::to_string(&summaries)
            .unwrap()
            .contains("s3cret"));
        assert_eq!(summaries[0].security, "forward_secrecy");
        assert_eq!(summaries[0].peers, 1);
        assert_eq!(summaries[1].security, "standard");
        assert_eq!(summaries[1].address.as_deref(), Some("10.144.144.5/24"));
        assert_eq!(summaries[2].security, "none");
    }

    #[test]
    fn outbound_keys_match_the_apple_clients() {
        assert_eq!(outbound_key("office-1.lab_a"), "mesh-office-1.lab_a");
        assert_eq!(outbound_key("a b"), "mesh-x612062");
        assert_ne!(outbound_key("a b"), outbound_key("a_b"));
        assert!(outbound_key("办公室")
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')));
    }

    #[test]
    fn only_joined_networks_become_outbounds() {
        let networks = [
            SavedNetwork {
                name: "a".into(),
                joined: true,
                config: config("a", "k"),
            },
            SavedNetwork {
                name: "b".into(),
                joined: false,
                config: config("b", "k"),
            },
        ];
        let outbounds = outbounds(&networks);
        assert_eq!(outbounds.keys().collect::<Vec<_>>(), ["mesh-a"]);
        assert_eq!(outbounds["mesh-a"]["type"], "mesh");
        assert_eq!(outbounds["mesh-a"]["network"]["secret"], "k");
    }

    #[test]
    fn adds_outbounds_beside_a_routing_section() {
        let profile = json!({
            "schema_version": 1,
            "routing": { "outbounds": { "n1": { "type": "shadowsocks" } }, "rules": ["FINAL,n1"], "groups": [] },
        });
        let mut mesh = Map::new();
        mesh.insert("mesh-a".into(), json!({ "type": "mesh" }));
        mesh.insert("n1".into(), json!({ "type": "mesh" }));
        let meshed = with_outbounds(&profile, &mesh).unwrap();
        assert_eq!(meshed["routing"]["outbounds"]["mesh-a"]["type"], "mesh");
        assert_eq!(meshed["routing"]["outbounds"]["n1"]["type"], "shadowsocks");
        assert_eq!(meshed["routing"]["rules"], json!(["FINAL,n1"]));
        assert!(with_outbounds(&profile, &Map::new()).is_none());
    }

    #[test]
    fn gives_a_single_node_a_routing_section() {
        let profile = json!({ "schema_version": 1, "outbound": { "type": "direct" } });
        let mut mesh = Map::new();
        mesh.insert("mesh-a".into(), json!({ "type": "mesh" }));
        let meshed = with_outbounds(&profile, &mesh).unwrap();
        assert!(meshed.get("outbound").is_none());
        assert_eq!(meshed["routing"]["outbounds"]["node"]["type"], "direct");
        assert_eq!(meshed["routing"]["rules"], json!(["FINAL,node"]));
        assert_eq!(meshed["routing"]["global"], "node");
    }

    #[test]
    fn a_missing_file_is_no_networks_and_a_broken_one_an_error() {
        let dir = std::env::temp_dir().join(format!("mesh-load-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).unwrap().is_empty());
        fs::write(path(&dir), b"{").unwrap();
        assert!(load(&dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
