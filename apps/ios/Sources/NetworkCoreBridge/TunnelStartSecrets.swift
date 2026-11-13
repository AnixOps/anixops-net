import Foundation

/// The Keychain values the packet tunnel needs, handed over at start on macOS.
///
/// A Developer ID build runs the tunnel as a System Extension: a root process that cannot read the user's
/// Keychain, where the app keeps the node key, the node library, the MITM root and the HTTPS decryption payload.
/// The app reads them itself and passes them in the `startVPNTunnel(options:)` dictionary. The extension keeps the
/// last copy in its own container, readable by root only, for the starts the system makes without options (on
/// demand, after sleep, from System Settings). Nothing here is ever logged.
public struct TunnelStartSecrets: Codable, Equatable {
    /// The selected node's key (`NodeSecretStore.tunnelProfileAccount`).
    public var nodeKey: String?
    /// The node library with the subscription's rules and groups (`NodeLibrary.keychainAccount`).
    public var nodeLibrary: String?
    /// The MITM root certificate and key, as `MitmCertificateStore` keeps them.
    public var mitmAuthority: Data?
    /// The HTTPS decryption payload's Keychain chunks (`MitmProfileBundle`), in order.
    public var mitmBundleChunks: [String]?
    /// This device's ANX license state (`AnxLicense.keychainAccount`).
    public var anxLicense: String?

    public init(
        nodeKey: String? = nil,
        nodeLibrary: String? = nil,
        mitmAuthority: Data? = nil,
        mitmBundleChunks: [String]? = nil,
        anxLicense: String? = nil
    ) {
        self.nodeKey = nodeKey
        self.nodeLibrary = nodeLibrary
        self.mitmAuthority = mitmAuthority
        self.mitmBundleChunks = mitmBundleChunks
        self.anxLicense = anxLicense
    }

    static let optionsKey = "networkcore.start-secrets"

    /// File name of the extension's copy, under its Application Support directory.
    public static let fileName = "start-secrets.json"

    public var isEmpty: Bool {
        nodeKey == nil && nodeLibrary == nil && mitmAuthority == nil && mitmBundleChunks == nil && anxLicense == nil
    }

    /// Reads the values from the Keychain; a value that cannot be read is left out, and the extension reports
    /// what is missing the same way as when its own Keychain read fails.
    public static func fromKeychain() -> TunnelStartSecrets {
        TunnelStartSecrets(
            nodeKey: try? NodeSecretStore.value(account: NodeSecretStore.tunnelProfileAccount),
            nodeLibrary: try? NodeSecretStore.value(account: NodeLibrary.keychainAccount),
            mitmAuthority: try? MitmCertificateStore.storedValue(),
            mitmBundleChunks: MitmProfileBundle.storedChunks(),
            anxLicense: try? NodeSecretStore.value(account: AnxLicense.keychainAccount)
        )
    }

    /// The `startVPNTunnel(options:)` dictionary.
    public var options: [String: NSObject] {
        guard let data = try? JSONEncoder().encode(self) else {
            return [:]
        }
        return [Self.optionsKey: data as NSData]
    }

    /// The values the app passed to `startVPNTunnel(options:)`, or nil when it passed none.
    public init?(options: [String: NSObject]?) {
        guard let data = options?[Self.optionsKey] as? Data,
              let decoded = try? JSONDecoder().decode(TunnelStartSecrets.self, from: data)
        else {
            return nil
        }
        self = decoded
    }

    /// Writes the copy atomically with mode 0600.
    public func save(to url: URL) throws {
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true
        )
        try JSONEncoder().encode(self).write(to: url, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }

    /// The saved copy, or nil when there is none or it does not parse.
    public static func load(from url: URL) -> TunnelStartSecrets? {
        guard let data = try? Data(contentsOf: url) else {
            return nil
        }
        return try? JSONDecoder().decode(TunnelStartSecrets.self, from: data)
    }
}
