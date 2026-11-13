//! When the desktop clients tell the engine to rebuild its connections
//! (`{"cmd":"reset_network"}`), the counterpart of the Apple tunnel's
//! `NetworkRecovery`.
//!
//! The engine's connections to proxy servers outlive a network change: a
//! shared QUIC session keeps sending to the old path and open flows hang
//! until they time out. A background thread samples [`current_path`] every
//! [`POLL`] and feeds it to [`NetworkWatch::observe`], which asks for a reset
//! when the path changed or when the gap between two samples shows the
//! computer slept.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

/// How often the clients sample the path.
pub const POLL: Duration = Duration::from_secs(3);

/// A gap this much longer than [`POLL`] between two samples means the
/// computer slept (a thread does not run while it sleeps).
pub const SLEEP_SLACK: Duration = Duration::from_secs(20);

/// Sleeps at least this long close the open connections on wake; shorter
/// ones only drop pooled sessions. NAT mappings for idle TCP usually last
/// about a minute or more.
pub const CLOSE_AFTER_SLEEP: Duration = Duration::from_secs(60);

/// A reset the engine should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reset {
    pub close_connections: bool,
    pub reason: Reason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NetworkChange,
    Wake,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::NetworkChange => "network change",
            Reason::Wake => "wake",
        }
    }
}

impl Reset {
    /// The engine control request for this reset.
    pub fn request(&self) -> Vec<u8> {
        format!(
            r#"{{"cmd":"reset_network","close_connections":{}}}"#,
            self.close_connections
        )
        .into_bytes()
    }
}

/// Decides on resets from successive path samples.
#[derive(Debug, Default)]
pub struct NetworkWatch {
    started: bool,
    current: Option<String>,
    last_sample: Option<SystemTime>,
}

impl NetworkWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// One sample: `path` from [`current_path`] (None while offline) at
    /// wall-clock time `now`. Returns a reset when a different network, or
    /// any network after none, is up, or after a sleep. The first sample
    /// (the network the engine started on) and repeats return none.
    pub fn observe(&mut self, now: SystemTime, path: Option<String>) -> Option<Reset> {
        let slept = self
            .last_sample
            .and_then(|last| now.duration_since(last).ok())
            .filter(|gap| *gap > POLL + SLEEP_SLACK)
            .map(|gap| gap.saturating_sub(POLL));
        self.last_sample = Some(now);
        let started = std::mem::replace(&mut self.started, true);
        let changed = started && path.is_some() && path != self.current;
        self.current = path;
        if changed {
            return Some(Reset {
                close_connections: true,
                reason: Reason::NetworkChange,
            });
        }
        slept.map(|slept| Reset {
            close_connections: slept >= CLOSE_AFTER_SLEEP,
            reason: Reason::Wake,
        })
    }
}

/// Public addresses only used to pick a route; nothing is sent to them.
const PROBE_V4: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53);
const PROBE_V6: SocketAddr = SocketAddr::new(
    IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111)),
    53,
);

/// The local addresses the system would use toward the internet over IPv4
/// and IPv6, as one string; None when neither has a route. Connecting a UDP
/// socket only looks up the route and sends nothing. A different Wi-Fi, a
/// cable plugged in or a VPN on the default route changes it.
pub fn current_path() -> Option<String> {
    let v4 = local_toward("0.0.0.0:0", PROBE_V4);
    let v6 = local_toward("[::]:0", PROBE_V6);
    if v4.is_none() && v6.is_none() {
        return None;
    }
    Some(format!(
        "{}|{}",
        v4.map(|ip| ip.to_string()).unwrap_or_default(),
        v6.map(|ip| ip.to_string()).unwrap_or_default()
    ))
}

fn local_toward(bind: &str, target: SocketAddr) -> Option<IpAddr> {
    let socket = UdpSocket::bind(bind).ok()?;
    socket.connect(target).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + seconds)
    }

    fn path(text: &str) -> Option<String> {
        Some(text.to_string())
    }

    #[test]
    fn the_first_sample_and_repeats_reset_nothing() {
        let mut watch = NetworkWatch::new();
        assert_eq!(watch.observe(at(0), path("192.168.1.5|")), None);
        assert_eq!(watch.observe(at(3), path("192.168.1.5|")), None);
    }

    #[test]
    fn another_network_closes_connections() {
        let mut watch = NetworkWatch::new();
        watch.observe(at(0), path("192.168.1.5|"));
        let reset = watch.observe(at(3), path("10.0.0.8|")).unwrap();
        assert!(reset.close_connections);
        assert_eq!(reset.reason, Reason::NetworkChange);
        assert_eq!(
            reset.request(),
            br#"{"cmd":"reset_network","close_connections":true}"#.to_vec()
        );
    }

    #[test]
    fn offline_waits_for_the_network_to_return() {
        let mut watch = NetworkWatch::new();
        watch.observe(at(0), path("192.168.1.5|"));
        assert_eq!(watch.observe(at(3), None), None);
        assert_eq!(watch.observe(at(6), None), None);
        assert!(watch.observe(at(9), path("192.168.1.5|")).is_some());
    }

    #[test]
    fn a_long_gap_is_a_wake() {
        let mut watch = NetworkWatch::new();
        watch.observe(at(0), path("192.168.1.5|"));
        let short = watch.observe(at(30), path("192.168.1.5|")).unwrap();
        assert_eq!(short.reason, Reason::Wake);
        assert!(!short.close_connections);
        let long = watch.observe(at(30 + 600), path("192.168.1.5|")).unwrap();
        assert!(long.close_connections);
        // Back to the normal cadence: nothing.
        assert_eq!(watch.observe(at(633), path("192.168.1.5|")), None);
    }

    #[test]
    fn a_new_network_after_sleep_counts_as_a_change() {
        let mut watch = NetworkWatch::new();
        watch.observe(at(0), path("192.168.1.5|"));
        let reset = watch.observe(at(10), path("10.0.0.8|")).unwrap();
        assert_eq!(reset.reason, Reason::NetworkChange);
    }

    #[test]
    fn twenty_switches_reset_twenty_times() {
        let mut watch = NetworkWatch::new();
        watch.observe(at(0), path("a|"));
        let resets = (1..=20)
            .filter(|cycle| {
                let next = if cycle % 2 == 1 { "b|" } else { "a|" };
                watch.observe(at(cycle * 3), path(next)).is_some()
            })
            .count();
        assert_eq!(resets, 20);
    }

    #[test]
    fn a_clock_set_backwards_is_no_wake() {
        let mut watch = NetworkWatch::new();
        watch.observe(at(100), path("a|"));
        assert_eq!(watch.observe(at(0), path("a|")), None);
    }
}
