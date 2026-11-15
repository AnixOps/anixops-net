//! The core and a window over a real channel (loopback TCP stands in for the pipe or socket).

use std::io;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use app_notifications::Notice;
use client_core::backend::session::UserSession;
use client_core::backend::system_proxy::{Desktop, ProxyBackup};
use client_ipc::{Connection, Handler, Peer, Role};
use client_service::{ask, event, Commands, WindowEnd, Windows};
use serde_json::{json, Value};

const SOON: Duration = Duration::from_secs(10);

#[cfg(windows)]
const DESKTOP: Desktop = Desktop::Windows;
#[cfg(not(windows))]
const DESKTOP: Desktop = Desktop::Gnome;

macro_rules! fixture {
    ($name:literal) => {
        serde_json::from_str::<Value>(include_str!(concat!("../fixtures/", $name, ".json")))
            .unwrap()
    };
}

/// The samples for the system this test runs on.
fn described() -> Value {
    if cfg!(windows) {
        fixture!("describe-answer-windows")
    } else {
        fixture!("describe-answer-linux")
    }
}

fn applied() -> Value {
    if cfg!(windows) {
        fixture!("apply-proxy-answer-windows")
    } else {
        fixture!("apply-proxy-answer-linux")
    }
}

fn restore_request() -> Value {
    if cfg!(windows) {
        fixture!("restore-proxy-request-windows")
    } else {
        fixture!("restore-proxy-request-linux")
    }
}

fn backup() -> ProxyBackup {
    serde_json::from_value(applied()).unwrap()
}

fn pipe() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let near = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (far, _) = listener.accept().unwrap();
    (near, far)
}

/// Stands in for the core: answers every command with its own name and arguments.
struct Echo {
    arrived: Mutex<Sender<()>>,
}

impl Commands for Echo {
    fn invoke(&self, command: &str, args: Value) -> Result<Value, String> {
        Ok(json!({ "command": command, "args": args }))
    }

    fn session_arrived(&self) {
        let _ = self.arrived.lock().unwrap().send(());
    }
}

/// The service's side of a test: the windows, what the core is handed, and when a session arrives.
struct Service {
    windows: Arc<Windows>,
    commands: Arc<Echo>,
    arrived: Receiver<()>,
    session: Box<dyn UserSession>,
}

impl Service {
    fn new() -> Self {
        let windows = Windows::new();
        let (sender, arrived) = mpsc::channel();
        Self {
            session: windows.session(),
            windows,
            commands: Arc::new(Echo {
                arrived: Mutex::new(sender),
            }),
            arrived,
        }
    }

    /// Serves one window on a thread of its own, as the service does for each connection.
    fn serve(&self, stream: TcpStream) -> JoinHandle<io::Result<()>> {
        let windows = Arc::clone(&self.windows);
        let commands = Arc::clone(&self.commands);
        thread::spawn(move || windows.serve(commands, stream.try_clone().unwrap(), stream, "0.3.0"))
    }

    fn wait_for_session(&self) {
        self.arrived.recv_timeout(SOON).unwrap();
    }
}

/// A window on the other end of `stream`, answering with `handler`.
struct Window {
    core: Arc<Peer>,
    stream: TcpStream,
}

impl Window {
    fn open(stream: TcpStream, handler: Arc<dyn Handler>) -> Self {
        let mut connection =
            Connection::new(stream.try_clone().unwrap(), stream.try_clone().unwrap());
        connection.greet(Role::Window, "0.3.0", Role::Core).unwrap();
        let (reader, writer) = connection.into_parts();
        let core = Peer::new(writer);
        let reading = Arc::clone(&core);
        thread::spawn(move || reading.run(reader, handler));
        Self { core, stream }
    }

    fn close(&self) {
        self.stream.shutdown(Shutdown::Both).unwrap();
    }
}

/// A window that answers from the samples and writes down what it was asked and told.
struct Scripted {
    asked: Mutex<Sender<(String, Value)>>,
    told: Mutex<Sender<(String, Value)>>,
}

type Heard = Receiver<(String, Value)>;

fn scripted() -> (Arc<Scripted>, Heard, Heard) {
    let (asked, questions) = mpsc::channel();
    let (told, events) = mpsc::channel();
    let window = Scripted {
        asked: Mutex::new(asked),
        told: Mutex::new(told),
    };
    (Arc::new(window), questions, events)
}

impl Handler for Scripted {
    fn request(&self, command: &str, args: Value) -> Result<Value, String> {
        let _ = self.asked.lock().unwrap().send((command.to_string(), args));
        match command {
            ask::DESCRIBE => Ok(described()),
            ask::APPLY_PROXY => Ok(applied()),
            ask::RESTORE_PROXY
            | ask::SET_START_AFTER_LOGIN
            | ask::TRUST_ROOT
            | ask::UNTRUST_ROOT => Ok(Value::Null),
            ask::STARTS_AFTER_LOGIN | ask::ROOT_TRUSTED => Ok(json!(true)),
            _ => Err(format!("{command} is not something a window does")),
        }
    }

    fn event(&self, name: &str, payload: Value) {
        let _ = self.told.lock().unwrap().send((name.to_string(), payload));
    }
}

fn next(heard: &Heard) -> (String, Value) {
    heard.recv_timeout(SOON).unwrap()
}

#[test]
fn without_a_window_the_users_settings_are_out_of_reach() {
    let service = Service::new();
    assert_eq!(service.session.proxy_settings(), None);
    let error = service.session.apply_proxy(7890).unwrap_err();
    assert!(error.contains("window is not open"), "{error}");
    assert!(service.session.restore_proxy(&backup()).is_err());
    assert!(!service.session.starts_after_login());
    assert!(service.session.set_start_after_login(true).is_err());
    // Not trusted as far as anyone can tell, and the reason is given when the user tries to change it.
    assert_eq!(service.session.trusts_root("certificate"), Some(false));
    let error = service.session.trust_root("certificate").unwrap_err();
    assert!(error.contains("window is not open"), "{error}");
    assert!(service.session.untrust_root("certificate").is_err());
}

#[test]
fn the_core_asks_the_window_what_the_samples_say() {
    let service = Service::new();
    let (near, far) = pipe();
    service.serve(far);
    let (handler, questions, _) = scripted();
    let _window = Window::open(near, handler);
    service.wait_for_session();
    assert_eq!(next(&questions), (ask::DESCRIBE.to_string(), json!({})));

    assert_eq!(service.session.proxy_settings(), Some(DESKTOP.name()));
    assert_eq!(service.session.apply_proxy(7890), Ok(backup()));
    assert_eq!(
        next(&questions),
        (
            ask::APPLY_PROXY.to_string(),
            fixture!("apply-proxy-request")
        )
    );
    assert_eq!(service.session.restore_proxy(&backup()), Ok(()));
    assert_eq!(
        next(&questions),
        (ask::RESTORE_PROXY.to_string(), restore_request())
    );
    assert!(service.session.starts_after_login());
    assert_eq!(
        next(&questions),
        (ask::STARTS_AFTER_LOGIN.to_string(), json!({}))
    );
    assert_eq!(service.session.set_start_after_login(true), Ok(()));
    assert_eq!(
        next(&questions),
        (
            ask::SET_START_AFTER_LOGIN.to_string(),
            fixture!("set-start-after-login-request")
        )
    );

    // Trust in the decryption root is the user's own, kept by the window in the user's store.
    let root = fixture!("root-request");
    let certificate = root["certificate"].as_str().unwrap();
    assert_eq!(service.session.trusts_root(certificate), Some(true));
    assert_eq!(
        next(&questions),
        (ask::ROOT_TRUSTED.to_string(), root.clone())
    );
    assert_eq!(service.session.trust_root(certificate), Ok(()));
    assert_eq!(
        next(&questions),
        (ask::TRUST_ROOT.to_string(), root.clone())
    );
    assert_eq!(service.session.untrust_root(certificate), Ok(()));
    assert_eq!(next(&questions), (ask::UNTRUST_ROOT.to_string(), root));
}

#[test]
fn a_session_without_proxy_settings_is_still_a_session() {
    struct Bare;
    impl Handler for Bare {
        fn request(&self, command: &str, _args: Value) -> Result<Value, String> {
            match command {
                ask::DESCRIBE => Ok(fixture!("describe-answer-unsupported")),
                ask::STARTS_AFTER_LOGIN => Ok(json!(true)),
                _ => Err("not here".to_string()),
            }
        }
    }
    let service = Service::new();
    let (near, far) = pipe();
    service.serve(far);
    let _window = Window::open(near, Arc::new(Bare));
    service.wait_for_session();
    assert_eq!(service.session.proxy_settings(), None);
    assert!(service.session.starts_after_login());
}

#[test]
fn the_core_answers_a_windows_commands_but_not_a_windows_own_questions() {
    let service = Service::new();
    let (near, far) = pipe();
    service.serve(far);
    let (handler, _, _) = scripted();
    let window = Window::open(near, handler);
    assert_eq!(
        window.core.ask("snapshot", json!({ "full": true }), SOON),
        Ok(json!({ "command": "snapshot", "args": { "full": true } }))
    );
    let error = window
        .core
        .ask(ask::APPLY_PROXY, json!({ "port": 1 }), SOON)
        .unwrap_err();
    assert!(error.contains("not a command of the core"), "{error}");
}

#[test]
fn the_window_opened_last_is_asked_and_the_earlier_one_again_once_it_has_left() {
    let service = Service::new();
    let (first_near, first_far) = pipe();
    service.serve(first_far);
    let (handler, first_questions, _) = scripted();
    let _first = Window::open(first_near, handler);
    service.wait_for_session();
    assert_eq!(next(&first_questions).0, ask::DESCRIBE);

    let (second_near, second_far) = pipe();
    let serving = service.serve(second_far);
    let (handler, second_questions, _) = scripted();
    let second = Window::open(second_near, handler);
    service.wait_for_session();
    assert_eq!(next(&second_questions).0, ask::DESCRIBE);

    assert!(service.session.starts_after_login());
    assert_eq!(next(&second_questions).0, ask::STARTS_AFTER_LOGIN);
    assert!(first_questions.try_recv().is_err());

    second.close();
    serving.join().unwrap().unwrap();
    assert!(service.session.starts_after_login());
    assert_eq!(next(&first_questions).0, ask::STARTS_AFTER_LOGIN);
}

#[test]
fn a_window_that_left_is_not_asked_any_more() {
    let service = Service::new();
    let (near, far) = pipe();
    let serving = service.serve(far);
    let (handler, _, _) = scripted();
    let window = Window::open(near, handler);
    service.wait_for_session();
    window.close();
    serving.join().unwrap().unwrap();
    assert_eq!(service.session.proxy_settings(), None);
    assert!(service.session.apply_proxy(7890).is_err());
}

#[test]
fn a_program_that_only_sends_commands_is_never_asked_for_a_session() {
    struct Tool {
        asked: Mutex<Sender<String>>,
    }
    impl Handler for Tool {
        fn request(&self, command: &str, _args: Value) -> Result<Value, String> {
            let _ = self.asked.lock().unwrap().send(command.to_string());
            Err(format!("{command} is not something this program does"))
        }
    }
    let service = Service::new();
    let (near, far) = pipe();
    let serving = service.serve(far);
    let (asked, questions) = mpsc::channel();
    let tool = Window::open(
        near,
        Arc::new(Tool {
            asked: Mutex::new(asked),
        }),
    );
    assert_eq!(questions.recv_timeout(SOON).unwrap(), ask::DESCRIBE);
    assert_eq!(
        tool.core.ask("snapshot", json!({}), SOON),
        Ok(json!({ "command": "snapshot", "args": {} }))
    );
    tool.close();
    serving.join().unwrap().unwrap();
    // Serving waits for the question to be settled, so by now the tool would have been kept if ever.
    assert!(service.arrived.try_recv().is_err());
    assert!(questions.try_recv().is_err());
}

#[test]
fn notifications_go_to_the_window() {
    let service = Service::new();
    let platform = service.windows.platform();
    let (near, far) = pipe();
    service.serve(far);
    let (handler, _, events) = scripted();
    let _window = Window::open(near, handler);
    service.wait_for_session();
    platform.post_notice(&Notice {
        title: "Disconnected".to_string(),
        body: "The connection was lost".to_string(),
        open_url: None,
    });
    assert_eq!(
        next(&events),
        (event::NOTICE.to_string(), fixture!("notice-event"))
    );
}

/// The session a window runs in, written down instead of changed.
#[derive(Default)]
struct Recorded {
    login: Mutex<bool>,
    restored: Mutex<Vec<ProxyBackup>>,
}

struct Recording(Arc<Recorded>);

impl UserSession for Recording {
    fn proxy_settings(&self) -> Option<&'static str> {
        Some(DESKTOP.name())
    }

    fn apply_proxy(&self, port: u16) -> Result<ProxyBackup, String> {
        if port == 7890 {
            Ok(backup())
        } else {
            Err(format!("port {port} is not the one expected"))
        }
    }

    fn restore_proxy(&self, backup: &ProxyBackup) -> Result<(), String> {
        self.0.restored.lock().unwrap().push(backup.clone());
        Ok(())
    }

    fn starts_after_login(&self) -> bool {
        *self.0.login.lock().unwrap()
    }

    fn set_start_after_login(&self, enabled: bool) -> Result<(), String> {
        *self.0.login.lock().unwrap() = enabled;
        Ok(())
    }
}

#[test]
fn a_rust_window_does_what_the_core_asks_in_its_own_session() {
    let service = Service::new();
    let platform = service.windows.platform();
    let (near, far) = pipe();
    service.serve(far);
    let recorded = Arc::new(Recorded::default());
    let (told, events) = mpsc::channel();
    let told = Mutex::new(told);
    let end = WindowEnd::new(
        Some(DESKTOP),
        Box::new(Recording(Arc::clone(&recorded))),
        move |name, payload| {
            let _ = told.lock().unwrap().send((name.to_string(), payload));
        },
    );
    let _window = Window::open(near, Arc::new(end));
    service.wait_for_session();

    assert_eq!(service.session.proxy_settings(), Some(DESKTOP.name()));
    assert_eq!(service.session.apply_proxy(7890), Ok(backup()));
    // What the window's session refuses reaches the core as the reason.
    let error = service.session.apply_proxy(1).unwrap_err();
    assert!(error.contains("port 1"), "{error}");
    assert_eq!(service.session.restore_proxy(&backup()), Ok(()));
    assert_eq!(*recorded.restored.lock().unwrap(), vec![backup()]);
    assert!(!service.session.starts_after_login());
    assert_eq!(service.session.set_start_after_login(true), Ok(()));
    assert!(service.session.starts_after_login());

    platform.post_notice(&Notice {
        title: "Disconnected".to_string(),
        body: "The connection was lost".to_string(),
        open_url: None,
    });
    assert_eq!(
        next(&events),
        (event::NOTICE.to_string(), fixture!("notice-event"))
    );
}

#[test]
fn a_window_refuses_what_it_does_not_know_and_arguments_that_are_wrong() {
    let end = WindowEnd::new(
        None,
        Box::new(Recording(Arc::new(Recorded::default()))),
        |_, _| {},
    );
    assert_eq!(
        end.request(ask::DESCRIBE, json!({})),
        Ok(fixture!("describe-answer-unsupported"))
    );
    assert!(end.request("connect", json!({})).is_err());
    let error = end.request(ask::APPLY_PROXY, json!({})).unwrap_err();
    assert!(error.contains("port"), "{error}");
    assert!(end
        .request(ask::APPLY_PROXY, json!({ "port": 70000 }))
        .is_err());
    assert!(end
        .request(ask::RESTORE_PROXY, json!({ "backup": "none" }))
        .is_err());
    assert!(end
        .request(ask::SET_START_AFTER_LOGIN, json!({ "enabled": "yes" }))
        .is_err());
}
