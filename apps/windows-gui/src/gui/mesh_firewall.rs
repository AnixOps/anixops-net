//! Windows Firewall rules of the joined 组网 networks (requirement R5, audit #372 P3/A6). This window runs the mesh
//! engine as a full node that listens on TCP and UDP (`node.listen`, default 11010), so that members can reach it:
//!
//! - while the engine runs a joined network, this process holds the kernel's firewall guard for that network
//!   (`platform_windows::mesh_host`), which allows inbound traffic to the network's listen ports and blocks inbound
//!   traffic to this program that arrives on the mesh adapter, so that nothing else of the window (the proxy
//!   listener, the control port) is reachable from mesh members;
//! - the guard is dropped when the network is left or the engine stops (the kernel removes the rules on a
//!   background thread), and the window waits for those removals, for at most [`EXIT_WAIT`], as it exits;
//! - one process per host holds such changes: when another one does (the mesh node service), this window adds no
//!   rules and says so once. Rules a crash left behind are removed by the recovery that runs before the first guard
//!   is taken, and by the uninstaller ([`recover`]);
//! - nothing is forwarded or translated: the client advertises no subnets yet, so there is no forwarding guard and no
//!   WinNAT. The rules are never written with `netsh`; the kernel's own cmdlets do it.

use std::collections::BTreeMap;
use std::io;
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::Duration;

use platform_windows::managed::append_managed_log;
use platform_windows::mesh_host::{self, FirewallGuard, RecoveryReport};

/// How long the window waits for the rules' removal as it exits. What is left is removed by the next start's
/// recovery.
pub const EXIT_WAIT: Duration = Duration::from_secs(10);

/// How long an update waits for removals still running before it adds rules: the kernel refuses a network whose
/// earlier rules are still being removed.
const REPLACE_WAIT: Duration = Duration::from_secs(5);

/// A held guard with the ports it was made for.
struct Held {
    ports: Vec<u16>,
    _guard: FirewallGuard,
}

struct State {
    /// Whether the recovery of an earlier process's leftovers ran.
    recovered: bool,
    /// Another process holds the host changes: no rules are added.
    busy: bool,
    held: BTreeMap<String, Held>,
    /// The last failure that was logged, so that a repeated one is not logged on every update.
    failure: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State {
    recovered: false,
    busy: false,
    held: BTreeMap::new(),
    failure: None,
});

/// Brings the held rules in line with `wanted` (network name, listen ports): drops those of networks that are no
/// longer wanted or whose ports changed and adds the rest, for this program, the mesh adapter being `adapter` (its
/// interface alias). The first call recovers an earlier process's leftovers. Failures are logged, never raised: a
/// network without rules still runs.
pub fn update(wanted: &[(String, Vec<u16>)], adapter: &str) {
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    if !state.recovered {
        state.recovered = true;
        match mesh_host::recover_mesh_host() {
            Ok(report) => log_recovery(&report),
            Err(error) if error.kind() == io::ErrorKind::ResourceBusy => {
                state.busy = true;
                log("another process holds the mesh host changes; firewall rules are left to it");
            }
            Err(error) => {
                let text = format!("the leftover firewall rules could not be recovered: {error}");
                log(&text);
                state.failure = Some(text);
            }
        }
    }
    if state.busy {
        return;
    }
    let held: Vec<(String, Vec<u16>)> = state
        .held
        .iter()
        .map(|(name, held)| (name.clone(), held.ports.clone()))
        .collect();
    let (drop_names, add) = diff(&held, wanted);
    for name in &drop_names {
        state.held.remove(name);
    }
    if add.is_empty() {
        return;
    }
    // The kernel refuses a network whose earlier rules are still being removed (dropped here, or by a `clear` when
    // the engine stopped just before): wait for those removals first. Nothing pending returns at once.
    finish(REPLACE_WAIT);
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => {
            let text = format!("the program's path is unknown, no firewall rules: {error}");
            if state.failure.as_deref() != Some(text.as_str()) {
                log(&text);
                state.failure = Some(text);
            }
            return;
        }
    };
    for (name, ports) in add {
        match mesh_host::add_mesh_firewall_rules(&name, &program, &ports, adapter) {
            Ok(guard) => {
                log(&format!(
                    "firewall rules for {name} in place (inbound to port {})",
                    port_text(&ports)
                ));
                state.held.insert(
                    name,
                    Held {
                        ports,
                        _guard: guard,
                    },
                );
                state.failure = None;
            }
            Err(error) => {
                let text = format!(
                    "firewall rules for {name} were not added: {error}{}",
                    mesh_host::last_firewall_error()
                        .map(|detail| format!(" ({detail})"))
                        .unwrap_or_default()
                );
                if state.failure.as_deref() != Some(text.as_str()) {
                    log(&text);
                    state.failure = Some(text);
                }
            }
        }
    }
}

/// Drops every guard: the engine stopped or the last network left. The kernel removes the rules on its own thread
/// ([`finish`] waits for that).
pub fn clear() {
    if let Ok(mut state) = STATE.lock() {
        if !state.held.is_empty() {
            state.held.clear();
            log("firewall rules dropped");
        }
        state.failure = None;
    }
}

/// Waits for the removals [`clear`] started, for at most `timeout`; answers whether they finished. When they did
/// not, the rest is left to the next start's recovery.
pub fn finish(timeout: Duration) -> bool {
    let done = wait_at_most(mesh_host::finish_mesh_firewall_removals, timeout);
    if !done {
        log(&format!(
            "warning: the firewall rules were not all removed within {} s; the next start cleans up",
            timeout.as_secs()
        ));
    }
    done
}

/// Removes what an earlier process left (a crash, the uninstaller), once. Another process holding the host changes
/// is not an error: the rules are then its own.
pub fn recover() -> io::Result<()> {
    match mesh_host::recover_mesh_host() {
        Ok(report) => {
            log_recovery(&report);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::ResourceBusy => Ok(()),
        Err(error) => Err(error),
    }
}

/// Runs `work` on its own thread and waits for it for at most `timeout`: `true` when it finished. Work that is
/// still running afterwards is left to run; it cannot be cancelled.
fn wait_at_most(work: impl FnOnce() + Send + 'static, timeout: Duration) -> bool {
    let (sender, receiver) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("mesh-firewall-wait".into())
        .spawn(move || {
            work();
            let _ = sender.send(());
        });
    if spawned.is_err() {
        return false;
    }
    receiver.recv_timeout(timeout).is_ok()
}

/// What to drop and what to add to go from the `held` networks (with the ports they were made for) to `wanted`:
/// the names to drop (gone, or with other ports) and the networks to add. Ports are compared sorted and without
/// duplicates and zero; a wanted network without a port gets no rules.
fn diff(
    held: &[(String, Vec<u16>)],
    wanted: &[(String, Vec<u16>)],
) -> (Vec<String>, Vec<(String, Vec<u16>)>) {
    let wanted: BTreeMap<&str, Vec<u16>> = wanted
        .iter()
        .map(|(name, ports)| (name.as_str(), normal(ports)))
        .filter(|(_, ports)| !ports.is_empty())
        .collect();
    let held: BTreeMap<&str, Vec<u16>> = held
        .iter()
        .map(|(name, ports)| (name.as_str(), normal(ports)))
        .collect();
    let dropped = held
        .iter()
        .filter(|(name, ports)| wanted.get(*name) != Some(ports))
        .map(|(name, _)| (*name).to_string())
        .collect();
    let added = wanted
        .iter()
        .filter(|(name, ports)| held.get(*name) != Some(ports))
        .map(|(name, ports)| ((*name).to_string(), ports.clone()))
        .collect();
    (dropped, added)
}

fn normal(ports: &[u16]) -> Vec<u16> {
    let mut ports: Vec<u16> = ports.iter().copied().filter(|port| *port != 0).collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn port_text(ports: &[u16]) -> String {
    ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn log_recovery(report: &RecoveryReport) {
    if report.firewall_cleared
        || report.nat_cleared
        || report.forwarding_restored > 0
        || report.state_set_aside
    {
        log(&format!(
            "recovered what an earlier run left: firewall rules removed {}, translation removed {}, forwarding restored {}{}",
            report.firewall_cleared,
            report.nat_cleared,
            report.forwarding_restored,
            if report.state_set_aside {
                ", an unreadable state file was set aside"
            } else {
                ""
            }
        ));
    }
}

fn log(message: &str) {
    let _ = append_managed_log("tauri-gui", &format!("mesh: {message}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(name: &str, ports: &[u16]) -> (String, Vec<u16>) {
        (name.to_string(), ports.to_vec())
    }

    #[test]
    fn quick_work_finishes_within_the_wait() {
        assert!(wait_at_most(|| {}, Duration::from_secs(2)));
    }

    #[test]
    fn slow_work_is_given_up_on_after_the_wait() {
        let started = std::time::Instant::now();
        assert!(!wait_at_most(
            || thread::sleep(Duration::from_secs(2)),
            Duration::from_millis(20)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_new_network_is_added_and_a_gone_one_dropped() {
        let (dropped, added) = diff(&[net("old", &[11010])], &[net("new", &[11010])]);
        assert_eq!(dropped, ["old"]);
        assert_eq!(added, [net("new", &[11010])]);
    }

    #[test]
    fn an_unchanged_network_is_left_alone() {
        let held = [net("office", &[11010, 11011])];
        let (dropped, added) = diff(&held, &[net("office", &[11011, 11010, 11010])]);
        assert!(dropped.is_empty());
        assert!(added.is_empty());
    }

    #[test]
    fn changed_ports_drop_and_add_the_network_again() {
        let (dropped, added) = diff(&[net("office", &[11010])], &[net("office", &[12000])]);
        assert_eq!(dropped, ["office"]);
        assert_eq!(added, [net("office", &[12000])]);
    }

    #[test]
    fn a_network_without_a_port_gets_no_rules() {
        let (dropped, added) = diff(&[], &[net("office", &[]), net("zero", &[0])]);
        assert!(dropped.is_empty());
        assert!(added.is_empty());
    }

    #[test]
    fn nothing_wanted_drops_everything() {
        let (dropped, added) = diff(&[net("a", &[1]), net("b", &[2])], &[]);
        assert_eq!(dropped, ["a", "b"]);
        assert!(added.is_empty());
    }
}
