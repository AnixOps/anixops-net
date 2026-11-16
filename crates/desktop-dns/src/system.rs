//! The device's own DNS servers, for the engine's `system` upstream (`dns.system_servers`).
//!
//! In TUN mode the system resolver of Windows and Linux points into the tunnel, so the engine cannot use it for
//! names the user sends to `system` (a company VPN's names, LAN names): it would ask itself. The client reads the
//! resolvers of the physical network instead, and the engine asks them directly over UDP on the physical interface.
//! Elsewhere the list is empty and the engine uses the system resolver: Android excludes the app from its own VPN,
//! and the local proxy mode leaves the system's DNS alone.

use std::net::IpAddr;

/// Most addresses the engine takes.
pub const MAX_SERVERS: usize = 8;

/// Addresses the clients give the tunnel's own DNS (`TunnelInterface`, `WindowsTunSettings`): never the device's.
const TUNNEL_DNS: [&str; 2] = ["198.18.0.2", "172.19.0.2"];

/// The device's resolvers, best first, as the engine writes them; empty when none can be read.
pub fn servers() -> Vec<String> {
    usable(platform_servers())
}

/// Keeps addresses the engine can reach directly: no loopback stub (it would ask the tunnel again), no address
/// that needs a scope, no placeholder, none twice, at most [`MAX_SERVERS`].
fn usable(found: Vec<IpAddr>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for address in found {
        let reachable = match address {
            IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_unspecified() && !v4.is_multicast(),
            IpAddr::V6(v6) => {
                let first = v6.segments()[0];
                !v6.is_loopback()
                    && !v6.is_unspecified()
                    && !v6.is_multicast()
                    // Link-local needs a scope id; fec0::/10 is Windows' placeholder when no IPv6 DNS is set.
                    && first & 0xffc0 != 0xfe80
                    && first & 0xffc0 != 0xfec0
            }
        };
        let text = address.to_string();
        if reachable && !TUNNEL_DNS.contains(&text.as_str()) && !kept.contains(&text) {
            kept.push(text);
        }
        if kept.len() == MAX_SERVERS {
            break;
        }
    }
    kept
}

/// `nameserver` lines of a resolv.conf.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn nameservers(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match words.next() {
                Some("nameserver") => words.next()?.parse().ok(),
                _ => None,
            }
        })
        .collect()
}

/// systemd-resolved keeps the real upstreams here while /etc/resolv.conf points at its stub, 127.0.0.53.
#[cfg(target_os = "linux")]
fn platform_servers() -> Vec<IpAddr> {
    for path in ["/run/systemd/resolve/resolv.conf", "/etc/resolv.conf"] {
        if let Ok(text) = std::fs::read_to_string(path) {
            let found = nameservers(&text);
            if !usable(found.clone()).is_empty() {
                return found;
            }
        }
    }
    Vec::new()
}

#[cfg(windows)]
fn platform_servers() -> Vec<IpAddr> {
    windows::servers()
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_servers() -> Vec<IpAddr> {
    Vec::new()
}

#[cfg(windows)]
mod windows {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST,
        GAA_FLAG_SKIP_UNICAST, IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKET_ADDRESS,
    };

    /// The TUN adapter's name (`WindowsTunSettings::adapter_name`): its DNS is the tunnel's.
    const TUN_ADAPTER: &str = "AnixOps Net";

    /// The DNS servers of the adapters that are up, those with a default gateway first.
    pub fn servers() -> Vec<IpAddr> {
        let Some(buffer) = adapters() else {
            return Vec::new();
        };
        let mut with_gateway = Vec::new();
        let mut others = Vec::new();
        // SAFETY: `buffer` holds the list GetAdaptersAddresses wrote; every pointer in it points into the buffer
        // or is null, and the buffer outlives the walk.
        unsafe {
            let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
            while !adapter.is_null() {
                let current = &*adapter;
                adapter = current.Next;
                if current.OperStatus != IfOperStatusUp
                    || current.IfType == IF_TYPE_SOFTWARE_LOOPBACK
                    || wide(current.FriendlyName) == TUN_ADAPTER
                {
                    continue;
                }
                let list = if current.FirstGatewayAddress.is_null() {
                    &mut others
                } else {
                    &mut with_gateway
                };
                let mut server = current.FirstDnsServerAddress;
                while !server.is_null() {
                    if let Some(address) = address_of(&(*server).Address) {
                        list.push(address);
                    }
                    server = (*server).Next;
                }
            }
        }
        with_gateway.extend(others);
        with_gateway
    }

    /// The adapter list in a buffer aligned for `IP_ADAPTER_ADDRESSES_LH`.
    fn adapters() -> Option<Vec<u64>> {
        let flags = GAA_FLAG_SKIP_UNICAST
            | GAA_FLAG_SKIP_ANYCAST
            | GAA_FLAG_SKIP_MULTICAST
            | GAA_FLAG_INCLUDE_GATEWAYS;
        let mut size: u32 = 16 * 1024;
        for _ in 0..4 {
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            // SAFETY: the buffer is `size` bytes or more and aligned to 8 bytes.
            let result = unsafe {
                GetAdaptersAddresses(
                    u32::from(AF_UNSPEC),
                    flags,
                    std::ptr::null(),
                    buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                    &mut size,
                )
            };
            match result {
                NO_ERROR => return Some(buffer),
                ERROR_BUFFER_OVERFLOW => continue,
                _ => return None,
            }
        }
        None
    }

    /// SAFETY: `address` comes from GetAdaptersAddresses.
    unsafe fn address_of(address: &SOCKET_ADDRESS) -> Option<IpAddr> {
        let raw = address.lpSockaddr as *const SOCKADDR;
        if raw.is_null() {
            return None;
        }
        match (*raw).sa_family {
            AF_INET => {
                let v4 = &*(raw as *const SOCKADDR_IN);
                // The address is in network byte order.
                Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(v4.sin_addr.S_un.S_addr))))
            }
            AF_INET6 => {
                let v6 = &*(raw as *const SOCKADDR_IN6);
                Some(IpAddr::V6(Ipv6Addr::from(v6.sin6_addr.u.Byte)))
            }
            _ => None,
        }
    }

    /// SAFETY: `text` is null or a NUL-terminated UTF-16 string.
    unsafe fn wide(text: *const u16) -> String {
        if text.is_null() {
            return String::new();
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolv_conf_nameservers_are_read() {
        let text = "# generated\nnameserver 127.0.0.53\nnameserver 192.168.1.1\nsearch lan\nnameserver fe80::1%eth0\nnameserver 2001:db8::53\n";
        assert_eq!(
            usable(nameservers(text)),
            vec!["192.168.1.1".to_string(), "2001:db8::53".to_string()]
        );
    }

    #[test]
    fn only_reachable_addresses_are_kept() {
        let found: Vec<IpAddr> = [
            "10.0.0.53",
            "10.0.0.53",
            "::1",
            "fe80::1",
            "fec0:0:0:ffff::1",
            "198.18.0.2",
            "172.19.0.2",
            "0.0.0.0",
        ]
        .iter()
        .map(|text| text.parse().unwrap())
        .collect();
        assert_eq!(usable(found), vec!["10.0.0.53".to_string()]);
        let many: Vec<IpAddr> = (1..=20).map(|i| IpAddr::from([10, 0, 0, i])).collect();
        assert_eq!(usable(many).len(), MAX_SERVERS);
    }
}
