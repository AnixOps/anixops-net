import Foundation
import Security

/// The Keychain values the packet tunnel needs, handed over at start on macOS.
///
/// A Developer ID build runs the tunnel as a System Extension: a root process that cannot read the user's
/// Keychain, where the app keeps the node key, the node library, the MITM root and the HTTPS decryption payload.
/// The app reads them itself and passes them in the `startVPNTunnel(options:)` dictionary. The extension keeps the
/// last copy in its own container, readable by root only, for the starts the system makes without options (on
/// demand, after sleep, from System Settings). Whenever the app saves the profile, the node library, the HTTPS
/// decryption payload, the MITM root or the license again while the tunnel runs, it hands the new values over in a
/// message (`TunnelAppMessage.startSecrets`) so that copy follows the saved profile. Nothing here is ever logged.
public struct TunnelStartSecrets: Codable, Equatable {
    /// The selected node's key: the saved profile's own (`NodeSecretStore.versionedAccount`), or the fixed account
    /// for a profile saved before keys had versions.
    public var nodeKey: String?
    /// The key version `nodeKey` belongs to (`TunnelProfileKeys.nodeKeyVersion`); nil for the fixed account. The
    /// extension uses `nodeKey` only for a profile of the same version.
    public var nodeKeyVersion: String?
    /// The node library with the subscription's rules and groups (`NodeLibrary.keychainAccount`).
    public var nodeLibrary: String?
    /// The MITM root certificate and key, as `MitmCertificateStore` keeps them.
    public var mitmAuthority: Data?
    /// The HTTPS decryption payload's Keychain chunks (`MitmProfileBundle`), in order.
    public var mitmBundleChunks: [String]?
    /// This device's ANX license state (`AnxLicense.keychainAccount`).
    public var anxLicense: String?
    /// The joined mesh networks' secrets by network name (`MeshNetworkLibrary.secretAccount`).
    public var meshSecrets: [String: String]?

    public init(
        nodeKey: String? = nil,
        nodeLibrary: String? = nil,
        mitmAuthority: Data? = nil,
        mitmBundleChunks: [String]? = nil,
        anxLicense: String? = nil,
        nodeKeyVersion: String? = nil,
        meshSecrets: [String: String]? = nil
    ) {
        self.nodeKey = nodeKey
        self.nodeKeyVersion = nodeKeyVersion
        self.nodeLibrary = nodeLibrary
        self.mitmAuthority = mitmAuthority
        self.mitmBundleChunks = mitmBundleChunks
        self.anxLicense = anxLicense
        self.meshSecrets = meshSecrets
    }

    static let optionsKey = "networkcore.start-secrets"

    /// File name of the extension's copy, under its Application Support directory.
    public static let fileName = "start-secrets.json"

    /// The handed-over node key for a profile saved with key version `version` (nil for a profile from before key
    /// versions): only a key of that same version, never one handed over for another profile.
    public func nodeKey(forVersion version: String?) -> String? {
        nodeKeyVersion == version ? nodeKey : nil
    }

    /// The extension's failure reason when it cannot read the node key of a profile saved with key version
    /// `version`. A copy handed over for another version means the profile was saved again after the copy was
    /// written and the app could not hand the new one over (the tunnel was not running): the app has to start the
    /// tunnel once with options. Anything else is a Keychain failure. Fixed text and codes only, never a key.
    public static func nodeKeyFailureReason(
        status: OSStatus,
        handedOver: TunnelStartSecrets?,
        version: String?
    ) -> String {
        if let handedOver, handedOver.nodeKey != nil, version != nil, handedOver.nodeKeyVersion != version {
            return staleCopyReason
        }
        return "node key is unavailable (keychain status \(status))"
    }

    /// The extension's failure reason when the profile carries an HTTPS decryption payload
    /// (`TunnelProfileKeys.mitmBundle`, `profileValue`) that the copy kept from an earlier start (`keptCopy`; nil
    /// when the app passed the values in this start's options, or on iOS) does not hold: plugins, scripts or debug
    /// mappings changed while the tunnel was not running, so the app could not hand the new chunks over, and the
    /// system then started the tunnel without the app. The payload is checked by its digest, so a copy for another
    /// payload never decrypts with the wrong plugins; without this the tunnel would run with HTTPS decryption off and
    /// say nothing. A copy without chunks falls back to the Keychain read the extension makes anyway (an app
    /// extension can, a System Extension cannot). Nil when the payload loads, or when the profile has none.
    public static func mitmBundleFailureReason(profileValue: Data?, keptCopy: TunnelStartSecrets?) -> String? {
        guard let keptCopy, let profileValue,
              (try? JSONDecoder().decode(MitmProfileBundle.Reference.self, from: profileValue)) != nil,
              MitmProfileBundle.load(fromProfileValue: profileValue, chunks: keptCopy.mitmBundleChunks) == nil
        else {
            return nil
        }
        return staleCopyReason
    }

    /// The extension's failure reason when the profile decrypts HTTPS (`decrypts`) and names a MITM root
    /// (`TunnelProfileKeys.mitmRootFingerprint`, `profileFingerprint`) other than the one in the copy kept from an
    /// earlier start (`keptCopy`; nil when the app passed the values in this start's options, or on iOS): the root
    /// was generated or replaced while the tunnel was not running, so the app could not hand it over. Starting on
    /// the old root would issue certificates from one the person is about to stop trusting, so the start fails
    /// with the stale-copy reason like a stale payload (`mitmBundleFailureReason`) and the app starts the tunnel
    /// once more with options. Nil for a profile that decrypts nothing or names no root (saved before the
    /// fingerprint existed), and when the roots match.
    public static func mitmRootFailureReason(
        profileFingerprint: String?, decrypts: Bool, keptCopy: TunnelStartSecrets?
    ) -> String? {
        guard let keptCopy, decrypts, !keptCopy.mitmRootMatches(profileFingerprint: profileFingerprint) else {
            return nil
        }
        return staleCopyReason
    }

    /// Whether this copy's MITM root is the one a profile naming `profileFingerprint`
    /// (`TunnelProfileKeys.mitmRootFingerprint`) was saved with. The app writes the fingerprint whenever the root
    /// changes, so a copy kept from an earlier start with an older root, or none, does not match. A profile from
    /// before the fingerprint names none: unknown, and the copy is used as before.
    public func mitmRootMatches(profileFingerprint: String?) -> Bool {
        guard let profileFingerprint else {
            return true
        }
        guard let mitmAuthority, let fingerprint = Self.rootFingerprint(ofStoredValue: mitmAuthority) else {
            return false
        }
        return fingerprint == profileFingerprint
    }

    /// The SHA-256 fingerprint of a root as `MitmCertificateStore` keeps it, or nil when it does not decode.
    public static func rootFingerprint(ofStoredValue value: Data) -> String? {
        (try? MitmCertificateStore.authority(fromStoredValue: value))?.sha256Fingerprint
    }

    /// The fixed reason for a copy older than the profile (`nodeKeyFailureReason`, `mitmBundleFailureReason`,
    /// `mitmRootFailureReason`); the app looks for it in the system's record of the last stop.
    public static let staleCopyReason = "start secrets are older than the profile; open the app and connect"

    /// Whether the description of the last stop (`NEVPNConnection.fetchLastDisconnectError`, with its underlying
    /// errors) is the extension's stale-copy failure.
    public static func isStaleCopyFailure(_ description: String?) -> Bool {
        description?.contains(staleCopyReason) == true
    }

    /// What identifies a saved profile for `retriesStaleCopy`: its node key version, its HTTPS decryption payload
    /// reference (`TunnelProfileKeys.mitmBundle`, a digest and a chunk count) and its MITM root fingerprint
    /// (`TunnelProfileKeys.mitmRootFingerprint`). A plugin change keeps the key version but changes the reference,
    /// and a new root changes only the fingerprint, so each counts as another profile. Nil when the profile has none
    /// of them, as the extension then never reports a stale copy. Holds no secret: the reference is only hashed
    /// again, and the fingerprint is public.
    public static func staleCopyProfileStamp(
        nodeKeyVersion: String?, mitmBundle: Data?, mitmRoot: String? = nil
    ) -> String? {
        guard nodeKeyVersion != nil || mitmBundle != nil || mitmRoot != nil else {
            return nil
        }
        let base = (nodeKeyVersion ?? "-") + "/" + (mitmBundle.map(MitmProfileBundle.digest) ?? "-")
        return mitmRoot.map { base + "/" + $0 } ?? base
    }

    /// Whether the app starts the tunnel again, with options, after a start failed on a stale copy: once for each
    /// saved profile (`profile`, a `staleCopyProfileStamp`), so a start that fails again for another reason does
    /// not loop.
    public static func retriesStaleCopy(failure: String?, profile: String?, retriedProfile: String?) -> Bool {
        guard isStaleCopyFailure(failure), let profile else {
            return false
        }
        return profile != retriedProfile
    }

    /// The `TunnelAppMessage.startSecrets` payload and the start options' value. Keys are sorted, so the same
    /// values always encode the same and the app can skip handing over what it handed over already.
    public func encoded() -> Data? {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        return try? encoder.encode(self)
    }

    /// A `TunnelAppMessage.startSecrets` payload, or nil when it does not parse.
    public static func decoded(from data: Data) -> TunnelStartSecrets? {
        try? JSONDecoder().decode(TunnelStartSecrets.self, from: data)
    }

    /// The extension's answer once it kept a handed-over copy.
    public static let handOverAnswer = Data("ok".utf8)

    /// Whether the extension kept the copy. An extension older than `TunnelAppMessage.startSecrets` answers the
    /// runtime status instead (or nothing), which is not this.
    public static func handOverAccepted(_ answer: Data?) -> Bool {
        answer == handOverAnswer
    }

    public var isEmpty: Bool {
        nodeKey == nil && nodeLibrary == nil && mitmAuthority == nil && mitmBundleChunks == nil && anxLicense == nil
            && meshSecrets == nil
    }

    /// Reads the values from the Keychain, the node key of the profile saved with `nodeKeyVersion`; a value that
    /// cannot be read is left out, and the extension reports what is missing the same way as when its own Keychain
    /// read fails.
    public static func fromKeychain(nodeKeyVersion: String?) -> TunnelStartSecrets {
        let account = nodeKeyVersion.map(NodeSecretStore.versionedAccount) ?? NodeSecretStore.tunnelProfileAccount
        return TunnelStartSecrets(
            nodeKey: try? NodeSecretStore.value(account: account),
            nodeLibrary: try? NodeSecretStore.value(account: NodeLibrary.keychainAccount),
            mitmAuthority: try? MitmCertificateStore.storedValue(),
            mitmBundleChunks: MitmProfileBundle.storedChunks(),
            anxLicense: try? NodeSecretStore.value(account: AnxLicense.keychainAccount),
            nodeKeyVersion: nodeKeyVersion,
            meshSecrets: MeshNetworkLibrary.joinedSecrets()
        )
    }

    /// The `startVPNTunnel(options:)` dictionary.
    public var options: [String: NSObject] {
        guard let data = encoded() else {
            return [:]
        }
        return [Self.optionsKey: data as NSData]
    }

    /// The values the app passed to `startVPNTunnel(options:)`, or nil when it passed none.
    public init?(options: [String: NSObject]?) {
        guard let data = options?[Self.optionsKey] as? Data, let decoded = Self.decoded(from: data) else {
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
        return decoded(from: data)
    }
}
