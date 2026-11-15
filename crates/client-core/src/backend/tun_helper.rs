//! Linux desktop TUN mode. The app stays unprivileged: a polkit helper (this same program started through `pkexec`
//! with [`HELPER_FLAG`]) creates a TUN device, gives it its address and routes and hands the open descriptor back
//! over a Unix socket, then exits. The device belongs to that descriptor, so closing it when the client
//! disconnects removes the device and its routes with no second password prompt.
//!
//! The engine's own sockets are bound to the physical interface (`SO_BINDTODEVICE`, which Linux 5.7 and later
//! allow without privileges), or they would loop back into the device.

use std::ffi::CString;
use std::fs;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use engine_embedded::protect::{set_socket_protector, RawSocketHandle};

/// The argument that makes the program run as the helper; the polkit action names it.
pub const HELPER_FLAG: &str = "--tun-helper";
/// The polkit action the packaged policy defines (`io.anixops.net.tun.policy`).
pub const POLKIT_ACTION: &str = "io.anixops.net.tun";

/// The device name pattern; the kernel picks the first free number.
const DEVICE_PATTERN: &str = "anxtun%d";
/// The interface addresses, as Android's VPN (`TunnelInterface::proxy`): the userspace stack answers on any
/// address, and DNS to 198.18.0.2 is answered by the engine.
const ADDRESS_V4: &str = "198.18.0.1/24";
const ADDRESS_V6: &str = "fd00:4e43::1/64";
/// Two halves of each address space: more specific than the default route, which stays as it was.
const ROUTES_V4: [&str; 2] = ["0.0.0.0/1", "128.0.0.0/1"];
const ROUTES_V6: [&str; 2] = ["::/1", "8000::/1"];
const MIN_MTU: u32 = 1280;
const MAX_MTU: u32 = 9000;

/// Set while this module's socket protector is installed.
static PROTECTING: AtomicBool = AtomicBool::new(false);

/// Starts the helper through `pkexec` (the desktop asks for the administrator password), takes the TUN descriptor
/// it hands back, and binds the engine's sockets to the interface the default route used before.
pub fn open(mtu: u32) -> Result<RawFd, String> {
    let physical = default_interface()
        .ok_or("no default route: connect to a network before turning on TUN mode")?;
    let program = helper_program()?;
    let (ours, theirs) =
        UnixStream::pair().map_err(|error| format!("the helper socket failed: {error}"))?;
    let output = Command::new("pkexec")
        .arg(&program)
        .arg(HELPER_FLAG)
        .arg(mtu.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(theirs)))
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => {
                "TUN mode needs pkexec (polkit); install it or use the system proxy".to_string()
            }
            _ => format!("the TUN helper could not start: {error}"),
        })?;
    if !output.status.success() {
        // pkexec exits 126 when the password dialog is dismissed and 127 when it is not authorized.
        return Err(match output.status.code() {
            Some(126) | Some(127) => "TUN mode was not allowed".to_string(),
            _ => format!(
                "the TUN helper failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    let (fd, _device) = receive_descriptor(&ours)
        .map_err(|error| format!("the TUN device was not handed over: {error}"))?;
    protect_sockets(physical);
    Ok(fd)
}

/// Removes the socket protector [`open`] installed; the descriptor itself is closed by the engine.
pub fn release() {
    if PROTECTING.swap(false, Ordering::AcqRel) {
        set_socket_protector(None);
    }
}

fn protect_sockets(interface: String) {
    let name = Arc::new(interface);
    set_socket_protector(Some(Arc::new(
        move |socket: RawSocketHandle, _target: std::net::SocketAddr| bind_to_device(socket, &name),
    )));
    PROTECTING.store(true, Ordering::Release);
}

fn bind_to_device(socket: RawFd, interface: &str) -> io::Result<()> {
    // SAFETY: the name is valid for its length; the socket is the engine's, open for this call.
    let result = unsafe {
        libc::setsockopt(
            socket,
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            interface.as_ptr().cast(),
            libc::socklen_t::try_from(interface.len()).unwrap_or(0),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The program pkexec runs: the installed binary. An AppImage runs from a mount only its user can read, so it
/// cannot be its own helper.
fn helper_program() -> Result<String, String> {
    if std::env::var_os("APPIMAGE").is_some() {
        return Err(
            "TUN mode needs the .deb or tarball install; an AppImage cannot run the helper"
                .to_string(),
        );
    }
    let path = std::env::current_exe().map_err(|error| error.to_string())?;
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| "the program path is not UTF-8".to_string())
}

/// The interface of the lowest-metric IPv4 default route (`/proc/net/route`), not one of ours.
pub fn default_interface() -> Option<String> {
    let table = fs::read_to_string("/proc/net/route").ok()?;
    parse_default_interface(&table)
}

fn parse_default_interface(table: &str) -> Option<String> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (name, destination, mask, metric) = (
                fields.first()?,
                fields.get(1)?,
                fields.get(7)?,
                fields.get(6)?,
            );
            let default = *destination == "00000000" && *mask == "00000000";
            (default && !name.starts_with("anxtun"))
                .then(|| (metric.parse::<u32>().unwrap_or(u32::MAX), name.to_string()))
        })
        .min()
        .map(|(_, name)| name)
}

// ---- The helper, run as root through pkexec ----

/// Runs the helper: `args` are what follows [`HELPER_FLAG`]. The descriptor goes to standard output, which must
/// be the Unix socket the app passed; anything else fails before a device is made.
pub fn run_helper(args: &[String]) -> i32 {
    match helper(args) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            1
        }
    }
}

fn helper(args: &[String]) -> Result<(), String> {
    // SAFETY: plain getter.
    if unsafe { libc::geteuid() } != 0 {
        return Err("the TUN helper runs only through pkexec".to_string());
    }
    let mtu = match args {
        [mtu] => mtu
            .parse::<u32>()
            .ok()
            .filter(|mtu| (MIN_MTU..=MAX_MTU).contains(mtu))
            .ok_or("the MTU must be a number from 1280 to 9000")?,
        _ => return Err(format!("usage: {HELPER_FLAG} <mtu>")),
    };
    // SAFETY: fd 1 stays open for the life of the process; it is borrowed only for this check and the send.
    let out = std::mem::ManuallyDrop::new(unsafe { UnixStream::from_raw_fd(1) });
    out.peer_addr()
        .map_err(|_| "standard output must be the app's socket".to_string())?;
    let (device, name) = create_device()?;
    configure(&name, mtu)?;
    send_descriptor(&out, device.as_raw_fd(), &name)
        .map_err(|error| format!("the descriptor could not be handed over: {error}"))
    // `device` closes here; the app's copy keeps the interface.
}

fn create_device() -> Result<(OwnedFd, String), String> {
    let path = CString::new("/dev/net/tun").expect("no NUL");
    // SAFETY: a valid path and flags.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(format!("/dev/net/tun: {}", io::Error::last_os_error()));
    }
    // SAFETY: `fd` was just opened and is owned here.
    let device = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut request = InterfaceRequest::default();
    request.name[..DEVICE_PATTERN.len()].copy_from_slice(DEVICE_PATTERN.as_bytes());
    request.flags = (libc::IFF_TUN | libc::IFF_NO_PI) as libc::c_short;
    // SAFETY: `request` is a properly sized `struct ifreq`.
    if unsafe { libc::ioctl(device.as_raw_fd(), TUNSETIFF, &mut request) } < 0 {
        return Err(format!("TUNSETIFF: {}", io::Error::last_os_error()));
    }
    let end = request
        .name
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(request.name.len());
    let name = String::from_utf8_lossy(&request.name[..end]).into_owned();
    Ok((device, name))
}

fn configure(name: &str, mtu: u32) -> Result<(), String> {
    let mtu = mtu.to_string();
    ip(&["link", "set", "dev", name, "mtu", &mtu, "up"])?;
    ip(&["addr", "add", ADDRESS_V4, "dev", name])?;
    for route in ROUTES_V4 {
        ip(&["route", "add", route, "dev", name])?;
    }
    // IPv6 may be turned off on this machine; the IPv4 half still works.
    if ip(&["-6", "addr", "add", ADDRESS_V6, "dev", name]).is_ok() {
        for route in ROUTES_V6 {
            let _ = ip(&["-6", "route", "add", route, "dev", name]);
        }
    }
    Ok(())
}

fn ip(args: &[&str]) -> Result<(), String> {
    let output = Command::new("ip")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .map_err(|error| format!("ip {}: {error}", args.join(" ")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "ip {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// `_IOW('T', 202, int)`.
const TUNSETIFF: libc::c_ulong = 0x4004_54ca;

/// `struct ifreq` with the name and the flags member of its union.
#[repr(C)]
struct InterfaceRequest {
    name: [u8; libc::IFNAMSIZ],
    flags: libc::c_short,
    _rest: [u8; 22],
}

impl Default for InterfaceRequest {
    fn default() -> Self {
        Self {
            name: [0; libc::IFNAMSIZ],
            flags: 0,
            _rest: [0; 22],
        }
    }
}

/// Sends `fd` with the device name as the message.
fn send_descriptor(socket: &UnixStream, fd: RawFd, name: &str) -> io::Result<()> {
    let mut payload = name.as_bytes().to_vec();
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    // SAFETY: CMSG_SPACE is a pure size computation.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    let mut control = vec![0u8; space];
    // SAFETY: zeroed msghdr is valid; the pointers set below outlive the call.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = space as _;
    // SAFETY: `message` has a control buffer of CMSG_SPACE(int) bytes, so the first header fits.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(header).cast::<RawFd>(), fd);
    }
    // SAFETY: `message` is fully initialized.
    if unsafe { libc::sendmsg(socket.as_raw_fd(), &message, 0) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Receives the descriptor and device name [`send_descriptor`] sent.
fn receive_descriptor(socket: &UnixStream) -> io::Result<(RawFd, String)> {
    let mut payload = vec![0u8; libc::IFNAMSIZ];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    // SAFETY: CMSG_SPACE is a pure size computation.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    let mut control = vec![0u8; space];
    // SAFETY: zeroed msghdr is valid; the pointers set below outlive the call.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = space as _;
    socket.set_nonblocking(true)?;
    // SAFETY: `message` is fully initialized; MSG_CMSG_CLOEXEC marks the received descriptor close-on-exec.
    let read = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut message, libc::MSG_CMSG_CLOEXEC) };
    if read < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the kernel filled `message`; the header, when present, lies within `control`.
    let fd = unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null()
            || (*header).cmsg_level != libc::SOL_SOCKET
            || (*header).cmsg_type != libc::SCM_RIGHTS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the helper sent no descriptor",
            ));
        }
        std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<RawFd>())
    };
    let length = usize::try_from(read).unwrap_or(0).min(payload.len());
    let name = String::from_utf8_lossy(&payload[..length]).into_owned();
    // Hand the descriptor out as a plain fd: the engine's TUN module owns and closes it.
    // SAFETY: the kernel just installed `fd` in this process.
    Ok((unsafe { OwnedFd::from_raw_fd(fd) }.into_raw_fd(), name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_lowest_metric_default_route() {
        let table =
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
            anxtun0\t00000000\t00000000\t0001\t0\t0\t0\t00000000\t0\t0\t0\n\
            wlp2s0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
            enp3s0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
            enp3s0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_default_interface(table), Some("enp3s0".to_string()));
        assert_eq!(parse_default_interface("Iface\n"), None);
    }

    #[test]
    fn hands_a_descriptor_over_a_socket_pair() {
        let (left, right) = UnixStream::pair().unwrap();
        let (reader, writer) = UnixStream::pair().unwrap();
        send_descriptor(&left, writer.as_raw_fd(), "anxtun7").unwrap();
        let (fd, name) = receive_descriptor(&right).unwrap();
        assert_eq!(name, "anxtun7");
        // The received copy writes into the same socket.
        // SAFETY: `fd` was just received and is owned here.
        let mut copy = unsafe { UnixStream::from_raw_fd(fd) };
        copy.write_all(b"x").unwrap();
        let mut byte = [0u8; 1];
        use std::io::Read;
        (&reader).read_exact(&mut byte).unwrap();
        assert_eq!(&byte, b"x");
    }

    #[test]
    fn refuses_to_run_without_root_or_with_bad_arguments() {
        // Tests do not run as root in CI; when they do, the argument check still refuses.
        assert!(helper(&[]).is_err());
        assert!(helper(&["70000".to_string()]).is_err());
    }
}
