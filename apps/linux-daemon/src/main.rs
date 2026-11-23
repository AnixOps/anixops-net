//! `anixops-netd`: see the library's documentation.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("anixops-netd runs on Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    use std::sync::Arc;
    use std::thread;

    use anixops_netd::{raise_descriptor_limit, Daemon, Options, SignedIn};

    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    // Before any thread exists, so every thread inherits the mask and the signals wait for `stop_signal`.
    let signals = block_stop_signals();
    // Also before any thread exists. The unit raises the limit as well; this covers a daemon started by hand.
    if let Err(error) = raise_descriptor_limit() {
        eprintln!("anixops-netd keeps its open file limit: {error}");
    }
    let daemon = match Daemon::start(&options, Arc::new(SignedIn::default())) {
        Ok(daemon) => daemon,
        Err(error) => {
            eprintln!("anixops-netd could not start: {error}");
            std::process::exit(1);
        }
    };
    {
        let daemon = Arc::clone(&daemon);
        thread::spawn(move || {
            if let Err(error) = daemon.run() {
                eprintln!("anixops-netd stopped listening: {error}");
                daemon.shutdown();
                std::process::exit(1);
            }
        });
    }
    stop_signal(&signals);
    daemon.shutdown();
}

/// Holds back the signals that ask the daemon to stop (systemd sends SIGTERM) and returns their set.
#[cfg(target_os = "linux")]
fn block_stop_signals() -> libc::sigset_t {
    // SAFETY: the set is initialised by `sigemptyset` before it is read, and the calls only change this
    // thread's signal mask.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

/// Waits until one of `signals` arrives.
#[cfg(target_os = "linux")]
fn stop_signal(signals: &libc::sigset_t) {
    let mut signal: libc::c_int = 0;
    // SAFETY: `signals` is an initialised set and `signal` is valid for the number written to it.
    while unsafe { libc::sigwait(signals, &mut signal) } != 0 {}
}
