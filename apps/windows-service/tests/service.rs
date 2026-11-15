//! The service with the real core, over its real pipe.

#![cfg(windows)]

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anixops_net_service::{data_dir, Options, Service};
use client_ipc::transport::connect;
use client_ipc::{Connection, Handler, Peer, Role};
use serde_json::{json, Value};

const SOON: Duration = Duration::from_secs(30);

/// A program that only sends commands.
struct Tool;

impl Handler for Tool {
    fn request(&self, command: &str, _args: Value) -> Result<Value, String> {
        Err(format!("{command} is not something this program does"))
    }
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
fn the_service_answers_a_window_and_keeps_its_data_closed() {
    let id = std::process::id();
    let root = std::env::temp_dir().join(format!("anixops-net-service-test-{id}"));
    let _ = std::fs::remove_dir_all(&root);
    let options = Options {
        dir: root.join("data"),
        endpoint: format!(r"\\.\pipe\anixops-net-service-test-{id}"),
        // An old folder without saved state: nothing to carry over, and that is not an error.
        legacy_dir: Some(root.join("old")),
        console: true,
        cleanup: None,
    };
    let service = Service::start(&options).unwrap();
    {
        let service = Arc::clone(&service);
        thread::spawn(move || service.run());
    }

    // A second service on the same pipe does not start.
    assert!(Service::start(&options).is_err());

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
    // The folder the core saves in is closed to ordinary users.
    let described = data_dir::describe(&options.dir).unwrap();
    assert!(data_dir::closed(&described), "{described}");

    service.shutdown();
}
