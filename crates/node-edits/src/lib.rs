//! Node edits of the desktop and Android clients, as on iOS (`NodeManagement.swift`): a node of the selected
//! subscription gets a new name, server and port, or is removed. The edits are kept beside the subscription and
//! applied each time its nodes are parsed; updating the subscription drops them, so it brings back what the
//! provider serves.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use control_domain::NodeDescriptor;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A node's new name, server and port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub name: String,
    pub server: String,
    pub port: u16,
}

/// The edit form of one node, in the interface's `NodeForm` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeForm {
    pub name: String,
    pub server: String,
    pub port: u16,
    /// Why the node cannot be removed; `None` when it can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_reason: Option<&'static str>,
}

/// The edits of one subscription.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeEdits {
    #[serde(default)]
    pub changed: BTreeMap<String, Change>,
    #[serde(default)]
    pub removed: BTreeSet<String>,
}

/// Why what the user typed cannot be saved ([`check`]). A window that speaks another language tells them
/// apart by these.
pub const SERVER_EMPTY: &str = "Enter the server address.";
pub const SERVER_INVALID: &str = "The server address cannot contain spaces or /.";
pub const PORT_INVALID: &str = "The port must be 1–65535.";

/// Why a node cannot be removed ([`keep_reason`]).
pub const KEEP_LAST: &str = "The last node cannot be removed.";
pub const KEEP_USED: &str = "The subscription's policy groups or rules use this node.";

/// Checks what the user typed, with the rules of the iOS edit form.
pub fn check(name: &str, server: &str, port: &str) -> Result<Change, &'static str> {
    let server = server.trim();
    if server.is_empty() {
        return Err(SERVER_EMPTY);
    }
    if server.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err(SERVER_INVALID);
    }
    let port = port
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or(PORT_INVALID)?;
    Ok(Change {
        name: name.trim().to_string(),
        server: server.to_string(),
        port,
    })
}

impl NodeEdits {
    /// The saved edits; none when the file is missing or unreadable.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Saves the edits, or deletes the file when there are none.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if self.is_empty() {
            return match fs::remove_file(path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string(self).map_err(io::Error::other)?;
        fs::write(path, content)
    }

    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }

    /// The nodes with the edits applied: removed nodes left out, changed ones renamed and pointed at the new
    /// server and port. An empty name keeps the subscription's.
    pub fn applied(&self, nodes: &[NodeDescriptor]) -> Vec<NodeDescriptor> {
        nodes
            .iter()
            .filter(|node| !self.removed.contains(&node.id))
            .map(|node| {
                let mut node = node.clone();
                if let Some(change) = self.changed.get(&node.id) {
                    if !change.name.is_empty() {
                        node.name = change.name.clone();
                    }
                    node.endpoint.host = change.server.clone();
                    node.endpoint.port = change.port;
                }
                node
            })
            .collect()
    }

    /// Records a change of node `id`.
    pub fn change(&mut self, id: &str, change: Change) {
        self.changed.insert(id.to_string(), change);
    }

    /// Records that node `id` is removed.
    pub fn remove(&mut self, id: &str) {
        self.changed.remove(id);
        self.removed.insert(id.to_string());
    }
}

/// The edit form of node `id` among `nodes` (the edits already applied). `profile` is the routed profile
/// the nodes run in, if any: a node its policy groups or rules name cannot be removed, and one node must remain.
pub fn form(nodes: &[NodeDescriptor], profile: Option<&Value>, id: &str) -> Option<NodeForm> {
    let node = nodes.iter().find(|node| node.id == id)?;
    Some(NodeForm {
        name: node.name.clone(),
        server: node.endpoint.host.clone(),
        port: node.endpoint.port,
        keep_reason: keep_reason(nodes.len(), profile, id),
    })
}

/// Why node `id` cannot be removed from a list of `count` nodes run in `profile`.
pub fn keep_reason(count: usize, profile: Option<&Value>, id: &str) -> Option<&'static str> {
    if count <= 1 {
        return Some(KEEP_LAST);
    }
    let routing = profile.and_then(|profile| profile.get("routing"));
    if ["groups", "rules"]
        .iter()
        .filter_map(|key| routing.and_then(|routing| routing.get(*key)))
        .any(|section| names(section, id))
    {
        return Some(KEEP_USED);
    }
    None
}

/// Whether `value` names `id`: a string equal to it, or a comma-separated rule line with it as one field.
fn names(value: &Value, id: &str) -> bool {
    match value {
        Value::String(text) => text == id || text.split(',').any(|field| field.trim() == id),
        Value::Array(items) => items.iter().any(|item| names(item, id)),
        Value::Object(map) => map.values().any(|item| names(item, id)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_domain::{Endpoint, Protocol};
    use serde_json::json;

    fn node(id: &str, name: &str) -> NodeDescriptor {
        NodeDescriptor {
            id: id.to_string(),
            name: name.to_string(),
            protocol: Protocol::Shadowsocks,
            endpoint: Endpoint {
                host: "a.example".to_string(),
                port: 443,
            },
            tags: Vec::new(),
            metadata: Vec::new(),
        }
    }

    #[test]
    fn checks_the_form_like_ios() {
        assert_eq!(
            check(" 香港 ", " hk.example ", " 8443 "),
            Ok(Change {
                name: "香港".to_string(),
                server: "hk.example".to_string(),
                port: 8443
            })
        );
        assert!(check("a", " ", "1").is_err());
        assert!(check("a", "a b", "1").is_err());
        assert!(check("a", "a/b", "1").is_err());
        assert!(check("a", "a", "0").is_err());
        assert!(check("a", "a", "65536").is_err());
        assert!(check("a", "a", "x").is_err());
    }

    #[test]
    fn applies_changes_and_removals() {
        let mut edits = NodeEdits::default();
        edits.change(
            "1",
            Change {
                name: "新".to_string(),
                server: "b.example".to_string(),
                port: 8443,
            },
        );
        edits.remove("2");
        let nodes = edits.applied(&[node("1", "旧"), node("2", "删"), node("3", "留")]);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "新");
        assert_eq!(nodes[0].endpoint.host, "b.example");
        assert_eq!(nodes[0].endpoint.port, 8443);
        assert_eq!(nodes[1].name, "留");
    }

    #[test]
    fn an_empty_name_keeps_the_subscriptions() {
        let mut edits = NodeEdits::default();
        edits.change(
            "1",
            Change {
                name: String::new(),
                server: "b.example".to_string(),
                port: 1,
            },
        );
        assert_eq!(edits.applied(&[node("1", "旧")])[0].name, "旧");
    }

    #[test]
    fn keeps_nodes_the_routing_names_and_the_last_one() {
        let profile = json!({
            "routing": {"outbounds": {"1": {}, "2": {}, "3": {"server": "3"}},"groups": [{"tag": "Proxy", "members": ["1"]}], "rules": ["DOMAIN-SUFFIX,example.com,2"]}
        });
        assert!(keep_reason(3, Some(&profile), "1").is_some());
        assert!(keep_reason(3, Some(&profile), "2").is_some());
        assert!(keep_reason(3, Some(&profile), "3").is_none());
        assert!(keep_reason(1, None, "3").is_some());
        assert!(keep_reason(2, None, "3").is_none());
    }

    #[test]
    fn saves_and_loads_and_deletes_when_empty() {
        let dir = std::env::temp_dir().join(format!("node-edits-{}", std::process::id()));
        let path = dir.join("edits.json");
        let mut edits = NodeEdits::default();
        edits.remove("2");
        edits.save(&path).unwrap();
        assert_eq!(NodeEdits::load(&path), edits);
        NodeEdits::default().save(&path).unwrap();
        assert!(!path.exists());
        assert_eq!(NodeEdits::load(&path), NodeEdits::default());
        let _ = fs::remove_dir_all(dir);
    }
}
