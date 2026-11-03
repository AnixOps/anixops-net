import Foundation
import NetworkCoreBridge
import NetworkCoreRuntimeC

/// Imports a plugin with the kernel (crates/engine-embedded `networkcore_ios_mitm_inspect_plugin`) to learn how much
/// of it the runtime supports and which scripts and `[Map Local]` bodies it needs. Loon plugins, Surge modules and
/// Quantumult X snippets go through the same import as in the tunnel, so the app no longer guesses from section names.
enum MitmPluginInspector {
    static func inspect(_ text: String) -> MitmPluginInspection? {
        let bytes = Array(text.utf8)
        guard !bytes.isEmpty else {
            return nil
        }
        var buffer = NetworkCoreBuffer(ptr: nil, len: 0, cap: 0)
        let code = bytes.withUnsafeBufferPointer { input in
            networkcore_ios_mitm_inspect_plugin(input.baseAddress, input.count, &buffer)
        }
        defer { networkcore_ios_runtime_free_buffer(buffer) }
        guard code == NETWORKCORE_IOS_OK, let pointer = buffer.ptr, buffer.len > 0 else {
            return nil
        }
        return MitmPluginInspection.decode(Data(bytes: pointer, count: buffer.len))
    }
}
