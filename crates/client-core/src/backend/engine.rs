//! Subscription parsing and the in-process engine: the selected node, or every node with the subscription's
//! rules and policy groups, runs behind a local SOCKS5/HTTP listener, as in the Windows desktop mode, with no
//! external engine binary.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use config_core::routing_import::import_subscription_routing;
use config_core::CoreSubscriptionService;
use control_domain::{
    DiagnosticSeverity, MetadataEntry, NodeDescriptor, Protocol, RawSubscription,
    SubscriptionService, NODE_METADATA_ANX_LICENSE_ORIGIN, NODE_METADATA_PATH, NODE_PATH_TRUNK,
};
use engine_embedded::checkup::{run_checkup, CheckupOptions, CheckupReport};
use engine_embedded::config::RuntimeConfig;
use engine_embedded::delay::measure_delay;
use engine_embedded::local_proxy::LocalProxy;
use engine_embedded::nodes::{outbound_for_node, routing_envelope_with_sets, single_node_envelope};
use inbound_mixed::MixedSettings;

#[cfg(any(target_os = "android", target_os = "linux", windows))]
use super::tun::{self, TunEngine};
#[cfg(any(target_os = "android", target_os = "linux", windows))]
pub use super::tun::{attach as attach_tunnel, attached as tunnel_attached};
#[cfg(any(target_os = "android", target_os = "linux"))]
pub use super::tun::{
    expect_replacement as expect_tunnel_replacement,
    replacement_failed as tunnel_replacement_failed,
};
use serde_json::{json, Value};

/// The network the engine runs on, as its `set_network` command takes it, for `subnet` policy groups.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkFacts {
    /// `wifi`, `wired` or `cellular`; none when unknown.
    pub kind: Option<&'static str>,
    /// The Wi-Fi name when the system gives it (1–32 bytes).
    pub ssid: Option<String>,
    /// The local addresses toward the internet.
    pub addresses: Vec<std::net::IpAddr>,
}

impl NetworkFacts {
    /// What the Wi-Fi rules read (`Platform::current_network`) plus `addresses`.
    pub fn new(network: Option<&on_demand::Network>, addresses: Vec<std::net::IpAddr>) -> Self {
        let (kind, ssid) = match network {
            Some(on_demand::Network::Wifi(ssid)) => (Some("wifi"), ssid.clone()),
            Some(on_demand::Network::WifiWithheld) => (Some("wifi"), None),
            Some(on_demand::Network::Cellular) => (Some("cellular"), None),
            _ => (None, None),
        };
        let ssid = ssid.filter(|ssid| !ssid.is_empty() && ssid.len() <= on_demand::MAX_SSID_BYTES);
        Self {
            kind,
            ssid,
            addresses,
        }
    }

    /// The engine control request; absent fields stay unknown.
    pub fn request(&self) -> Vec<u8> {
        let mut request = json!({"cmd": "set_network"});
        if let Some(kind) = self.kind {
            request["type"] = json!(kind);
        }
        if let Some(ssid) = &self.ssid {
            request["ssid"] = json!(ssid);
        }
        if !self.addresses.is_empty() {
            let addresses: Vec<String> = self.addresses.iter().map(ToString::to_string).collect();
            request["addresses"] = json!(addresses);
        }
        request.to_string().into_bytes()
    }
}

const PROFILE_ID: &str = "linux-gui";
pub const DELAY_TIMEOUT: Duration = Duration::from_secs(5);
/// Concurrent delay tests when picking the fastest node, as on Windows.
const DELAY_BATCH: usize = 16;

/// Parses subscription content (a URL's body, a file, or pasted share links) into its nodes.
pub fn parse_nodes(content: &str) -> Result<Vec<NodeDescriptor>, String> {
    let service = CoreSubscriptionService::new();
    let raw = RawSubscription {
        source_id: PROFILE_ID.to_string(),
        content: content.to_string(),
    };
    // The kernel's refusal keeps its code (`subscription.core.clash_yaml_unsupported: …`), so the window can say
    // what is wrong in its own language (`crate::refusal`).
    let document = service.parse(&raw).map_err(|error| error.to_string())?;
    let catalog = service
        .normalize(&document)
        .map_err(|error| error.to_string())?;
    if catalog.nodes.is_empty() {
        return Err(crate::refusal::coded(
            "error.no_nodes",
            "no nodes were found in the subscription",
        ));
    }
    Ok(catalog.nodes)
}

/// Lower-case protocol name, as the interface's protocol badges expect.
pub fn protocol_name(protocol: &Protocol) -> String {
    match protocol {
        Protocol::Other(name) => name.to_ascii_lowercase(),
        known => format!("{known:?}").to_ascii_lowercase(),
    }
}

/// Whether the embedded engine can run this node.
pub fn is_supported(node: &NodeDescriptor) -> bool {
    outbound_for_node(node).is_ok()
}

/// The node to run: the saved choice if it is still in the subscription and supported, else the first
/// supported node.
pub fn choose_node<'a>(
    nodes: &'a [NodeDescriptor],
    saved: Option<&str>,
) -> Option<&'a NodeDescriptor> {
    saved
        .and_then(|id| {
            nodes
                .iter()
                .find(|node| node.id == id && is_supported(node))
        })
        .or_else(|| {
            nodes
                .iter()
                .find(|node| is_trunk(node) && is_supported(node))
        })
        .or_else(|| nodes.iter().find(|node| is_supported(node)))
}

/// Whether the subscription marks the node as a domestic ingress on a shared trunk (`#anixops:path=trunk`).
/// Such a node is the default pick and wins the fastest-node search whenever one answers (anx
/// FUTURE-IMPROVEMENTS F-51); direct nodes stay available to pick by hand.
pub fn is_trunk(node: &NodeDescriptor) -> bool {
    node.metadata
        .iter()
        .any(|entry| entry.key == NODE_METADATA_PATH && entry.value.trim() == NODE_PATH_TRUNK)
}

/// The data directory whose `anx-license` folder holds this device's ANX keys and licenses ([`use_license_dir`]).
static LICENSE_DATA_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// The ANX license directory (`anx_license`) of the client whose data directory is `dir`.
pub fn license_dir(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("anx-license")
}

/// Keeps the ANX licenses in the data directory `dir` (the backend's `Config.dir`), set when the backend opens.
/// Never `$HOME` or `$XDG_CONFIG_HOME`: a system service, the Linux daemon and the Android app have neither,
/// or one they cannot write. The first directory set stays, as with the other files the backend names.
pub fn use_license_dir(dir: &std::path::Path) {
    let _ = LICENSE_DATA_DIR.set(dir.to_path_buf());
}

/// This device's ANX license directory (`anx_license`), beside the saved state; none before the backend opened.
pub fn license_store() -> Option<anx_license::LicenseStore> {
    LICENSE_DATA_DIR
        .get()
        .map(|dir| anx_license::LicenseStore::new(license_dir(dir)))
}

/// Hands each subscription origin's device key and license file to the ANX outbounds of that origin's
/// nodes (`anx_license`, marked by [`mark_license_origin`]); an origin that does not offer ANX licensing
/// gives its nodes nothing, and no node ever carries another origin's key (anx spec §2.4.1).
pub fn sync_anx_license() {
    node_import::set_anx_license(None);
    node_import::set_anx_licenses(anx_licenses(license_store().as_ref()));
}

/// The licenses by origin as `node_import::set_anx_licenses` takes them.
pub fn anx_licenses(
    store: Option<&anx_license::LicenseStore>,
) -> std::collections::BTreeMap<String, node_import::AnxLicense> {
    store
        .and_then(|store| store.licenses().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|license| {
            (
                license.origin,
                node_import::AnxLicense {
                    device_key: license.device.private_key_hex(),
                    device_alg: license.device.alg().to_string(),
                    license_cert_path: license.license_path.to_string_lossy().into_owned(),
                },
            )
        })
        .collect()
}

/// Marks the ANX nodes of the source at `location` with its origin (`anx.license_origin`), so each gets
/// that origin's license; a file or pasted links have none. Whatever the content itself put there is
/// dropped first: only where the client fetched it decides.
pub fn mark_license_origin(nodes: &mut [NodeDescriptor], location: &str) {
    let origin = anx_license::origin(location);
    for node in nodes {
        node.metadata
            .retain(|entry| entry.key != NODE_METADATA_ANX_LICENSE_ORIGIN);
        if let (Some(origin), Protocol::Anx) = (&origin, &node.protocol) {
            node.metadata.push(MetadataEntry {
                key: NODE_METADATA_ANX_LICENSE_ORIGIN.to_string(),
                value: origin.clone(),
            });
        }
    }
}

/// A profile with no node: everything leaves through the engine's `direct` outbound.
pub fn direct_envelope() -> String {
    serde_json::json!({
        "schema_version": 1,
        "profile_id": PROFILE_ID,
        "outbound": { "type": "direct" },
    })
    .to_string()
}

pub fn envelope(nodes: &[NodeDescriptor], node_id: &str) -> Result<String, String> {
    sync_anx_license();
    single_node_envelope(PROFILE_ID, nodes, Some(node_id))
        .map(|(envelope, _)| envelope)
        .map_err(str::to_string)
}

/// The envelope with every node and the subscription's rules and policy groups, when `content` carries them,
/// every node can run on the engine and the engine accepts the result. Otherwise one node runs at a time.
/// `parsed` are the subscription's nodes as served, which its rules and groups name; `nodes` are the ones to run,
/// with the user's edits (`node_edits`).
pub fn subscription_routing(
    content: &str,
    parsed: &[NodeDescriptor],
    nodes: &[NodeDescriptor],
) -> Option<String> {
    let imported = import_subscription_routing(content, parsed);
    let spec = imported.spec?;
    sync_anx_license();
    // The subscription's own rule sets go with its rules: without them the engine refuses every rule naming one.
    routing_envelope_with_sets(PROFILE_ID, nodes, &spec, &imported.rule_sets)
        .ok()
        .filter(|envelope| RuntimeConfig::from_json(envelope.as_bytes()).is_ok())
}

/// Most [`routing_warnings`] lines; one more says how many were left out.
const MAX_ROUTING_WARNINGS: usize = 32;

/// The code of the line [`routing_warnings`] adds when it leaves lines out.
pub const ROUTING_WARNINGS_MORE: &str = "routing_warnings.more";

/// What importing `content`'s rules leaves out (rules of unsupported types counted by type, Surge's built-in
/// `RULE-SET,SYSTEM`, targets that were not imported, ...), so the rules page can show it instead of the rules
/// silently matching less. One line each, `<kernel code>: <English>` as the kernel words it (`crate::refusal`):
/// the window names the kind of line (a rule, a policy group, a member, an option skipped) in its own language
/// and shows the English as its detail. Never a rule-set URL.
pub fn routing_warnings(content: &str, parsed: &[NodeDescriptor]) -> Vec<String> {
    let mut warnings = import_subscription_routing(content, parsed)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>();
    if warnings.len() > MAX_ROUTING_WARNINGS {
        let more = warnings.len() - MAX_ROUTING_WARNINGS;
        warnings.truncate(MAX_ROUTING_WARNINGS);
        warnings.push(crate::refusal::coded(
            ROUTING_WARNINGS_MORE,
            format!("{more} more were left out"),
        ));
    }
    warnings
}

/// A saved source other than the selected one, with the nodes the engine can run.
#[derive(Debug, Clone)]
pub struct OtherSource {
    pub id: String,
    pub name: String,
    pub nodes: Vec<NodeDescriptor>,
}

impl OtherSource {
    /// The id node `node_id` of this source has in a linked profile and in the node list.
    pub fn outbound_id(&self, node_id: &str) -> String {
        format!("{}.{node_id}", self.id)
    }

    /// The select policy group this source becomes in the selected source's rules.
    pub fn group_id(&self) -> String {
        format!("订阅·{}", self.name.replace(',', " "))
    }
}

/// Adds every other source to a routed envelope as a select policy group of its nodes, and that group to
/// every select group. A source is left out when its ids clash or the
/// engine would reject the result (it allows 64 nodes and 64 groups).
pub fn link_sources(envelope: &str, others: &[OtherSource]) -> String {
    let Ok(mut profile) = serde_json::from_str::<Value>(envelope) else {
        return envelope.to_string();
    };
    for other in others {
        if let Some(linked) = linked(&profile, other) {
            profile = linked;
        }
    }
    profile.to_string()
}

fn linked(profile: &Value, other: &OtherSource) -> Option<Value> {
    let mut profile = profile.clone();
    let group_id = other.group_id();
    let routing = profile.get_mut("routing")?.as_object_mut()?;
    let outbounds = routing.get_mut("outbounds")?.as_object_mut()?;
    if outbounds.contains_key(&group_id) {
        return None;
    }
    let mut members = Vec::new();
    for node in &other.nodes {
        let id = other.outbound_id(&node.id);
        if outbounds.contains_key(&id) {
            return None;
        }
        outbounds.insert(id.clone(), outbound_for_node(node).ok()?);
        members.push(Value::String(id));
    }
    if members.is_empty() {
        return None;
    }
    let groups = routing.get_mut("groups")?.as_array_mut()?;
    if groups
        .iter()
        .any(|group| group.get("id").and_then(Value::as_str) == Some(group_id.as_str()))
    {
        return None;
    }
    for group in groups.iter_mut() {
        if group.get("type").and_then(Value::as_str) == Some("select") {
            group
                .get_mut("members")?
                .as_array_mut()?
                .push(Value::String(group_id.clone()));
        }
    }
    groups.push(json!({ "id": group_id, "type": "select", "members": members }));
    RuntimeConfig::from_json(profile.to_string().as_bytes())
        .is_ok()
        .then_some(profile)
}

/// Kernel control commands a window may send (`runtime_control`): reading traffic, connections and rule sets,
/// closing connections, refreshing remote rule sets, packet capture, the per-domain decryption results and the mesh
/// status, join and leave. Mode and group changes go through the saved profile instead. What only the backend
/// sends (the capture session storage and key, saved sessions, `reload_rules`, saving a mesh network) is not here:
/// it goes through [`Engine::request`]. The
/// Windows/Linux Tauri app checks its window's requests with the same list ([`activity_request`]).
pub const ACTIVITY_COMMANDS: [&str; 33] = [
    "traffic",
    "connections",
    "close_connection",
    "close_all",
    "rule_sets",
    "update_rule_sets",
    // The rule tester and per-rule hit counts on the rules page.
    "rule_hits",
    "clear_rule_hits",
    "test_route",
    "capture",
    "set_capture",
    "clear_capture",
    "capture_connection",
    "export_capture",
    "capture_requests",
    "capture_exchange",
    "capture_h2_frames",
    // Per-domain decryption results for the decryption list; they only read or clear counters.
    "mitm_hosts",
    "clear_mitm_hosts",
    // 组网 (docs/architecture/mesh-client.md): the status snapshot, joining and leaving a saved network, the
    // internal domain names and a subnet's local or mesh route. Saving and removing a network go through the backend,
    // which holds the secret.
    "mesh_status",
    "mesh_join",
    "mesh_leave",
    "mesh_set_dns",
    "mesh_set_subnet_route",
    // Kernel 0.4, full nodes: relaying, host mode, advertising a subnet and asking for another region; kernel 0.5:
    // the default path policy and the custom paths. An engine without them answers `unsupported`. The kernel 0.4
    // commands are also refused while `mesh::V04_ENABLED` is off.
    "mesh_set_relay",
    "mesh_set_host_mode",
    "mesh_advertise",
    "mesh_withdraw",
    "mesh_request_region",
    "mesh_withdraw_region_request",
    "mesh_set_policy",
    "mesh_set_paths",
    // A one-time credential from the panel that joins this computer (个人电脑用入网凭据加入).
    "mesh_join_credential",
];

/// The request bytes for an activity page command, or why it is refused.
pub fn activity_request(request: &serde_json::Value) -> Result<Vec<u8>, String> {
    let command = request
        .get("cmd")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let gated = command.starts_with("mesh_")
        && (!crate::backend::mesh::ENABLED
            || !crate::backend::mesh::V04_ENABLED
                && crate::backend::mesh::V04_COMMANDS.contains(&command));
    if gated
        || !ACTIVITY_COMMANDS.contains(&command)
            && !desktop_mitm::phone::CONTROL_COMMANDS.contains(&command)
    {
        return Err(format!(
            "runtime control command {command:?} is not available"
        ));
    }
    serde_json::to_vec(request).map_err(|error| error.to_string())
}

/// The engine that runs for the mesh alone on the kept TUN descriptor while the proxy is off (Android's VPN,
/// mesh-status.md, 只开组网). Apart from [`Engine`], so that nothing that asks whether the proxy is connected sees it.
#[derive(Default)]
pub struct MeshOnly {
    #[cfg(any(target_os = "android", target_os = "linux", windows))]
    tunnel: Option<TunEngine>,
}

impl MeshOnly {
    /// Starts `envelope` (`mesh::only_profile`) on the kept descriptor, after stopping one that runs.
    #[cfg(any(target_os = "android", target_os = "linux", windows))]
    pub fn start(&mut self, envelope: &str) -> Result<(), String> {
        // A refused start still stops the one that runs: no stale engine stays behind the error.
        self.stop();
        super::mesh::refuse_gated_envelope(envelope)?;
        self.tunnel = Some(TunEngine::start(envelope)?);
        Ok(())
    }

    #[cfg(not(any(target_os = "android", target_os = "linux", windows)))]
    pub fn start(&mut self, _envelope: &str) -> Result<(), String> {
        Err("this platform has no tunnel for the mesh alone".to_string())
    }

    /// Stops it; the descriptor stays. Answers whether it ran.
    pub fn stop(&mut self) -> bool {
        #[cfg(any(target_os = "android", target_os = "linux", windows))]
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.stop();
            return true;
        }
        false
    }

    #[cfg(any(target_os = "android", target_os = "linux", windows))]
    pub fn is_running(&self) -> bool {
        self.tunnel.as_ref().is_some_and(TunEngine::is_running)
    }

    #[cfg(not(any(target_os = "android", target_os = "linux", windows)))]
    pub fn is_running(&self) -> bool {
        false
    }

    /// One control request while it runs.
    pub fn request(&self, request: &serde_json::Value) -> Result<serde_json::Value, String> {
        #[cfg(any(target_os = "android", target_os = "linux", windows))]
        if let Some(tunnel) = self.tunnel.as_ref().filter(|tunnel| tunnel.is_running()) {
            let request = serde_json::to_vec(request).map_err(|error| error.to_string())?;
            let answer = tunnel.control_json(&request)?;
            return serde_json::from_slice(&answer).map_err(|error| error.to_string());
        }
        let _ = request;
        Err("the engine is not running".to_string())
    }
}

/// The running engine, if any: a local proxy on the desktop, the VPN's TUN interface on Android, and on the Linux
/// desktop in TUN mode the device the polkit helper made ([`super::tun_helper`]).
#[derive(Default)]
pub struct Engine {
    #[cfg(not(target_os = "android"))]
    proxy: Option<LocalProxy>,
    #[cfg(any(target_os = "android", target_os = "linux", windows))]
    tunnel: Option<TunEngine>,
    /// The running configuration, for the exit address probe ([`Engine::probe_envelope`]).
    #[cfg(any(target_os = "android", target_os = "linux", windows))]
    envelope: Option<String>,
    node_id: Option<String>,
}

impl Engine {
    /// Starts (or restarts) the engine. The desktop listens on `port`; on a kept TUN descriptor (Android's VPN,
    /// Linux TUN mode) it runs on the interface and returns the interface address.
    pub fn start(
        &mut self,
        envelope: &str,
        node_id: &str,
        port: u16,
    ) -> Result<SocketAddr, String> {
        // A refused start still stops the engine that runs: no stale engine stays behind the error.
        self.stop_runtime();
        if let Err(refusal) = super::mesh::refuse_gated_envelope(envelope) {
            self.node_id = None;
            return Err(refusal);
        }
        let address = self.start_runtime(envelope, port)?;
        self.node_id = Some(node_id.to_string());
        // The lists read the new engine now; a session the viewer held after the last stop is let go.
        desktop_mitm::sessions::close_viewer();
        Ok(address)
    }

    #[cfg(target_os = "android")]
    fn start_runtime(&mut self, envelope: &str, port: u16) -> Result<SocketAddr, String> {
        let _ = port;
        self.start_tunnel(envelope)
    }

    #[cfg(not(target_os = "android"))]
    fn start_runtime(&mut self, envelope: &str, port: u16) -> Result<SocketAddr, String> {
        #[cfg(any(target_os = "linux", windows))]
        if tun::attached() {
            return self.start_tunnel(envelope);
        }
        let listen = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let proxy = LocalProxy::start(envelope.as_bytes(), listen, MixedSettings::default())
            .map_err(|error| error.message.to_string())?;
        let address = proxy.local_addr();
        self.proxy = Some(proxy);
        Ok(address)
    }

    #[cfg(any(target_os = "android", target_os = "linux", windows))]
    fn start_tunnel(&mut self, envelope: &str) -> Result<SocketAddr, String> {
        self.tunnel = Some(TunEngine::start(envelope)?);
        self.envelope = Some(envelope.to_string());
        Ok(SocketAddr::from((Ipv4Addr::new(198, 18, 0, 1), 0)))
    }

    fn stop_runtime(&mut self) {
        #[cfg(not(target_os = "android"))]
        if let Some(mut proxy) = self.proxy.take() {
            proxy.stop();
        }
        #[cfg(any(target_os = "android", target_os = "linux", windows))]
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.stop();
        }
    }

    /// Stops the engine; a kept TUN descriptor is closed too, which takes the interface (and on the Linux
    /// desktop its routes) down.
    pub fn stop(&mut self) {
        self.stop_runtime();
        #[cfg(any(target_os = "android", target_os = "linux", windows))]
        tun::detach();
        #[cfg(target_os = "linux")]
        super::tun_helper::release();
        self.node_id = None;
    }

    pub fn is_running(&self) -> bool {
        self.tunnel_running() || self.proxy_running()
    }

    #[cfg(any(target_os = "android", target_os = "linux", windows))]
    fn tunnel_running(&self) -> bool {
        self.tunnel.as_ref().is_some_and(TunEngine::is_running)
    }

    #[cfg(not(any(target_os = "android", target_os = "linux", windows)))]
    fn tunnel_running(&self) -> bool {
        false
    }

    #[cfg(not(target_os = "android"))]
    fn proxy_running(&self) -> bool {
        self.proxy.as_ref().is_some_and(LocalProxy::is_running)
    }

    #[cfg(target_os = "android")]
    fn proxy_running(&self) -> bool {
        false
    }

    /// One kernel control request to the running engine. With no engine running, the capture commands go to the
    /// kernel's capture viewer (`desktop_mitm::sessions::viewer_control`), so saved sessions open, and the session
    /// saved at the last stop stays on screen, while disconnected.
    fn control_bytes(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        #[cfg(any(target_os = "android", target_os = "linux", windows))]
        if let Some(tunnel) = self.tunnel.as_ref().filter(|tunnel| tunnel.is_running()) {
            return tunnel.control_json(request);
        }
        if !self.is_running() {
            if let Some(answer) = desktop_mitm::sessions::viewer_control(request) {
                return answer;
            }
        }
        self.proxy_control(request)
    }

    #[cfg(not(target_os = "android"))]
    fn proxy_control(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        self.proxy
            .as_ref()
            .filter(|proxy| proxy.is_running())
            .ok_or("the engine is not running")?
            .control_json(request)
            .map_err(|error| error.message.to_string())
    }

    #[cfg(target_os = "android")]
    fn proxy_control(&self, _request: &[u8]) -> Result<Vec<u8>, String> {
        Err("the engine is not running".to_string())
    }

    /// Answers one activity page request (see [`activity_request`]) while the engine runs.
    /// A `mesh_status` answer loses the kernel 0.4 fields while that gate is closed
    /// ([`crate::backend::mesh::gate_status`]).
    pub fn control(&self, request: &serde_json::Value) -> Result<serde_json::Value, String> {
        let status = request.get("cmd").and_then(serde_json::Value::as_str) == Some("mesh_status");
        let request = activity_request(request)?;
        let answer = self.control_bytes(&request)?;
        let mut answer: serde_json::Value =
            serde_json::from_slice(&answer).map_err(|error| error.to_string())?;
        if status {
            crate::backend::mesh::gate_status(&mut answer);
        }
        Ok(answer)
    }

    /// Answers one request the backend itself makes while the engine runs: the capture session storage and
    /// key, saved sessions, `reload_rules` and the decryption results. It skips [`activity_request`], so it
    /// must never carry a window's request as it came; windows go through [`Engine::control`].
    pub(crate) fn request(&self, request: &serde_json::Value) -> Result<serde_json::Value, String> {
        let request = serde_json::to_vec(request).map_err(|error| error.to_string())?;
        let answer = self.control_bytes(&request)?;
        serde_json::from_slice(&answer).map_err(|error| error.to_string())
    }

    /// The replay proxy's state (`start` none), or starts it afresh with HTTPS decryption on or off. Not one of the
    /// window's activity commands: only the backend asks, and the credentials stay in it.
    pub fn replay_proxy(&self, start: Option<bool>) -> Result<serde_json::Value, String> {
        let request = match start {
            None => serde_json::json!({ "cmd": "replay_proxy" }),
            Some(decrypt) => serde_json::json!({ "cmd": "start_replay_proxy", "decrypt": decrypt }),
        };
        let request = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        let answer = self.control_bytes(&request)?;
        serde_json::from_slice(&answer).map_err(|error| error.to_string())
    }

    /// What scripts posted since the last read (the kernel's `script_notifications`), while the engine runs.
    pub fn script_notifications(&self) -> Option<serde_json::Value> {
        let answer = self
            .control_bytes(br#"{"cmd":"script_notifications"}"#)
            .ok()?;
        serde_json::from_slice(&answer).ok()
    }

    /// The engine's Prometheus text (the kernel's `metrics` command) while it runs.
    pub fn metrics(&self) -> Option<String> {
        let answer = self.control_bytes(br#"{"cmd":"metrics"}"#).ok()?;
        let answer: serde_json::Value = serde_json::from_slice(&answer).ok()?;
        answer
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }

    /// Sends the engine a `reset_network` request (see `network_watch`) while it runs; not an activity page
    /// command, so it skips [`activity_request`].
    pub fn reset_network(&self, reset: network_watch::Reset) -> Result<(), String> {
        self.control_bytes(&reset.request()).map(|_| ())
    }

    /// Reports the network under the engine (`set_network`, see `network_watch`) for its `subnet` groups.
    /// Engines without the command answer an error, which is ignored.
    pub fn set_network(&self, network: &NetworkFacts) -> Result<(), String> {
        self.control_bytes(&network.request()).map(|_| ())
    }

    /// The local proxy's address; none through a TUN interface, where traffic comes in through the device.
    #[cfg(not(target_os = "android"))]
    pub fn address(&self) -> Option<SocketAddr> {
        self.proxy.as_ref().map(LocalProxy::local_addr)
    }

    /// Android has no local proxy: traffic comes in through the VPN.
    #[cfg(target_os = "android")]
    pub fn address(&self) -> Option<SocketAddr> {
        None
    }

    /// The running configuration where the exit address cannot be probed through the engine's own listener: on
    /// a TUN interface the probe runs a short-lived local proxy on it ([`through_temporary_proxy`]).
    #[cfg(any(target_os = "android", target_os = "linux", windows))]
    pub fn probe_envelope(&self) -> Option<String> {
        self.envelope.clone().filter(|_| self.tunnel_running())
    }

    #[cfg(not(any(target_os = "android", target_os = "linux", windows)))]
    pub fn probe_envelope(&self) -> Option<String> {
        None
    }

    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }
}

/// Runs `probe` with the port of a local proxy started on `envelope` for as long as it takes, on a free
/// loopback port, then stops the proxy.
pub fn through_temporary_proxy<T>(
    envelope: &str,
    probe: impl FnOnce(u16) -> T,
) -> Result<T, String> {
    let listen = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let mut proxy = LocalProxy::start(envelope.as_bytes(), listen, MixedSettings::default())
        .map_err(|error| error.message.to_string())?;
    let result = probe(proxy.local_addr().port());
    proxy.stop();
    Ok(result)
}

/// Measures `url` through one node.
pub fn node_delay(nodes: &[NodeDescriptor], node_id: &str, url: &str) -> Result<Duration, String> {
    let envelope = envelope(nodes, node_id)?;
    measure_delay(envelope.as_bytes(), url, DELAY_TIMEOUT).map_err(|error| error.to_string())
}

/// Node checkup (节点体检) through one node: latency rounds and, when `options.throughput` is set, a
/// download with the latency under load. Only runs when the user starts it (docs/network-behavior.md).
pub fn node_checkup(
    nodes: &[NodeDescriptor],
    node_id: &str,
    options: &CheckupOptions,
) -> Result<CheckupReport, String> {
    let envelope = envelope(nodes, node_id)?;
    run_checkup(envelope.as_bytes(), options).map_err(|error| error.to_string())
}

/// The link other clients import for `node`; Snell gives a Surge proxy line.
pub fn share_link(node: &NodeDescriptor) -> Result<String, String> {
    node_import::outbound_json(node)
        .and_then(|outbound| node_import::share_link(&node.name, &outbound))
        .map_err(|error| format!("This node cannot be shared: {error}"))
}

/// Tests each listed node on its own, `DELAY_BATCH` at a time, in the order given.
pub fn node_delays(
    nodes: &[(String, Option<NodeDescriptor>)],
    url: &str,
) -> Vec<(String, Result<Duration, String>)> {
    let mut results = Vec::with_capacity(nodes.len());
    for batch in nodes.chunks(DELAY_BATCH) {
        let delays: Vec<_> = thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|(_, node)| {
                    scope.spawn(move || {
                        let node = node.as_ref().ok_or("the node is not in any subscription")?;
                        node_delay(std::slice::from_ref(node), &node.id, url)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err("delay test stopped".to_string()))
                })
                .collect()
        });
        results.extend(batch.iter().map(|(id, _)| id.clone()).zip(delays));
    }
    results
}

/// Tests every supported node and returns the fastest one with its delay: the fastest trunk node when one
/// answers, otherwise the fastest node.
pub fn fastest_node(nodes: &[NodeDescriptor], url: &str) -> Option<(String, Duration)> {
    let candidates: Vec<&NodeDescriptor> = nodes.iter().filter(|node| is_supported(node)).collect();
    let mut best: Option<(String, Duration)> = None;
    let mut best_trunk: Option<(String, Duration)> = None;
    for batch in candidates.chunks(DELAY_BATCH) {
        let (sender, receiver) = mpsc::channel();
        thread::scope(|scope| {
            for node in batch {
                let sender = sender.clone();
                scope.spawn(move || {
                    let result = node_delay(nodes, &node.id, url).ok();
                    let _ = sender.send((node.id.clone(), result));
                });
            }
        });
        drop(sender);
        for (id, delay) in receiver {
            if let Some(delay) = delay {
                let trunk = nodes.iter().any(|node| node.id == id && is_trunk(node));
                if trunk
                    && best_trunk
                        .as_ref()
                        .is_none_or(|(_, current)| delay < *current)
                {
                    best_trunk = Some((id.clone(), delay));
                }
                if best.as_ref().is_none_or(|(_, current)| delay < *current) {
                    best = Some((id, delay));
                }
            }
        }
    }
    best_trunk.or(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_network_request_carries_what_is_known() {
        let addresses = vec![
            "192.168.1.5".parse().unwrap(),
            "2001:db8::5".parse().unwrap(),
        ];
        let wifi = on_demand::Network::Wifi(Some("Home \"5G\"".to_string()));
        let request: Value =
            serde_json::from_slice(&NetworkFacts::new(Some(&wifi), addresses).request()).unwrap();
        assert_eq!(
            request,
            json!({"cmd": "set_network", "type": "wifi", "ssid": "Home \"5G\"",
                   "addresses": ["192.168.1.5", "2001:db8::5"]})
        );
        let unknown = NetworkFacts::new(Some(&on_demand::Network::Other), Vec::new());
        assert_eq!(unknown.request(), br#"{"cmd":"set_network"}"#.to_vec());
        let hidden = NetworkFacts::new(Some(&on_demand::Network::Wifi(None)), Vec::new());
        assert_eq!(
            hidden.request(),
            br#"{"cmd":"set_network","type":"wifi"}"#.to_vec()
        );
        let withheld = NetworkFacts::new(Some(&on_demand::Network::WifiWithheld), Vec::new());
        assert_eq!(withheld, hidden);
    }

    #[test]
    fn anx_nodes_are_marked_with_the_origin_they_came_from() {
        let node = |protocol: Protocol, stamped: Option<&str>| NodeDescriptor {
            id: "n".into(),
            name: "n".into(),
            protocol,
            endpoint: control_domain::Endpoint {
                host: "203.0.113.7".into(),
                port: 443,
            },
            tags: Vec::new(),
            metadata: stamped
                .map(|value| MetadataEntry {
                    key: NODE_METADATA_ANX_LICENSE_ORIGIN.into(),
                    value: value.into(),
                })
                .into_iter()
                .collect(),
        };
        let origin_of = |node: &NodeDescriptor| {
            node.metadata
                .iter()
                .filter(|entry| entry.key == NODE_METADATA_ANX_LICENSE_ORIGIN)
                .map(|entry| entry.value.clone())
                .collect::<Vec<_>>()
        };
        // What the content claims is replaced by where it was fetched from.
        let mut nodes = vec![
            node(Protocol::Anx, Some("https://other.example")),
            node(Protocol::Shadowsocks, None),
        ];
        mark_license_origin(&mut nodes, "https://Panel.Example/s/token");
        assert_eq!(origin_of(&nodes[0]), ["https://panel.example"]);
        assert!(origin_of(&nodes[1]).is_empty());
        // A file or pasted links: no origin, so no license.
        mark_license_origin(&mut nodes, "/home/user/nodes.txt");
        assert!(origin_of(&nodes[0]).is_empty());
    }

    #[test]
    fn no_licenses_without_a_store() {
        assert!(anx_licenses(None).is_empty());
    }

    #[test]
    fn licenses_live_in_the_data_directory() {
        let dir = std::path::Path::new("/var/lib/anixops-net");
        assert_eq!(
            license_dir(dir),
            std::path::PathBuf::from("/var/lib/anixops-net/anx-license")
        );
    }

    const LINK: &str = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@82.47.34.99:11111#HK";

    #[test]
    fn reports_a_missing_node_in_a_batch_delay_test() {
        let results = node_delays(&[("gone".to_string(), None)], "http://127.0.0.1:9/");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "gone");
        assert!(results[0].1.is_err());
    }

    #[test]
    fn picks_the_marked_trunk_node_by_default() {
        let text = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@82.47.34.99:11111#JP%20direct\n\
            #anixops:path=trunk\n\
            ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.9:11111#SH%20CN2";
        let nodes = parse_nodes(text).unwrap();
        let trunk: Vec<_> = nodes.iter().map(is_trunk).collect();
        assert_eq!(trunk, [false, true]);
        assert_eq!(choose_node(&nodes, None).unwrap().id, nodes[1].id);
        assert_eq!(choose_node(&nodes, Some("gone")).unwrap().id, nodes[1].id);
        // A node the user picked stays picked.
        assert_eq!(
            choose_node(&nodes, Some(&nodes[0].id)).unwrap().id,
            nodes[0].id
        );
        // Without a mark the first node stays the default.
        let plain = parse_nodes(LINK).unwrap();
        assert_eq!(choose_node(&plain, None).unwrap().id, plain[0].id);
    }

    #[test]
    fn shares_a_node_as_the_link_it_came_from() {
        let node = &parse_nodes(LINK).unwrap()[0];
        let link = share_link(node).unwrap();
        assert!(link.starts_with("ss://"), "{link}");
        assert_eq!(parse_nodes(&link).unwrap()[0].endpoint, node.endpoint);
    }

    #[test]
    fn activity_requests_only_read_and_close_connections() {
        assert!(activity_request(&serde_json::json!({"cmd": "connections"})).is_ok());
        assert!(activity_request(&serde_json::json!({"cmd": "close_all"})).is_ok());
        assert!(activity_request(&serde_json::json!({"cmd": "rule_sets"})).is_ok());
        assert!(
            activity_request(&serde_json::json!({"cmd": "export_capture", "format": "har"}))
                .is_ok()
        );
        assert!(activity_request(&serde_json::json!({"cmd": "select", "group": "a"})).is_err());
        // 组网 only in a build that turns it on (`mesh::ENABLED`).
        assert_eq!(
            activity_request(&serde_json::json!({"cmd": "mesh_status"})).is_ok(),
            crate::backend::mesh::ENABLED
        );
        assert_eq!(
            activity_request(&serde_json::json!({"cmd": "mesh_join", "network": "office"})).is_ok(),
            crate::backend::mesh::ENABLED
        );
        // Kernel 0.4 commands need the second gate as well.
        for command in crate::backend::mesh::V04_COMMANDS {
            assert!(ACTIVITY_COMMANDS.contains(&command), "{command}");
            assert_eq!(
                activity_request(&serde_json::json!({ "cmd": command })).is_ok(),
                crate::backend::mesh::V04_ENABLED,
                "{command}"
            );
        }
        // The secret is the backend's: a window never saves a network itself.
        assert!(activity_request(&serde_json::json!({"cmd": "mesh_save_network"})).is_err());
        assert!(activity_request(&serde_json::json!({"cmd": "mesh_remove_network"})).is_err());
        assert!(Engine::default()
            .control(&serde_json::json!({"cmd": "traffic"}))
            .is_err());
    }

    #[test]
    fn windows_may_read_decryption_results_but_not_backend_commands() {
        // The decryption list asks through `runtime_control` on the Linux desktop.
        assert!(activity_request(&json!({"cmd": "mitm_hosts"})).is_ok());
        assert!(activity_request(&json!({"cmd": "clear_mitm_hosts"})).is_ok());
        // Only the backend names the session storage and key, picks saved sessions and reloads rules.
        for command in [
            desktop_mitm::sessions::STORAGE_COMMAND,
            "capture_sessions",
            "save_capture",
            "load_capture",
            "delete_capture",
            "reload_rules",
            "replay_proxy",
            "start_replay_proxy",
        ] {
            assert!(
                activity_request(&json!({ "cmd": command })).is_err(),
                "{command}"
            );
        }
    }

    #[test]
    fn backend_requests_reach_the_engine_without_the_window_list() {
        // A stopped engine answers that it is not running, not that the command is unavailable.
        let error = Engine::default()
            .request(&json!({"cmd": "reload_rules", "rules": []}))
            .unwrap_err();
        assert_eq!(error, "the engine is not running");
        let refused = Engine::default()
            .control(&json!({"cmd": "reload_rules", "rules": []}))
            .unwrap_err();
        assert!(refused.contains("is not available"), "{refused}");
    }

    #[test]
    fn parses_pasted_links_and_names_protocols() {
        let nodes = parse_nodes(LINK).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(protocol_name(&nodes[0].protocol), "shadowsocks");
        assert_eq!(
            protocol_name(&Protocol::Other("Mieru2".to_string())),
            "mieru2"
        );
        assert!(parse_nodes("not a subscription").is_err());
    }

    #[test]
    fn chooses_a_supported_node() {
        let nodes = parse_nodes(LINK).unwrap();
        let chosen = choose_node(&nodes, Some("missing")).map(|node| node.id.clone());
        if is_supported(&nodes[0]) {
            assert_eq!(chosen, Some(nodes[0].id.clone()));
            assert!(envelope(&nodes, &nodes[0].id)
                .unwrap()
                .contains("\"outbound\""));
        } else {
            assert_eq!(chosen, None);
        }
        assert!(envelope(&nodes, "missing").is_err());
    }

    #[test]
    fn a_stopped_engine_reports_nothing() {
        let mut engine = Engine::default();
        assert!(!engine.is_running());
        engine.stop();
        assert_eq!(engine.address(), None);
        assert_eq!(engine.node_id(), None);
    }
}
