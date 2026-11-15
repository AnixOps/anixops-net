//! The link over a real socket, against a stand-in for the core.

#![cfg(target_os = "linux")]

use std::net::Shutdown;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use client_ipc::transport::{Listener, Stream};
use client_ipc::{Connection, Handler, Peer, Role};
use client_session::session::LocalSession;
use client_window::{ask, event, CoreLink, LinkOptions, LinkState, WindowEnd};
use serde_json::{json, Value};

const SOON: Duration = Duration::from_secs(10);
const CORE_VERSION: &str = "9.8.7";

/// A socket path no other test or program uses.
fn endpoint(name: &str) -> String {
    std::env::temp_dir()
        .join(format!(
            "anixops-net-link-{name}-{}.sock",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

fn options(endpoint: &str) -> LinkOptions {
    LinkOptions {
        endpoint: endpoint.to_string(),
        retry: Duration::from_millis(50),
        greeting: Duration::from_millis(500),
        ..LinkOptions::new("0.3.0")
    }
}

/// A window in a session without proxy settings or login entries.
fn window_end(events: impl Fn(&str, Value) + Send + Sync + 'static) -> Arc<dyn Handler> {
    let session = LocalSession::new(None, None, PathBuf::from("/usr/bin/anixops-net"));
    Arc::new(WindowEnd::new(None, Box::new(session), events))
}

/// Starts a link whose changes can be read one at a time.
fn link(endpoint: &str, handler: Arc<dyn Handler>) -> (CoreLink, Receiver<LinkState>) {
    let (changed, changes) = mpsc::channel();
    let link = CoreLink::start(options(endpoint), handler, move |state| {
        let _ = changed.send(state.clone());
    });
    (link, changes)
}

/// The core's side of the conversation: answers every command with what it was asked.
struct Echo;

impl Handler for Echo {
    fn request(&self, command: &str, args: Value) -> Result<Value, String> {
        Ok(json!({ "command": command, "args": args }))
    }
}

/// Takes the next window as the core would. The thread ends when the channel has closed.
fn admit(listener: &Listener) -> (Arc<Peer>, Stream, thread::JoinHandle<()>) {
    let stream = listener.accept().unwrap();
    let mut connection = Connection::new(stream.try_clone().unwrap(), stream.try_clone().unwrap());
    let window = connection
        .greet(Role::Core, CORE_VERSION, Role::Window)
        .unwrap();
    assert_eq!(window.version, "0.3.0");
    let (reader, writer) = connection.into_parts();
    let peer = Peer::new(writer);
    let reading = Arc::clone(&peer);
    let ended = thread::spawn(move || {
        let _ = reading.run(reader, Arc::new(Echo));
    });
    (peer, stream, ended)
}

fn connected() -> LinkState {
    LinkState::Connected {
        core_version: CORE_VERSION.to_string(),
    }
}

fn next(changes: &Receiver<LinkState>) -> LinkState {
    changes.recv_timeout(SOON).expect("the link did not change")
}

#[test]
fn the_link_finds_the_core_when_it_starts_and_again_after_it_went_away() {
    let endpoint = endpoint("again");
    let (events, heard) = mpsc::channel();
    let handler = window_end(move |name, payload| {
        let _ = events.send((name.to_string(), payload));
    });
    let (link, changes) = link(&endpoint, handler);

    // No core yet: the window is told once, however often the link tries.
    assert!(matches!(next(&changes), LinkState::Unavailable { .. }));
    assert!(matches!(link.state(), LinkState::Unavailable { .. }));
    assert_eq!(
        link.ask("snapshot", json!({}), SOON),
        Err("the core service is not connected".to_string())
    );
    thread::sleep(Duration::from_millis(200));
    assert!(changes.try_recv().is_err());

    // The core starts.
    let listener = Listener::bind(&endpoint).unwrap();
    let (core, stream, ended) = admit(&listener);
    assert_eq!(next(&changes), connected());
    assert_eq!(link.state(), connected());

    // Both ways: the window asks the core, the core asks the window and tells it things.
    assert_eq!(
        link.ask("connect", json!({ "node": 2 }), SOON),
        Ok(json!({ "command": "connect", "args": { "node": 2 } }))
    );
    assert_eq!(
        core.ask(ask::DESCRIBE, json!({}), SOON),
        Ok(json!({ "desktop": null }))
    );
    core.notify(event::NOTICE, json!({ "title": "Disconnected" }))
        .unwrap();
    assert_eq!(
        heard.recv_timeout(SOON).unwrap(),
        (
            event::NOTICE.to_string(),
            json!({ "title": "Disconnected" })
        )
    );

    // The core goes away, as when it is upgraded.
    stream.shutdown(Shutdown::Both).unwrap();
    ended.join().unwrap();
    drop(listener);
    assert!(matches!(next(&changes), LinkState::Unavailable { .. }));
    assert!(link.ask("snapshot", json!({}), SOON).is_err());

    // And comes back: the link finds it without being told.
    let listener = Listener::bind(&endpoint).unwrap();
    let (core, _stream, ended) = admit(&listener);
    assert_eq!(next(&changes), connected());
    assert_eq!(
        core.ask(ask::STARTS_AFTER_LOGIN, json!({}), SOON),
        Ok(json!(false))
    );

    // Dropping the link closes the channel and tells the window nothing more.
    drop(link);
    ended.join().unwrap();
    assert!(changes.recv_timeout(SOON).is_err());
}

#[test]
fn a_program_that_is_not_the_core_is_not_connected_to() {
    let endpoint = endpoint("other");
    let listener = Listener::bind(&endpoint).unwrap();
    let (link, changes) = link(&endpoint, window_end(|_, _| {}));

    // Another window's program on the core's socket.
    let stream = listener.accept().unwrap();
    let mut connection = Connection::new(stream.try_clone().unwrap(), stream);
    let _ = connection.greet(Role::Window, "0.3.0", Role::Window);
    assert!(matches!(next(&changes), LinkState::Unavailable { .. }));
    drop(connection);

    // One that accepts and then says nothing: the link gives up on it and tries again.
    let silent = listener.accept().unwrap();
    let again = listener.accept().unwrap();
    drop((silent, again));

    // The real core on the same socket is found afterwards.
    let (_core, _stream, _ended) = admit(&listener);
    assert_eq!(next(&changes), connected());
    drop(link);
}
