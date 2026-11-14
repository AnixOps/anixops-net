import CryptoKit
import Foundation

/// The HTTPS decryption payload of the tunnel profile: the enabled plugin texts, the scripts the user allowed in
/// them and their downloaded `[Map Local]` bodies.
///
/// iOS refuses to save a VPN configuration whose `providerConfiguration` is too large, before it even asks for
/// permission, so a few MB of plugins made every save fail ("Saving the VPN configuration failed"). The payload
/// therefore lives in the shared Keychain group, which the app and the packet tunnel extension both read, split
/// into chunks; the profile only carries a small `Reference` (digest and chunk count) under
/// `TunnelProfileKeys.mitmBundle`. The digest also makes the profile change whenever the payload changes.
public struct MitmProfileBundle: Codable, Equatable {
    public var plugins: [String]
    public var scripts: [String: String]
    public var resources: [String: Data]

    public init(plugins: [String] = [], scripts: [String: String] = [:], resources: [String: Data] = [:]) {
        self.plugins = plugins
        // Scripts and bodies only matter with a plugin that uses them.
        self.scripts = plugins.isEmpty ? [:] : scripts
        self.resources = plugins.isEmpty ? [:] : resources
    }

    public var isEmpty: Bool {
        plugins.isEmpty
    }

    /// What the profile stores under `TunnelProfileKeys.mitmBundle`.
    struct Reference: Codable, Equatable {
        var digest: String
        var chunks: Int
    }

    static let accountPrefix = "networkcore.mitm-bundle."
    /// Keeps each Keychain item small; the payload is compressed first.
    static let chunkBytes = 256 * 1024

    func packed() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return try ProfileCompression.pack(encoder.encode(self))
    }

    static func digest(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    static func chunks(of packed: Data) -> [Data] {
        stride(from: 0, to: max(packed.count, 1), by: chunkBytes).map {
            Data(packed[$0..<min($0 + chunkBytes, packed.count)])
        }
    }

    /// The profile value for this payload, without touching the Keychain.
    public func profileValue() throws -> Data {
        let packed = try packed()
        return try JSONEncoder().encode(Reference(digest: Self.digest(packed), chunks: Self.chunks(of: packed).count))
    }

    /// Saves the payload to the Keychain and returns the profile value that points at it. Chunks left over from a
    /// larger earlier payload are removed.
    public func save() throws -> Data {
        let packed = try packed()
        let chunks = Self.chunks(of: packed)
        for (index, chunk) in chunks.enumerated() {
            _ = try NodeSecretStore.save(chunk.base64EncodedString(), account: Self.accountPrefix + String(index))
        }
        Self.removeChunks(from: chunks.count)
        return try JSONEncoder().encode(Reference(digest: Self.digest(packed), chunks: chunks.count))
    }

    /// The saved chunks in order, as Keychain values, for `TunnelStartSecrets`; nil when there are none.
    public static func storedChunks() -> [String]? {
        var chunks: [String] = []
        while let text = try? NodeSecretStore.value(account: accountPrefix + String(chunks.count)) {
            chunks.append(text)
        }
        return chunks.isEmpty ? nil : chunks
    }

    /// Removes every saved chunk, for a profile without HTTPS decryption.
    public static func clear() {
        removeChunks(from: 0)
    }

    private static func removeChunks(from first: Int) {
        var index = first
        while (try? NodeSecretStore.value(account: accountPrefix + String(index))) != nil {
            try? NodeSecretStore.delete(account: accountPrefix + String(index))
            index += 1
        }
    }

    /// Reads the payload a profile value points at. Nil when the value is absent or malformed, or when the
    /// Keychain no longer holds what the digest names.
    ///
    /// `chunks` replaces the Keychain read with the chunks a macOS app handed to its System Extension
    /// (`TunnelStartSecrets`), which cannot read the user's Keychain.
    public static func load(fromProfileValue value: Data?, chunks: [String]? = nil) -> MitmProfileBundle? {
        guard let value, let reference = try? JSONDecoder().decode(Reference.self, from: value), reference.chunks > 0
        else {
            return nil
        }
        var packed = Data()
        for index in 0..<reference.chunks {
            let text: String?
            if let chunks {
                text = index < chunks.count ? chunks[index] : nil
            } else {
                text = try? NodeSecretStore.value(account: accountPrefix + String(index))
            }
            guard let text,
                  let chunk = Data(base64Encoded: text)
            else {
                return nil
            }
            packed.append(chunk)
        }
        guard digest(packed) == reference.digest, let raw = ProfileCompression.unpack(packed) else {
            return nil
        }
        return try? JSONDecoder().decode(MitmProfileBundle.self, from: raw)
    }

    /// The payload of a saved profile: the Keychain bundle, or the plugin keys profiles carried before it existed.
    public static func fromProfile(_ configuration: [String: Any]?, chunks: [String]? = nil) -> MitmProfileBundle {
        if let value = configuration?[TunnelProfileKeys.mitmBundle] as? Data {
            return load(fromProfileValue: value, chunks: chunks) ?? MitmProfileBundle()
        }
        return MitmProfileBundle(
            plugins: MitmPluginLibrary.texts(fromProfileValue: configuration?[TunnelProfileKeys.mitmPlugins] as? Data),
            scripts: MitmScriptLibrary.sources(fromProfileValue: configuration?[TunnelProfileKeys.mitmScripts] as? Data),
            resources: MitmResourceLibrary.data(
                fromProfileValue: configuration?[TunnelProfileKeys.mitmResources] as? Data
            )
        )
    }

    /// Puts this payload into `configuration`: saves it to the Keychain, sets `mitmBundle` and drops the plugin
    /// keys of older profiles, which would keep the configuration too large to save.
    public func apply(to configuration: inout [String: Any]) throws {
        configuration.removeValue(forKey: TunnelProfileKeys.mitmPlugins)
        configuration.removeValue(forKey: TunnelProfileKeys.mitmScripts)
        configuration.removeValue(forKey: TunnelProfileKeys.mitmResources)
        if isEmpty {
            configuration.removeValue(forKey: TunnelProfileKeys.mitmBundle)
            Self.clear()
        } else {
            configuration[TunnelProfileKeys.mitmBundle] = try save()
        }
    }
}
