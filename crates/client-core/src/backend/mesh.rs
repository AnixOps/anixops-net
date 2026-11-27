//! The 组网 (mesh) networks this client saved (docs/architecture/mesh-client.md; networkcore_anixops
//! docs/architecture/mesh-module.md, 配置): each the kernel's `config_version: 1` configuration with its secret, in an
//! owner-only file beside the state. Every saved network that is joined goes into the engine's profile as a
//! `{"type":"mesh"}` outbound; the engine sends the network's routes to it ahead of the rules (R23), so no rule names
//! it. Joining, leaving, saving and removing also go to a running engine directly, without a restart. Windows never
//! see a secret: [`summaries`] leaves it out. On Windows the file is sealed for the Windows account with Data
//! Protection, as the LAN share's password is. On Android it is sealed with AES-GCM under a key the Android Keystore
//! wraps ([`set_key_source`]); without the key nothing is read or written.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(target_os = "android")]
use std::sync::Mutex;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
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

/// The file under the client's data folder, sealed with the Keystore-wrapped key. A plain [`FILE`] left by an older
/// version is read once, sealed into this file, and deleted.
#[cfg(any(target_os = "android", test))]
pub const SEALED_FILE: &str = "mesh-networks.sealed";
#[cfg(any(target_os = "android", test))]
const SEALED_WHAT: &str = "mesh networks file";

/// The device's instance ID file under the client's data folder, beside [`FILE`].
pub const INSTANCE_FILE: &str = "mesh-instance-id";

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
    #[cfg(target_os = "android")]
    return dir.join(SEALED_FILE);
    #[cfg(not(target_os = "android"))]
    dir.join(FILE)
}

/// Where the Keystore-wrapped key comes from: called until it succeeds, then the key is kept for the process.
#[cfg(target_os = "android")]
type KeySource = Box<dyn Fn() -> Result<desktop_mitm::sessions::SessionKey, String> + Send + Sync>;

#[cfg(target_os = "android")]
struct KeySlot {
    source: Option<KeySource>,
    key: Option<desktop_mitm::sessions::SessionKey>,
}

#[cfg(target_os = "android")]
static KEY: Mutex<KeySlot> = Mutex::new(KeySlot {
    source: None,
    key: None,
});

/// Android: names where the file's key comes from (the platform's Keystore); the client calls it once before the
/// backend opens. Until it is called, or while the source fails, [`load`] and [`store`] refuse.
#[cfg(target_os = "android")]
pub fn set_key_source(source: KeySource) {
    if let Ok(mut slot) = KEY.lock() {
        slot.source = Some(source);
        slot.key = None;
    }
}

#[cfg(target_os = "android")]
fn file_key() -> Result<desktop_mitm::sessions::SessionKey, String> {
    let mut slot = KEY
        .lock()
        .map_err(|_| "the mesh networks file key is not available".to_string())?;
    if let Some(key) = &slot.key {
        return Ok(key.clone());
    }
    let source = slot
        .source
        .as_ref()
        .ok_or_else(|| "the mesh networks file key is not available".to_string())?;
    let key = source()
        .map_err(|error| format!("the mesh networks file key is not available: {error}"))?;
    slot.key = Some(key.clone());
    Ok(key)
}

/// Seals `bytes` for the file.
#[cfg(any(target_os = "android", test))]
fn seal(bytes: &[u8], key: Option<&desktop_mitm::sessions::SessionKey>) -> Result<Vec<u8>, String> {
    key.ok_or_else(|| "the mesh networks file key is not available".to_string())?
        .seal(bytes, SEALED_WHAT)
}

/// Opens sealed bytes with `key`. No key, a wrong key or damage is an error, never an empty list.
#[cfg(any(target_os = "android", test))]
fn open(bytes: &[u8], key: Option<&desktop_mitm::sessions::SessionKey>) -> Result<Vec<u8>, String> {
    key.ok_or_else(|| "the mesh networks file key is not available".to_string())?
        .open(bytes, SEALED_WHAT)
}

/// Whether `bytes` are a legacy plain file: JSON whose first non-blank byte is `[`.
#[cfg(any(target_os = "android", test))]
fn is_legacy_plain(bytes: &[u8]) -> bool {
    bytes.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'[')
}

/// Android's [`load`]: the sealed file, else the legacy plain one (sealed into the sealed file at once when the
/// key is there, and the plain file deleted). A failure leaves every file as it was.
#[cfg(any(target_os = "android", test))]
fn load_sealed(
    dir: &Path,
    key: Option<&desktop_mitm::sessions::SessionKey>,
) -> Result<Vec<SavedNetwork>, String> {
    let sealed = dir.join(SEALED_FILE);
    let plain = dir.join(FILE);
    match fs::read(&sealed) {
        Ok(bytes) => {
            let bytes = open(&bytes, key)?;
            return serde_json::from_slice(&bytes)
                .map_err(|error| format!("the mesh networks file is not valid: {error}"));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("the mesh networks file could not be read: {error}")),
    }
    let bytes = match fs::read(&plain) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("the mesh networks file could not be read: {error}")),
    };
    if !is_legacy_plain(&bytes) {
        return Err("the mesh networks file is not valid".to_string());
    }
    let networks: Vec<SavedNetwork> = serde_json::from_slice(&bytes)
        .map_err(|error| format!("the mesh networks file is not valid: {error}"))?;
    // Sealed at once when the key is there; the networks stay readable when it is not, and nothing is written.
    if key.is_some() {
        let _ = store_sealed(dir, &networks, key);
    }
    Ok(networks)
}

/// Android's [`store`]: seals with `key` (no key: nothing is written), then deletes a legacy plain file.
#[cfg(any(target_os = "android", test))]
fn store_sealed(
    dir: &Path,
    networks: &[SavedNetwork],
    key: Option<&desktop_mitm::sessions::SessionKey>,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(networks).map_err(|error| error.to_string())?;
    let bytes = seal(&bytes, key)?;
    desktop_mitm::sessions::write_private(&dir.join(SEALED_FILE), &bytes)
        .map_err(|error| format!("the mesh networks could not be saved: {error}"))?;
    match fs::remove_file(dir.join(FILE)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "the old mesh networks file could not be deleted: {error}"
        )),
    }
}

/// The saved networks; none when nothing was saved yet. A file that cannot be read or understood is an error, so
/// that the next save does not replace it with fewer networks.
pub fn load(dir: &Path) -> Result<Vec<SavedNetwork>, String> {
    #[cfg(target_os = "android")]
    return load_sealed(dir, file_key().ok().as_ref());
    #[cfg(not(target_os = "android"))]
    load_plain(dir)
}

#[cfg(not(target_os = "android"))]
fn load_plain(dir: &Path) -> Result<Vec<SavedNetwork>, String> {
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

/// Keeps `networks`, through a temporary file and a rename: owner-only, and on Windows and Android sealed.
pub fn store(dir: &Path, networks: &[SavedNetwork]) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return store_sealed(dir, networks, file_key().ok().as_ref());
    #[cfg(not(target_os = "android"))]
    store_plain(dir, networks)
}

#[cfg(not(target_os = "android"))]
fn store_plain(dir: &Path, networks: &[SavedNetwork]) -> Result<(), String> {
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
    // The network's own static key: the saved one stays, else the incoming valid one, else a new one.
    let existing = networks
        .iter()
        .find(|network| network.name == name)
        .map(|network| &network.config);
    let config = with_private_key(config, existing)?;
    let config = with_instance_id(config, &instance_id(dir)?);
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
    let mut next = self::joined(&networks, name, joined).ok_or_else(|| not_found(name))?;
    // A network saved before keys or the instance ID existed gets them when it is joined; the engine is told
    // before it joins.
    let mut keyed_config = None;
    if joined {
        let id = instance_id(dir)?;
        if let Some(network) = next.iter_mut().find(|network| network.name == name) {
            let mut config = network.config.clone();
            if !has_private_key(&config) {
                config = with_private_key(config, None)?;
            }
            let config = with_instance_id(config, &id);
            if config != network.config {
                network.config = config.clone();
                keyed_config = Some(config);
            }
        }
    }
    if joined {
        for other in networks
            .iter()
            .filter(|network| network.joined && network.name != name)
        {
            request(json!({ "cmd": "mesh_leave", "network": other.name }));
        }
    }
    if let Some(config) = keyed_config {
        let saved = request(json!({ "cmd": "mesh_save_network", "config": config }));
        if let Some(refused) = saved.filter(|answer| !took_effect(answer)) {
            return Ok(refused);
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
    joined_into(dir, envelope, ENABLED)
}

/// [`with_joined`] with the gate as a value, so that a test sees what an open gate hands the engine.
fn joined_into(dir: &Path, envelope: String, open: bool) -> (String, Option<String>) {
    if !open {
        return (envelope, None);
    }
    let networks = match load(dir) {
        Ok(networks) => networks,
        Err(error) => return (envelope, Some(error)),
    };
    let networks = match with_instance_ids(dir, networks) {
        Ok(networks) => networks,
        Err(error) => return (envelope, Some(error)),
    };
    let Some(meshed) = serde_json::from_str::<Value>(&envelope)
        .ok()
        .and_then(|profile| with_outbounds(&profile, &outbounds(&networks)))
    else {
        return (envelope, None);
    };
    let mut meshed = meshed;
    set_allow_mesh(&mut meshed, open);
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

/// Whether `config` has a `node.private_key` that is standard base64 of exactly 32 bytes.
fn has_private_key(config: &Value) -> bool {
    config
        .get("node")
        .and_then(|node| node.get("private_key"))
        .and_then(Value::as_str)
        .is_some_and(|key| STANDARD.decode(key).is_ok_and(|bytes| bytes.len() == 32))
}

/// `config` with the network's static secure-mode private key as `node.private_key` (created `node` if missing):
/// the one in `existing`, the network's already saved configuration, when valid, so that saving again never changes
/// it; else a valid one `config` brings; else 32 random bytes from the operating system. The key is never logged,
/// nor shown to a window ([`summaries`]). Fails only when the system cannot give random bytes.
pub fn with_private_key(mut config: Value, existing: Option<&Value>) -> Result<Value, String> {
    let key = match existing.filter(|existing| has_private_key(existing)) {
        Some(existing) => existing["node"]["private_key"].clone(),
        None if has_private_key(&config) => config["node"]["private_key"].clone(),
        None => {
            let mut bytes = [0u8; 32];
            getrandom::getrandom(&mut bytes).map_err(|_| {
                crate::refusal::coded("mesh.key", "the network key could not be generated")
            })?;
            Value::String(STANDARD.encode(bytes))
        }
    };
    let Some(root) = config.as_object_mut() else {
        return Ok(config);
    };
    let node = root
        .entry("node")
        .or_insert_with(|| Value::Object(Map::new()));
    if !node.is_object() {
        *node = Value::Object(Map::new());
    }
    node["private_key"] = key;
    Ok(config)
}

/// Whether `id` is a valid `node.instance_id`: `^[0-9A-Za-z][0-9A-Za-z-]{0,63}$`.
fn is_instance_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

/// A lowercase UUID v4 text from 16 random bytes (version and variant bits set).
fn uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut text = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// This device's mesh instance ID (`node.instance_id` of every network, and `self.id` of the status snapshot): made
/// on first use from 16 random bytes as a UUID v4 and kept in [`INSTANCE_FILE`] under `dir`. A file that is there
/// but holds no valid ID is an error, never replaced: a changing ID would split the device in the panel. Not a
/// secret, so the file is not sealed.
pub fn instance_id(dir: &Path) -> Result<String, String> {
    let file = dir.join(INSTANCE_FILE);
    match fs::read(&file) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let id = text.trim();
            if is_instance_id(id) {
                Ok(id.to_string())
            } else {
                Err("the mesh instance ID file is not valid".to_string())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut bytes = [0u8; 16];
            getrandom::getrandom(&mut bytes).map_err(|_| {
                crate::refusal::coded("mesh.instance_id", "the instance ID could not be generated")
            })?;
            let id = uuid_v4(bytes);
            desktop_mitm::sessions::write_private(&file, id.as_bytes())
                .map_err(|error| format!("the mesh instance ID could not be saved: {error}"))?;
            Ok(id)
        }
        Err(error) => Err(format!("the mesh instance ID could not be read: {error}")),
    }
}

/// `config` with `node.instance_id` set to `id` (created `node` if missing); a different one it brought is replaced,
/// the rest stays.
pub fn with_instance_id(mut config: Value, id: &str) -> Value {
    let Some(root) = config.as_object_mut() else {
        return config;
    };
    let node = root
        .entry("node")
        .or_insert_with(|| Value::Object(Map::new()));
    if !node.is_object() {
        *node = Value::Object(Map::new());
    }
    node["instance_id"] = Value::String(id.to_string());
    config
}

/// `networks` whose joined ones carry this device's instance ID, for the engine's profile (saved configs already do;
/// this covers ones saved before it existed). The ID is made only when a network is joined.
pub fn with_instance_ids(
    dir: &Path,
    mut networks: Vec<SavedNetwork>,
) -> Result<Vec<SavedNetwork>, String> {
    if networks.iter().any(|network| network.joined) {
        let id = instance_id(dir)?;
        for network in networks.iter_mut().filter(|network| network.joined) {
            network.config = with_instance_id(network.config.clone(), &id);
        }
    }
    Ok(networks)
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

/// The refusal code of a profile with a mesh outbound while the release gate is closed (`error.<name>`,
/// [`crate::refusal`]); every window words it itself.
pub const GATED_CODE: &str = "error.mesh_gated";

/// The refusal of a profile that cannot be read while the gate is closed and that mentions mesh: nothing can tell what
/// it would build. A plain one (no code), as any invalid configuration; not [`GATED_CODE`].
const UNREADABLE_PROFILE: &str =
    "this profile is not a valid configuration (it cannot be read and mentions mesh)";

/// Whether `error` is the refusal [`refuse_gated_outbounds`] makes: it is certain to repeat, so nothing retries it.
pub fn is_gated_refusal(error: &str) -> bool {
    crate::refusal::split(error).is_some_and(|(code, _)| code == GATED_CODE)
}

/// Refuses `config`, the final engine profile, while the release gate is closed ([`ENABLED`]) when it holds a mesh
/// outbound: the engine has mesh compiled in and its only switch is `allow_mesh` (the second layer behind this refusal), so a `"type":"mesh"` entry anywhere the kernel
/// builds an outbound would start it, a user-written or imported profile included ([`has_mesh_outbound`]). Nothing
/// is stripped (a rule may name the tag). Called with the profile of every engine start.
pub fn refuse_gated_outbounds(config: &Value) -> Result<(), String> {
    refuse_outbounds(ENABLED, config)
}

fn refuse_outbounds(enabled: bool, config: &Value) -> Result<(), String> {
    if enabled || !has_mesh_outbound(config) {
        return Ok(());
    }
    Err(crate::refusal::coded(
        GATED_CODE,
        "this profile contains a mesh outbound; mesh networks are not available in this version",
    ))
}

/// How deep an inline `via` chain is followed. The kernel refuses a chain over 4 (`MAX_VIA_CHAIN` in the kernel's config), so a deeper one is
/// no config it runs; it counts as holding a mesh outbound (fail closed), so the gate does not depend on the JSON
/// parser's own nesting limit.
const MAX_VIA_DEPTH: usize = 64;

/// Whether the profile builds a mesh outbound, where the kernel builds one: the top-level `outbound`, every entry of
/// `routing.outbounds` (the object form, tag to entry, or an array of entries), and the inline `via` objects of
/// either, however deep (past [`MAX_VIA_DEPTH`] counts as one).
fn has_mesh_outbound(config: &Value) -> bool {
    config
        .get("outbound")
        .is_some_and(|entry| entry_is_mesh(entry, 0))
        || match config.pointer("/routing/outbounds") {
            Some(Value::Object(map)) => map.values().any(|entry| entry_is_mesh(entry, 0)),
            Some(Value::Array(list)) => list.iter().any(|entry| entry_is_mesh(entry, 0)),
            _ => false,
        }
}

/// An outbound entry that is of type `mesh` or whose inline `via` chain holds one, or is deeper than [`MAX_VIA_DEPTH`].
fn entry_is_mesh(entry: &Value, depth: usize) -> bool {
    if depth > MAX_VIA_DEPTH {
        return true;
    }
    entry.get("type").and_then(Value::as_str) == Some("mesh")
        || entry
            .get("via")
            .is_some_and(|via| via.is_object() && entry_is_mesh(via, depth + 1))
}

/// [`refuse_gated_outbounds`] for an envelope in text; text that is not JSON is left to the engine to refuse.
pub fn refuse_gated_envelope(envelope: &str) -> Result<(), String> {
    if ENABLED {
        return Ok(());
    }
    match serde_json::from_str::<Value>(envelope) {
        Ok(config) => refuse_gated_outbounds(&config),
        Err(_) => Ok(()),
    }
}

/// What every engine start hands the engine: `envelope` after the client's own refusal ([`refuse_gated_outbounds`])
/// and with the engine's gate written ([`with_allow_mesh_text`]). Text that is not JSON is left to the engine unless
/// the gate is closed and it names mesh.
pub fn start_envelope(envelope: &str) -> Result<String, String> {
    prepared_start(envelope, ENABLED)
}

/// [`start_envelope`] with the gate as a value: the one function the start paths take their envelope from, so that a
/// test sees both values of the gate reach the engine. Never called with a value but the release gate outside tests.
pub(crate) fn prepared_start(envelope: &str, open: bool) -> Result<String, String> {
    if let Ok(config) = serde_json::from_str::<Value>(envelope) {
        refuse_outbounds(open, &config)?;
    }
    allow_mesh_text(envelope, open)
}

/// Sets the engine's own gate, `allow_mesh`, to the release gate ([`ENABLED`]) in `config`, a final engine envelope:
/// the second layer behind [`refuse_gated_outbounds`] (the kernel's default is `false`, and while it is `false` the
/// engine refuses any mesh outbound and answers no `mesh_*` command). Whatever the envelope said is replaced, so no
/// profile can open the engine's gate by itself. The only place the client writes the field.
pub fn with_allow_mesh(config: &mut Value) {
    set_allow_mesh(config, ENABLED);
}

fn set_allow_mesh(config: &mut Value, open: bool) {
    if let Some(object) = config.as_object_mut() {
        object.insert("allow_mesh".to_string(), Value::Bool(open));
    }
}

/// [`with_allow_mesh`] for an envelope in text: what every engine start hands the engine. Text that is not JSON is
/// left to the engine to refuse, unless it names mesh and the gate is closed: then it is refused here as an invalid
/// configuration ([`UNREADABLE_PROFILE`]), as nothing can tell what it would build.
pub fn with_allow_mesh_text(envelope: &str) -> Result<String, String> {
    allow_mesh_text(envelope, ENABLED)
}

pub(crate) fn allow_mesh_text(envelope: &str, open: bool) -> Result<String, String> {
    match serde_json::from_str::<Value>(envelope) {
        Ok(mut config) => {
            set_allow_mesh(&mut config, open);
            Ok(config.to_string())
        }
        // Not the gated refusal: the profile is damaged, and mesh may be only a name in it.
        Err(_) if !open && envelope.contains("mesh") => Err(UNREADABLE_PROFILE.to_string()),
        Err(_) => Ok(envelope.to_string()),
    }
}

/// [`with_allow_mesh_text`] for an envelope in bytes (the Windows desktop's start paths). Bytes that are not text are
/// refused while the gate is closed if they name mesh, else left to the engine to refuse.
pub fn with_allow_mesh_bytes(envelope: &[u8]) -> Result<Vec<u8>, String> {
    allow_mesh_bytes(envelope, ENABLED)
}

fn allow_mesh_bytes(envelope: &[u8], open: bool) -> Result<Vec<u8>, String> {
    match std::str::from_utf8(envelope) {
        Ok(text) => allow_mesh_text(text, open).map(String::into_bytes),
        Err(_) if !open && envelope.windows(4).any(|window| window == b"mesh") => {
            allow_mesh_text("mesh", open).map(String::into_bytes)
        }
        Err(_) => Ok(envelope.to_vec()),
    }
}

/// An engine start error as a window shows it: the kernel's own refusal of a mesh outbound while `allow_mesh` is off
/// (`config_rejected`, [`engine_embedded::config::MESH_NOT_ALLOWED`]) is the gated refusal, worded as the client's.
pub fn map_kernel_refusal(error: String) -> String {
    if error.contains(engine_embedded::config::MESH_NOT_ALLOWED) {
        return crate::refusal::coded(
            GATED_CODE,
            "this profile contains a mesh outbound; mesh networks are not available in this version",
        );
    }
    error
}

/// Checks `envelope` the way the engine would take it: with the engine's gate written ([`with_allow_mesh_text`]), for
/// the validators that only parse it (saving DNS settings, rules, lists). The kernel's refusal of a mesh outbound
/// is the client's gated one ([`map_kernel_refusal`]); any other refusal is worded by `wrap` from the engine's message.
pub fn check_envelope(envelope: &str, wrap: impl FnOnce(String) -> String) -> Result<(), String> {
    let envelope = with_allow_mesh_text(envelope)?;
    engine_embedded::config::RuntimeConfig::from_json(envelope.as_bytes())
        .map(|_| ())
        .map_err(|error| {
            let error = map_kernel_refusal(error.message.to_string());
            if is_gated_refusal(&error) {
                error
            } else {
                wrap(error)
            }
        })
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
    *SUPPORTED.get_or_init(|| runs_mesh(ENABLED))
}

/// Whether the engine builds a mesh-only profile with its gate `open` (a test's value; [`engine_runs_mesh`] passes the
/// release gate). The same answer the engine gives to `{"cmd":"capabilities"}`: its config builds with the mesh module
/// compiled in and `allow_mesh` true.
fn runs_mesh(open: bool) -> bool {
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
    only_profile(&outbounds(&networks)).is_some_and(|mut profile| {
        set_allow_mesh(&mut profile, open);
        engine_embedded::config::RuntimeConfig::from_json(profile.to_string().as_bytes()).is_ok()
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

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesh-sealed-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn key(byte: u8) -> desktop_mitm::sessions::SessionKey {
        desktop_mitm::sessions::SessionKey::new(
            &[byte; desktop_mitm::sessions::KEY_LEN],
            desktop_mitm::sessions::Protection::Keystore,
        )
        .unwrap()
    }

    fn saved_one() -> Vec<SavedNetwork> {
        vec![SavedNetwork {
            name: "office".into(),
            joined: true,
            config: config("office", "s3cret"),
        }]
    }

    #[test]
    fn sealed_file_round_trips_and_hides_the_secret() {
        let dir = test_dir("round");
        let key = key(1);
        store_sealed(&dir, &saved_one(), Some(&key)).unwrap();
        let raw = fs::read(dir.join(SEALED_FILE)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("s3cret"));
        assert!(serde_json::from_slice::<Value>(&raw).is_err());
        assert_eq!(load_sealed(&dir, Some(&key)).unwrap(), saved_one());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_plain_file_is_read_sealed_and_deleted() {
        let dir = test_dir("legacy");
        let plain = serde_json::to_vec_pretty(&saved_one()).unwrap();
        assert!(is_legacy_plain(&plain));
        assert!(is_legacy_plain(b"  \n[]"));
        assert!(!is_legacy_plain(b"{}"));
        fs::write(dir.join(FILE), &plain).unwrap();
        let key = key(1);
        assert_eq!(load_sealed(&dir, Some(&key)).unwrap(), saved_one());
        assert!(!dir.join(FILE).exists());
        assert_eq!(load_sealed(&dir, Some(&key)).unwrap(), saved_one());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_plain_file_stays_when_there_is_no_key() {
        let dir = test_dir("legacy-nokey");
        let plain = serde_json::to_vec_pretty(&saved_one()).unwrap();
        fs::write(dir.join(FILE), &plain).unwrap();
        assert_eq!(load_sealed(&dir, None).unwrap(), saved_one());
        assert_eq!(fs::read(dir.join(FILE)).unwrap(), plain);
        assert!(!dir.join(SEALED_FILE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_or_missing_key_is_an_error_and_changes_nothing() {
        let dir = test_dir("wrong");
        store_sealed(&dir, &saved_one(), Some(&key(1))).unwrap();
        let before = fs::read(dir.join(SEALED_FILE)).unwrap();
        assert!(load_sealed(&dir, Some(&key(2))).is_err());
        assert!(load_sealed(&dir, None).is_err());
        assert!(store_sealed(&dir, &[], None).is_err());
        assert_eq!(fs::read(dir.join(SEALED_FILE)).unwrap(), before);
        let _ = fs::remove_dir_all(&dir);
        let dir = test_dir("nokey-store");
        assert!(store_sealed(&dir, &saved_one(), None).is_err());
        assert!(!dir.join(SEALED_FILE).exists());
        let _ = fs::remove_dir_all(&dir);
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

    fn key_of(config: &Value) -> String {
        config["node"]["private_key"].as_str().unwrap().to_string()
    }

    #[test]
    fn a_new_network_gets_a_random_32_byte_key() {
        let first = with_private_key(config("a", "k"), None).unwrap();
        let second = with_private_key(config("a", "k"), None).unwrap();
        assert_eq!(STANDARD.decode(key_of(&first)).unwrap().len(), 32);
        assert_ne!(key_of(&first), key_of(&second));
        assert_eq!(first["node"]["role"], "access");
        // Without a node section, one is made.
        let bare = with_private_key(json!({ "config_version": 1 }), None).unwrap();
        assert!(has_private_key(&bare));
    }

    #[test]
    fn saving_again_keeps_the_saved_key() {
        let saved_config = with_private_key(config("a", "k"), None).unwrap();
        let kept = with_private_key(config("a", "k2"), Some(&saved_config)).unwrap();
        assert_eq!(key_of(&kept), key_of(&saved_config));
        assert_eq!(kept["network"]["secret"], "k2");
        // Even when the new configuration brings a different valid key.
        let other = with_private_key(config("a", "k"), None).unwrap();
        let kept = with_private_key(other, Some(&saved_config)).unwrap();
        assert_eq!(key_of(&kept), key_of(&saved_config));
    }

    #[test]
    fn a_valid_incoming_key_stays_and_an_invalid_one_is_replaced() {
        let key = STANDARD.encode([7u8; 32]);
        let mut incoming = config("a", "k");
        incoming["node"]["private_key"] = json!(key);
        let kept = with_private_key(incoming, None).unwrap();
        assert_eq!(key_of(&kept), key);
        // A saved configuration without a valid key does not count.
        let mut old = config("a", "k");
        old["node"]["private_key"] = json!("not base64");
        let mut incoming = config("a", "k");
        incoming["node"]["private_key"] = json!(key);
        assert_eq!(
            key_of(&with_private_key(incoming, Some(&old)).unwrap()),
            key
        );
        for bad in [
            json!("not base64!"),
            json!(STANDARD.encode([1u8; 31])),
            json!(STANDARD.encode([1u8; 33])),
            json!(5),
        ] {
            let mut incoming = config("a", "k");
            incoming["node"]["private_key"] = bad.clone();
            let fixed = with_private_key(incoming, None).unwrap();
            assert!(has_private_key(&fixed));
            assert_ne!(fixed["node"]["private_key"], bad);
        }
    }

    #[test]
    fn the_instance_id_is_a_lowercase_uuid_v4() {
        let first = uuid_v4([0xff; 16]);
        assert_eq!(first, "ffffffff-ffff-4fff-bfff-ffffffffffff");
        assert!(is_instance_id(&first));
        let mut one = [0u8; 16];
        let mut two = [0u8; 16];
        getrandom::getrandom(&mut one).unwrap();
        getrandom::getrandom(&mut two).unwrap();
        let (one, two) = (uuid_v4(one), uuid_v4(two));
        assert_ne!(one, two);
        for id in [&one, &two] {
            assert!(is_instance_id(id));
            assert_eq!(id.len(), 36);
            assert_eq!(id.as_bytes()[14], b'4');
            assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
            assert_eq!(id.to_lowercase(), **id);
        }
    }

    #[test]
    fn the_instance_id_format_is_checked() {
        assert!(is_instance_id("a"));
        assert!(is_instance_id("0-A-z"));
        assert!(is_instance_id(&"a".repeat(64)));
        assert!(!is_instance_id(""));
        assert!(!is_instance_id("-abc"));
        assert!(!is_instance_id(&"a".repeat(65)));
        assert!(!is_instance_id("abc_def"));
        assert!(!is_instance_id("abc def"));
        assert!(!is_instance_id("设备"));
        assert!(!is_instance_id("abc\u{e9}"));
    }

    #[test]
    fn injecting_the_instance_id_overwrites_a_foreign_one_and_keeps_the_rest() {
        let mut incoming = config("a", "s");
        incoming["node"] = json!({ "instance_id": "someone-else", "address": "10.0.0.2" });
        let fixed = with_instance_id(incoming, "mine-1");
        assert_eq!(fixed["node"]["instance_id"], "mine-1");
        assert_eq!(fixed["node"]["address"], "10.0.0.2");
        assert_eq!(fixed["network"]["name"], "a");
        let bare = with_instance_id(json!({ "config_version": 1 }), "mine-1");
        assert_eq!(bare["node"]["instance_id"], "mine-1");
    }

    #[test]
    fn the_instance_id_file_is_made_once_and_a_bad_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("mesh-instance-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let first = instance_id(&dir).unwrap();
        assert!(is_instance_id(&first));
        assert_eq!(instance_id(&dir).unwrap(), first);
        fs::write(dir.join(INSTANCE_FILE), b"-bad").unwrap();
        assert!(instance_id(&dir).is_err());
        assert_eq!(fs::read(dir.join(INSTANCE_FILE)).unwrap(), b"-bad");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_keeps_one_instance_id_in_every_network() {
        if !ENABLED {
            return;
        }
        let dir = std::env::temp_dir().join(format!("mesh-iid-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut foreign = config("a", "s");
        foreign["node"] = json!({ "instance_id": "someone-else" });
        save_network(&dir, foreign, |_| None).unwrap();
        save_network(&dir, config("b", "s"), |_| None).unwrap();
        save_network(&dir, config("a", "s2"), |_| None).unwrap();
        let id = instance_id(&dir).unwrap();
        for network in load(&dir).unwrap() {
            assert_eq!(network.config["node"]["instance_id"], id.as_str());
        }
        // A network saved before the ID existed gets it when joined and in the engine's profile.
        let mut old = config("old", "s");
        old["node"] = json!({ "instance_id": "stale" });
        let old = SavedNetwork {
            name: "old".into(),
            joined: true,
            config: old,
        };
        let carried = with_instance_ids(&dir, vec![old]).unwrap();
        assert_eq!(carried[0].config["node"]["instance_id"], id.as_str());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_and_joining_store_one_key_per_network() {
        if !ENABLED {
            return;
        }
        let dir = std::env::temp_dir().join(format!("mesh-key-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        save_network(&dir, config("a", "s"), |_| None).unwrap();
        let first = key_of(&load(&dir).unwrap()[0].config);
        save_network(&dir, config("a", "s2"), |_| None).unwrap();
        assert_eq!(key_of(&load(&dir).unwrap()[0].config), first);
        // An old network without a key gets one when it is joined, and the engine hears of it.
        store(
            &dir,
            &[SavedNetwork {
                name: "old".into(),
                joined: false,
                config: config("old", "s"),
            }],
        )
        .unwrap();
        let mut sent = Vec::new();
        set_joined(&dir, "old", true, |request| {
            sent.push(request["cmd"].as_str().unwrap().to_string());
            Some(json!({ "ok": true }))
        })
        .unwrap();
        assert!(has_private_key(&load(&dir).unwrap()[0].config));
        assert_eq!(sent, ["mesh_save_network", "mesh_join"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_network_with_no_key_is_known_before_it_is_joined() {
        let old = config("old", "s");
        assert!(!has_private_key(&old));
        let keyed = with_private_key(old, None).unwrap();
        assert!(has_private_key(&keyed));
        let again = with_private_key(keyed.clone(), None).unwrap();
        assert_eq!(key_of(&again), key_of(&keyed));
    }

    #[test]
    fn summaries_and_the_offline_status_leave_the_key_out() {
        let keyed = with_private_key(config("a", "s3cret"), None).unwrap();
        let key = key_of(&keyed);
        let networks = [SavedNetwork {
            name: "a".into(),
            joined: true,
            config: keyed,
        }];
        assert!(!serde_json::to_string(&summaries(&networks))
            .unwrap()
            .contains(&key));
        assert!(!offline_status(&networks).to_string().contains(&key));
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

    #[test]
    fn a_closed_gate_refuses_a_profile_with_a_mesh_outbound() {
        let object = json!({ "routing": { "outbounds": {
            "node": { "type": "ss" }, "mesh-x": { "type": "mesh" } }, "rules": ["FINAL,mesh-x"] } });
        let array = json!({ "routing": { "outbounds": [{ "type": "ss" }, { "type": "mesh" }] } });
        let top = json!({ "outbound": { "type": "mesh" } });
        let top_via = json!({ "outbound": { "type": "ss", "via": { "type": "mesh" } } });
        let entry_via = json!({ "routing": { "outbounds": {
            "a": { "type": "ss", "via": { "type": "ss", "via": { "type": "mesh" } } } } } });
        let array_via =
            json!({ "routing": { "outbounds": [{ "type": "ss", "via": { "type": "mesh" } }] } });
        for config in [&object, &array, &top, &top_via, &entry_via, &array_via] {
            let error = refuse_outbounds(false, config).unwrap_err();
            assert!(is_gated_refusal(&error), "{error}");
            assert!(error.starts_with("error.mesh_gated: "), "{error}");
            assert_eq!(refuse_outbounds(true, config), Ok(()));
        }
    }

    #[test]
    fn a_closed_gate_lets_other_profiles_through() {
        let plain = json!({ "routing": { "outbounds": { "node": { "type": "ss" }, "Mesh": { "type": "Mesh" } } } });
        let plain_via =
            json!({ "outbound": { "type": "ss", "via": { "type": "ss", "via": "mesh" } } });
        for config in [
            &plain,
            &plain_via,
            &json!({ "outbound": { "type": "ss" } }),
            &json!({}),
            &json!({ "routing": { "outbounds": 1 } }),
        ] {
            assert_eq!(refuse_outbounds(false, config), Ok(()));
        }
        assert_eq!(refuse_gated_envelope("not json"), Ok(()));
    }

    /// Both starts refuse a mesh profile before anything runs, so that removing the call fails here (a build with
    /// the gate open has nothing to refuse).
    #[test]
    fn engine_and_mesh_only_starts_are_gated() {
        if ENABLED {
            return;
        }
        let envelope = r#"{"outbound":{"type":"mesh"}}"#;
        let error = super::super::engine::Engine::default()
            .start(envelope, "node", 0)
            .unwrap_err();
        assert!(is_gated_refusal(&error), "{error}");
        #[cfg(any(target_os = "android", target_os = "linux", windows))]
        {
            let error = super::super::engine::MeshOnly::default()
                .start(envelope)
                .unwrap_err();
            assert!(is_gated_refusal(&error), "{error}");
        }
    }

    /// A refused start stops the engine that ran before it (no stale engine behind the error). Moving the check back
    /// before `stop_runtime` fails here. (`MeshOnly` needs a TUN descriptor to run at all, so only `Engine` can be
    /// started in a test.)
    #[cfg(not(target_os = "android"))]
    #[test]
    fn a_refused_start_stops_the_engine_that_ran() {
        if ENABLED {
            return;
        }
        let mut engine = super::super::engine::Engine::default();
        engine
            .start(&super::super::engine::direct_envelope(), "direct", 0)
            .unwrap();
        assert!(engine.is_running());
        let error = engine
            .start(r#"{"outbound":{"type":"mesh"}}"#, "node", 0)
            .unwrap_err();
        assert!(is_gated_refusal(&error), "{error}");
        assert!(!engine.is_running());
        assert_eq!(engine.node_id(), None);
    }

    #[test]
    fn a_via_chain_deeper_than_the_cap_counts_as_mesh() {
        fn chain(depth: usize, last: Value) -> Value {
            let mut entry = last;
            for _ in 0..depth {
                entry = json!({ "type": "ss", "via": entry });
            }
            json!({ "outbound": entry })
        }
        // Within the cap the answer follows the chain's end.
        assert!(!has_mesh_outbound(&chain(10, json!({ "type": "ss" }))));
        assert!(has_mesh_outbound(&chain(10, json!({ "type": "mesh" }))));
        // Past it the chain is refused whatever it ends in.
        let deep = chain(MAX_VIA_DEPTH + 2, json!({ "type": "ss" }));
        assert!(has_mesh_outbound(&deep));
        assert!(is_gated_refusal(
            &refuse_outbounds(false, &deep).unwrap_err()
        ));
        // The deep value is dropped with the stack as it is; nothing else walks it.
        assert_eq!(refuse_outbounds(true, &deep), Ok(()));
    }

    /// The final envelope carries the release gate in `allow_mesh`, and never more than the gate.
    #[test]
    fn the_final_envelope_carries_the_gate_in_allow_mesh() {
        let envelope = r#"{"schema_version":1,"profile_id":"p","outbound":{"type":"direct"}}"#;
        let closed: Value =
            serde_json::from_str(&allow_mesh_text(envelope, false).unwrap()).unwrap();
        assert_eq!(closed["allow_mesh"], json!(false));
        let open: Value = serde_json::from_str(&allow_mesh_text(envelope, true).unwrap()).unwrap();
        assert_eq!(open["allow_mesh"], json!(true));
        // A profile that opens the engine's gate itself is overwritten by a closed one.
        let forced = r#"{"allow_mesh":true,"outbound":{"type":"direct"}}"#;
        let closed: Value = serde_json::from_str(&allow_mesh_text(forced, false).unwrap()).unwrap();
        assert_eq!(closed["allow_mesh"], json!(false));
        let mut value = json!({ "allow_mesh": true });
        set_allow_mesh(&mut value, false);
        assert_eq!(value, json!({ "allow_mesh": false }));
        // The build's own gate.
        let mut value = json!({ "allow_mesh": !ENABLED });
        with_allow_mesh(&mut value);
        assert_eq!(value["allow_mesh"], json!(ENABLED));
        let text: Value = serde_json::from_str(&with_allow_mesh_text(envelope).unwrap()).unwrap();
        assert_eq!(text["allow_mesh"], json!(ENABLED));
        // Text that cannot be read is refused while the gate is closed if it names mesh, else left to the engine.
        let error = allow_mesh_text(r#"{"outbound":{"type":"mesh""#, false).unwrap_err();
        assert!(!is_gated_refusal(&error), "{error}");
        assert_eq!(error, UNREADABLE_PROFILE);
        assert_eq!(
            allow_mesh_text("not json", false),
            Ok("not json".to_string())
        );
        assert_eq!(
            allow_mesh_text(r#"{"outbound":{"type":"mesh""#, true),
            Ok(r#"{"outbound":{"type":"mesh""#.to_string())
        );
    }

    #[test]
    fn bytes_get_the_same_gate() {
        let closed = allow_mesh_bytes(br#"{"allow_mesh":true}"#, false).unwrap();
        assert_eq!(closed, br#"{"allow_mesh":false}"#.to_vec());
        let open = allow_mesh_bytes(br#"{}"#, true).unwrap();
        assert_eq!(open, br#"{"allow_mesh":true}"#.to_vec());
        let error = allow_mesh_bytes(b"\xff mesh", false).unwrap_err();
        assert!(!is_gated_refusal(&error), "{error}");
        assert_eq!(error, UNREADABLE_PROFILE);
        assert_eq!(allow_mesh_bytes(b"\xff", false), Ok(b"\xff".to_vec()));
    }

    /// The kernel's own refusal is the client's gated one, in one wording.
    #[test]
    fn the_kernels_refusal_reads_as_the_gated_one() {
        let kernel = format!(
            "config_rejected: {}",
            engine_embedded::config::MESH_NOT_ALLOWED
        );
        assert!(is_gated_refusal(&map_kernel_refusal(kernel)));
        assert_eq!(map_kernel_refusal("bad port".to_string()), "bad port");
    }

    /// With the gate closed the engine itself refuses a mesh outbound, so a client that forgot its own check still
    /// starts nothing: the envelope the client sends parses with the kernel's `MESH_NOT_ALLOWED` refusal.
    #[test]
    fn a_closed_gate_leaves_the_engine_to_refuse_mesh_too() {
        let envelope = r#"{"schema_version":1,"profile_id":"p","outbound":{"type":"mesh"}}"#;
        let sent = allow_mesh_text(envelope, false).unwrap();
        let Err(error) = engine_embedded::config::RuntimeConfig::from_json(sent.as_bytes()) else {
            panic!("the engine accepted a mesh outbound with allow_mesh false");
        };
        assert_eq!(error.message, engine_embedded::config::MESH_NOT_ALLOWED);
    }

    /// The probe the 组网 entry depends on: the engine builds a mesh-only profile with its gate open, and not with it
    /// closed. Deleting the write in `runs_mesh` fails here.
    #[test]
    fn the_capability_probe_needs_the_engine_gate_open() {
        assert!(runs_mesh(true));
        assert!(!runs_mesh(false));
        assert_eq!(supported(), ENABLED);
    }

    /// With the gate open the joined networks reach the engine with `allow_mesh` true and the engine accepts the
    /// result; closed, the envelope stays as it is.
    #[cfg(not(target_os = "android"))]
    #[test]
    fn joined_networks_reach_the_engine_with_the_gate_open() {
        let dir = test_dir("joined-gate");
        store(&dir, &saved_one()).unwrap();
        let envelope = super::super::engine::direct_envelope();
        let (kept, why) = joined_into(&dir, envelope.clone(), false);
        assert_eq!((kept.as_str(), why), (envelope.as_str(), None));
        let (meshed, why) = joined_into(&dir, envelope.clone(), true);
        assert_eq!(why, None, "{meshed}");
        let sent: Value = serde_json::from_str(&meshed).unwrap();
        assert_eq!(sent["allow_mesh"], json!(true));
        assert!(has_mesh_outbound(&sent));
        assert!(engine_embedded::config::RuntimeConfig::from_json(meshed.as_bytes()).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    /// `Engine::start` and `MeshOnly::start` take their envelope from [`prepared_start`]: with the gate open a mesh
    /// profile gets `allow_mesh` true and the engine builds it; closed, it is refused as gated.
    #[test]
    fn the_start_envelope_carries_true_with_the_gate_open() {
        let profile = only_profile(&outbounds(&saved_one())).expect("a mesh-only profile");
        let text = profile.to_string();
        let sent: Value = serde_json::from_str(&prepared_start(&text, true).unwrap()).unwrap();
        assert_eq!(sent["allow_mesh"], json!(true));
        assert!(
            engine_embedded::config::RuntimeConfig::from_json(sent.to_string().as_bytes()).is_ok()
        );
        let error = prepared_start(&text, false).unwrap_err();
        assert!(is_gated_refusal(&error), "{error}");
    }

    /// The same through `Engine::start`'s own seam (the local proxy, which only builds): open, a mesh profile starts;
    /// closed, it is refused before anything runs.
    #[cfg(not(target_os = "android"))]
    #[test]
    fn engine_start_hands_the_open_gate_to_the_engine() {
        let text = only_profile(&outbounds(&saved_one()))
            .expect("a mesh-only profile")
            .to_string();
        let mut engine = super::super::engine::Engine::default();
        let error = engine.start_gated(&text, "mesh", 0, false).unwrap_err();
        assert!(is_gated_refusal(&error), "{error}");
        engine
            .start_gated(&text, "mesh", 0, true)
            .expect("the engine builds the mesh profile");
        assert!(engine.is_running());
        engine.stop();
    }

    /// The exit probe's own engine: the running envelope (mesh outbounds inside, no `allow_mesh`) is accepted with the
    /// gate open and refused by the engine itself, as the gated refusal, with it closed.
    #[cfg(not(target_os = "android"))]
    #[test]
    fn the_exit_probe_hands_the_open_gate_to_its_engine() {
        let text = only_profile(&outbounds(&saved_one()))
            .expect("a mesh-only profile")
            .to_string();
        let opened = super::super::engine::through_proxy_gated(&text, true, |port| port);
        assert!(opened.is_ok_and(|port| port != 0));
        let error =
            super::super::engine::through_proxy_gated(&text, false, |port| port).unwrap_err();
        assert!(is_gated_refusal(&error), "{error}");
    }
}
