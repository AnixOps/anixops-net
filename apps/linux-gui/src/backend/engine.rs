//! Subscription parsing and the in-process engine: the selected node, or every node with the subscription's
//! rules and policy groups, runs behind a local SOCKS5/HTTP listener, as in the Windows desktop mode, with no
//! external engine binary.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use config_core::routing_import::import_subscription_routing;
use config_core::CoreSubscriptionService;
use control_domain::{NodeDescriptor, Protocol, RawSubscription, SubscriptionService};
use engine_embedded::config::RuntimeConfig;
use engine_embedded::delay::measure_delay;
use engine_embedded::local_proxy::LocalProxy;
use engine_embedded::nodes::{outbound_for_node, routing_envelope, single_node_envelope};
use inbound_mixed::MixedSettings;
use serde_json::{json, Value};

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
    let document = service.parse(&raw).map_err(|error| error.message)?;
    let catalog = service
        .normalize(&document)
        .map_err(|error| error.message)?;
    if catalog.nodes.is_empty() {
        return Err("no nodes were found in the subscription".to_string());
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
        .or_else(|| nodes.iter().find(|node| is_supported(node)))
}

pub fn envelope(nodes: &[NodeDescriptor], node_id: &str) -> Result<String, String> {
    single_node_envelope(PROFILE_ID, nodes, Some(node_id))
        .map(|(envelope, _)| envelope)
        .map_err(str::to_string)
}

/// The envelope with every node and the subscription's rules and policy groups, when `content` carries them,
/// every node can run on the engine and the engine accepts the result. Otherwise one node runs at a time.
pub fn subscription_routing(content: &str, nodes: &[NodeDescriptor]) -> Option<String> {
    let spec = import_subscription_routing(content, nodes).spec?;
    routing_envelope(PROFILE_ID, nodes, &spec)
        .ok()
        .filter(|envelope| RuntimeConfig::from_json(envelope.as_bytes()).is_ok())
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
/// every select group, as Loon does with subscription groups. A source is left out when its ids clash or the
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

/// The running local proxy, if any.
#[derive(Default)]
pub struct Engine {
    proxy: Option<LocalProxy>,
    node_id: Option<String>,
}

impl Engine {
    pub fn start(
        &mut self,
        envelope: &str,
        node_id: &str,
        port: u16,
    ) -> Result<SocketAddr, String> {
        self.stop();
        let listen = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let proxy = LocalProxy::start(envelope.as_bytes(), listen, MixedSettings::default())
            .map_err(|error| error.message.to_string())?;
        let address = proxy.local_addr();
        self.proxy = Some(proxy);
        self.node_id = Some(node_id.to_string());
        Ok(address)
    }

    pub fn stop(&mut self) {
        if let Some(mut proxy) = self.proxy.take() {
            proxy.stop();
        }
        self.node_id = None;
    }

    pub fn is_running(&self) -> bool {
        self.proxy.as_ref().is_some_and(LocalProxy::is_running)
    }

    pub fn address(&self) -> Option<SocketAddr> {
        self.proxy.as_ref().map(LocalProxy::local_addr)
    }

    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }
}

/// Measures `url` through one node.
pub fn node_delay(nodes: &[NodeDescriptor], node_id: &str, url: &str) -> Result<Duration, String> {
    let envelope = envelope(nodes, node_id)?;
    measure_delay(envelope.as_bytes(), url, DELAY_TIMEOUT).map_err(|error| error.to_string())
}

/// Tests every supported node and returns the fastest one with its delay.
pub fn fastest_node(nodes: &[NodeDescriptor], url: &str) -> Option<(String, Duration)> {
    let candidates: Vec<&NodeDescriptor> = nodes.iter().filter(|node| is_supported(node)).collect();
    let mut best: Option<(String, Duration)> = None;
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
                if best.as_ref().is_none_or(|(_, current)| delay < *current) {
                    best = Some((id, delay));
                }
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@82.47.34.99:11111#HK";

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
