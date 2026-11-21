//! Traffic statistics of the desktop and Android clients, as on iOS (`TrafficStats.swift`): the last seven days in
//! hourly buckets, by host and by policy, counted from the engine's connection list (`{"cmd":"connections"}`)
//! polled every few seconds. Only host names and byte counts are kept, never addresses or paths: a connection the
//! engine saw no host name for counts under [`NO_HOST_NAME`], not under its address (third-party review F13).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Hours kept: seven days.
pub const KEPT_HOURS: u64 = 7 * 24;
/// Hosts kept per hour; the rest add up under [`OTHER_HOSTS`].
pub const HOSTS_PER_HOUR: usize = 64;
pub const OTHER_HOSTS: &str = "*";
/// Where connections without a host name add up; no host name contains `#`.
pub const NO_HOST_NAME: &str = "#";
const HOUR: u64 = 3600;

/// One hour of traffic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hour {
    /// The start of the hour, Unix seconds.
    pub start: u64,
    pub upload: u64,
    pub download: u64,
    #[serde(default)]
    pub hosts: BTreeMap<String, u64>,
    #[serde(default)]
    pub policies: BTreeMap<String, u64>,
}

/// The kept hours, oldest first, in the interface's `TrafficStats` shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficStats {
    #[serde(default)]
    pub hours: Vec<Hour>,
}

impl TrafficStats {
    /// The saved statistics; none when the file is missing or unreadable.
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
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, path)
    }

    /// Adds bytes one connection moved since it was last counted, at Unix time `now`.
    pub fn add(&mut self, host: &str, policy: &str, upload: u64, download: u64, now: u64) {
        let bytes = upload.saturating_add(download);
        if bytes == 0 {
            return;
        }
        let start = now - now % HOUR;
        let index = match self.hours.iter().rposition(|hour| hour.start == start) {
            Some(index) => index,
            None => {
                self.hours.push(Hour {
                    start,
                    ..Hour::default()
                });
                self.hours.sort_by_key(|hour| hour.start);
                self.prune(now);
                match self.hours.iter().rposition(|hour| hour.start == start) {
                    Some(index) => index,
                    None => return,
                }
            }
        };
        let hour = &mut self.hours[index];
        hour.upload = hour.upload.saturating_add(upload);
        hour.download = hour.download.saturating_add(download);
        let key = if hour.hosts.contains_key(host) || hour.hosts.len() < HOSTS_PER_HOUR {
            host
        } else {
            OTHER_HOSTS
        };
        let entry = hour.hosts.entry(key.to_string()).or_default();
        *entry = entry.saturating_add(bytes);
        let entry = hour.policies.entry(policy.to_string()).or_default();
        *entry = entry.saturating_add(bytes);
    }

    /// Drops hours older than seven days before `now`.
    pub fn prune(&mut self, now: u64) {
        let oldest = (now - now % HOUR).saturating_sub((KEPT_HOURS - 1) * HOUR);
        self.hours.retain(|hour| hour.start >= oldest);
    }

    pub fn clear(&mut self) {
        self.hours.clear();
    }
}

#[derive(Clone, Copy)]
struct Seen {
    upload: u64,
    download: u64,
}

/// Turns successive connection lists into byte deltas: each connection counts only what it moved since the last
/// list, so polling never counts a byte twice.
#[derive(Default)]
pub struct Counter {
    seen: HashMap<u64, Seen>,
}

impl Counter {
    /// Counts the engine's answer to `{"cmd":"connections"}` into `stats` at Unix time `now`; `true` when it
    /// added anything.
    pub fn ingest(&mut self, connections: &Value, stats: &mut TrafficStats, now: u64) -> bool {
        let mut next = HashMap::new();
        let mut added = false;
        let lists = ["active", "closed"]
            .iter()
            .filter_map(|key| connections.get(*key).and_then(Value::as_array));
        for connection in lists.flatten() {
            let Some(id) = connection["id"].as_u64() else {
                continue;
            };
            let current = Seen {
                upload: connection["upload"].as_u64().unwrap_or_default(),
                download: connection["download"].as_u64().unwrap_or_default(),
            };
            let before = self.seen.get(&id).copied();
            let upload = delta(current.upload, before.map(|seen| seen.upload));
            let download = delta(current.download, before.map(|seen| seen.download));
            if upload > 0 || download > 0 {
                stats.add(
                    &host_name(connection),
                    &policy(connection),
                    upload,
                    download,
                    now,
                );
                added = true;
            }
            next.insert(id, current);
        }
        self.seen = next;
        added
    }

    /// Forgets the connections seen so far, for a new engine.
    pub fn reset(&mut self) {
        self.seen.clear();
    }
}

/// A smaller count than before means the engine restarted and reused the id: count it from zero.
fn delta(now: u64, before: Option<u64>) -> u64 {
    match before {
        Some(before) if now >= before => now - before,
        _ => now,
    }
}

/// The host name, else [`NO_HOST_NAME`]: the statistics are kept on disk for seven days and hold no addresses.
fn host_name(connection: &Value) -> String {
    connection["host"]
        .as_str()
        .filter(|host| !host.is_empty())
        .unwrap_or(NO_HOST_NAME)
        .to_string()
}

/// The first policy of the chain, else the outbound.
fn policy(connection: &Value) -> String {
    connection["chain"]
        .as_array()
        .and_then(|chain| chain.first())
        .and_then(Value::as_str)
        .or_else(|| connection["outbound"].as_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: u64 = 1_763_150_000;

    fn list(upload: u64, download: u64) -> Value {
        json!({
            "active": [{"id": 1, "host": "a.example", "destination": "1.2.3.4:443", "chain": ["Proxy", "HK"], "outbound": "HK", "upload": upload, "download": download}],
            "closed": [{"id": 2, "host": null, "destination": "[2001:db8::1]:443", "chain": [], "outbound": "direct", "upload": 5, "download": 0}]
        })
    }

    #[test]
    fn counts_only_what_moved_since_the_last_list() {
        let mut stats = TrafficStats::default();
        let mut counter = Counter::default();
        assert!(counter.ingest(&list(10, 100), &mut stats, NOW));
        assert!(counter.ingest(&list(15, 100), &mut stats, NOW + 5));
        assert!(!counter.ingest(&list(15, 100), &mut stats, NOW + 10));
        assert_eq!(stats.hours.len(), 1);
        let hour = &stats.hours[0];
        assert_eq!(hour.start, NOW - NOW % 3600);
        assert_eq!((hour.upload, hour.download), (20, 100));
        assert_eq!(hour.hosts["a.example"], 115);
        // No host name: counted, but not under its address.
        assert_eq!(hour.hosts[NO_HOST_NAME], 5);
        assert!(!hour.hosts.contains_key("2001:db8::1"));
        assert!(!serde_json::to_string(&stats)
            .unwrap()
            .contains("2001:db8::1"));
        assert_eq!(hour.policies["Proxy"], 115);
        assert_eq!(hour.policies["direct"], 5);

        // A restarted engine starts its counts again.
        counter.reset();
        counter.ingest(&list(1, 1), &mut stats, NOW + 15);
        assert_eq!(stats.hours[0].upload, 26);
    }

    #[test]
    fn keeps_seven_days_and_folds_extra_hosts() {
        let mut stats = TrafficStats::default();
        stats.add("old", "p", 1, 0, NOW - 8 * 24 * 3600);
        stats.add("new", "p", 1, 0, NOW);
        assert_eq!(stats.hours.len(), 1);
        for index in 0..HOSTS_PER_HOUR + 3 {
            stats.add(&format!("h{index}"), "p", 1, 0, NOW);
        }
        assert_eq!(stats.hours[0].hosts.len(), HOSTS_PER_HOUR + 1);
        assert_eq!(stats.hours[0].hosts[OTHER_HOSTS], 4);
    }

    #[test]
    fn connections_without_a_host_name_keep_no_address() {
        // Saved statistics never hold an address (third-party review F13).
        assert_eq!(
            host_name(&json!({"destination": "1.2.3.4:80"})),
            NO_HOST_NAME
        );
        assert_eq!(host_name(&json!({"destination": "[::1]:80"})), NO_HOST_NAME);
        assert_eq!(
            host_name(&json!({"host": "", "destination": "x"})),
            NO_HOST_NAME
        );
        assert_eq!(
            host_name(&json!({"host": "example.com", "destination": "1.2.3.4:443"})),
            "example.com"
        );
    }

    #[test]
    fn saves_and_loads() {
        let path = std::env::temp_dir()
            .join(format!("traffic-stats-{}", std::process::id()))
            .join("stats.json");
        let mut stats = TrafficStats::default();
        stats.add("a", "p", 1, 2, NOW);
        stats.save(&path).unwrap();
        assert_eq!(TrafficStats::load(&path), stats);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
