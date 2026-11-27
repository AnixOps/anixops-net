import Foundation

/// What the embedded engine takes in one start, mirrored from the kernel the app is built with
/// (crates/engine-embedded): the whole JSON config handed to `networkcore_ios_runtime_init` (`MAX_CONFIG_LEN` in
/// ffi.rs) and the plugin texts of HTTPS decryption together (`MAX_POLICY_BYTES` in mitm/mod.rs). Over either, the
/// engine refuses the whole config or the whole decryption section. The extension measures before handing a config
/// over, so it can leave out the most expendable extra (scripts, then decryption, then the rules) and say why,
/// instead of finding out from a bare refusal; the app checks the decryption budget when plugins or mappings grow,
/// and warns when the plugins alone cannot fit in the config (`decryptionExceedsConfig`).
///
/// `maxConfigBytes` must follow the kernel the app pins: when the kernel raises `MAX_CONFIG_LEN` or takes the
/// decryption and routing sections as files, change it in the same commit as the kernel rev
/// (docs/manual-intervention.md, "iOS / macOS 引擎配置 64 KiB 上限").
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
    /// refuse it for its size. Every config the extension hands the engine goes through here, so each carries
    /// `allow_mesh` set to the release gate (`withAllowMesh`).
    public static func encoded(_ config: EmbeddedRuntimeConfig) throws -> Data {
        let data = try config.withAllowMesh().encoded()
        guard data.count <= maxConfigBytes else {
            throw TooLarge(bytes: data.count, limit: maxConfigBytes)
        }
        return data
    }

    /// What `plugins` count against `maxDecryptionBytes`.
    public static func decryptionBytes(_ plugins: [String]) -> Int {
        plugins.reduce(0) { $0 + $1.utf8.count }
    }

    /// Whether HTTPS decryption with `plugins` (the texts a tunnel hands over, the debug mappings' plugin included)
    /// can never fit in `maxConfigBytes`. The config's `mitm` section carries each text as a JSON string, which is
    /// never shorter than the text, so over the limit the extension always leaves decryption out
    /// (`HttpsDecryptionState.Reason.tooLarge`) although the texts are within `maxDecryptionBytes`. Under it the
    /// node, the rules and the root take their share of the same limit; the requests page reports that case after
    /// connecting. The app warns with this before connecting, on the plugins and mappings pages.
    public static func decryptionExceedsConfig(_ plugins: [String]) -> Bool {
        guard !plugins.isEmpty else {
            return false
        }
        let bytes = decryptionBytes(plugins)
        guard bytes <= maxConfigBytes else {
            return true
        }
        // Within the limit as text: measure the escaped strings, as the config encodes them.
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let escaped = (try? encoder.encode(plugins).count) ?? bytes
        return escaped > maxConfigBytes
    }

    /// Throws `TooLarge` when `plugins` are over `maxDecryptionBytes`.
    public static func checkDecryption(_ plugins: [String]) throws {
        let bytes = decryptionBytes(plugins)
        guard bytes <= maxDecryptionBytes else {
            throw TooLarge(bytes: bytes, limit: maxDecryptionBytes)
        }
    }
}
