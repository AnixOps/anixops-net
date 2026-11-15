//! Android: the engine carries the system VPN's TUN interface, the same packet path as the iOS Network
//! Extension (`EmbeddedRuntime`: IP packets in, IP packets out), with the userspace TCP/IP stack in process.
//!
//! The VpnService (apps/android) hands over the descriptor once the user allows the VPN; [`attach`] keeps it
//! across engine restarts (a node switch starts a new runtime on the same interface), and [`detach`] closes it
//! when the client disconnects. The app's own sockets are left out of the VPN, so the engine reaches its
//! servers directly without protecting each socket.

use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use engine_embedded::tunnel::PacketWriter;
use engine_embedded::EmbeddedRuntime;

/// How long a read waits before it looks at the stop flag again.
const POLL_MS: i32 = 250;
/// Larger than any MTU the interface is given.
const READ_BUFFER: usize = 65_535;

struct Descriptor {
    fd: RawFd,
    /// Set when a read fails: the VPN was taken away (another VPN, or the system settings).
    dead: Arc<AtomicBool>,
}

static TUN: Mutex<Option<Descriptor>> = Mutex::new(None);

fn tun() -> MutexGuard<'static, Option<Descriptor>> {
    TUN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Keeps the VPN's descriptor; one kept before is closed. Call only while no runtime runs.
pub fn attach(fd: RawFd) {
    let previous = tun().replace(Descriptor {
        fd,
        dead: Arc::new(AtomicBool::new(false)),
    });
    if let Some(previous) = previous {
        close(previous.fd);
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
        close(descriptor.fd);
    }
}

fn close(fd: RawFd) {
    // SAFETY: the descriptor came from the VpnService and is closed once, here.
    unsafe { libc::close(fd) };
}

/// Writes the engine's outgoing packets to the interface, one packet per write.
struct TunWriter(RawFd);

impl PacketWriter for TunWriter {
    fn write(&self, packet: &[u8]) {
        // SAFETY: `packet` is valid for its length; the descriptor stays open until the runtime has stopped.
        // A full interface queue drops the packet, as a congested link would.
        unsafe { libc::write(self.0, packet.as_ptr().cast(), packet.len()) };
    }
}

/// One runtime on the kept descriptor, with the thread that feeds it the packets read from the interface.
pub struct TunEngine {
    runtime: Arc<Mutex<EmbeddedRuntime>>,
    stop: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl TunEngine {
    pub fn start(envelope: &str) -> Result<Self, String> {
        let (fd, dead) = tun()
            .as_ref()
            .filter(|descriptor| !descriptor.dead.load(Ordering::Acquire))
            .map(|descriptor| (descriptor.fd, descriptor.dead.clone()))
            .ok_or("the VPN is not on; connect again")?;
        let mut runtime = EmbeddedRuntime::from_json(envelope.as_bytes())
            .map_err(|error| error.message.to_string())?;
        runtime
            .start(TunWriter(fd))
            .map_err(|error| error.message.to_string())?;
        let runtime = Arc::new(Mutex::new(runtime));
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let (runtime, stop, dead) = (runtime.clone(), stop.clone(), dead.clone());
            thread::Builder::new()
                .name("tun-read".into())
                .spawn(move || read_loop(fd, &runtime, &stop, &dead))
                .map_err(|error| format!("the VPN reader could not start: {error}"))?
        };
        Ok(Self {
            runtime,
            stop,
            dead,
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
        self.reader.is_some() && !self.dead.load(Ordering::Acquire)
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

fn read_loop(fd: RawFd, runtime: &Mutex<EmbeddedRuntime>, stop: &AtomicBool, dead: &AtomicBool) {
    let mut buffer = vec![0u8; READ_BUFFER];
    while !stop.load(Ordering::Acquire) {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, POLL_MS) };
        if ready < 0 {
            if interrupted() {
                continue;
            }
            break;
        }
        if ready == 0 {
            continue;
        }
        if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            break;
        }
        // SAFETY: `buffer` is writable for its length.
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        match usize::try_from(read) {
            Ok(0) => break,
            Ok(length) => {
                lock(runtime).input_packet(&buffer[..length]);
            }
            Err(_) if interrupted() || would_block() => continue,
            Err(_) => break,
        }
    }
    if !stop.load(Ordering::Acquire) {
        dead.store(true, Ordering::Release);
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
