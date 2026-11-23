import Foundation

/// What the embedded engine takes in one start, mirrored from the kernel the app is built with
/// (crates/engine-embedded): the whole JSON config handed to `networkcore_ios_runtime_init` (`MAX_CONFIG_LEN` in
/// ffi.rs) and the plugin texts of HTTPS decryption together (`MAX_POLICY_BYTES` in mitm/mod.rs). Over either, the
/// engine refuses the whole config or the whole decryption section. The extension measures before handing a config
/// over, so it can leave out the most expendable extra (scripts, then decryption, then the rules) and say why,
/// instead of finding out from a bare refusal; the app checks the decryption budget when plugins or mappings grow.
public enum EngineBudget {
    /// The largest config `networkcore_ios_runtime_init` accepts, in bytes of its JSON.
    public static let maxConfigBytes = 64 * 1024
    /// The plugin texts of one tunnel, in total UTF-8 bytes (Rust's `String::len`), the debug mappings' included.
    public static let maxDecryptionBytes = 8 * 1024 * 1024

    /// A config or plugin set over its budget: `bytes` measured against `limit`.
    public struct TooLarge: Error, Equatable {
        public let bytes: Int
        public let limit: Int

        public init(bytes: Int, limit: Int) {
            self.bytes = bytes
            self.limit = limit
        }
    }

    /// `config` as the engine gets it (`EmbeddedRuntimeConfig.encoded()`), or `TooLarge` when the engine would
    /// refuse it for its size.
    public static func encoded(_ config: EmbeddedRuntimeConfig) throws -> Data {
        let data = try config.encoded()
        guard data.count <= maxConfigBytes else {
            throw TooLarge(bytes: data.count, limit: maxConfigBytes)
        }
        return data
    }

    /// What `plugins` count against `maxDecryptionBytes`.
    public static func decryptionBytes(_ plugins: [String]) -> Int {
        plugins.reduce(0) { $0 + $1.utf8.count }
    }

    /// Throws `TooLarge` when `plugins` are over `maxDecryptionBytes`.
    public static func checkDecryption(_ plugins: [String]) throws {
        let bytes = decryptionBytes(plugins)
        guard bytes <= maxDecryptionBytes else {
            throw TooLarge(bytes: bytes, limit: maxDecryptionBytes)
        }
    }
}
