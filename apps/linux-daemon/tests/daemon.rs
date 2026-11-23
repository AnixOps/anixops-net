//! The daemon with the real core, over its real socket.

#![cfg(target_os = "linux")]

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anixops_netd::{Access, Daemon, Limits, Options};
use client_ipc::transport::connect;
use client_ipc::{Connection, Handler, Peer, Role};
use serde_json::{json, Value};

const SOON: Duration = Duration::from_secs(30);

/// Lets this test's own user in or not.
struct Switch(AtomicBool);

impl Access for Switch {
    fn allows(&self, _uid: u32) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A program that only sends commands.
struct Tool;

impl Handler for Tool {
    fn request(&self, command: &str, _args: Value) -> Result<Value, String> {
        Err(format!("{command} is not something this program does"))
    }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn open(endpoint: &str) -> Arc<Peer> {
    let stream = connect(endpoint).unwrap();
    let mut connection = Connection::new(stream.try_clone().unwrap(), stream);
    let core = connection.greet(Role::Window, "0.3.0", Role::Core).unwrap();
    assert_eq!(core.version, env!("CARGO_PKG_VERSION"));
    let (reader, writer) = connection.into_parts();
    let peer = Peer::new(writer);
    let reading = Arc::clone(&peer);
    thread::spawn(move || reading.run(reader, Arc::new(Tool)));
    peer
}

#[test]
fn the_daemon_answers_users_it_lets_in_and_tells_the_others_why_not() {
    let root = std::env::temp_dir().join(format!("anixops-netd-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let options = Options {
        dir: root.join("data"),
        endpoint: root
            .join("run")
            .join("core.sock")
            .to_string_lossy()
            .into_owned(),
    };
    let access = Arc::new(Switch(AtomicBool::new(true)));
    let daemon = Daemon::start(&options, access.clone()).unwrap();
    {
        let daemon = Arc::clone(&daemon);
        thread::spawn(move || daemon.run());
    }

    // The data is closed to other users; the socket's folder is not.
    assert_eq!(mode(&options.dir), 0o700);
    assert_eq!(mode(&root.join("run")), 0o755);

    // A second daemon on the same socket does not start.
    assert!(Daemon::start(&options, access.clone()).is_err());

    let core = open(&options.endpoint);
    let snapshot = core.ask("runtime_snapshot", json!({}), SOON).unwrap();
    assert!(snapshot.get("connection").is_some(), "{snapshot}");
    assert_eq!(
        core.ask("list_subscriptions", json!({}), SOON),
        Ok(json!([]))
    );
    // What only a window may be asked is not a command of the core.
    assert!(core
        .ask("session.apply_proxy", json!({ "port": 1 }), SOON)
        .is_err());

    access.0.store(false, Ordering::SeqCst);
    let stranger = open(&options.endpoint);
    let error = stranger
        .ask("runtime_snapshot", json!({}), SOON)
        .unwrap_err();
    assert!(error.contains("may not control"), "{error}");
    // The user let in earlier keeps the connection it has.
    assert!(core.ask("runtime_snapshot", json!({}), SOON).is_ok());

    daemon.shutdown();
    assert!(!Path::new(&options.endpoint).exists());
    let _ = fs::remove_dir_all(&root);
}

/// Anyone who can reach the socket can open connections and say nothing; the daemon closes them after the
/// greeting time, holds each user to a number of them, and goes on answering (client-core-ipc-05).
#[test]
fn silent_connections_neither_pile_up_nor_stop_the_daemon() {
    let root = std::env::temp_dir().join(format!("anixops-netd-limits-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let options = Options {
        dir: root.join("data"),
        endpoint: root
            .join("run")
            .join("core.sock")
            .to_string_lossy()
            .into_owned(),
    };
    let limits = Limits {
        per_user: 4,
        total: 8,
        greeting: Duration::from_millis(1500),
        refused_idle: Duration::from_millis(1500),
    };
    let daemon =
        Daemon::start_with(&options, Arc::new(Switch(AtomicBool::new(true))), limits).unwrap();
    {
        let daemon = Arc::clone(&daemon);
        thread::spawn(move || daemon.run());
    }

    // More silent connections than this user may have: the ones past the limit are closed at once.
    let mut silent: Vec<_> = (0..limits.per_user + 2)
        .map(|_| connect(&options.endpoint).unwrap())
        .collect();
    let mut closed_at_once = 0;
    for stream in &mut silent {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut byte = [0_u8; 1];
        // The daemon's greeting arrives on a connection it serves; a closed one reads nothing.
        if matches!(stream.read(&mut byte), Ok(0)) {
            closed_at_once += 1;
        }
    }
    assert_eq!(closed_at_once, 2);

    // The silent ones are closed once the greeting time has passed, and a window is answered again.
    thread::sleep(Duration::from_secs(3));
    let core = open(&options.endpoint);
    assert!(core.ask("runtime_snapshot", json!({}), SOON).is_ok());
    for stream in &mut silent {
        stream.set_read_timeout(Some(SOON)).unwrap();
        let mut rest = Vec::new();
        // Whatever was sent before (the greeting), then the end.
        assert!(stream.read_to_end(&mut rest).is_ok());
    }

    // A window that has introduced itself may stay quiet longer than the greeting time.
    thread::sleep(Duration::from_secs(2));
    assert!(core.ask("runtime_snapshot", json!({}), SOON).is_ok());

    daemon.shutdown();
    let _ = fs::remove_dir_all(&root);
}
