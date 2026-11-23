import Darwin
import Foundation

/// The Mac as the gateway of other devices on the local network (macOS only).
///
/// Another device (a game console, a TV, a phone) sets this Mac's address as its router and `dnsServer` as its DNS
/// server. With IPv4 forwarding on, the Mac forwards that device's packets along its default route, which the
/// tunnel holds, so they reach the engine like the Mac's own: rules, MITM, capture and the connection list apply.
/// DNS to `dnsServer` (inside the tunnel's subnet) is answered by the engine; every UDP query to port 53 is.
///
/// It needs a mode that routes traffic through the tunnel (`TrafficTakeover.routesTraffic`) and the userspace
/// stack: a pasted WireGuard config, the whole-device tunnel, seals packets straight to its peer, which would not
/// route the device's addresses back. The app keeps the choice in its own defaults and carries a copy in the tunnel
/// profile (`TunnelProfileKeys.gateway`). The tunnel turns IPv4 forwarding on when the system lets it and back off
/// when it stops; otherwise the user runs `forwardingCommand` once.
public enum GatewayMode {
    public static let storageKey = "networkcore.gateway"
    /// The tunnel's DNS address (`TunnelInterface.proxy`), what the device enters as its DNS server.
    public static let dnsServer = "198.18.0.2"
    public static let forwardingCommand = "sudo sysctl -w net.inet.ip.forwarding=1"

    /// Whether a saved profile runs as a gateway: only on macOS, in a mode that routes traffic.
    public static func saved(in providerConfiguration: [String: Any]?, takeover: TrafficTakeover, macOS: Bool) -> Bool {
        macOS && takeover.routesTraffic && providerConfiguration?[TunnelProfileKeys.gateway] as? Bool == true
    }
}

/// The kernel's IPv4 forwarding switch (`net.inet.ip.forwarding`).
public enum IPv4Forwarding {
    static let name = "net.inet.ip.forwarding"

    /// Whether forwarding is on; nil when the value cannot be read.
    public static func isOn() -> Bool? {
        var value: Int32 = 0
        var size = MemoryLayout<Int32>.size
        guard sysctlbyname(name, &value, &size, nil, 0) == 0 else {
            return nil
        }
        return value != 0
    }

    /// Turns forwarding on or off; false when the system refuses (no privilege, or the sandbox).
    @discardableResult
    public static func set(_ on: Bool) -> Bool {
        var value: Int32 = on ? 1 : 0
        return sysctlbyname(name, nil, nil, &value, MemoryLayout<Int32>.size) == 0
    }
}

/// This device's addresses on the local network, for the gateway instructions.
public enum LocalNetworkAddresses {
    /// The private IPv4 addresses (RFC 1918) of the interfaces that are up, Ethernet and Wi-Fi (`en*`) first.
    public static func privateIPv4() -> [String] {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0, let first = head else {
            return []
        }
        defer { freeifaddrs(head) }
        var found: [(name: String, address: String)] = []
        for entry in sequence(first: first, next: { $0.pointee.ifa_next }) {
            let flags = Int32(entry.pointee.ifa_flags)
            guard flags & IFF_UP != 0, flags & IFF_LOOPBACK == 0,
                  let socket = entry.pointee.ifa_addr, socket.pointee.sa_family == UInt8(AF_INET)
            else {
                continue
            }
            var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            guard getnameinfo(socket, socklen_t(socket.pointee.sa_len), &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0
            else {
                continue
            }
            let address = String(cString: host)
            if isPrivateIPv4(address) {
                found.append((String(cString: entry.pointee.ifa_name), address))
            }
        }
        return ordered(found)
    }

    /// `en*` interfaces first, keeping the system's order otherwise, each address once.
    static func ordered(_ found: [(name: String, address: String)]) -> [String] {
        let sorted = found.filter { $0.name.hasPrefix("en") } + found.filter { !$0.name.hasPrefix("en") }
        var seen = Set<String>()
        return sorted.map(\.address).filter { seen.insert($0).inserted }
    }

    /// 10/8, 172.16/12 and 192.168/16; not the tunnel's 198.18/15, link-local or shared CGNAT space.
    public static func isPrivateIPv4(_ text: String) -> Bool {
        let parts = text.split(separator: ".").compactMap { UInt8($0) }
        guard parts.count == 4, IPAddressText.isIPv4(text) else {
            return false
        }
        switch (parts[0], parts[1]) {
        case (10, _):
            return true
        case (172, 16...31):
            return true
        case (192, 168):
            return true
        default:
            return false
        }
    }
}
