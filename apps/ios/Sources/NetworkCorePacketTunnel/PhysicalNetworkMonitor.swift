// Kept out of PacketTunnelProvider.swift: importing Network there makes `NWPath` ambiguous with the NetworkExtension
// class of the same name.
import Foundation
import Network
import NetworkCoreBridge

/// Watches the physical network under the tunnel (tunnel interfaces are type `other` and left out) and reports each
/// change as a `NetworkPathKey`, nil while there is no usable network. Runs on iOS and macOS.
final class PhysicalNetworkMonitor {
    private let queue = DispatchQueue(label: "networkcore.packet-tunnel.physical-network")
    private var monitor: NWPathMonitor?

    /// Calls `changed` on the monitor's queue with the first path and every change after it, and what the engine's
    /// `subnet` groups match on it (nil while there is no usable network).
    func start(changed: @escaping (NetworkPathKey?, NetworkFacts?) -> Void) {
        stop()
        let monitor = NWPathMonitor(prohibitedInterfaceTypes: [.other, .loopback])
        self.monitor = monitor
        monitor.pathUpdateHandler = { path in
            changed(Self.key(path), Self.facts(path))
        }
        monitor.start(queue: queue)
    }

    func stop() {
        monitor?.cancel()
        monitor = nil
    }

    private static func key(_ path: Network.NWPath) -> NetworkPathKey? {
        guard path.status == .satisfied, let interface = path.availableInterfaces.first else {
            return nil
        }
        return NetworkPathKey(interface: interface.name, gateways: path.gateways.map { "\($0)" })
    }

    private static func facts(_ path: Network.NWPath) -> NetworkFacts? {
        guard path.status == .satisfied, let interface = path.availableInterfaces.first else {
            return nil
        }
        let kind: String?
        switch interface.type {
        case .wifi:
            kind = "wifi"
        case .wiredEthernet:
            kind = "wired"
        case .cellular:
            kind = "cellular"
        default:
            kind = nil
        }
        let gateways = path.gateways.compactMap(host)
        let router = gateways.first { !$0.contains(":") } ?? gateways.first
        return NetworkFacts(kind: kind, router: router, addresses: addresses(of: interface.name))
    }

    /// The address of a gateway endpoint, without an IPv6 zone.
    private static func host(_ endpoint: NWEndpoint) -> String? {
        guard case let .hostPort(host, _) = endpoint else {
            return nil
        }
        switch host {
        case let .ipv4(address):
            return "\(address)"
        case let .ipv6(address):
            return "\(address)".split(separator: "%").first.map(String.init)
        default:
            return nil
        }
    }

    /// The IPv4 and IPv6 addresses of interface `name` (link-local IPv6 left out), at most `NetworkFacts.maxAddresses`.
    private static func addresses(of name: String) -> [String] {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0, let first = head else {
            return []
        }
        defer { freeifaddrs(head) }
        var found: [String] = []
        for entry in sequence(first: first, next: { $0.pointee.ifa_next }) {
            let item = entry.pointee
            guard let address = item.ifa_addr, String(cString: item.ifa_name) == name else {
                continue
            }
            let family = Int32(address.pointee.sa_family)
            guard family == AF_INET || family == AF_INET6 else {
                continue
            }
            let length = socklen_t(family == AF_INET ? MemoryLayout<sockaddr_in>.size : MemoryLayout<sockaddr_in6>.size)
            var text = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            guard getnameinfo(address, length, &text, socklen_t(text.count), nil, 0, NI_NUMERICHOST) == 0 else {
                continue
            }
            let ip = String(cString: text).split(separator: "%").first.map(String.init) ?? ""
            if ip.isEmpty || ip.lowercased().hasPrefix("fe80") {
                continue
            }
            found.append(ip)
            if found.count == NetworkFacts.maxAddresses {
                break
            }
        }
        return found
    }
}
