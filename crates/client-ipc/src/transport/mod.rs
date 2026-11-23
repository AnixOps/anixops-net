//! What carries the frames: a Unix socket on Linux, a named pipe on Windows.
//!
//! Both give the same three things: a [`Listener`] the core binds, [`connect`] for the window, and a
//! [`Stream`] that reads, writes and can be cloned so one thread reads while others write.
//!
//! Who may connect is decided where the channel is made, not by a password in the protocol:
//!
//! - Linux: the socket file's mode and the folder it is in. The core also reads the caller's user id from
//!   the kernel ([`peer_uid`]) to decide what that user may do.
//! - Windows: the pipe's access list lets in the system, administrators and users signed in at the machine,
//!   and refuses callers from the network. Signed-in users may connect but not serve instances of their own.
//!   The pipe is made with the "first instance" flag, so the core notices when another program has taken
//!   the name, and the window ([`connect`]) checks that the pipe it reached belongs to the system or the
//!   administrators, so a pipe another user made while the core was stopped is refused.

#[cfg(target_os = "linux")]
mod unix;
#[cfg(target_os = "linux")]
pub use unix::{connect, peer_uid, Listener, Stream, DEFAULT_ENDPOINT};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{connect, Listener, Stream, DEFAULT_ENDPOINT};

#[cfg(all(test, any(target_os = "linux", windows)))]
mod tests {
    use super::*;
    use crate::{Connection, Handler, Peer, Role};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    const SOON: Duration = Duration::from_secs(10);

    /// An endpoint no other test or program uses.
    fn endpoint(name: &str) -> String {
        let id = std::process::id();
        if cfg!(windows) {
            format!(r"\\.\pipe\anixops-net-test-{name}-{id}")
        } else {
            std::env::temp_dir()
                .join(format!("anixops-net-test-{name}-{id}.sock"))
                .to_string_lossy()
                .into_owned()
        }
    }

    struct Echo;

    impl Handler for Echo {
        fn request(&self, command: &str, args: Value) -> Result<Value, String> {
            Ok(json!({ "command": command, "args": args }))
        }
    }

    /// Greets as `role` and then answers and asks over `stream`.
    fn join(stream: Stream, role: Role, expected: Role) -> Arc<Peer> {
        let mut connection = Connection::new(stream.try_clone().unwrap(), stream);
        connection.greet(role, "0.3.0", expected).unwrap();
        let (reader, writer) = connection.into_parts();
        let peer = Peer::new(writer);
        let reading = Arc::clone(&peer);
        thread::spawn(move || reading.run(reader, Arc::new(Echo)));
        peer
    }

    #[test]
    fn a_window_talks_to_the_core_and_a_second_one_can_follow() {
        let endpoint = endpoint("talk");
        let listener = Listener::bind(&endpoint).unwrap();
        let core = thread::spawn(move || {
            // Two windows, one after the other, as when the tray is restarted.
            for _ in 0..2 {
                let stream = listener.accept().unwrap();
                let window = join(stream, Role::Core, Role::Window);
                assert_eq!(
                    window.ask("apply_proxy", json!({ "port": 7890 }), SOON),
                    Ok(json!({ "command": "apply_proxy", "args": { "port": 7890 } }))
                );
            }
        });
        for number in 0..2 {
            let stream = connect(&endpoint).unwrap();
            let core = join(stream, Role::Window, Role::Core);
            assert_eq!(
                core.ask("snapshot", json!({ "number": number }), SOON),
                Ok(json!({ "command": "snapshot", "args": { "number": number } }))
            );
            // A message far larger than a pipe's buffer arrives whole.
            let large = "x".repeat(300_000);
            assert_eq!(
                core.ask("echo", json!(large), SOON),
                Ok(json!({ "command": "echo", "args": large }))
            );
        }
        core.join().unwrap();
    }

    /// Greets as the core; `None` when the other end went away first.
    fn try_join(stream: Stream) -> Option<Arc<Peer>> {
        let mut connection = Connection::new(stream.try_clone().ok()?, stream);
        connection.greet(Role::Core, "0.3.0", Role::Window).ok()?;
        let (reader, writer) = connection.into_parts();
        let peer = Peer::new(writer);
        let reading = Arc::clone(&peer);
        thread::spawn(move || reading.run(reader, Arc::new(Echo)));
        Some(peer)
    }

    /// A window that opens the channel and closes it again before the core accepts it is no failure of the
    /// listener (client-core-ipc-04): on Windows its pipe instance is left closing, which `accept` used to
    /// report as an error that stopped the service.
    #[test]
    fn a_window_that_comes_and_goes_does_not_stop_the_core() {
        let endpoint = endpoint("probe");
        let listener = Listener::bind(&endpoint).unwrap();
        // Before the core waits for anyone.
        drop(connect(&endpoint).unwrap());
        let core = thread::spawn(move || {
            let mut served = 0;
            while served < 2 {
                // Never an error: a probe either gives a stream that closes at once or none at all.
                let stream = listener.accept().unwrap();
                let Some(window) = try_join(stream) else {
                    continue;
                };
                assert_eq!(
                    window.ask("apply_proxy", json!({ "port": 7890 }), SOON),
                    Ok(json!({ "command": "apply_proxy", "args": { "port": 7890 } }))
                );
                served += 1;
            }
        });
        for number in 0..2 {
            let stream = connect(&endpoint).unwrap();
            let core = join(stream, Role::Window, Role::Core);
            assert_eq!(
                core.ask("snapshot", json!({ "number": number }), SOON),
                Ok(json!({ "command": "snapshot", "args": { "number": number } }))
            );
            drop(core);
            // Between two accepts, while the core is busy with the window before.
            for _ in 0..20 {
                if let Ok(probe) = connect(&endpoint) {
                    drop(probe);
                }
            }
        }
        core.join().unwrap();
    }

    #[test]
    fn an_endpoint_in_use_is_not_taken_over() {
        let endpoint = endpoint("taken");
        let _listener = Listener::bind(&endpoint).unwrap();
        assert!(Listener::bind(&endpoint).is_err());
    }

    #[test]
    fn nothing_listening_is_an_error_for_the_window() {
        assert!(connect(&endpoint("nobody")).is_err());
    }
}
