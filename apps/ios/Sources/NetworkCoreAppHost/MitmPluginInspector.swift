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

extension MitmPluginLibrary {
    /// The saved plugin library with every plugin inspected by the kernel, for views that show which hosts plugins
    /// decrypt. Plugins saved before the app inspected them are inspected once and saved, as the plugin page
    /// (`PluginsModel.load`) does, so a view opened before that page counts their hosts as the engine reads them.
    /// Callers are views on the main thread, as the plugin page is, so reading and saving happen in one step that no
    /// other change to the file can come in between.
    static func loadInspected() -> MitmPluginLibrary {
        guard var library = try? load(from: defaultURL) else {
            return MitmPluginLibrary()
        }
        if library.inspectMissing(MitmPluginInspector.inspect) {
            try? library.save(to: defaultURL)
        }
        return library
    }
}
