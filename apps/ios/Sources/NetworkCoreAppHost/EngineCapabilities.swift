import Foundation
import NetworkCoreBridge
import NetworkCoreRuntimeC

/// Asks the linked engine what it can do (`{"cmd":"capabilities"}`) in the app's own process, before any tunnel runs,
/// so the 组网 entry can show while disconnected (`MeshSupport.available`). The engine in the probe is built with the
/// release gate's `allow_mesh` (`EmbeddedRuntimeConfig.withAllowMesh`), never started, and holds no mesh outbound.
enum EngineCapabilities {
    /// Gives `MeshSupport` its probe; the app calls it once at launch.
    static func install() {
        MeshSupport.installCapabilitiesProbe { answer() }
    }

    /// The engine's answer to `MeshSupport.capabilitiesRequest`; nil when it could not be asked.
    static func answer() -> Data? {
        let envelope: [String: Any] = EmbeddedRuntimeConfig.withAllowMesh([
            "schema_version": EmbeddedRuntimeConfig.schemaVersion,
            "profile_id": "capabilities-probe",
            "outbound": ["type": "direct"],
        ])
        guard let config = try? JSONSerialization.data(withJSONObject: envelope) else {
            return nil
        }
        var handle: OpaquePointer?
        let created = config.withUnsafeBytes { raw -> Int32 in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else {
                return NetworkCoreRuntimeFFI.Code.invalidArgument
            }
            return networkcore_ios_runtime_init(base, raw.count, &handle)
        }
        guard created == NetworkCoreRuntimeFFI.Code.ok, let handle else {
            return nil
        }
        defer { networkcore_ios_runtime_free_handle(handle) }
        var buffer = NetworkCoreBuffer(ptr: nil, len: 0, cap: 0)
        let code = MeshSupport.capabilitiesRequest.withUnsafeBytes { (input: UnsafeRawBufferPointer) -> Int32 in
            networkcore_ios_runtime_control(
                handle, input.bindMemory(to: UInt8.self).baseAddress, input.count, &buffer
            )
        }
        defer { networkcore_ios_runtime_free_buffer(buffer) }
        guard code == NetworkCoreRuntimeFFI.Code.ok else {
            return nil
        }
        return buffer.ptr.map { Data(bytes: $0, count: buffer.len) }
    }
}
