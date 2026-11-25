//! Android and Linux desktop TUN mode: the engine carries a TUN interface, the same packet path as the iOS
//! Network Extension (`EmbeddedRuntime`: IP packets in, IP packets out), with the userspace TCP/IP stack in
//! process.
//!
//! On Android the VpnService (apps/android-native) hands over the descriptor once the user allows the VPN, and the
//! app's own sockets are left out of the VPN. On Linux the polkit helper ([`super::tun_helper`]) creates the
//! device and hands it over, and the engine's sockets are bound to the physical interface. Either way
//! [`attach`] keeps the descriptor across engine restarts (a node switch starts a new runtime on the same
//! interface), and [`detach`] closes it when the client disconnects.
//!
//! A descriptor attached while a runtime runs replaces the kept one under it: Android's VPN gets a new interface,
//! and with it a new descriptor, each time its routes change (the mesh alone, `TunnelInterface::mesh`), and the
//! mesh keeps running across that.

use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::thread::{self, JoinHandle};

use engine_embedded::tunnel::PacketWriter;
use engine_embedded::EmbeddedRuntime;

/// How long a read waits before it looks at the stop flag again.
const POLL_MS: i32 = 250;
/// Larger than any MTU the interface is given.
const READ_BUFFER: usize = 65_535;
/// How long a failed read waits for the replacement interface before it reads again.
const REPLACE_WAIT_MS: u64 = 20;

/// The kept descriptor, shared with the runtime's reader and writer.
struct Descriptor {
    fd: AtomicI32,
    /// Held to read or write the descriptor, and exclusively to replace or close it, so no packet goes to a
    /// descriptor already closed (whose number the system may have given to another file).
    gate: RwLock<()>,
    /// Set when a read fails: the VPN was taken away (another VPN, or the system settings).
    dead: AtomicBool,
    /// A new interface is on its way ([`expect_replacement`]): a failed read waits for it instead of ending.
    replacing: AtomicBool,
}

impl Descriptor {
    fn read_gate(&self) -> std::sync::RwLockReadGuard<'_, ()> {
        self.gate.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_gate(&self) -> std::sync::RwLockWriteGuard<'_, ()> {
        self.gate.write().unwrap_or_else(PoisonError::into_inner)
    }
}

static TUN: Mutex<Option<Arc<Descriptor>>> = Mutex::new(None);

fn tun() -> MutexGuard<'static, Option<Arc<Descriptor>>> {
    TUN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Keeps the VPN's descriptor. One kept before is closed; a runtime running on it goes on with the new one.
pub fn attach(fd: RawFd) {
    let mut kept = tun();
    match kept.as_ref() {
        Some(descriptor) => {
            let _gate = descriptor.write_gate();
            let previous = descriptor.fd.swap(fd, Ordering::AcqRel);
            descriptor.dead.store(false, Ordering::Release);
            descriptor.replacing.store(false, Ordering::Release);
            close(previous);
        }
        None => {
            *kept = Some(Arc::new(Descriptor {
                fd: AtomicI32::new(fd),
                gate: RwLock::new(()),
                dead: AtomicBool::new(false),
                replacing: AtomicBool::new(false),
            }));
        }
    }
}

/// A new interface replaces the kept one shortly ([`attach`]); until then, or [`replacement_failed`], the runtime
/// waits for it when the old one goes down.
pub fn expect_replacement() {
    if let Some(descriptor) = tun().as_ref() {
        descriptor.replacing.store(true, Ordering::Release);
    }
}

/// The new interface did not come: a failed read ends the runtime again.
pub fn replacement_failed() {
    if let Some(descriptor) = tun().as_ref() {
        descriptor.replacing.store(false, Ordering::Release);
    }
}

/// Whether a live descriptor is kept.
pub fn attached() -> bool {
    tun()
        .as_ref()
        .is_some_and(|descriptor| !descriptor.dead.load(Ordering::Acquire))
}

/// Closes the kept descriptor, which takes the VPN interface down. Call only while no runtime runs.
pub fn detach() {
    if let Some(descriptor) = tun().take() {
        let _gate = descriptor.write_gate();
        close(descriptor.fd.load(Ordering::Acquire));
    }
}

fn close(fd: RawFd) {
    // SAFETY: the descriptor came from the VpnService and is closed once, here.
    unsafe { libc::close(fd) };
}

/// Writes the engine's outgoing packets to the interface, one packet per write.
struct TunWriter(Arc<Descriptor>);

impl PacketWriter for TunWriter {
    fn write(&self, packet: &[u8]) {
        let _gate = self.0.read_gate();
        let fd = self.0.fd.load(Ordering::Acquire);
        // SAFETY: `packet` is valid for its length; the gate keeps the descriptor open for the write.
        // A full interface queue drops the packet, as a congested link would.
        unsafe { libc::write(fd, packet.as_ptr().cast(), packet.len()) };
    }
}

/// One runtime on the kept descriptor, with the thread that feeds it the packets read from the interface.
pub struct TunEngine {
    runtime: Arc<Mutex<EmbeddedRuntime>>,
    stop: Arc<AtomicBool>,
    descriptor: Arc<Descriptor>,
    reader: Option<JoinHandle<()>>,
}

impl TunEngine {
    pub fn start(envelope: &str) -> Result<Self, String> {
        let descriptor = tun()
            .as_ref()
            .filter(|descriptor| !descriptor.dead.load(Ordering::Acquire))
            .cloned()
            .ok_or_else(|| {
                crate::refusal::coded("error.vpn_off", "the VPN is not on; connect again")
            })?;
        let mut runtime = EmbeddedRuntime::from_json(envelope.as_bytes())
            .map_err(|error| error.message.to_string())?;
        runtime
            .start(TunWriter(descriptor.clone()))
            .map_err(|error| error.message.to_string())?;
        let runtime = Arc::new(Mutex::new(runtime));
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let (runtime, stop, descriptor) = (runtime.clone(), stop.clone(), descriptor.clone());
            thread::Builder::new()
                .name("tun-read".into())
                .spawn(move || read_loop(&descriptor, &runtime, &stop))
                .map_err(|error| format!("the VPN reader could not start: {error}"))?
        };
        Ok(Self {
            runtime,
            stop,
            descriptor,
            reader: Some(reader),
        })
    }

    /// Stops the reader and the runtime; the descriptor stays open for the next runtime.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        lock(&self.runtime).stop();
    }

    pub fn is_running(&self) -> bool {
        // A reader that ended stays ended, also once a new descriptor clears `dead`.
        self.reader
            .as_ref()
            .is_some_and(|reader| !reader.is_finished())
            && !self.descriptor.dead.load(Ordering::Acquire)
    }

    pub fn control_json(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        lock(&self.runtime)
            .control_json(request)
            .map_err(|error| error.message.to_string())
    }
}

impl Drop for TunEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lock(runtime: &Mutex<EmbeddedRuntime>) -> MutexGuard<'_, EmbeddedRuntime> {
    runtime.lock().unwrap_or_else(PoisonError::into_inner)
}

fn read_loop(descriptor: &Descriptor, runtime: &Mutex<EmbeddedRuntime>, stop: &AtomicBool) {
    let mut buffer = vec![0u8; READ_BUFFER];
    while !stop.load(Ordering::Acquire) {
        match read_once(descriptor, &mut buffer) {
            Read::Packet(length) => lock(runtime).input_packet(&buffer[..length]),
            Read::Nothing => {}
            // The old interface goes down before its replacement is attached: the reader waits for it.
            Read::Failed if descriptor.replacing.load(Ordering::Acquire) => {
                thread::sleep(std::time::Duration::from_millis(REPLACE_WAIT_MS));
            }
            Read::Failed => break,
        }
    }
    if !stop.load(Ordering::Acquire) {
        descriptor.dead.store(true, Ordering::Release);
    }
}

enum Read {
    Packet(usize),
    /// Nothing to read within [`POLL_MS`], or a read to try again.
    Nothing,
    /// The interface is gone.
    Failed,
}

/// One wait and read, holding the gate (at most [`POLL_MS`]), so a replacement waits no longer than that.
fn read_once(descriptor: &Descriptor, buffer: &mut [u8]) -> Read {
    let _gate = descriptor.read_gate();
    let fd = descriptor.fd.load(Ordering::Acquire);
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd.
    let ready = unsafe { libc::poll(&mut poll, 1, POLL_MS) };
    if ready < 0 {
        return if interrupted() {
            Read::Nothing
        } else {
            Read::Failed
        };
    }
    if ready == 0 {
        return Read::Nothing;
    }
    if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Read::Failed;
    }
    // SAFETY: `buffer` is writable for its length.
    let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
    match usize::try_from(read) {
        Ok(0) => Read::Failed,
        Ok(length) => Read::Packet(length),
        Err(_) if interrupted() || would_block() => Read::Nothing,
        Err(_) => Read::Failed,
    }
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn interrupted() -> bool {
    errno() == libc::EINTR
}

fn would_block() -> bool {
    let code = errno();
    code == libc::EAGAIN || code == libc::EWOULDBLOCK
}
