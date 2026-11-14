// macOS only. Kept out of PacketTunnelProvider.swift: importing Network there makes `NWPath` ambiguous with the
// NetworkExtension class of the same name.
#if os(macOS)
import Foundation
import Network
import NetworkCoreBridge
import NetworkCoreRuntimeC

/// Keeps the engine's own sockets on the physical network in the macOS packet tunnel.
///
/// On iOS a Network Extension's sockets bypass its tunnel; a macOS packet tunnel System Extension's sockets follow
/// the routing table, so with the default route in the tunnel the engine's connections to proxy servers, DNS
/// upstreams and direct destinations would loop back into the engine and nothing would work. This watches the
/// best interface other than a tunnel (`utun` is type `other`) and hands its index to the engine
/// (`networkcore_ios_set_bound_interface`, `IP_BOUND_IF`), again on every network change.
final class BoundInterfaceMonitor {
    private let queue = DispatchQueue(label: "networkcore.packet-tunnel.bound-interface")
    private var monitor: NWPathMonitor?
    private var index: UInt32 = 0

    /// Starts watching and calls `ready` once the first interface is pinned, or after two seconds without one
    /// (the engine then runs unpinned and the diagnostics say so).
    func start(events: TunnelDiagnosticLog, ready: @escaping () -> Void) {
        stop()
        let monitor = NWPathMonitor(prohibitedInterfaceTypes: [.other, .loopback])
        self.monitor = monitor
        var pending: (() -> Void)? = ready
        let fire = {
            guard let next = pending else {
                return
            }
            pending = nil
            DispatchQueue.main.async(execute: next)
        }
        monitor.pathUpdateHandler = { [weak self] path in
            guard let self else {
                return
            }
            let next = path.status == .satisfied ? UInt32(path.availableInterfaces.first?.index ?? 0) : 0
            if next != self.index {
                self.index = next
                let code = networkcore_ios_set_bound_interface(next, next)
                if next == 0 {
                    events.record("bound interface: none")
                } else {
                    events.record("bound interface \(path.availableInterfaces.first?.name ?? "?") (code \(code))")
                }
            }
            if next != 0 {
                fire()
            }
        }
        monitor.start(queue: queue)
        queue.asyncAfter(deadline: .now() + 2) {
            if pending != nil {
                events.record("bound interface: none after 2 s, engine unpinned")
            }
            fire()
        }
    }

    func stop() {
        monitor?.cancel()
        monitor = nil
        queue.sync {
            index = 0
        }
        _ = networkcore_ios_set_bound_interface(0, 0)
    }
}
#endif
