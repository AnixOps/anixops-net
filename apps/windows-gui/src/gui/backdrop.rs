//! Windows 11 Mica behind the window, so the sidebar and title bar take the desktop-tinted system material the
//! way native Windows 11 apps do. The window is transparent in tauri.conf.json; the page keeps opaque colours
//! until `page_script` tells it Mica is really there (Windows 10 has no Mica and would otherwise show through).

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::window::{Effect, EffectsBuilder};
use tauri::{Runtime, WebviewWindow};

/// Windows 11 is build 22000 and later; Mica needs it.
const FIRST_WINDOWS_11_BUILD: u32 = 22000;

static MICA: AtomicBool = AtomicBool::new(false);

/// Applies Mica to the main window when the system supports it. Failure leaves the opaque window as it was.
pub(super) fn apply<R: Runtime>(window: &WebviewWindow<R>) {
    if windows_build() < FIRST_WINDOWS_11_BUILD {
        return;
    }
    let effects = EffectsBuilder::new().effect(Effect::Mica).build();
    if window.set_effects(effects).is_ok() {
        MICA.store(true, Ordering::Relaxed);
    }
}

/// The script that lets the page drop its window background once Mica is in place (see `data-backdrop` in
/// styles.css), or `None` when the window stays opaque.
pub(super) fn page_script() -> Option<&'static str> {
    MICA.load(Ordering::Relaxed)
        .then_some("document.documentElement.dataset.backdrop = 'mica';")
}

/// The real Windows build number. `GetVersionEx` reports what the manifest declares, so ask ntdll instead.
fn windows_build() -> u32 {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    type RtlGetVersion = unsafe extern "system" fn(*mut OsVersionInfo) -> i32;

    let module: Vec<u16> = "ntdll.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: ntdll is always loaded; RtlGetVersion has this signature and only writes the struct it is given,
    // whose size field is set first as the API requires.
    unsafe {
        let ntdll = GetModuleHandleW(module.as_ptr());
        if ntdll.is_null() {
            return 0;
        }
        let Some(address) = GetProcAddress(ntdll, b"RtlGetVersion\0".as_ptr()) else {
            return 0;
        };
        let rtl_get_version =
            std::mem::transmute::<unsafe extern "system" fn() -> isize, RtlGetVersion>(address);
        let mut info: OsVersionInfo = std::mem::zeroed();
        info.size = std::mem::size_of::<OsVersionInfo>() as u32;
        if rtl_get_version(&mut info) != 0 {
            return 0;
        }
        info.build
    }
}
