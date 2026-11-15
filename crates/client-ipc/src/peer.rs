//! One side of an open channel: answers the other side's requests, asks its own and waits for the answers,
//! and passes events on.
//!
//! Both programs use this the same way. The core answers the window's commands and, while it does, may ask
//! the window for something only the user's session can do; so a request is answered on its own thread and
//! the reading loop keeps reading, or the answer to that question could never arrive.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::frame;
use crate::message::Message;

const CLOSED: &str = "the channel to the other program closed";
const NO_ANSWER: &str = "the other program did not answer in time";
const FAILED: &str = "the command stopped unexpectedly";

/// What this side does with what the other side sends.
pub trait Handler: Send + Sync + 'static {
    /// Answers a request: the result, or a sentence for the user saying why there is none.
    fn request(&self, command: &str, args: Value) -> Result<Value, String>;

    /// Takes an event. Events arrive in the order they were sent, one at a time.
    fn event(&self, name: &str, payload: Value) {
        let _ = (name, payload);
    }
}

type Waiting = HashMap<u64, Sender<Result<Value, String>>>;

/// The sending half of a channel, shared by every thread that speaks on it.
pub struct Peer {
    writer: Mutex<Box<dyn Write + Send>>,
    /// Requests this side sent that have no answer yet; `None` once the channel has closed.
    waiting: Mutex<Option<Waiting>>,
    next_id: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Peer {
    /// A peer that writes to `writer`. Call [`Peer::run`] with the matching reader to hear the other side.
    pub fn new(writer: impl Write + Send + 'static) -> Arc<Self> {
        Arc::new(Self {
            writer: Mutex::new(Box::new(writer)),
            waiting: Mutex::new(Some(HashMap::new())),
            next_id: AtomicU64::new(1),
        })
    }

    fn send(&self, message: &Message) -> io::Result<()> {
        let payload = serde_json::to_vec(message).map_err(io::Error::other)?;
        frame::write(&mut *lock(&self.writer), &payload)
    }

    /// Tells the other side something that needs no answer.
    pub fn notify(&self, name: &str, payload: Value) -> io::Result<()> {
        self.send(&Message::event(name, payload))
    }

    /// Asks the other side to do `command` and waits up to `patience` for its answer.
    pub fn ask(&self, command: &str, args: Value, patience: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = mpsc::channel();
        match lock(&self.waiting).as_mut() {
            Some(waiting) => waiting.insert(id, answer),
            None => return Err(CLOSED.to_string()),
        };
        if let Err(error) = self.send(&Message::request(id, command, args)) {
            self.forget(id);
            return Err(format!("{CLOSED}: {error}"));
        }
        match answered.recv_timeout(patience) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                // An answer that still comes is dropped: nobody waits for it any more.
                self.forget(id);
                Err(NO_ANSWER.to_string())
            }
            Err(RecvTimeoutError::Disconnected) => Err(CLOSED.to_string()),
        }
    }

    fn forget(&self, id: u64) {
        if let Some(waiting) = lock(&self.waiting).as_mut() {
            waiting.remove(&id);
        }
    }

    fn answered(&self, id: u64, result: Result<Value, String>) {
        let waiter = lock(&self.waiting)
            .as_mut()
            .and_then(|waiting| waiting.remove(&id));
        if let Some(waiter) = waiter {
            let _ = waiter.send(result);
        }
    }

    /// Ends every wait for an answer; later asks fail at once.
    fn close(&self) {
        // Dropping the senders wakes the waiting threads.
        lock(&self.waiting).take();
    }

    /// Reads what the other side sends until the channel closes, which is `Ok`, or breaks, which is the
    /// error. Requests go to `handler` on a thread each; events go to it in order on this thread. When this
    /// returns, every ask still waiting has failed and no new one can be made.
    pub fn run(
        self: &Arc<Self>,
        mut reader: impl Read,
        handler: Arc<dyn Handler>,
    ) -> io::Result<()> {
        let outcome = self.read_until_closed(&mut reader, &handler);
        self.close();
        outcome
    }

    fn read_until_closed(
        self: &Arc<Self>,
        reader: &mut impl Read,
        handler: &Arc<dyn Handler>,
    ) -> io::Result<()> {
        while let Some(payload) = frame::read(reader)? {
            let message: Message = serde_json::from_slice(&payload)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            match message {
                Message::Request(request) => {
                    let peer = Arc::clone(self);
                    let handler = Arc::clone(handler);
                    thread::spawn(move || {
                        let result = panic::catch_unwind(AssertUnwindSafe(|| {
                            handler.request(&request.command, request.args)
                        }))
                        .unwrap_or_else(|_| Err(FAILED.to_string()));
                        // A channel that closed meanwhile has nobody left to tell.
                        let _ = peer.send(&Message::answer(request.id, result));
                    });
                }
                Message::Response(response) => {
                    let id = response.id;
                    self.answered(id, response.into_result());
                }
                Message::Event(event) => handler.event(&event.name, event.payload),
                Message::Hello(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the other side introduced itself twice",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::mpsc::Receiver;
    use std::thread::JoinHandle;

    const SOON: Duration = Duration::from_secs(10);

    /// Two connected streams, standing in for the two ends of a pipe.
    fn pipe() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let near = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (far, _) = listener.accept().unwrap();
        (near, far)
    }

    /// A side that answers `echo` with its arguments, fails `fail`, panics on `panic`, never answers `hang`
    /// in time, and records events.
    struct Echo {
        events: Mutex<Sender<(String, Value)>>,
    }

    impl Handler for Echo {
        fn request(&self, command: &str, args: Value) -> Result<Value, String> {
            match command {
                "echo" => Ok(args),
                "fail" => Err("no such node".to_string()),
                "panic" => panic!("a bug in a command"),
                "hang" => {
                    thread::sleep(Duration::from_millis(300));
                    Ok(Value::Null)
                }
                other => Err(format!("unknown command {other}")),
            }
        }

        fn event(&self, name: &str, payload: Value) {
            let _ = lock(&self.events).send((name.to_string(), payload));
        }
    }

    type Running = JoinHandle<io::Result<()>>;

    fn start(stream: TcpStream, handler: Arc<dyn Handler>) -> (Arc<Peer>, Running) {
        let peer = Peer::new(stream.try_clone().unwrap());
        let reading = Arc::clone(&peer);
        let running = thread::spawn(move || reading.run(stream, handler));
        (peer, running)
    }

    fn echo() -> (Arc<dyn Handler>, Receiver<(String, Value)>) {
        let (events, received) = mpsc::channel();
        (
            Arc::new(Echo {
                events: Mutex::new(events),
            }),
            received,
        )
    }

    #[test]
    fn a_request_gets_its_answer() {
        let (near, far) = pipe();
        let (window, _) = start(near, echo().0);
        let (_core, _) = start(far, echo().0);
        assert_eq!(
            window.ask("echo", json!({ "nodeId": "node-hk" }), SOON),
            Ok(json!({ "nodeId": "node-hk" }))
        );
        assert_eq!(
            window.ask("fail", json!({}), SOON),
            Err("no such node".to_string())
        );
    }

    #[test]
    fn answers_find_their_own_request() {
        let (near, far) = pipe();
        let (window, _) = start(near, echo().0);
        let (_core, _) = start(far, echo().0);
        let askers: Vec<_> = (0..8_usize)
            .map(|number| {
                let window = Arc::clone(&window);
                thread::spawn(move || window.ask("echo", json!({ "number": number }), SOON))
            })
            .collect();
        for (number, asker) in askers.into_iter().enumerate() {
            assert_eq!(asker.join().unwrap(), Ok(json!({ "number": number })));
        }
    }

    /// The core asks the window for something while it is still answering the window's own request.
    #[test]
    fn a_side_can_ask_while_it_answers() {
        struct Core(Mutex<Option<Arc<Peer>>>);
        impl Handler for Core {
            fn request(&self, command: &str, _args: Value) -> Result<Value, String> {
                assert_eq!(command, "connect");
                let window = lock(&self.0).clone().unwrap();
                let backup = window.ask("echo", json!({ "port": 7890 }), SOON)?;
                Ok(json!({ "proxy": backup }))
            }
        }

        let (near, far) = pipe();
        let (window, _) = start(near, echo().0);
        let core = Arc::new(Core(Mutex::new(None)));
        let (core_peer, _) = start(far, core.clone());
        *lock(&core.0) = Some(core_peer);
        assert_eq!(
            window.ask("connect", json!({}), SOON),
            Ok(json!({ "proxy": { "port": 7890 } }))
        );
    }

    #[test]
    fn events_arrive_in_order() {
        let (near, far) = pipe();
        let (handler, received) = echo();
        let (_window, _) = start(near, handler);
        let (core, _) = start(far, echo().0);
        for number in 0..20 {
            core.notify("traffic", json!({ "number": number })).unwrap();
        }
        for number in 0..20 {
            assert_eq!(
                received.recv_timeout(SOON).unwrap(),
                ("traffic".to_string(), json!({ "number": number }))
            );
        }
    }

    #[test]
    fn a_command_that_panics_still_gets_an_answer() {
        let (near, far) = pipe();
        let (window, _) = start(near, echo().0);
        let (_core, _) = start(far, echo().0);
        assert_eq!(
            window.ask("panic", json!({}), SOON),
            Err(FAILED.to_string())
        );
        // The channel is still usable.
        assert_eq!(window.ask("echo", json!(1), SOON), Ok(json!(1)));
    }

    #[test]
    fn an_answer_that_takes_too_long_is_given_up_on() {
        let (near, far) = pipe();
        let (window, _) = start(near, echo().0);
        let (_core, _) = start(far, echo().0);
        assert_eq!(
            window.ask("hang", json!({}), Duration::from_millis(20)),
            Err(NO_ANSWER.to_string())
        );
        // The late answer is dropped and later requests are answered as usual.
        thread::sleep(Duration::from_millis(400));
        assert_eq!(window.ask("echo", json!(2), SOON), Ok(json!(2)));
    }

    #[test]
    fn closing_the_channel_ends_the_wait_and_the_reading() {
        let (near, far) = pipe();
        let (window, running) = start(near, echo().0);
        let asking = {
            let window = Arc::clone(&window);
            thread::spawn(move || window.ask("echo", json!({}), SOON))
        };
        // The other program reads the request and goes away without answering.
        let mut far = far;
        assert!(frame::read(&mut far).unwrap().is_some());
        far.shutdown(Shutdown::Both).unwrap();
        drop(far);

        assert_eq!(asking.join().unwrap(), Err(CLOSED.to_string()));
        assert!(running.join().unwrap().is_ok());
        assert_eq!(window.ask("echo", json!({}), SOON), Err(CLOSED.to_string()));
    }

    #[test]
    fn something_that_is_not_a_message_breaks_the_channel() {
        let (near, mut far) = pipe();
        let (_window, running) = start(near, echo().0);
        frame::write(&mut far, b"not json").unwrap();
        let error = running.join().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
