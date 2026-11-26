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
//!
//! The same events stop the engine for the 组网 mesh alone, so that its adapter and routes are taken down rather
//! than left to the end of the process; a cancelled end lets it run again. A crash leaves nothing behind either: the
//! adapter goes with the process, and the next start replaces one of the same fixed GUID.

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

/// What the guard asks of the window.
struct Hooks {
    /// Whether the engine still runs, for a cancelled session end.
    still_connected: Box<dyn Fn() -> bool + Send + Sync>,
    /// Stops the engine for the mesh alone as the session ends; the reason goes to the log.
    mesh_end: Box<dyn Fn(&str) + Send + Sync>,
    /// Lets it run again after a cancelled end.
    mesh_resume: Box<dyn Fn() + Send + Sync>,
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();
/// The record put back for a session end that may yet be cancelled.
static RELEASED: Mutex<Option<OwnedProxy>> = Mutex::new(None);

/// Starts the guard once; `still_connected` tells whether the engine still runs, `mesh_end` stops the engine for
/// the mesh alone and `mesh_resume` lets it run again.
pub fn install(
    still_connected: impl Fn() -> bool + Send + Sync + 'static,
    mesh_end: impl Fn(&str) + Send + Sync + 'static,
    mesh_resume: impl Fn() + Send + Sync + 'static,
) {
    let hooks = Hooks {
        still_connected: Box::new(still_connected),
        mesh_end: Box::new(mesh_end),
        mesh_resume: Box::new(mesh_resume),
    };
    if HOOKS.set(hooks).is_err() {
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

/// Stops the engine for the mesh alone and puts the proxy back, keeping the record in case the session end is
/// cancelled.
fn release(reason: &str) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.mesh_end)(&format!("session end, {reason}"));
    }
    // The firewall rules were dropped with the engine (`mesh::end`); the session may end once they are removed.
    super::mesh_firewall::finish(super::mesh_firewall::EXIT_WAIT);
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

/// The session goes on after all: the proxy comes back while the engine still runs, and the engine for the mesh
/// alone may run again.
fn resume() {
    if let Some(hooks) = HOOKS.get() {
        (hooks.mesh_resume)();
    }
    let record = RELEASED
        .lock()
        .ok()
        .and_then(|mut released| released.take());
    let Some(record) = record else {
        return;
    };
    if !HOOKS.get().is_some_and(|hooks| (hooks.still_connected)()) {
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
