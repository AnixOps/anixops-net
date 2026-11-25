//! What the backend tells an AI tool (docs/mcp-desktop.md): the user's access settings, and the state and nodes in
//! the shapes of `client-mcp`, which carry names, protocols and delays but never an address, a link or a
//! credential. The gate that decides whether a tool is answered at all is `Client::agent`.

use std::collections::HashMap;

use client_mcp::{Access, ConnectionState, ContentLevel, Node, NodeGroup, Status};

use super::state::AgentAccess;
use super::Backend;

impl AgentAccess {
    /// The settings as the tool reads them. Actions only together with reading.
    pub fn access(&self) -> Access {
        Access {
            read: self.read,
            act: self.read && self.act,
            content: content_level(&self.content),
        }
    }
}

/// The saved content level; anything unknown is the narrowest.
pub fn content_level(saved: &str) -> ContentLevel {
    match saved {
        "headers" => ContentLevel::Headers,
        "bodies" => ContentLevel::Bodies,
        _ => ContentLevel::Summary,
    }
}

/// The name a content level is saved and sent under, or `None` for one that does not exist.
pub fn content_name(level: &str) -> Option<&'static str> {
    match level {
        "summary" => Some("summary"),
        "headers" => Some("headers"),
        "bodies" => Some("bodies"),
        _ => None,
    }
}

impl Backend {
    pub fn agent_access(&self) -> Access {
        self.state.agent_access.access()
    }

    /// Saves the user's access settings and answers them as they now are. `act` unset keeps what was saved;
    /// turning reading off turns actions off with it.
    pub fn set_agent_access(
        &mut self,
        read: bool,
        act: Option<bool>,
        content: &str,
    ) -> Result<Access, String> {
        let content = content_name(content)
            .ok_or_else(|| format!("unknown content level {content}"))?
            .to_string();
        let act = read && act.unwrap_or(self.state.agent_access.act);
        let previous = std::mem::replace(
            &mut self.state.agent_access,
            AgentAccess { read, act, content },
        );
        if let Err(error) = self.save() {
            self.state.agent_access = previous;
            return Err(error);
        }
        Ok(self.agent_access())
    }

    /// One engine request an AI tool makes, after `Client::agent` let it through the policy. It does not go
    /// through the window's command list: the policy is narrower, and it also lists the policy groups, the
    /// outbound health and the rule check, which no window sends as an activity command.
    pub fn agent_engine_request(
        &self,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.engine.request(request)
    }

    /// The state as the main window shows it, read without asking the user's session anything.
    pub fn agent_status(&self) -> Status {
        let running = self.engine.is_running();
        let selected = self.current_node_id();
        let node = selected.as_deref().and_then(|id| {
            self.nodes()
                .into_iter()
                .find(|node| node.id == id)
                .map(|node| node.label)
        });
        let subscription = self.state.selected_source().map(|source| {
            subscription_meta::display_name(source.name.as_deref(), &source.location)
        });
        let takeover = if !running {
            None
        } else if self.engine.address().is_none() {
            // Without a local proxy the engine carries everything through the TUN device.
            Some("tun".to_string())
        } else if self.state.proxy_backup.is_some() {
            Some("system_proxy".to_string())
        } else {
            None
        };
        Status {
            connection: if running {
                ConnectionState::Connected
            } else {
                ConnectionState::Disconnected
            },
            mode: Some(self.state.routing_mode.as_str().to_string()),
            subscription: subscription.filter(|name| !looks_like_location(name)),
            node,
            takeover,
        }
    }

    /// The nodes by subscription, the selected subscription first, each with its last delay test's result
    /// (`delays`, by node id; `None` for a test that got no answer).
    pub fn agent_nodes(&self, delays: &HashMap<String, Option<u64>>) -> Vec<NodeGroup> {
        let own_source = self.state.selected_source().map(|source| source.id.clone());
        let mut groups: Vec<NodeGroup> = Vec::new();
        for node in self.nodes() {
            let name = node.group.clone().unwrap_or_default();
            let current = own_source.is_some() && node.source == own_source;
            let entry = Node {
                name: node.label,
                protocol: node.protocol,
                latency_ms: delays
                    .get(&node.id)
                    .copied()
                    .flatten()
                    .and_then(|delay| u32::try_from(delay).ok()),
                selected: node.selected,
            };
            match groups
                .iter_mut()
                .find(|group| group.name == name && group.current == current)
            {
                Some(group) => group.nodes.push(entry),
                None => groups.push(NodeGroup {
                    name,
                    current,
                    nodes: vec![entry],
                }),
            }
        }
        groups
    }
}

impl Backend {
    /// The id of the node an AI tool names for `select_node`: by its name, in the subscription `group` names when
    /// given. Of several with that name, the one in the subscription in use; more than one left is an error.
    pub fn agent_find_node(&self, name: &str, group: Option<&str>) -> Result<String, String> {
        let own_source = self.state.selected_source().map(|source| source.id.clone());
        let mut found: Vec<_> = self
            .nodes()
            .into_iter()
            .filter(|node| node.label == name)
            .filter(|node| group.is_none_or(|group| node.group.as_deref() == Some(group)))
            .collect();
        if found.len() > 1 && own_source.is_some() {
            let own: Vec<_> = found
                .iter()
                .filter(|node| node.source == own_source)
                .cloned()
                .collect();
            if own.len() == 1 {
                found = own;
            }
        }
        match found.as_slice() {
            [node] => Ok(node.id.clone()),
            [] => Err(match group {
                Some(group) => format!("No node named \"{name}\" is in the subscription \"{group}\"."),
                None => format!("No node is named \"{name}\"."),
            }),
            _ => Err(format!(
                "Several subscriptions have a node named \"{name}\". Name the subscription with group."
            )),
        }
    }
}

/// A subscription without a name of its own is named after its location's host; a location that is a local file
/// has no host and would be its whole path, which is not for an AI tool.
fn looks_like_location(name: &str) -> bool {
    name.contains('/') || name.contains('\\')
}
