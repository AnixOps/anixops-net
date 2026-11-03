import Foundation
import NetworkCoreBridge
import NetworkCoreRuntimeC
import NetworkExtension

/// One embedded Rust runtime inside the packet tunnel process.
///
/// Packets read from `packetFlow` go to the runtime; packets the runtime emits are written back on the
/// runtime's own thread. `stop()` joins that thread, so no packet is written after it returns.
final class EmbeddedRuntimeSession {
    /// Failure carrying only the C ABI code, never config content.
    struct Failure: Error {
        let code: Int32

        var fact: IosEmbeddedRuntimeFact {
            NetworkCoreRuntimeFFI.embeddedRuntimeFact(forFailureCode: code)
        }
    }

    static func reportedHandshake() -> NetworkCoreRuntimeFFI.Handshake {
        NetworkCoreRuntimeFFI.Handshake(
            abiVersion: networkcore_ios_abi_version(),
            dtoSchemaVersion: networkcore_ios_schema_version(),
            featureSetVersion: networkcore_ios_feature_set_version()
        )
    }

    private let packetFlow: NEPacketTunnelFlow
    private let lock = NSLock()
    private var handle: OpaquePointer?
    private var running = false
    private var writer: Unmanaged<PacketWriter>?

    init(config: Data, packetFlow: NEPacketTunnelFlow) throws {
        self.packetFlow = packetFlow
        var created: OpaquePointer?
        let code = config.withUnsafeBytes { raw -> Int32 in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else {
                return NetworkCoreRuntimeFFI.Code.invalidArgument
            }
            return networkcore_ios_runtime_init(base, raw.count, &created)
        }
        guard code == NetworkCoreRuntimeFFI.Code.ok, let created = created else {
            throw Failure(code: code)
        }
        handle = created
    }

    deinit {
        stop()
        if let handle = handle {
            networkcore_ios_runtime_free_handle(handle)
        }
    }

    func start() throws {
        lock.lock()
        defer { lock.unlock() }
        guard let handle = handle, !running else {
            throw Failure(code: NetworkCoreRuntimeFFI.Code.invalidArgument)
        }
        let writer = Unmanaged.passRetained(PacketWriter(packetFlow: packetFlow))
        // Batched writer (feature set 4): one callback and one writePackets call per burst of packets.
        let code = networkcore_ios_runtime_start_tunnel_batched(handle, { context, packets, count in
            guard let context = context, let packets = packets, count > 0 else {
                return
            }
            Unmanaged<PacketWriter>.fromOpaque(context).takeUnretainedValue()
                .write(UnsafeBufferPointer(start: packets, count: count))
        }, writer.toOpaque())
        guard code == NetworkCoreRuntimeFFI.Code.ok else {
            writer.release()
            throw Failure(code: code)
        }
        self.writer = writer
        running = true
        readPackets()
    }

    func stop() {
        lock.lock()
        defer { lock.unlock() }
        guard running, let handle = handle else {
            return
        }
        running = false
        _ = networkcore_ios_runtime_stop_tunnel(handle)
        writer?.release()
        writer = nil
    }

    /// Sanitized status JSON from the runtime, for the app's status view.
    func statusJSON() -> Data? {
        lock.lock()
        defer { lock.unlock() }
        guard let handle = handle else {
            return nil
        }
        var buffer = NetworkCoreBuffer(ptr: nil, len: 0, cap: 0)
        guard networkcore_ios_runtime_collect_status(handle, &buffer) == NetworkCoreRuntimeFFI.Code.ok else {
            return nil
        }
        defer { networkcore_ios_runtime_free_buffer(buffer) }
        guard let pointer = buffer.ptr, buffer.len > 0 else {
            return nil
        }
        return Data(bytes: pointer, count: buffer.len)
    }

    /// Answers a runtime control request (`RuntimeControl`) with the runtime's JSON; nil when it refuses it.
    func control(_ request: Data) -> Data? {
        lock.lock()
        defer { lock.unlock() }
        guard let handle = handle else {
            return nil
        }
        var buffer = NetworkCoreBuffer(ptr: nil, len: 0, cap: 0)
        let code = request.withUnsafeBytes { raw -> Int32 in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else {
                return NetworkCoreRuntimeFFI.Code.invalidArgument
            }
            return networkcore_ios_runtime_control(handle, base, raw.count, &buffer)
        }
        guard code == NetworkCoreRuntimeFFI.Code.ok else {
            return nil
        }
        defer { networkcore_ios_runtime_free_buffer(buffer) }
        guard let pointer = buffer.ptr, buffer.len > 0 else {
            return nil
        }
        return Data(bytes: pointer, count: buffer.len)
    }

    private func runningHandle() -> OpaquePointer? {
        lock.lock()
        defer { lock.unlock() }
        return running ? handle : nil
    }

    private func readPackets() {
        packetFlow.readPackets { [weak self] packets, _ in
            guard let self = self, let handle = self.runningHandle() else {
                return
            }
            // One FFI call per chunk instead of one per packet. A full queue drops packets; TCP retransmits them.
            var start = packets.startIndex
            while start < packets.endIndex {
                let end = min(start + Self.inputChunk, packets.endIndex)
                var views: [NetworkCorePacket] = []
                views.reserveCapacity(end - start)
                Self.withPacketViews(packets[start..<end], into: &views) { views in
                    views.withUnsafeBufferPointer { buffer in
                        _ = networkcore_ios_runtime_input_packets(handle, buffer.baseAddress, buffer.count, nil)
                    }
                }
                start = end
            }
            self.readPackets()
        }
    }

    /// Packets per `input_packets` call; also bounds the recursion below.
    private static let inputChunk = 64

    /// Pins each packet's bytes for the duration of `body`, which sees one pointer/length view per non-empty packet.
    private static func withPacketViews(
        _ packets: ArraySlice<Data>,
        into views: inout [NetworkCorePacket],
        _ body: ([NetworkCorePacket]) -> Void
    ) {
        guard let first = packets.first else {
            body(views)
            return
        }
        first.withUnsafeBytes { raw in
            if let base = raw.bindMemory(to: UInt8.self).baseAddress, raw.count > 0 {
                views.append(NetworkCorePacket(ptr: base, len: raw.count))
            }
            withPacketViews(packets.dropFirst(), into: &views, body)
        }
    }
}

/// Writer context handed to the runtime. Retained while the tunnel runs.
private final class PacketWriter {
    private let packetFlow: NEPacketTunnelFlow
    private let ipv4 = NSNumber(value: AF_INET)
    private let ipv6 = NSNumber(value: AF_INET6)

    init(packetFlow: NEPacketTunnelFlow) {
        self.packetFlow = packetFlow
    }

    /// Called once per burst on the runtime's own long-lived thread, which has no autorelease pool of its own: without the
    /// explicit pool every packet's bridged array, number and data objects stay alive until the thread exits, so
    /// memory grows with traffic and a heavy download pushes the extension past the system memory limit
    /// ("The VPN session failed because an internal error occurred").
    func write(_ packets: UnsafeBufferPointer<NetworkCorePacket>) {
        autoreleasepool {
            var datas: [Data] = []
            var families: [NSNumber] = []
            datas.reserveCapacity(packets.count)
            families.reserveCapacity(packets.count)
            for packet in packets {
                guard let pointer = packet.ptr, packet.len > 0 else {
                    continue
                }
                families.append((pointer.pointee >> 4) == 6 ? ipv6 : ipv4)
                datas.append(Data(bytes: pointer, count: packet.len))
            }
            if !datas.isEmpty {
                packetFlow.writePackets(datas, withProtocols: families)
            }
        }
    }
}
