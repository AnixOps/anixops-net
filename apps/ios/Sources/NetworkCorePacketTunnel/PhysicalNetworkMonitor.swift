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

    /// Calls `changed` on the monitor's queue with the first path and every change after it.
    func start(changed: @escaping (NetworkPathKey?) -> Void) {
        stop()
        let monitor = NWPathMonitor(prohibitedInterfaceTypes: [.other, .loopback])
        self.monitor = monitor
        monitor.pathUpdateHandler = { path in
            changed(Self.key(path))
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
}
