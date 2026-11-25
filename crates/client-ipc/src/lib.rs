//! The local channel between a client's window and its background core.
//!
//! The native desktop clients run the core (`client-core`) in a system service or daemon and the window in
//! the user's session (`docs/architecture/desktop-native.md`). The two talk over a named pipe on Windows and
//! a Unix socket on Linux. This crate is what goes over that channel, whatever carries it:
//!
//! - [`frame`]: each message is one frame, a four-byte big-endian length followed by that many bytes of
//!   UTF-8 JSON.
//! - [`message`]: the JSON itself. Either side sends requests and answers the other's; the core also sends
//!   events that expect no answer.
//! - [`Connection`]: frames and messages over any reader and writer, used for the greeting.
//! - [`transport`]: the Unix socket and the named pipe that carry the frames.
//! - [`peer`]: after the greeting, each side answers the other's requests, asks its own and takes events.
//!
//! The window is written in another language on Windows, so the wire format is fixed by the samples under
//! `fixtures/`, which the tests here read and which the other implementation checks too. A change to the
//! format changes [`PROTOCOL`].

pub mod frame;
pub mod message;
pub mod peer;
pub mod transport;

use std::io::{self, Read, Write};

pub use message::{Hello, Message, Role};
pub use peer::{Handler, Peer};

/// The version of the wire format. The two sides exchange it first and stop when they differ.
pub const PROTOCOL: u32 = 1;

/// One end of the channel.
pub struct Connection<R, W> {
    reader: R,
    writer: W,
}

impl<R: Read, W: Write> Connection<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    /// Sends one message as one frame.
    pub fn send(&mut self, message: &Message) -> io::Result<()> {
        let payload = serde_json::to_vec(message).map_err(io::Error::other)?;
        frame::write(&mut self.writer, &payload)
    }

    /// The next message; `None` once the other side has closed the channel between two frames. A frame that
    /// is not a message of this protocol is an error: the channel cannot be trusted after it.
    pub fn receive(&mut self) -> io::Result<Option<Message>> {
        let Some(payload) = frame::read(&mut self.reader)? else {
            return Ok(None);
        };
        serde_json::from_slice(&payload)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// The reader and the writer back, once the greeting is done, for [`Peer`].
    pub fn into_parts(self) -> (R, W) {
        (self.reader, self.writer)
    }

    /// Opens the conversation: says who this side is and checks that the other side, which must be
    /// `expected`, speaks the same protocol. Both sides call this first; neither waits for the other to
    /// speak, so the order does not matter.
    pub fn greet(&mut self, role: Role, version: &str, expected: Role) -> io::Result<Hello> {
        self.greet_one_of(role, version, &[expected])
    }

    /// [`Connection::greet`] for the side that serves more than one kind of program (the core serves windows
    /// and AI tools): the other side must be one of `expected`, and the greeting says which it is.
    pub fn greet_one_of(
        &mut self,
        role: Role,
        version: &str,
        expected: &[Role],
    ) -> io::Result<Hello> {
        self.send(&Message::Hello(Hello {
            protocol: PROTOCOL,
            role,
            version: version.to_string(),
        }))?;
        match self.receive()? {
            Some(Message::Hello(hello)) if hello.protocol != PROTOCOL => Err(invalid(format!(
                "the other side speaks protocol {}, this side {PROTOCOL}",
                hello.protocol
            ))),
            Some(Message::Hello(hello)) if !expected.contains(&hello.role) => Err(invalid(
                "the other side is not the program expected on this channel",
            )),
            Some(Message::Hello(hello)) => Ok(hello),
            Some(_) => Err(invalid("the other side did not introduce itself first")),
            None => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the channel closed before the other side introduced itself",
            )),
        }
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    /// What one side wrote, as the other side's reader.
    fn written(messages: &[Message]) -> Cursor<Vec<u8>> {
        let mut connection = Connection::new(io::empty(), Vec::new());
        for message in messages {
            connection.send(message).unwrap();
        }
        Cursor::new(connection.writer)
    }

    fn hello(role: Role, protocol: u32) -> Message {
        Message::Hello(Hello {
            protocol,
            role,
            version: "0.3.0".to_string(),
        })
    }

    #[test]
    fn messages_arrive_in_order_and_the_end_is_clean() {
        let sent = [
            Message::request(1, "snapshot", json!({})),
            Message::event("snapshot_changed", json!({ "connection": "connected" })),
            Message::answer(1, Ok(json!({ "connection": "connected" }))),
        ];
        let mut connection = Connection::new(written(&sent), Vec::new());
        for message in &sent {
            assert_eq!(connection.receive().unwrap().as_ref(), Some(message));
        }
        assert_eq!(connection.receive().unwrap(), None);
    }

    #[test]
    fn both_sides_greet_without_waiting() {
        let mut window = Connection::new(written(&[hello(Role::Core, PROTOCOL)]), Vec::new());
        let core = window.greet(Role::Window, "0.3.0", Role::Core).unwrap();
        assert_eq!(core.role, Role::Core);
        assert_eq!(core.version, "0.3.0");
        // What the window wrote is what the core reads as its greeting.
        let mut core_side = Connection::new(Cursor::new(window.writer), Vec::new());
        assert_eq!(
            core_side.receive().unwrap(),
            Some(hello(Role::Window, PROTOCOL))
        );
    }

    #[test]
    fn another_protocol_is_refused() {
        let mut window = Connection::new(written(&[hello(Role::Core, PROTOCOL + 1)]), Vec::new());
        let error = window.greet(Role::Window, "0.3.0", Role::Core).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn the_wrong_program_is_refused() {
        // Another window on the core's end of the channel.
        let mut window = Connection::new(written(&[hello(Role::Window, PROTOCOL)]), Vec::new());
        assert!(window.greet(Role::Window, "0.3.0", Role::Core).is_err());
    }

    #[test]
    fn the_core_learns_which_program_it_serves() {
        let mut core = Connection::new(written(&[hello(Role::Agent, PROTOCOL)]), Vec::new());
        let agent = core
            .greet_one_of(Role::Core, "0.3.0", &[Role::Window, Role::Agent])
            .unwrap();
        assert_eq!(agent.role, Role::Agent);
        // A core that serves only windows refuses an AI tool.
        let mut core = Connection::new(written(&[hello(Role::Agent, PROTOCOL)]), Vec::new());
        assert!(core.greet(Role::Core, "0.3.0", Role::Window).is_err());
    }

    #[test]
    fn a_side_that_skips_the_greeting_is_refused() {
        let mut window = Connection::new(
            written(&[Message::request(1, "connect", json!({}))]),
            Vec::new(),
        );
        assert!(window.greet(Role::Window, "0.3.0", Role::Core).is_err());
        let mut window = Connection::new(Cursor::new(Vec::new()), Vec::new());
        let error = window.greet(Role::Window, "0.3.0", Role::Core).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn a_frame_that_is_not_a_message_is_an_error() {
        let mut bytes = Vec::new();
        frame::write(&mut bytes, br#"{"type":"unknown"}"#).unwrap();
        let mut connection = Connection::new(Cursor::new(bytes), Vec::new());
        let error = connection.receive().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
