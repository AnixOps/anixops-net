//! Windows Firewall rules of the joined 组网 networks (requirement R5, audit #372 P3/A6). This window runs the mesh
//! engine as a full node that listens on TCP and UDP (`node.listen`, default 11010), so that members can reach it:
//!
//! - while the engine runs a joined network, this process holds the kernel's firewall guard for that network
//!   (`platform_windows::mesh_host`), which allows inbound traffic to the network's listen ports and blocks inbound
//!   traffic to this program that arrives on the mesh adapter, so that nothing else of the window (the proxy
//!   listener, the control port) is reachable from mesh members;
//! - the guard is dropped when the network is left or the engine stops (the kernel removes the rules on a
//!   background thread). On exit the rules are dropped only after every engine has stopped ([`clear`], then
//!   [`finish`] for at most [`EXIT_WAIT`]); at the end of a session they stay, for the next start's recovery;
//! - one process per host holds such changes: when another one does (the mesh node service), this window adds no
//!   rules, shows that on the 组网 page ([`issues`]) and tries again on every update and on the periodic sync until
//!   the holder is gone. Rules a crash left behind are removed by the recovery that runs before the first guard
//!   is taken, and by the uninstaller ([`recover`]);
//! - nothing is forwarded or translated: the client advertises no subnets yet, so there is no forwarding guard and no
//!   WinNAT. The rules are never written with `netsh`; the kernel's own cmdlets do it.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

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
    /// Another process holds the host changes: no rules are added, and every update tries again.
    busy: bool,
    held: BTreeMap<String, Held>,
    /// The last failure that was logged, so that a repeated one is not logged on every update.
    failure: Option<String>,
    /// The last update wanted rules for at least one network.
    wanted: bool,
    /// The last update could not add the rules of some network (any error, `AlreadyExists` included).
    failed: bool,
    /// Counts the [`clear`]s, so that an update that was adding rules meanwhile does not keep them.
    epoch: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    recovered: false,
    busy: false,
    held: BTreeMap::new(),
    failure: None,
    wanted: false,
    failed: false,
    epoch: 0,
});

/// Serializes [`update`]s, so that [`STATE`] is never held while the kernel's slow calls (PowerShell, waits) run:
/// [`issues`] and [`clear`] from the window's threads must not wait for them.
static UPDATES: Mutex<()> = Mutex::new(());

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Brings the held rules in line with `wanted` (network name, listen ports): drops those of networks that are no
/// longer wanted or whose ports changed and adds the rest, for this program, the mesh adapter being `adapter` (its
/// interface alias). The first call recovers an earlier process's leftovers; while another process holds the host
/// changes, each call tries that again. Failures are logged and shown by [`issues`], never raised: a network
/// without rules still runs.
pub fn update(wanted: &[(String, Vec<u16>)], adapter: &str) {
    let _serial = UPDATES.lock().unwrap_or_else(|e| e.into_inner());
    let (recover_now, was_busy, epoch) = {
        let mut state = state();
        state.wanted = wanted.iter().any(|(_, ports)| !normal(ports).is_empty());
        (!state.recovered || state.busy, state.busy, state.epoch)
    };
    if recover_now {
        let outcome = mesh_host::recover_mesh_host();
        let mut state = state();
        match outcome {
            Ok(report) => {
                state.recovered = true;
                state.busy = false;
                if was_busy {
                    log("the other process let go of the mesh host changes; firewall rules resume");
                }
                log_recovery(&report);
            }
            Err(error) if error.kind() == io::ErrorKind::ResourceBusy => {
                if !was_busy {
                    log("another process holds the mesh host changes; firewall rules are left to it");
                }
                state.recovered = true;
                state.busy = true;
                return;
            }
            Err(error) => {
                state.recovered = true;
                state.busy = false;
                let text = format!("the leftover firewall rules could not be recovered: {error}");
                log(&text);
                state.failure = Some(text);
            }
        }
    }
    let add = {
        let mut state = state();
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
            state.failed = false;
        }
        add
    };
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
            let mut state = state();
            if state.epoch == epoch {
                state.failed = true;
                if state.failure.as_deref() != Some(text.as_str()) {
                    log(&text);
                    state.failure = Some(text);
                }
            }
            return;
        }
    };
    let mut failed = false;
    for (name, ports) in add {
        let added = mesh_host::add_mesh_firewall_rules(&name, &program, &ports, adapter);
        let mut state = state();
        if state.epoch != epoch {
            // The engine stopped meanwhile: the rules are not wanted any more (the guard drops here).
            return;
        }
        match added {
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
                failed = true;
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
    let mut state = state();
    if state.epoch == epoch {
        state.failed = failed;
    }
}

/// What the 组网 page shows about the rules (`network.local_dns`, code `inbound_unprotected`): none, or one issue
/// while a joined network that should have rules has none, because another process holds the host changes (detail
/// `"busy"`) or because adding them failed (detail null). Tried again by [`update`] ([`retry_due`]).
pub fn issues() -> Vec<Value> {
    let state = state();
    issues_of(state.wanted, state.busy, state.failed)
}

/// [`issues`] as a function of the state.
fn issues_of(wanted: bool, busy: bool, failed: bool) -> Vec<Value> {
    if !wanted || !(busy || failed) {
        return Vec::new();
    }
    vec![json!({
        "code": "inbound_unprotected",
        "detail": if busy { json!("busy") } else { Value::Null },
    })]
}

/// How many polls of the route watch (2 s each) pass between the retries of rules that are missing: about 30 s.
pub const RETRY_EVERY_POLLS: u32 = 15;

/// Counts one poll of a periodic caller: `true` once every [`RETRY_EVERY_POLLS`] polls while rules are missing
/// ([`issues`] is not empty). Without a problem the count starts over.
pub fn retry_due(polls: &mut u32) -> bool {
    counted(polls, !issues().is_empty())
}

fn counted(polls: &mut u32, problem: bool) -> bool {
    if !problem {
        *polls = 0;
        return false;
    }
    *polls = polls.saturating_add(1);
    if *polls >= RETRY_EVERY_POLLS {
        *polls = 0;
        return true;
    }
    false
}

/// Drops every guard: the engine stopped or the last network left. The kernel removes the rules on its own thread
/// ([`finish`] waits for that). On exit, call it only after every engine has stopped: while one runs, the rules are
/// what keeps mesh members from the window's other services.
pub fn clear() {
    let mut state = state();
    state.epoch = state.epoch.wrapping_add(1);
    if !state.held.is_empty() {
        state.held.clear();
        log("firewall rules dropped");
    }
    state.failure = None;
    state.wanted = false;
    state.failed = false;
}

/// Waits for the removals [`clear`] started, for at most `timeout`; answers whether they finished. When they did
/// not, the rest is left to the next start's recovery.
pub fn finish(timeout: Duration) -> bool {
    let done = REMOVALS.wait(mesh_host::finish_mesh_firewall_removals, timeout);
    if !done {
        log(&format!(
            "warning: the firewall rules were not all removed within {} s; the next start cleans up",
            timeout.as_secs()
        ));
    }
    done
}

/// Removes what an earlier process left (a crash, the uninstaller), once. Another process holding the host changes
/// is an error here: the rules may be this window's own, and a window that is forced to end afterwards leaves them
/// for good, so the purge log must show it.
pub fn recover() -> io::Result<()> {
    match mesh_host::recover_mesh_host() {
        Ok(report) => {
            log_recovery(&report);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::ResourceBusy => Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "another process holds them",
        )),
        Err(error) => Err(error),
    }
}

/// The waits of [`finish`]. The kernel's `finish_mesh_firewall_removals` takes every pending removal and joins
/// them, so a call that was given up on keeps them in its thread and a later call would find none and return at
/// once. One waiter thread therefore runs at a time and every wait joins it ([`Waiter`]).
static REMOVALS: Waiter = Waiter {
    current: Mutex::new(None),
};

/// Completion signal of one run of the work.
struct Signal {
    done: Mutex<bool>,
    ready: Condvar,
}

impl Signal {
    fn is_done(&self) -> bool {
        *self.done.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn finish(&self) {
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.ready.notify_all();
    }

    fn wait(&self, timeout: Duration) -> bool {
        let done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        let (done, _) = self
            .ready
            .wait_timeout_while(done, timeout, |done| !*done)
            .unwrap_or_else(|e| e.into_inner());
        *done
    }
}

/// Sets the signal when the work ends, a panic included.
struct Finishes(Arc<Signal>);

impl Drop for Finishes {
    fn drop(&mut self) {
        self.0.finish();
    }
}

struct Waiter {
    current: Mutex<Option<Arc<Signal>>>,
}

impl Waiter {
    /// Runs `work` on its own thread and waits for it for at most `timeout`: `true` when it finished. Work that is
    /// still running afterwards is left to run (it cannot be cancelled), and the next wait joins it first, for the
    /// time it is given, then runs `work` again for what was added meanwhile.
    fn wait(&self, work: impl FnOnce() + Send + 'static, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut work = Some(work);
        loop {
            let (signal, fresh) = {
                let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
                match current.as_ref().filter(|signal| !signal.is_done()) {
                    Some(running) => (running.clone(), false),
                    None => {
                        let Some(work) = work.take() else {
                            return false;
                        };
                        let signal = Arc::new(Signal {
                            done: Mutex::new(false),
                            ready: Condvar::new(),
                        });
                        let finishes = Finishes(signal.clone());
                        let spawned = thread::Builder::new()
                            .name("mesh-firewall-wait".into())
                            .spawn(move || {
                                let _finishes = finishes;
                                work();
                            });
                        if spawned.is_err() {
                            return false;
                        }
                        *current = Some(signal.clone());
                        (signal, true)
                    }
                }
            };
            if !signal.wait(deadline.saturating_duration_since(Instant::now())) {
                return false;
            }
            if fresh {
                return true;
            }
        }
    }
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

    const fn new_waiter() -> Waiter {
        Waiter {
            current: Mutex::new(None),
        }
    }

    #[test]
    fn quick_work_finishes_within_the_wait() {
        assert!(new_waiter().wait(|| {}, Duration::from_secs(2)));
    }

    #[test]
    fn slow_work_is_given_up_on_after_the_wait() {
        let started = Instant::now();
        assert!(!new_waiter().wait(
            || thread::sleep(Duration::from_secs(2)),
            Duration::from_millis(20)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_later_wait_still_waits_for_work_an_earlier_one_gave_up_on() {
        let waiter = new_waiter();
        assert!(!waiter.wait(
            || thread::sleep(Duration::from_millis(300)),
            Duration::from_millis(20)
        ));
        let started = Instant::now();
        // The work of this call returns at once, but the earlier one still runs: this call waits for it.
        assert!(waiter.wait(|| {}, Duration::from_secs(5)));
        assert!(started.elapsed() >= Duration::from_millis(150));
        // And is bounded by its own timeout.
        let slow = new_waiter();
        assert!(!slow.wait(
            || thread::sleep(Duration::from_millis(400)),
            Duration::from_millis(10)
        ));
        assert!(!slow.wait(|| {}, Duration::from_millis(20)));
    }

    #[test]
    fn rules_are_missing_only_for_a_wanted_network_that_has_none() {
        assert!(issues_of(false, true, true).is_empty());
        assert!(issues_of(true, false, false).is_empty());
        assert_eq!(
            issues_of(true, true, false),
            [json!({ "code": "inbound_unprotected", "detail": "busy" })]
        );
        assert_eq!(
            issues_of(true, false, true),
            [json!({ "code": "inbound_unprotected", "detail": null })]
        );
        // Busy is the more telling reason when both hold.
        assert_eq!(issues_of(true, true, true)[0]["detail"], "busy");
    }

    #[test]
    fn a_problem_is_retried_every_so_many_polls_and_a_cleared_one_starts_over() {
        let mut polls = 0;
        let due: Vec<bool> = (0..RETRY_EVERY_POLLS * 2)
            .map(|_| counted(&mut polls, true))
            .collect();
        assert_eq!(due.iter().filter(|due| **due).count(), 2);
        assert!(due[RETRY_EVERY_POLLS as usize - 1]);
        assert!(!due[0]);
        counted(&mut polls, true);
        assert!(!counted(&mut polls, false));
        assert_eq!(polls, 0);
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
