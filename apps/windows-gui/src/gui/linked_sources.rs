//! Subscriptions other than the one in use. Windows runs one imported profile at a time, so each subscription's
//! nodes are cached when it is imported; the node list shows them after the active profile's, and a routed
//! profile gets each of them as a `订阅·<name>` select policy group, as Loon does with subscription groups.

use engine_embedded::config::RuntimeConfig;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// The id prefix of the policy groups made from other subscriptions.
pub const GROUP_PREFIX: &str = "订阅·";

/// A cached node of a subscription, with the outbound the embedded engine runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedNode {
    pub id: String,
    pub label: String,
    pub protocol: String,
    pub outbound: Value,
}

/// Another subscription and its cached nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct OtherSource {
    pub id: String,
    pub name: String,
    pub nodes: Vec<CachedNode>,
}

impl OtherSource {
    /// The id node `node_id` of this subscription has in the node list and in a linked profile.
    pub fn outbound_id(&self, node_id: &str) -> String {
        format!("{}.{node_id}", self.id)
    }

    pub fn group_id(&self) -> String {
        format!("{GROUP_PREFIX}{}", self.name.replace(',', " "))
    }

    /// The node behind a listed id of this subscription.
    pub fn node(&self, listed_id: &str) -> Option<&CachedNode> {
        let node_id = listed_id.strip_prefix(&self.id)?.strip_prefix('.')?;
        self.nodes.iter().find(|node| node.id == node_id)
    }
}

fn cache_path(directory: &Path, source_id: &str) -> PathBuf {
    directory.join(format!("{source_id}.json"))
}

pub fn write_cache(directory: &Path, source_id: &str, nodes: &[CachedNode]) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let content = serde_json::to_string(nodes).map_err(|error| error.to_string())?;
    fs::write(cache_path(directory, source_id), content).map_err(|error| error.to_string())
}

pub fn read_cache(directory: &Path, source_id: &str) -> Option<Vec<CachedNode>> {
    let content = fs::read_to_string(cache_path(directory, source_id)).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn remove_cache(directory: &Path, source_id: &str) {
    let _ = fs::remove_file(cache_path(directory, source_id));
}

/// A profile that runs one cached node, for delay tests.
pub fn node_profile(profile_id: &str, schema_version: u32, node: &CachedNode) -> String {
    json!({
        "schema_version": schema_version,
        "profile_id": profile_id,
        "outbound": node.outbound,
    })
    .to_string()
}

/// Takes the linked subscription groups and their nodes out of a routed profile.
pub fn unlink(profile: &mut Value) {
    let Some(routing) = profile.get_mut("routing").and_then(Value::as_object_mut) else {
        return;
    };
    let mut removed = Vec::new();
    if let Some(groups) = routing.get_mut("groups").and_then(Value::as_array_mut) {
        groups.retain(|group| {
            let id = group.get("id").and_then(Value::as_str).unwrap_or_default();
            if !id.starts_with(GROUP_PREFIX) {
                return true;
            }
            removed.push(id.to_string());
            removed.extend(
                group
                    .get("members")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string),
            );
            false
        });
        for group in groups.iter_mut() {
            if let Some(members) = group.get_mut("members").and_then(Value::as_array_mut) {
                members.retain(|member| {
                    !member
                        .as_str()
                        .is_some_and(|member| removed.iter().any(|id| id == member))
                });
            }
            let stale = group
                .get("default")
                .and_then(Value::as_str)
                .is_some_and(|default| removed.iter().any(|id| id == default));
            if stale {
                if let Some(group) = group.as_object_mut() {
                    group.remove("default");
                }
            }
        }
    }
    if let Some(outbounds) = routing.get_mut("outbounds").and_then(Value::as_object_mut) {
        for id in &removed {
            outbounds.remove(id);
        }
    }
}

/// Adds each other subscription to a routed profile as a select group of its nodes, offered by every select
/// group. A subscription is left out when its ids clash or the engine would reject the result (it allows 64
/// nodes and 64 groups). A profile without rules is left as it is.
pub fn link(profile: &mut Value, others: &[OtherSource]) {
    for other in others {
        if let Some(linked) = linked(profile, other) {
            *profile = linked;
        }
    }
}

fn linked(profile: &Value, other: &OtherSource) -> Option<Value> {
    let mut profile = profile.clone();
    let group_id = other.group_id();
    let routing = profile.get_mut("routing")?.as_object_mut()?;
    let outbounds = routing.get_mut("outbounds")?.as_object_mut()?;
    if outbounds.contains_key(&group_id) || other.nodes.is_empty() {
        return None;
    }
    let mut members = Vec::new();
    for node in &other.nodes {
        let id = other.outbound_id(&node.id);
        if outbounds.contains_key(&id) {
            return None;
        }
        outbounds.insert(id.clone(), node.outbound.clone());
        members.push(Value::String(id));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn outbound(server: &str) -> Value {
        json!({
            "type": "shadowsocks",
            "server": server,
            "port": 8388,
            "method": "aes-256-gcm",
            "password": "secret"
        })
    }

    fn routed() -> Value {
        json!({
            "schema_version": engine_embedded::config::CONFIG_SCHEMA_VERSION,
            "profile_id": "windows-tauri-profile",
            "routing": {
                "outbounds": { "hk": outbound("hk.example"), "jp": outbound("jp.example") },
                "rules": ["FINAL,Proxy"],
                "groups": [
                    { "id": "Proxy", "type": "select", "members": ["Auto", "hk", "DIRECT"], "default": "hk" },
                    { "id": "Auto", "type": "url-test", "members": ["hk", "jp"] }
                ]
            }
        })
    }

    fn other(id: &str, name: &str, nodes: &[&str]) -> OtherSource {
        OtherSource {
            id: id.to_string(),
            name: name.to_string(),
            nodes: nodes
                .iter()
                .map(|node| CachedNode {
                    id: node.to_string(),
                    label: node.to_uppercase(),
                    protocol: "Shadowsocks".to_string(),
                    outbound: outbound(&format!("{node}.example")),
                })
                .collect(),
        }
    }

    #[test]
    fn other_subscriptions_join_the_select_groups_and_come_out_again() {
        assert!(RuntimeConfig::from_json(routed().to_string().as_bytes()).is_ok());
        let mut profile = routed();
        let two = other("source-2", "Two, B", &["fr", "de"]);
        link(&mut profile, &[two.clone()]);

        let groups = profile["routing"]["groups"].as_array().unwrap();
        assert_eq!(
            groups[0]["members"],
            json!(["Auto", "hk", "DIRECT", "订阅·Two  B"])
        );
        assert_eq!(groups[1]["members"], json!(["hk", "jp"]));
        assert_eq!(groups[2]["id"], "订阅·Two  B");
        assert_eq!(groups[2]["members"], json!(["source-2.fr", "source-2.de"]));
        assert!(profile["routing"]["outbounds"].get("source-2.de").is_some());
        assert_eq!(
            two.node("source-2.de").map(|node| node.label.as_str()),
            Some("DE")
        );
        assert_eq!(two.node("source-3.de"), None);

        profile["routing"]["groups"][0]["default"] = json!("订阅·Two  B");
        unlink(&mut profile);
        assert_eq!(profile["routing"]["groups"].as_array().unwrap().len(), 2);
        assert_eq!(
            profile["routing"]["groups"][0]["members"],
            json!(["Auto", "hk", "DIRECT"])
        );
        assert!(profile["routing"]["groups"][0].get("default").is_none());
        assert!(profile["routing"]["outbounds"].get("source-2.fr").is_none());
        assert!(RuntimeConfig::from_json(profile.to_string().as_bytes()).is_ok());
    }

    #[test]
    fn linking_stays_within_the_engine_limits() {
        let mut profile = routed();
        let names: Vec<String> = (0..62).map(|index| format!("n{index}")).collect();
        let big = other(
            "source-big",
            "Big",
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        let small = other("source-small", "Small", &["x"]);
        let empty = other("source-empty", "Empty", &[]);
        // 2 + 62 nodes fit exactly; one more would pass 64.
        link(&mut profile, &[big, small, empty]);
        let ids: Vec<&str> = profile["routing"]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|group| group["id"].as_str())
            .collect();
        assert_eq!(ids, ["Proxy", "Auto", "订阅·Big"]);

        // A profile without rules is left alone.
        let mut single =
            json!({ "schema_version": 1, "profile_id": "p", "outbound": outbound("a") });
        let before = single.clone();
        link(&mut single, &[other("source-2", "Two", &["fr"])]);
        unlink(&mut single);
        assert_eq!(single, before);
    }

    #[test]
    fn caches_round_trip_and_build_delay_profiles() {
        let directory = std::env::temp_dir().join(format!("anixops-linked-{}", std::process::id()));
        let source = other("source-1", "One", &["fr"]);
        write_cache(&directory, "source-1", &source.nodes).unwrap();
        assert_eq!(
            read_cache(&directory, "source-1"),
            Some(source.nodes.clone())
        );
        let profile: Value =
            serde_json::from_str(&node_profile("windows-tauri-profile", 1, &source.nodes[0]))
                .unwrap();
        assert_eq!(profile["outbound"]["server"], "fr.example");
        remove_cache(&directory, "source-1");
        assert_eq!(read_cache(&directory, "source-1"), None);
        let _ = fs::remove_dir_all(directory);
    }
}
