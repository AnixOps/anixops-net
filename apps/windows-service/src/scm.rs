//! Running under the Windows service manager: reporting the service's state and stopping when told to.

use std::ffi::c_void;
use std::io;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread;

use windows_sys::Win32::Foundation::{ERROR_CALL_NOT_IMPLEMENTED, NO_ERROR};
use windows_sys::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SetServiceStatus, StartServiceCtrlDispatcherW,
    SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING,
    SERVICE_STATUS, SERVICE_STOPPED, SERVICE_STOP_PENDING, SERVICE_TABLE_ENTRYW,
    SERVICE_WIN32_OWN_PROCESS,
};

use crate::{Options, Service, SERVICE_NAME};

/// How long the service manager is told to wait while the service starts or stops, in milliseconds.
const WAIT_HINT: u32 = 30_000;

/// What the service runs with; set before the dispatcher is started.
static OPTIONS: OnceLock<Options> = OnceLock::new();
/// The handle the service manager gave for reporting state.
static STATUS: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
/// Tells the service's main function that a stop was asked for.
static STOP: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

fn name() -> Vec<u16> {
    SERVICE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

/// Tells the service manager the service's state. `exit_code` is a Windows error code, 0 for none.
fn report(state: u32, exit_code: u32) {
    let handle = STATUS.load(Ordering::SeqCst);
    if handle.is_null() {
        return;
    }
    let pending = state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING;
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        // While starting, a stop cannot be taken yet.
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: u32::from(pending),
        dwWaitHint: if pending { WAIT_HINT } else { 0 },
    };
    // SAFETY: `handle` came from the service manager and `status` is valid for the call.
    unsafe {
        SetServiceStatus(handle, &status);
    }
}

/// Called by the service manager on a thread of its own when it wants something of the service.
unsafe extern "system" fn control(
    control: u32,
    _event_type: u32,
    _event_data: *mut c_void,
    _context: *mut c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            report(SERVICE_STOP_PENDING, NO_ERROR);
            if let Some(stop) = STOP.get() {
                let _ = stop.lock().unwrap_or_else(PoisonError::into_inner).send(());
            }
            NO_ERROR
        }
        SERVICE_CONTROL_INTERROGATE => NO_ERROR,
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

/// The service's main function, called by the service manager once the dispatcher runs.
unsafe extern "system" fn service_main(_count: u32, _arguments: *mut *mut u16) {
    let name = name();
    // SAFETY: `name` is NUL-terminated and `control` has the signature the service manager calls.
    let handle = unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), null()) };
    if handle.is_null() {
        return;
    }
    STATUS.store(handle, Ordering::SeqCst);
    report(SERVICE_START_PENDING, NO_ERROR);

    let (stop, stopped) = mpsc::channel();
    let _ = STOP.set(Mutex::new(stop));
    let options = OPTIONS.get().cloned().unwrap_or_default();
    let service = match Service::start(&options) {
        Ok(service) => service,
        Err(error) => {
            // The service manager shows this code where the service is listed.
            let code = error.raw_os_error().map_or(1, |code| code as u32);
            report(SERVICE_STOPPED, code);
            return;
        }
    };
    report(SERVICE_RUNNING, NO_ERROR);
    {
        let service = Arc::clone(&service);
        thread::spawn(move || {
            // A pipe that fails leaves nothing to answer on: stop, and let the service manager restart.
            let _ = service.run();
            if let Some(stop) = STOP.get() {
                let _ = stop.lock().unwrap_or_else(PoisonError::into_inner).send(());
            }
        });
    }
    let _ = stopped.recv();
    report(SERVICE_STOP_PENDING, NO_ERROR);
    service.shutdown();
    report(SERVICE_STOPPED, NO_ERROR);
}

/// Hands the process to the service manager and returns when the service has stopped. An error means the
/// process was not started by the service manager (run it with `--console` instead).
pub fn dispatch(options: Options) -> io::Result<()> {
    let _ = OPTIONS.set(options);
    let mut name = name();
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: null_mut(),
            lpServiceProc: None,
        },
    ];
    // SAFETY: `table` ends with an empty entry and, with `name`, outlives the call, which returns only
    // when the service has stopped.
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
