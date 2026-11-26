//! The 组网 (mesh) networks this client saved (docs/architecture/mesh-client.md; networkcore_anixops
//! docs/architecture/mesh-module.md, 配置): each the kernel's `config_version: 1` configuration with its secret, in an
//! owner-only file beside the state. Every saved network that is joined goes into the engine's profile as a
//! `{"type":"mesh"}` outbound; the engine sends the network's routes to it ahead of the rules (R23), so no rule names
//! it. Joining, leaving, saving and removing also go to a running engine directly, without a restart. Windows never
//! see a secret: [`summaries`] leaves it out. On Windows the file is sealed for the Windows account with Data
//! Protection, as the LAN share's password is.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The file under the client's data folder.
#[cfg(not(windows))]
pub const FILE: &str = "mesh-networks.json";
/// The file under the client's data folder, sealed for this Windows account.
#[cfg(windows)]
pub const FILE: &str = "mesh-networks.dpapi";
#[cfg(windows)]
const WHAT: &str = "mesh networks file";

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
    let bytes = match fs::read(path(dir)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("the mesh networks file could not be read: {error}")),
    };
    #[cfg(windows)]
    let bytes = super::session_key::crypt(&bytes, false, WHAT)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("the mesh networks file is not valid: {error}"))
}

/// Keeps `networks`, through a temporary file and a rename: owner-only, and on Windows sealed for this account.
pub fn store(dir: &Path, networks: &[SavedNetwork]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(networks).map_err(|error| error.to_string())?;
    #[cfg(windows)]
    let bytes = super::session_key::crypt(&bytes, true, WHAT)?;
    desktop_mitm::sessions::write_private(&path(dir), &bytes)
        .map_err(|error| format!("the mesh networks could not be saved: {error}"))
}

/// Whether `answer`, a running engine's, says the command took effect.
fn took_effect(answer: &Value) -> bool {
    answer.get("ok") == Some(&Value::Bool(true))
}

/// Refuses saving and joining while the release gate is closed ([`ENABLED`]), for every caller (the windows'
/// commands, the engine requests and the platform shells alike); leaving and removing stay open so a device can always
/// clean up.
fn gate() -> Result<(), String> {
    if ENABLED {
        Ok(())
    } else {
        Err(crate::refusal::coded(
            "mesh.disabled",
            "this build does not offer mesh networks",
        ))
    }
}

fn not_found(name: &str) -> String {
    crate::refusal::coded(
        "mesh.not_found",
        format!("no saved network is named {name}"),
    )
}

/// Saves a network (`config`, the kernel's `config_version: 1` structure with its secret) under `dir` and joins it,
/// leaving the network joined before: a device joins one at a time. A running engine ([`request`] answers `None`
/// while none runs) takes it at once (`mesh_save_network`, then `mesh_join`); the saved profile carries it from the
/// next start. Answers the engine's refusal when it refused, else `{"ok":true}`.
pub fn save_network(
    dir: &Path,
    config: Value,
    mut request: impl FnMut(Value) -> Option<Value>,
) -> Result<Value, String> {
    gate()?;
    let name = validate(&config)?;
    let networks = load(dir)?;
    let before = networks
        .iter()
        .find(|network| network.joined && network.name != name)
        .map(|network| network.name.clone());
    store(dir, &saved(&networks, &name, config.clone()))?;
    if let Some(before) = before {
        request(json!({ "cmd": "mesh_leave", "network": before }));
    }
    let saved = request(json!({ "cmd": "mesh_save_network", "config": config }));
    if let Some(refused) = saved.filter(|answer| !took_effect(answer)) {
        return Ok(refused);
    }
    Ok(request(json!({ "cmd": "mesh_join", "network": name }))
        .unwrap_or_else(|| json!({ "ok": true })))
}

/// Joins (leaving the network joined before) or leaves a saved network, in a running engine at once and in the
/// saved profile. The engine's refusal is answered and nothing is saved; with no engine running only the saved
/// choice changes.
pub fn set_joined(
    dir: &Path,
    name: &str,
    joined: bool,
    mut request: impl FnMut(Value) -> Option<Value>,
) -> Result<Value, String> {
    if joined {
        gate()?;
    }
    let networks = load(dir)?;
    let next = self::joined(&networks, name, joined).ok_or_else(|| not_found(name))?;
    if joined {
        for other in networks
            .iter()
            .filter(|network| network.joined && network.name != name)
        {
            request(json!({ "cmd": "mesh_leave", "network": other.name }));
        }
    }
    let cmd = if joined { "mesh_join" } else { "mesh_leave" };
    let answer =
        request(json!({ "cmd": cmd, "network": name })).unwrap_or_else(|| json!({ "ok": true }));
    if took_effect(&answer) {
        store(dir, &next)?;
    }
    Ok(answer)
}

/// Removes a saved network and its secret, from a running engine too.
pub fn remove_network(
    dir: &Path,
    name: &str,
    mut request: impl FnMut(Value) -> Option<Value>,
) -> Result<Value, String> {
    let networks = load(dir)?;
    if !networks.iter().any(|network| network.name == name) {
        return Err(not_found(name));
    }
    let answer = request(json!({ "cmd": "mesh_remove_network", "network": name }));
    let next: Vec<_> = networks
        .into_iter()
        .filter(|network| network.name != name)
        .collect();
    store(dir, &next)?;
    Ok(answer.unwrap_or_else(|| json!({ "ok": true })))
}

/// `envelope` with the outbounds of the networks joined under `dir` ([`with_outbounds`]). Mesh is an extra, as
/// HTTPS decryption is: the envelope stays as it is when none is joined, and the second value says why the joined
/// ones were left out (the file could not be read, or the engine would refuse the result: one without the mesh
/// module).
pub fn with_joined(dir: &Path, envelope: String) -> (String, Option<String>) {
    if !ENABLED {
        return (envelope, None);
    }
    let networks = match load(dir) {
        Ok(networks) => networks,
        Err(error) => return (envelope, Some(error)),
    };
    let Some(meshed) = serde_json::from_str::<Value>(&envelope)
        .ok()
        .and_then(|profile| with_outbounds(&profile, &outbounds(&networks)))
    else {
        return (envelope, None);
    };
    let meshed = meshed.to_string();
    match engine_embedded::config::RuntimeConfig::from_json(meshed.as_bytes()) {
        Ok(_) => (meshed, None),
        Err(error) => (
            envelope,
            Some(format!(
                "mesh networks left out: the engine refused them: {}",
                error.message
            )),
        ),
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

/// The profile id of the engine that runs for the mesh alone.
pub const ONLY_PROFILE_ID: &str = "mesh-only";

/// The profile of the engine that runs for the mesh alone while the proxy is off (mesh-status.md, 只开组网): the
/// joined networks' outbounds and nothing else, in the rule mode, everything that is not a mesh route direct. The
/// tunnel carries only the mesh routes ([`Tunnel`]), so the final rule only matters for what reaches it anyway.
/// `None` without outbounds.
pub fn only_profile(outbounds: &Map<String, Value>) -> Option<Value> {
    if outbounds.is_empty() {
        return None;
    }
    Some(json!({
        "schema_version": 1,
        "profile_id": ONLY_PROFILE_ID,
        "routing": {
            "outbounds": outbounds,
            "rules": ["FINAL,DIRECT"],
            "groups": [],
        },
    }))
}

/// Whether this build offers 组网 at all. Off until kernel 0.3 is complete and audited, whatever the engine runs:
/// the release gate (requirements, 2026-11-25), not a capability. A build made with `ANIXOPS_MESH_PREVIEW=1` in the
/// environment turns it on for testing. While off, no window sees `mesh_*` ([`super::engine::activity_request`]),
/// no engine runs for the mesh alone and no saved network joins a profile.
pub const ENABLED: bool = match option_env!("ANIXOPS_MESH_PREVIEW") {
    Some(value) => matches!(value.as_bytes(), [b'1']),
    None => false,
};

/// Whether this build offers the kernel 0.4 parts of 组网 (regions, host groups, external networks, advertising,
/// relaying and host mode; client 1.2), a second release gate behind [`ENABLED`] that stays closed until kernel 0.4
/// is complete and audited. A build made with `ANIXOPS_MESH04_PREVIEW=1` in the environment turns it on for testing.
/// While off, a `mesh_status` answer loses the 0.4 fields ([`strip_v04`]) and [`V04_COMMANDS`] are refused
/// ([`super::engine::activity_request`]).
pub const V04_ENABLED: bool = ENABLED
    && match option_env!("ANIXOPS_MESH04_PREVIEW") {
        Some(value) => matches!(value.as_bytes(), [b'1']),
        None => false,
    };

// The 0.4 gate never opens without the 组网 gate.
const _: () = assert!(!V04_ENABLED || ENABLED);

/// The commands only kernel 0.4 answers; refused while [`V04_ENABLED`] is off.
pub const V04_COMMANDS: [&str; 6] = [
    "mesh_set_relay",
    "mesh_set_host_mode",
    "mesh_advertise",
    "mesh_withdraw",
    "mesh_request_region",
    "mesh_withdraw_region_request",
];

/// Removes the kernel 0.4 fields from a `mesh_status` answer (mesh-client.md, 发布闸门), so that the page reads as it
/// did on kernel 0.3: top-level `regions`, `links`, `regionRequests` and `externalBridge`; `self`'s `region`,
/// `regionBasis`, `regionState`, `regionRequest`, `regionSuggestion`, `backbone` and `hostMode`; each member's `region`,
/// `vms`, `externalBridge` and `bridgesExternal`; each subnet's `region`, `kind`, `hostGroup`, `bridge` and `name`.
pub fn strip_v04(status: &mut Value) {
    fn remove(value: &mut Value, names: &[&str]) {
        if let Some(object) = value.as_object_mut() {
            for name in names {
                object.remove(*name);
            }
        }
    }
    remove(
        status,
        &["regions", "links", "regionRequests", "externalBridge"],
    );
    if let Some(own) = status.get_mut("self") {
        remove(
            own,
            &[
                "region",
                "regionBasis",
                "regionState",
                "regionRequest",
                "regionSuggestion",
                "backbone",
                "hostMode",
            ],
        );
    }
    for (list, names) in [
        (
            "members",
            &["region", "vms", "externalBridge", "bridgesExternal"][..],
        ),
        (
            "subnets",
            &["region", "kind", "hostGroup", "bridge", "name"][..],
        ),
    ] {
        if let Some(items) = status.get_mut(list).and_then(Value::as_array_mut) {
            for item in items {
                remove(item, names);
            }
        }
    }
}

/// A `mesh_status` answer as a window may see it: without the 0.4 fields while [`V04_ENABLED`] is off.
pub fn gate_status(status: &mut Value) {
    if !V04_ENABLED {
        strip_v04(status);
    }
}

/// Whether this build runs mesh networks: [`ENABLED`], and its engine accepts a mesh-only profile. Known without an
/// engine running, so the 组网 entry shows while disconnected.
pub fn supported() -> bool {
    ENABLED && engine_runs_mesh()
}

fn engine_runs_mesh() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let probe = json!({
            "config_version": 1,
            "network": { "name": "probe", "secret": "probe", "kind": "managed", "secure_mode": true },
            "node": { "role": "access" },
            "peers": [{ "server": "192.0.2.1", "port": 11010, "transport": "tcp" }],
        });
        let networks = [SavedNetwork {
            name: "probe".to_string(),
            joined: true,
            config: probe,
        }];
        only_profile(&outbounds(&networks)).is_some_and(|profile| {
            engine_embedded::config::RuntimeConfig::from_json(profile.to_string().as_bytes()).is_ok()
        })
    })
}

/// The snapshot a window shows while no engine runs, on a build whose engine runs mesh networks: the joined
/// network, else the first saved one, without members; `{}` with none saved. As the Apple clients' (`MeshModel`).
pub fn offline_status(networks: &[SavedNetwork]) -> Value {
    let Some(network) = networks
        .iter()
        .find(|network| network.joined)
        .or_else(|| networks.first())
    else {
        return json!({});
    };
    let security = summaries(std::slice::from_ref(network))
        .first()
        .map(|summary| summary.security);
    json!({
        "network": {
            "id": network.name,
            "name": network.name,
            // Joined while the engine is down: the VPN is on its way up for it.
            "state": if network.joined { "joining" } else { "not_joined" },
            "security": security,
        }
    })
}

/// The `tunnel` section of a `mesh_status` answer (mesh-status.md, 隧道生命周期): the networks the tunnel carries
/// and the DNS for the internal domain names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tunnel {
    /// CIDR ranges, in the engine's order.
    pub routes: Vec<String>,
    /// Goes up by one each time `routes` change; 1 after the engine starts.
    pub routes_version: u64,
    /// The tunnel's DNS address for the internal domain names; empty without them.
    pub dns_servers: Vec<String>,
    /// The suffixes those servers answer.
    pub match_domains: Vec<String>,
    /// The engine's routes the tunnel does not carry because they are wider than a mesh's ([`too_wide`]).
    pub refused: Vec<String>,
}

/// The widest IPv4 and IPv6 prefixes a mesh route may have (intranet-mesh §15.4, U2): /8 and /7.
pub const ROUTE_MIN_PREFIX_V4: u8 = 8;
pub const ROUTE_MIN_PREFIX_V6: u8 = 7;

/// Whether `route` (CIDR text, or a bare address for one host) is wider than any mesh network: a default route, one
/// of its halves, or any other IPv4 range wider than /8 or IPv6 range wider than /7. The tunnel never carries such a
/// route whatever the engine names, so a fault or a tampered configuration cannot take the device's traffic into the
/// mesh. What does not read as a route is not judged here.
pub fn too_wide(route: &str) -> bool {
    let Some((address, length)) = route.trim().split_once('/') else {
        return false;
    };
    let Ok(length) = length.trim().parse::<u8>() else {
        return false;
    };
    match address.trim().parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(_)) => length < ROUTE_MIN_PREFIX_V4,
        Ok(std::net::IpAddr::V6(_)) => length < ROUTE_MIN_PREFIX_V6,
        Err(_) => false,
    }
}

impl Tunnel {
    /// The section of an answer; `None` for a refusal or an answer without one.
    pub fn from_status(status: &Value) -> Option<Self> {
        let tunnel = status.get("tunnel")?.as_object()?;
        let strings = |value: Option<&Value>| -> Vec<String> {
            value
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let dns = tunnel.get("dns");
        let (refused, routes) = strings(tunnel.get("routes"))
            .into_iter()
            .partition(|route| too_wide(route));
        Some(Self {
            routes,
            refused,
            routes_version: tunnel
                .get("routesVersion")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            dns_servers: strings(dns.and_then(|dns| dns.get("servers"))),
            match_domains: strings(dns.and_then(|dns| dns.get("matchDomains"))),
        })
    }

    /// The routes and DNS servers of the interface that carries the mesh alone: these routes, and, while internal
    /// domain names are on, each DNS server, as a host route so its queries reach the tunnel. A phone's VPN has
    /// no per-suffix DNS, so the engine then answers every name, the internal ones from the mesh.
    pub fn routes_and_dns(tunnel: Option<&Self>) -> (Vec<String>, Vec<String>) {
        let Some(tunnel) = tunnel else {
            return (Vec::new(), Vec::new());
        };
        let servers = if tunnel.match_domains.is_empty() {
            Vec::new()
        } else {
            tunnel.dns_servers.clone()
        };
        let mut routes = tunnel.routes.clone();
        for server in &servers {
            let host = if server.contains(':') {
                format!("{server}/128")
            } else {
                format!("{server}/32")
            };
            if !routes.contains(&host) && !routes.contains(server) {
                routes.push(host);
            }
        }
        (routes, servers)
    }
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

    #[test]
    fn saving_joining_and_removing_keep_the_file_and_tell_a_running_engine() {
        if !ENABLED {
            return;
        }
        let dir = std::env::temp_dir().join(format!("mesh-change-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut sent = Vec::new();
        let mut engine = |request: Value| {
            sent.push(request["cmd"].as_str().unwrap().to_string());
            Some(json!({ "ok": true }))
        };
        assert_eq!(
            save_network(&dir, config("a", "s"), &mut engine).unwrap(),
            json!({ "ok": true })
        );
        assert_eq!(
            save_network(&dir, config("b", "s"), &mut engine).unwrap(),
            json!({ "ok": true })
        );
        // A device joins one network at a time: saving b left a.
        let joined: Vec<_> = load(&dir)
            .unwrap()
            .into_iter()
            .map(|network| (network.name, network.joined))
            .collect();
        assert_eq!(joined, [("a".to_string(), false), ("b".to_string(), true)]);
        assert_eq!(
            sent,
            [
                "mesh_save_network",
                "mesh_join",
                "mesh_leave",
                "mesh_save_network",
                "mesh_join"
            ]
        );

        // A refusal changes nothing that was saved.
        let refused = json!({ "ok": false, "error": { "code": "mesh.join_failed" } });
        assert_eq!(
            set_joined(&dir, "a", true, |_| Some(refused.clone())).unwrap(),
            refused
        );
        assert!(load(&dir)
            .unwrap()
            .iter()
            .any(|network| network.name == "b" && network.joined));
        // With no engine running only the saved choice changes.
        assert_eq!(
            set_joined(&dir, "a", true, |_| None).unwrap(),
            json!({ "ok": true })
        );
        assert!(load(&dir)
            .unwrap()
            .iter()
            .all(|network| network.joined == (network.name == "a")));
        assert!(set_joined(&dir, "c", true, |_| None)
            .unwrap_err()
            .starts_with("mesh.not_found"));

        assert_eq!(
            remove_network(&dir, "a", |_| None).unwrap(),
            json!({ "ok": true })
        );
        assert_eq!(load(&dir).unwrap().len(), 1);
        assert!(remove_network(&dir, "a", |_| None).is_err());
        assert!(save_network(&dir, json!({ "config_version": 2 }), |_| None).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_release_gate_keeps_mesh_out_unless_the_build_turns_it_on() {
        if ENABLED {
            return;
        }
        assert!(!supported());
        let dir = std::env::temp_dir().join(format!("mesh-gate-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        store(
            &dir,
            &[SavedNetwork {
                name: "a".into(),
                joined: true,
                config: config("a", "s"),
            }],
        )
        .unwrap();
        let envelope = json!({ "schema_version": 1, "outbound": { "type": "direct" } }).to_string();
        assert_eq!(with_joined(&dir, envelope.clone()), (envelope, None));
        // Saving and joining are refused for every caller, without asking an engine; leaving and removing stay
        // open.
        let mut asked = false;
        assert!(save_network(&dir, config("b", "s"), |_| {
            asked = true;
            None
        })
        .unwrap_err()
        .starts_with("mesh.disabled"));
        assert!(set_joined(&dir, "a", true, |_| {
            asked = true;
            None
        })
        .unwrap_err()
        .starts_with("mesh.disabled"));
        assert!(!asked);
        assert_eq!(
            set_joined(&dir, "a", false, |_| None).unwrap(),
            json!({ "ok": true })
        );
        assert_eq!(
            remove_network(&dir, "a", |_| None).unwrap(),
            json!({ "ok": true })
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn joined_networks_join_the_envelope() {
        if !ENABLED {
            return;
        }
        let dir = std::env::temp_dir().join(format!("mesh-envelope-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let envelope = json!({ "schema_version": 1, "outbound": { "type": "direct" } }).to_string();
        // None joined: the envelope stays.
        assert_eq!(
            with_joined(&dir, envelope.clone()),
            (envelope.clone(), None)
        );
        fs::write(path(&dir), b"{").unwrap();
        let (kept, why) = with_joined(&dir, envelope.clone());
        assert_eq!(kept, envelope);
        assert!(why.is_some(), "an unreadable file is logged");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_mesh_only_profile_has_only_the_mesh_and_direct() {
        let networks = [SavedNetwork {
            name: "office".to_string(),
            joined: true,
            config: config("office", "s"),
        }];
        let profile = only_profile(&outbounds(&networks)).unwrap();
        assert!(profile.get("outbound").is_none());
        assert!(
            profile.get("mode").is_none(),
            "the rule mode is the default"
        );
        assert_eq!(profile["profile_id"], ONLY_PROFILE_ID);
        let routing = &profile["routing"];
        assert_eq!(
            routing["outbounds"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["mesh-office"]
        );
        assert_eq!(routing["rules"], json!(["FINAL,DIRECT"]));
        assert!(only_profile(&Map::new()).is_none());
    }

    #[test]
    fn the_offline_status_names_the_joined_network() {
        assert_eq!(offline_status(&[]), json!({}));
        let networks = [
            SavedNetwork {
                name: "a".to_string(),
                joined: false,
                config: config("a", "s"),
            },
            SavedNetwork {
                name: "b".to_string(),
                joined: true,
                config: config("b", "s"),
            },
        ];
        let status = offline_status(&networks);
        assert_eq!(status["network"]["id"], "b");
        assert_eq!(status["network"]["state"], "joining");
        assert_eq!(status["network"]["security"], "forward_secrecy");
        assert!(status.get("members").is_none());
        assert_eq!(
            offline_status(&networks[..1])["network"]["state"],
            "not_joined"
        );
    }

    #[test]
    fn reads_the_tunnel_section_and_routes_only_the_mesh() {
        let status = json!({
            "statusVersion": 1,
            "tunnel": {
                "routes": ["10.144.144.0/24", "192.168.20.0/24"],
                "routesVersion": 3,
                "dns": { "servers": ["198.18.0.2"], "matchDomains": ["office.internal"] },
            },
        });
        let tunnel = Tunnel::from_status(&status).unwrap();
        assert_eq!(tunnel.routes_version, 3);
        assert_eq!(tunnel.match_domains, ["office.internal"]);
        let (routes, dns) = Tunnel::routes_and_dns(Some(&tunnel));
        assert_eq!(
            routes,
            ["10.144.144.0/24", "192.168.20.0/24", "198.18.0.2/32"]
        );
        assert_eq!(dns, ["198.18.0.2"]);
        assert!(!routes.iter().any(|route| route == "0.0.0.0/0"));

        // Without suffixes the device keeps its own DNS.
        let bare = Tunnel {
            match_domains: Vec::new(),
            ..tunnel
        };
        assert_eq!(
            Tunnel::routes_and_dns(Some(&bare)),
            (bare.routes.clone(), Vec::new())
        );

        let no_dns = Tunnel::from_status(
            &json!({"tunnel": {"routes": [], "routesVersion": 1, "dns": null}}),
        );
        assert_eq!(no_dns.unwrap().dns_servers, Vec::<String>::new());
        assert!(
            Tunnel::from_status(&json!({"ok": false, "error": {"code": "unsupported"}})).is_none()
        );
        // Before the engine answers, nothing is routed.
        assert_eq!(Tunnel::routes_and_dns(None), (Vec::new(), Vec::new()));
    }

    #[test]
    fn the_tunnel_never_carries_a_route_wider_than_a_mesh() {
        let wide = [
            "0.0.0.0/0",
            "0.0.0.0/1",
            "128.0.0.0/1",
            "0.0.0.0/7",
            "::/0",
            "::/1",
            "8000::/1",
            "2000::/3",
            "fc00::/6",
        ];
        let narrow = [
            "10.0.0.0/8",
            "fc00::/7",
            "10.144.144.0/24",
            "198.18.0.2",
            "fd00::1",
        ];
        let routes: Vec<&str> = wide.iter().chain(narrow.iter()).copied().collect();
        let tunnel =
            Tunnel::from_status(&json!({ "tunnel": { "routes": routes, "routesVersion": 1 } }))
                .unwrap();
        assert_eq!(tunnel.routes, narrow);
        assert_eq!(tunnel.refused, wide);
        let (carried, _) = Tunnel::routes_and_dns(Some(&tunnel));
        assert!(!carried.iter().any(|route| too_wide(route)), "{carried:?}");
        assert!(!too_wide("not a route"));
        assert!(!too_wide("10.0.0.0/x"));
    }

    #[test]
    fn strip_v04_leaves_the_kernel_0_3_fields() {
        let mut status = json!({
            "network": { "state": "joined" },
            "regions": [{ "code": "eu" }],
            "links": [],
            "regionRequests": [],
            "externalBridge": {},
            "refusedAdvertisements": 1,
            "self": {
                "id": "a", "region": "eu", "regionBasis": {}, "regionState": "auto", "regionRequest": null,
                "regionSuggestion": null, "backbone": null, "hostMode": true, "address": "10.0.0.1/24"
            },
            "members": [{ "id": "b", "region": "eu", "vms": [], "externalBridge": true, "bridgesExternal": true, "role": "host" }],
            "subnets": [{ "cidr": "10.1.0.0/24", "region": "eu", "kind": "lan", "hostGroup": null, "bridge": "b", "name": "x", "state": "ok" }],
        });
        strip_v04(&mut status);
        assert_eq!(
            status,
            json!({
                "network": { "state": "joined" },
                "refusedAdvertisements": 1,
                "self": { "id": "a", "address": "10.0.0.1/24" },
                "members": [{ "id": "b", "role": "host" }],
                "subnets": [{ "cidr": "10.1.0.0/24", "state": "ok" }],
            })
        );
        // A snapshot that is not an object is left alone.
        let mut other = json!(null);
        strip_v04(&mut other);
        assert_eq!(other, json!(null));
    }

    #[test]
    fn the_0_4_gate_is_inside_the_mesh_gate() {
        let mut status = json!({ "regions": [], "members": [{ "id": "b", "vms": [] }] });
        gate_status(&mut status);
        assert_eq!(status.get("regions").is_some(), V04_ENABLED);
    }
}
