//! The native Android app's bridge to the client service (`client_core::client`). Kotlin calls three JNI entry
//! points on `io.anixops.net.core.NativeCore`:
//!
//! - `nativeInit(dataDir, bridge)` opens the one [`Client`] of the process and starts its background work;
//! - `nativeInvoke(command, argsJson)` answers one interface command, as `{"ok": answer}` or `{"error": "..."}`;
//! - `nativeShutdown()` puts things back when the process ends.
//!
//! What only Android can do (the VPN, Downloads, the Keystore, the current network, notifications) the client
//! asks of Kotlin through `bridge.call(method, argsJson)`, which answers in the same envelope ([`BridgePlatform`]).
//! Command names and JSON shapes are the shared interface's (`bridge.ts`), so the Tauri windows and this app
//! read one contract.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use app_notifications::Notice;
use client_core::client::{Client, Config, Network, Platform, TunnelInterface};
use serde_json::{json, Value};

/// The process's client, opened by the first `nativeInit` (the activity or the VPN service, whichever starts
/// first).
static CLIENT: OnceLock<Arc<Client>> = OnceLock::new();

/// How the client reaches Kotlin: `method` with its JSON arguments, answered in the `{"ok"}` / `{"error"}`
/// envelope. The JNI implementation is [`jni_bridge::JniBridge`]; tests use a fake.
pub trait Bridge: Send + Sync + 'static {
    fn call(&self, method: &str, args: &str) -> Result<String, String>;
}

/// The client's [`Platform`] on Android, every capability through the [`Bridge`].
pub struct BridgePlatform<B: Bridge> {
    bridge: B,
}

impl<B: Bridge> BridgePlatform<B> {
    pub fn new(bridge: B) -> Self {
        Self { bridge }
    }

    fn request(&self, method: &str, args: Value) -> Result<Value, String> {
        read_answer(&self.bridge.call(method, &args.to_string())?)
    }

    fn string_field(answer: &Value, field: &str, method: &str) -> Result<String, String> {
        answer
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("{method} answered without {field}"))
    }
}

impl<B: Bridge> Platform for BridgePlatform<B> {
    fn has_system_vpn(&self) -> bool {
        true
    }

    fn open_tunnel(&self, interface: &TunnelInterface) -> Result<i32, String> {
        let mut args = serde_json::to_value(interface).map_err(|error| error.to_string())?;
        args["session"] = json!("AnixOps Net");
        let answer = self.request("openTunnel", args)?;
        answer
            .get("fd")
            .and_then(Value::as_i64)
            .and_then(|fd| i32::try_from(fd).ok())
            .filter(|fd| *fd >= 0)
            .ok_or_else(|| "openTunnel answered without a descriptor".to_string())
    }

    fn close_tunnel(&self) {
        let _ = self.request("closeTunnel", json!({}));
    }

    fn close_tunnel_for_rules(&self) {
        let _ = self.request("closeTunnel", json!({"waitForNetwork": true}));
    }

    fn save_to_downloads(&self, name: &str, contents: &str) -> Result<String, String> {
        let answer = self.request(
            "saveToDownloads",
            json!({"name": name, "contents": contents, "base64": false, "mimeType": mime_type(name)}),
        )?;
        Self::string_field(&answer, "location", "saveToDownloads")
    }

    fn replace_in_downloads(&self, name: &str, contents: &str) -> Result<String, String> {
        let answer = self.request(
            "saveToDownloads",
            json!({
                "name": name,
                "contents": contents,
                "base64": false,
                "mimeType": mime_type(name),
                "replace": true,
            }),
        )?;
        Self::string_field(&answer, "location", "saveToDownloads")
    }

    fn save_base64_to_downloads(
        &self,
        name: &str,
        mime_type: &str,
        data_base64: &str,
    ) -> Result<String, String> {
        let answer = self.request(
            "saveToDownloads",
            json!({"name": name, "contents": data_base64, "base64": true, "mimeType": mime_type}),
        )?;
        Self::string_field(&answer, "location", "saveToDownloads")
    }

    fn capture_session_key(&self) -> Result<String, String> {
        let answer = self.request("captureSessionKey", json!({}))?;
        Self::string_field(&answer, "keyBase64", "captureSessionKey")
    }

    fn current_network(&self, ask: bool) -> Option<Network> {
        let answer = self.request("currentNetwork", json!({"ask": ask})).ok()?;
        let kind = answer.get("kind")?.as_str()?;
        let ssid = answer
            .get("ssid")
            .and_then(Value::as_str)
            .map(str::to_string);
        // The app may read the name, but the system hides it for now (the app is in the background, or
        // location is off): not the same as a Wi-Fi without a known name.
        let withheld = answer.get("ssidWithheld").and_then(Value::as_bool) == Some(true);
        if kind == "wifi" && withheld && ssid.as_deref().unwrap_or_default().is_empty() {
            return Some(Network::WifiWithheld);
        }
        Some(Network::from_kind(kind, ssid))
    }

    fn post_notice(&self, notice: &Notice) {
        let _ = self.request(
            "postNotice",
            json!({
                "kind": notice.kind.id(),
                "title": notice.title,
                "body": notice.body,
                "openUrl": notice.open_url,
            }),
        );
    }
}

/// The type Android files a text file under in Downloads.
fn mime_type(name: &str) -> &'static str {
    if name.ends_with(".crt") {
        "application/x-x509-ca-cert"
    } else {
        "text/plain"
    }
}

/// `{"ok": answer}` or `{"error": message}`.
pub fn answer(result: Result<Value, String>) -> String {
    match result {
        Ok(value) => json!({ "ok": value }),
        Err(error) => json!({ "error": error }),
    }
    .to_string()
}

/// Reads an `{"ok"}` / `{"error"}` envelope.
pub fn read_answer(text: &str) -> Result<Value, String> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|error| format!("unreadable answer: {error}"))?;
    if let Some(error) = value.get("error") {
        return Err(error
            .as_str()
            .map_or_else(|| error.to_string(), str::to_string));
    }
    match value.get_mut("ok") {
        Some(ok) => Ok(ok.take()),
        None => Err("answer has neither ok nor error".to_string()),
    }
}

/// Opens the process's client once; a later call keeps the first one.
pub fn init(data_dir: PathBuf, platform: Arc<dyn Platform>) {
    CLIENT.get_or_init(|| {
        let executable = std::env::current_exe().unwrap_or_else(|_| data_dir.clone());
        let client = Client::open(
            Config {
                dir: data_dir,
                executable,
            },
            platform,
        );
        client.start_background();
        client
    });
}

/// Answers one command in the envelope; a panic becomes an error instead of unwinding into the JVM.
pub fn invoke(command: &str, args: &str) -> String {
    let Some(client) = CLIENT.get() else {
        return answer(Err("the client is not open yet".to_string()));
    };
    let args = if args.trim().is_empty() {
        Ok(Value::Null)
    } else {
        serde_json::from_str(args).map_err(|error| format!("invalid arguments: {error}"))
    };
    let result = catch_unwind(AssertUnwindSafe(|| client.invoke(command, args?)))
        .unwrap_or_else(|_| Err(format!("{command} failed unexpectedly")));
    answer(result)
}

/// Puts things back when the process ends.
pub fn shutdown() {
    if let Some(client) = CLIENT.get() {
        client.shutdown();
    }
}

mod jni_bridge {
    use std::path::PathBuf;
    use std::sync::Arc;

    use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
    use jni::sys::jstring;
    use jni::{JNIEnv, JavaVM};

    use super::{Bridge, BridgePlatform};

    /// Calls `String call(String method, String args)` on the Kotlin `PlatformBridge` object.
    pub struct JniBridge {
        vm: JavaVM,
        target: GlobalRef,
    }

    impl Bridge for JniBridge {
        fn call(&self, method: &str, args: &str) -> Result<String, String> {
            let mut env = self
                .vm
                .attach_current_thread()
                .map_err(|error| format!("the JVM is not reachable: {error}"))?;
            let method = env.new_string(method).map_err(|error| error.to_string())?;
            let args = env.new_string(args).map_err(|error| error.to_string())?;
            let answer = env.call_method(
                &self.target,
                "call",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&method), JValue::Object(&args)],
            );
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
                return Err("the Android side threw while answering".to_string());
            }
            let answer = answer
                .and_then(|value| value.l())
                .map_err(|error| error.to_string())?;
            let answer = JString::from(answer);
            let text: String = env
                .get_string(&answer)
                .map_err(|error| error.to_string())?
                .into();
            Ok(text)
        }
    }

    fn string(env: &mut JNIEnv, value: &JString) -> Option<String> {
        env.get_string(value).ok().map(Into::into)
    }

    fn java_string(env: &mut JNIEnv, value: &str) -> jstring {
        env.new_string(value)
            .map(JString::into_raw)
            .unwrap_or(std::ptr::null_mut())
    }

    /// `String? nativeInit(String dataDir, PlatformBridge bridge)`: null when the client is open, else why not.
    #[no_mangle]
    pub extern "system" fn Java_io_anixops_net_core_NativeCore_nativeInit<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        data_dir: JString<'local>,
        bridge: JObject<'local>,
    ) -> jstring {
        let Some(dir) = string(&mut env, &data_dir) else {
            return java_string(&mut env, "the data directory is not readable");
        };
        let vm = match env.get_java_vm() {
            Ok(vm) => vm,
            Err(error) => return java_string(&mut env, &error.to_string()),
        };
        let target = match env.new_global_ref(bridge) {
            Ok(target) => target,
            Err(error) => return java_string(&mut env, &error.to_string()),
        };
        let platform = BridgePlatform::new(JniBridge { vm, target });
        let open = std::panic::AssertUnwindSafe(move || {
            super::init(PathBuf::from(dir), Arc::new(platform))
        });
        match std::panic::catch_unwind(open) {
            Ok(()) => std::ptr::null_mut(),
            Err(_) => java_string(&mut env, "the client could not be opened"),
        }
    }

    /// `String nativeInvoke(String command, String argsJson)`: the answer envelope.
    #[no_mangle]
    pub extern "system" fn Java_io_anixops_net_core_NativeCore_nativeInvoke<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        command: JString<'local>,
        args: JString<'local>,
    ) -> jstring {
        let answer = match (string(&mut env, &command), string(&mut env, &args)) {
            (Some(command), Some(args)) => super::invoke(&command, &args),
            _ => super::answer(Err("the command is not readable".to_string())),
        };
        java_string(&mut env, &answer)
    }

    /// `void nativeShutdown()`.
    #[no_mangle]
    pub extern "system" fn Java_io_anixops_net_core_NativeCore_nativeShutdown<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) {
        let _ = std::panic::catch_unwind(super::shutdown);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Records what the client asked and answers from a table.
    struct Fake {
        calls: Mutex<Vec<(String, Value)>>,
        answers: Vec<(&'static str, &'static str)>,
    }

    impl Bridge for Arc<Fake> {
        fn call(&self, method: &str, args: &str) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push((method.to_string(), serde_json::from_str(args).unwrap()));
            self.answers
                .iter()
                .find(|(name, _)| *name == method)
                .map(|(_, answer)| answer.to_string())
                .ok_or_else(|| format!("no answer for {method}"))
        }
    }

    fn platform(
        answers: Vec<(&'static str, &'static str)>,
    ) -> (Arc<Fake>, BridgePlatform<Arc<Fake>>) {
        let fake = Arc::new(Fake {
            calls: Mutex::default(),
            answers,
        });
        (fake.clone(), BridgePlatform::new(fake))
    }

    #[test]
    fn envelopes_round_trip() {
        assert_eq!(answer(Ok(json!({"a": 1}))), r#"{"ok":{"a":1}}"#);
        assert_eq!(answer(Err("no".to_string())), r#"{"error":"no"}"#);
        assert_eq!(read_answer(r#"{"ok":[1,2]}"#), Ok(json!([1, 2])));
        assert_eq!(read_answer(r#"{"ok":null}"#), Ok(Value::Null));
        assert_eq!(
            read_answer(r#"{"error":"denied"}"#),
            Err("denied".to_string())
        );
        assert!(read_answer("{}").is_err());
        assert!(read_answer("not json").is_err());
    }

    #[test]
    fn the_tunnel_is_opened_with_the_interface() {
        let (fake, platform) = platform(vec![("openTunnel", r#"{"ok":{"fd":42}}"#)]);
        assert!(platform.has_system_vpn());
        assert_eq!(platform.open_tunnel(&TunnelInterface::proxy()), Ok(42));
        let calls = fake.calls.lock().unwrap();
        let (method, args) = &calls[0];
        assert_eq!(method, "openTunnel");
        assert_eq!(
            args["addresses"],
            json!(["198.18.0.1/24", "fd00:4e43::1/64"])
        );
        assert_eq!(args["dnsServers"], json!(["198.18.0.2"]));
        assert_eq!(args["mtu"], json!(1500));
        assert_eq!(args["session"], json!("AnixOps Net"));
    }

    #[test]
    fn a_refused_or_empty_tunnel_is_an_error() {
        let (_, refused) = platform(vec![(
            "openTunnel",
            r#"{"error":"The VPN was not allowed"}"#,
        )]);
        assert_eq!(
            refused.open_tunnel(&TunnelInterface::proxy()),
            Err("The VPN was not allowed".to_string())
        );
        let (_, empty) = platform(vec![("openTunnel", r#"{"ok":{"fd":-1}}"#)]);
        assert!(empty.open_tunnel(&TunnelInterface::proxy()).is_err());
    }

    #[test]
    fn files_keys_and_networks_come_from_the_bridge() {
        let (fake, platform) = platform(vec![
            ("saveToDownloads", r#"{"ok":{"location":"Download/a.crt"}}"#),
            ("captureSessionKey", r#"{"ok":{"keyBase64":"AAAA"}}"#),
            ("currentNetwork", r#"{"ok":{"kind":"wifi","ssid":"Home"}}"#),
        ]);
        assert_eq!(
            platform.save_to_downloads("a.crt", "PEM"),
            Ok("Download/a.crt".to_string())
        );
        assert_eq!(
            platform.save_base64_to_downloads("c.har", "application/json", "e30="),
            Ok("Download/a.crt".to_string())
        );
        assert_eq!(
            platform.replace_in_downloads("b.crt", "PEM"),
            Ok("Download/a.crt".to_string())
        );
        assert_eq!(platform.capture_session_key(), Ok("AAAA".to_string()));
        assert_eq!(
            platform.current_network(false),
            Some(Network::from_kind("wifi", Some("Home".to_string())))
        );
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls[0].1["mimeType"], json!("application/x-x509-ca-cert"));
        assert_eq!(calls[0].1["base64"], json!(false));
        assert_eq!(calls[0].1.get("replace"), None);
        assert_eq!(calls[1].1["mimeType"], json!("application/json"));
        assert_eq!(calls[1].1["base64"], json!(true));
        assert_eq!(calls[2].1["replace"], json!(true));
        assert_eq!(calls[2].1["mimeType"], json!("application/x-x509-ca-cert"));
    }

    #[test]
    fn a_withheld_wifi_name_is_not_an_unnamed_wifi() {
        let (_, withheld) = platform(vec![(
            "currentNetwork",
            r#"{"ok":{"kind":"wifi","ssidWithheld":true}}"#,
        )]);
        assert_eq!(withheld.current_network(false), Some(Network::WifiWithheld));
        let (_, unnamed) = platform(vec![("currentNetwork", r#"{"ok":{"kind":"wifi"}}"#)]);
        assert_eq!(unnamed.current_network(false), Some(Network::Wifi(None)));
        let (_, named) = platform(vec![(
            "currentNetwork",
            r#"{"ok":{"kind":"wifi","ssid":"Home","ssidWithheld":false}}"#,
        )]);
        assert_eq!(
            named.current_network(false),
            Some(Network::Wifi(Some("Home".to_string())))
        );
    }

    #[test]
    fn closing_for_the_rules_keeps_waiting_and_notices_carry_their_kind() {
        let (fake, platform) = platform(vec![
            ("closeTunnel", r#"{"ok":{}}"#),
            ("postNotice", r#"{"ok":{}}"#),
        ]);
        platform.close_tunnel();
        platform.close_tunnel_for_rules();
        let notice = app_notifications::Settings::default()
            .disconnected(false)
            .unwrap();
        platform.post_notice(&notice);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls[0], ("closeTunnel".to_string(), json!({})));
        assert_eq!(
            calls[1],
            ("closeTunnel".to_string(), json!({"waitForNetwork": true}))
        );
        assert_eq!(calls[2].0, "postNotice");
        assert_eq!(calls[2].1["kind"], json!("connection"));
        assert_eq!(calls[2].1["openUrl"], Value::Null);
    }

    #[test]
    fn invoking_before_init_is_an_error_not_a_crash() {
        if CLIENT.get().is_none() {
            assert_eq!(
                invoke("runtime_snapshot", "{}"),
                r#"{"error":"the client is not open yet"}"#
            );
        }
    }
}
