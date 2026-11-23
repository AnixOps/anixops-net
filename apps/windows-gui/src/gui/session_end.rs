//! Puts the system proxy back when Windows ends the session while the window is connected: a sign-out, a
//! shutdown or restart, or an installer's Restart Manager closing the window. Closing the main window only hides
//! it, and Windows ends the process without any of the window's own exit paths running, which would leave the
//! proxy pointing at a local port nobody listens on.
//!
//! A hidden top-level window of its own (message-only windows do not get these broadcasts) answers
//! `WM_QUERYENDSESSION` by putting the proxy back (`owned_proxy::release_for_session_end`) and allowing the
//! session to end; `WM_ENDSESSION` puts it back too when the query was skipped, or, when the end was cancelled
//! and the engine still runs, applies the proxy again. A console control handler does the same when the process
//! has a console that is closed. The next start (`owned_proxy::recover_at_start`) covers what these miss, such
//! as a crash or a forced shutdown.

use super::owned_proxy::{self, OwnedProxy};
use platform_windows::managed::append_managed_log;
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, MSG, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
};

const LOG_SCOPE: &str = "tauri-gui";

type StillConnected = Box<dyn Fn() -> bool + Send + Sync>;

/// Whether the engine still runs, for a cancelled session end.
static STILL_CONNECTED: OnceLock<StillConnected> = OnceLock::new();
/// The record put back for a session end that may yet be cancelled.
static RELEASED: Mutex<Option<OwnedProxy>> = Mutex::new(None);

/// Starts the guard once; `still_connected` tells whether the engine still runs.
pub fn install(still_connected: impl Fn() -> bool + Send + Sync + 'static) {
    if STILL_CONNECTED.set(Box::new(still_connected)).is_err() {
        return;
    }
    // SAFETY: the handler is a plain function that lives as long as the process.
    if unsafe { SetConsoleCtrlHandler(Some(console_handler), 1) } == 0 {
        let _ = append_managed_log(LOG_SCOPE, "session end: console handler not installed");
    }
    let started = std::thread::Builder::new()
        .name("session-end".to_string())
        .spawn(run_window);
    if let Err(error) = started {
        let _ = append_managed_log(
            LOG_SCOPE,
            &format!("session end: guard thread not started: {error}"),
        );
    }
}

/// Puts the proxy back, keeping the record in case the session end is cancelled.
fn release(reason: &str) {
    let Some(record) = owned_proxy::release_for_session_end() else {
        return;
    };
    let _ = append_managed_log(
        LOG_SCOPE,
        &format!("session end ({reason}): system proxy restored"),
    );
    if let Ok(mut released) = RELEASED.lock() {
        *released = Some(record);
    }
}

/// The session goes on after all: the proxy comes back while the engine still runs.
fn resume() {
    let record = RELEASED
        .lock()
        .ok()
        .and_then(|mut released| released.take());
    let Some(record) = record else {
        return;
    };
    if !STILL_CONNECTED
        .get()
        .is_some_and(|still_connected| still_connected())
    {
        return;
    }
    let line = match owned_proxy::resume_after_cancelled_session_end(record) {
        Ok(()) => "session end cancelled: system proxy applied again".to_string(),
        Err(error) => format!("session end cancelled: system proxy not applied again: {error}"),
    };
    let _ = append_managed_log(LOG_SCOPE, &line);
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_QUERYENDSESSION => {
            release("query");
            // The session may end.
            1
        }
        WM_ENDSESSION => {
            if wparam == 0 {
                resume();
            } else {
                release("end");
            }
            0
        }
        // SAFETY: the arguments are the ones Windows passed in.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// A console that is closed, or a sign-out or shutdown reported to a console process: put the proxy back and
/// let the default handling end the process.
unsafe extern "system" fn console_handler(_event: u32) -> i32 {
    release("console");
    0
}

/// The hidden window and its message loop, on the guard's own thread.
fn run_window() {
    let class_name: Vec<u16> = "AnixOpsNetSessionEnd"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: a null name asks for this program's own module.
    let instance = unsafe { GetModuleHandleW(null()) };
    // SAFETY: WNDCLASSW is plain data; the fields that matter are set below.
    let mut class: WNDCLASSW = unsafe { zeroed() };
    class.lpfnWndProc = Some(window_proc);
    class.hInstance = instance;
    class.lpszClassName = class_name.as_ptr();
    // SAFETY: `class` and the name it points to live through the call.
    if unsafe { RegisterClassW(&class) } == 0 {
        let _ = append_managed_log(LOG_SCOPE, "session end: window class not registered");
        return;
    }
    // A top-level window that is never shown: it gets the session broadcasts.
    // SAFETY: the class was registered above; every pointer is valid or null.
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if window.is_null() {
        let _ = append_managed_log(LOG_SCOPE, "session end: window not created");
        return;
    }
    // SAFETY: MSG is plain data the calls fill and read.
    let mut message: MSG = unsafe { zeroed() };
    // SAFETY: `message` is valid for the calls, and the window belongs to this thread.
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
