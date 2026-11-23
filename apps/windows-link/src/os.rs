//! What talks to Windows: the client's receiving window, the handover to it, starting the client, the kept links
//! and the messages to the user.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW,
    FindWindowExW, GetWindowThreadProcessId, MessageBoxW, RegisterClassExW, SendMessageTimeoutW,
    HWND_MESSAGE, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MSGFLT_ALLOW, SMTO_ABORTIFHUNG,
    SMTO_BLOCK, SW_SHOWNORMAL, WM_COPYDATA, WNDCLASSEXW,
};

use crate::{
    keep_pending, payload, pending_path, quote_argument, read_payload, start_failure_message,
    take_pending, Language, Request, CLIENT_EXECUTABLE, RECEIVER_CLASS,
};

/// How long a handover waits for the running client to answer, in milliseconds.
const HAND_OVER_TIMEOUT_MS: u32 = 10_000;
/// `LOCALE_NAME_MAX_LENGTH`.
const LOCALE_NAME_LENGTH: usize = 85;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn wide_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// What the client's receiving window does with each request; set once by [`listen`].
type OnRequest = Box<dyn Fn(Request) + Send + Sync>;
static RECEIVER: OnceLock<OnRequest> = OnceLock::new();

unsafe extern "system" fn receive(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message != WM_COPYDATA || lparam == 0 {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    // SAFETY: for WM_COPYDATA, `lparam` points at a COPYDATASTRUCT whose `cbData` bytes at `lpData` stay valid
    // until this call returns (Windows copied them into this process).
    let data = &*(lparam as *const COPYDATASTRUCT);
    let bytes: &[u8] = if data.cbData == 0 || data.lpData.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize)
    };
    let taken = read_payload(data.dwData, bytes)
        .and_then(|request| RECEIVER.get().map(|on_request| on_request(request)))
        .is_some();
    LRESULT::from(taken)
}

/// Starts receiving links: a message-only window of [`RECEIVER_CLASS`] on this thread, whose message loop must
/// run (the client calls it on its window thread). The window takes `WM_COPYDATA` from processes with fewer
/// rights too, which Windows drops by default, and nothing else; what arrives is checked with
/// [`read_payload`] before `on_request` sees it. Answers the Windows error when the window could not be made.
pub fn listen<F: Fn(Request) + Send + Sync + 'static>(on_request: F) -> Result<(), u32> {
    if RECEIVER.set(Box::new(on_request)).is_err() {
        return Err(ERROR_ALREADY_EXISTS);
    }
    let class = wide(RECEIVER_CLASS);
    // SAFETY: a null name asks for this program's own module.
    let instance = unsafe { GetModuleHandleW(null()) };
    let description = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(receive),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class.as_ptr(),
        hIconSm: null_mut(),
    };
    // SAFETY: `description` is complete and its class name NUL-terminated; Windows copies what it keeps.
    if unsafe { RegisterClassExW(&description) } == 0 {
        return Err(unsafe { GetLastError() });
    }
    // SAFETY: the class was registered above; the window has no parent but HWND_MESSAGE, so it is never shown.
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            instance,
            null(),
        )
    };
    if window.is_null() {
        return Err(unsafe { GetLastError() });
    }
    // The handler runs without administrator rights and the client with them: User Interface Privilege
    // Isolation drops its message unless this window lets WM_COPYDATA in.
    // SAFETY: `window` was made above by this thread.
    if unsafe { ChangeWindowMessageFilterEx(window, WM_COPYDATA, MSGFLT_ALLOW, null_mut()) } == 0 {
        return Err(unsafe { GetLastError() });
    }
    Ok(())
}

/// Hands `request` to the running client, letting it come to the foreground. Answers false when no client
/// listens or it did not take the request in time.
pub fn hand_over(request: &Request) -> bool {
    let class = wide(RECEIVER_CLASS);
    // SAFETY: the class name is NUL-terminated; HWND_MESSAGE searches the message-only windows.
    let window = unsafe { FindWindowExW(HWND_MESSAGE, null_mut(), class.as_ptr(), null()) };
    if window.is_null() {
        return false;
    }
    let mut process = 0_u32;
    // SAFETY: `window` came from FindWindowExW and `process` is a valid place for the id.
    unsafe { GetWindowThreadProcessId(window, &mut process) };
    if process != 0 {
        // The link came from the user's click, so this program may bring the client forward; it passes that on.
        // SAFETY: plain call with a process id.
        unsafe { AllowSetForegroundWindow(process) };
    }
    let (tag, mut bytes) = payload(request);
    let data = COPYDATASTRUCT {
        dwData: tag,
        cbData: bytes.len() as u32,
        lpData: if bytes.is_empty() {
            null_mut()
        } else {
            bytes.as_mut_ptr().cast()
        },
    };
    let mut answer = 0_usize;
    // SAFETY: `data` and `bytes` outlive the call, which copies them into the client before it returns.
    let sent = unsafe {
        SendMessageTimeoutW(
            window,
            WM_COPYDATA,
            0,
            &data as *const COPYDATASTRUCT as LPARAM,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            HAND_OVER_TIMEOUT_MS,
            &mut answer,
        )
    };
    sent != 0 && answer == 1
}

/// Starts the client with `link` as its only argument, asking for administrator rights (its manifest requires
/// them, so Windows shows the prompt once). Answers the Windows error when it did not start; [`crate::DECLINED`]
/// when the user declined the prompt.
pub fn start_client(client: &Path, link: &str) -> Result<(), u32> {
    let verb = wide("runas");
    let file = wide_os(client.as_os_str());
    let parameters = wide(&quote_argument(link));
    let directory = client.parent().map(|folder| wide_os(folder.as_os_str()));
    // SAFETY: SHELLEXECUTEINFOW is plain data; all zero is "nothing set".
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    // Returns only once the start is decided, so the error (a declined prompt) is this call's.
    info.fMask = SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.lpDirectory = directory.as_ref().map_or(null(), |folder| folder.as_ptr());
    info.nShow = SW_SHOWNORMAL;
    // SAFETY: every string `info` points at is NUL-terminated and outlives the call.
    if unsafe { ShellExecuteExW(&mut info) } != 0 {
        Ok(())
    } else {
        Err(unsafe { GetLastError() })
    }
}

/// `%LOCALAPPDATA%` of the user this process runs as.
pub fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Takes the links the handler kept because the client could not be started for them ([`take_pending`]).
pub fn take_kept_links() -> Vec<String> {
    local_app_data()
        .map(|folder| take_pending(&pending_path(&folder), now()))
        .unwrap_or_default()
}

/// The language of the user's Windows locale, for the handler's messages (it runs before any window, so it
/// cannot read the language chosen in the client).
pub fn user_language() -> Language {
    let mut name = [0_u16; LOCALE_NAME_LENGTH];
    // SAFETY: `name` holds the `LOCALE_NAME_LENGTH` characters the call may write.
    let length = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    if length <= 1 {
        return Language::English;
    }
    Language::preferred(&String::from_utf16_lossy(&name[..length as usize - 1]))
}

/// Tells the user something in a message box of its own.
pub fn tell(language: Language, text: &str) {
    let title = wide(client_strings::text(language, "app.name"));
    let text = wide(text);
    // SAFETY: both strings are NUL-terminated.
    unsafe {
        MessageBoxW(
            null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND,
        );
    }
}

/// The handler: hands `link` to the running client, else starts the client with it, else keeps it and says so.
/// Answers the exit code.
pub fn forward(link: &str) -> i32 {
    if hand_over(&Request::Open(link.to_string())) {
        return 0;
    }
    let language = user_language();
    let client = std::env::current_exe()
        .ok()
        .and_then(|program| {
            program
                .parent()
                .map(|folder| folder.join(CLIENT_EXECUTABLE))
        })
        .filter(|client| client.is_file());
    let Some(client) = client else {
        tell(language, client_strings::text(language, "link.no_client"));
        return 1;
    };
    match start_client(&client, link) {
        Ok(()) => 0,
        Err(error) => {
            let kept = local_app_data()
                .is_some_and(|folder| keep_pending(&pending_path(&folder), link, now()).is_ok());
            tell(language, &start_failure_message(language, error, kept));
            i32::from(error != crate::DECLINED)
        }
    }
}
