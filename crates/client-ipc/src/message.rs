//! The JSON in each frame.
//!
//! Every message is an object with a `type`:
//!
//! - `hello`: the first message from each side, with the protocol version, who is speaking and the
//!   program's version.
//! - `request`: asks the other side to do something and answer. The window asks the core to run a command
//!   (`client_core::Client::invoke`'s names and arguments). The core asks the window for what only the
//!   user's session can do, such as writing the user's proxy settings.
//! - `response`: the answer to the request with the same `id`. It carries `ok` with the result, or `error`
//!   with a sentence for the user; never both.
//! - `event`: something the core tells the window that needs no answer, such as a changed status.
//!
//! Ids are chosen by the side that asks and only have to be unique among its own unanswered requests; the
//! two sides' ids are unrelated.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Hello(Hello),
    Request(Request),
    Response(Response),
    Event(Event),
}

impl Message {
    pub fn request(id: u64, command: &str, args: Value) -> Self {
        Self::Request(Request {
            id,
            command: command.to_string(),
            args,
        })
    }

    /// The answer to request `id`: the result, or a sentence saying why there is none.
    pub fn answer(id: u64, result: Result<Value, String>) -> Self {
        Self::Response(match result {
            Ok(ok) => Response {
                id,
                ok,
                error: None,
            },
            Err(error) => Response {
                id,
                ok: Value::Null,
                error: Some(error),
            },
        })
    }

    pub fn event(name: &str, payload: Value) -> Self {
        Self::Event(Event {
            name: name.to_string(),
            payload,
        })
    }
}

/// Which program is speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The window or tray in the user's session.
    Window,
    /// The background service or daemon that runs the engine.
    Core,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub role: Role,
    /// The program's own version, for the log and the About page; it decides nothing.
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub command: String,
    /// The command's arguments, an object; `{}` for a command without any.
    #[serde(default = "empty_object")]
    pub args: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    /// The result; `null` for a command that returns nothing and whenever `error` is set.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub ok: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn into_result(self) -> Result<Value, String> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.ok),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The samples every implementation of the channel reads and writes the same way.
    const SAMPLES: [(&str, &str); 7] = [
        ("hello", include_str!("../fixtures/hello.json")),
        ("request", include_str!("../fixtures/request.json")),
        (
            "request-without-args",
            include_str!("../fixtures/request-without-args.json"),
        ),
        ("response-ok", include_str!("../fixtures/response-ok.json")),
        (
            "response-empty",
            include_str!("../fixtures/response-empty.json"),
        ),
        (
            "response-error",
            include_str!("../fixtures/response-error.json"),
        ),
        ("event", include_str!("../fixtures/event.json")),
    ];

    fn sample(name: &str) -> Message {
        let text = SAMPLES
            .iter()
            .find(|(sample, _)| *sample == name)
            .map(|(_, text)| *text)
            .unwrap();
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn samples_mean_what_they_say() {
        assert_eq!(
            sample("hello"),
            Message::Hello(Hello {
                protocol: 1,
                role: Role::Window,
                version: "0.3.0".to_string(),
            })
        );
        assert_eq!(
            sample("request"),
            Message::request(7, "select_node", json!({ "nodeId": "node-hk" }))
        );
        assert_eq!(
            sample("request-without-args"),
            Message::request(8, "connect", json!({}))
        );
        assert_eq!(
            sample("response-ok"),
            Message::answer(7, Ok(json!({ "message": "Node selected" })))
        );
        assert_eq!(
            sample("response-empty"),
            Message::answer(8, Ok(Value::Null))
        );
        assert_eq!(
            sample("response-error"),
            Message::answer(9, Err("the node is not in any subscription".to_string()))
        );
        assert_eq!(
            sample("event"),
            Message::event("snapshot_changed", json!({ "connection": "connected" }))
        );
    }

    /// Writing a sample's message gives the sample's JSON back, so the other implementation can compare
    /// what it reads with what it writes.
    #[test]
    fn samples_are_written_as_they_are_read() {
        for (name, text) in SAMPLES {
            let read: Value = serde_json::from_str(text).unwrap();
            let message: Message = serde_json::from_value(read.clone()).unwrap();
            let mut written = serde_json::to_value(&message).unwrap();
            if name == "request-without-args" {
                // Arguments left out are read as none and written as `{}`.
                assert_eq!(written["args"], json!({}));
                written.as_object_mut().unwrap().remove("args");
            }
            assert_eq!(written, read, "{name}");
        }
    }

    #[test]
    fn a_response_is_a_result_or_an_error() {
        let Message::Response(ok) = sample("response-ok") else {
            panic!("not a response");
        };
        assert_eq!(ok.into_result(), Ok(json!({ "message": "Node selected" })));
        let Message::Response(empty) = sample("response-empty") else {
            panic!("not a response");
        };
        assert_eq!(empty.into_result(), Ok(Value::Null));
        let Message::Response(error) = sample("response-error") else {
            panic!("not a response");
        };
        assert_eq!(
            error.into_result(),
            Err("the node is not in any subscription".to_string())
        );
    }

    #[test]
    fn messages_without_a_known_type_are_refused() {
        assert!(serde_json::from_str::<Message>(r#"{"type":"shutdown"}"#).is_err());
        assert!(serde_json::from_str::<Message>(r#"{"id":1,"command":"connect"}"#).is_err());
        assert!(
            serde_json::from_str::<Message>(r#"{"type":"request","command":"connect"}"#).is_err()
        );
    }
}
