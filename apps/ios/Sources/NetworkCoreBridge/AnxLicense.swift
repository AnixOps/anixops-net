import CryptoKit
import Foundation

/// This device's ANX licenses (anx spec §2.4.1), the Apple side of `crates/anx-license`.
///
/// Only ANX needs a license; nothing else in the app is restricted, and nothing happens until a subscription server
/// announces a license CA (`X-AnixOps-Anx-License-Ca`):
///
/// - Every origin that offers licensing gets its own Ed25519 device key (user, 2026-11-19), created the first time it
///   offers it, so two panels never see the same key and cannot tell from it that they serve the same device. The
///   keys live in the Keychain with this state (`keychainAccount`), never in a file the user can see.
/// - `X-AnixOps-Device` goes only to an origin that offered licensing, and names only that origin's key. The first
///   time an origin offers it the app fetches once more (`record` answers true) to get the license at once.
/// - Each origin's license (`X-AnixOps-Anx-License`, hex) is kept with its panel's status word, so one panel's answer
///   never replaces another's. The packet tunnel writes each license to its own container and gives every ANX
///   outbound `device_key`, `device_alg` and `license_cert_path` of its node's origin (`NativeImportReport.Node`
///   `licenseOrigin`, `OutboundLicenses`); the outbound reads the file at each connection.
/// - When an origin stops announcing a CA it is forgotten with its key and license.
public struct AnxLicense: Codable, Equatable {
    public static let keychainAccount = "anx-license"

    public static let headerDevice = "X-AnixOps-Device"
    public static let headerLicense = "X-AnixOps-Anx-License"
    public static let headerCA = "X-AnixOps-Anx-License-Ca"
    public static let headerStatus = "X-AnixOps-Anx-License-Status"

    /// PKCS#8 v2 (OneAsymmetricKey, RFC 5958 / RFC 8410) for an Ed25519 key: the DER before the 32-byte seed. The
    /// ANX outbound parses the key with ring's `Ed25519KeyPair::from_pkcs8`, which accepts only v2 (version 1 with the
    /// public key); a v1 key makes it refuse the outbound, and the engine then rejects the whole tunnel profile.
    static let pkcs8Prefix = Data([0x30, 0x51, 0x02, 0x01, 0x01, 0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20])
    /// What follows the seed in PKCS#8 v2: `[1]` implicit BIT STRING, no unused bits, then the 32-byte public key.
    static let pkcs8PublicKeyTag = Data([0x81, 0x21, 0x00])

    /// The longest license accepted, in hex characters (a P-256 device certificate is 160 bytes).
    static let maxLicenseHex = 1024
    /// At most this many origins are kept, newest first.
    static let maxOrigins = 32

    /// One origin's device key, license and status.
    public struct Entry: Codable, Equatable {
        /// The device key's 32-byte Ed25519 seed.
        public var deviceKey: Data
        /// The license, hex, as the panel sent it.
        public var license: String?
        /// The panel's last status word (`ok`, `not_entitled`, `no_seat`, `revoked`, ...).
        public var status: String?

        public init(deviceKey: Data, license: String? = nil, status: String? = nil) {
            self.deviceKey = deviceKey
            self.license = license
            self.status = status
        }

        /// A new entry with a fresh device key.
        public static func generate() -> Entry {
            Entry(deviceKey: Curve25519.Signing.PrivateKey().rawRepresentation)
        }

        /// The hex of the device's public key.
        public var publicKeyHex: String {
            guard let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: deviceKey) else {
                return ""
            }
            return AnxLicense.hex(key.publicKey.rawRepresentation)
        }

        /// The `X-AnixOps-Device` value.
        public var deviceHeader: String { "ed25519:" + publicKeyHex }

        /// The device key as the ANX outbound takes it: PKCS#8 v2 DER (seed and public key), hex, the same layout as
        /// ring's `Ed25519KeyPair::generate_pkcs8` that the desktop clients use. Empty for a seed that is not a key.
        public var pkcs8Hex: String {
            guard let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: deviceKey) else {
                return ""
            }
            return AnxLicense.hex(AnxLicense.pkcs8Prefix + deviceKey + AnxLicense.pkcs8PublicKeyTag + key.publicKey.rawRepresentation)
        }

        /// The fields this origin's ANX outbounds get, or nil for a seed that is not a key.
        public func outboundFields(licensePath: String) -> [String: JSONValue]? {
            guard !publicKeyHex.isEmpty else {
                return nil
            }
            return [
                "device_key": .string(pkcs8Hex),
                "device_alg": .string("ed25519"),
                "license_cert_path": .string(licensePath),
            ]
        }
    }

    /// The origins (`scheme://host:port`) that offered licensing, newest first, each with its own key.
    public var origins: [String]
    public var entries: [String: Entry]

    public init(origins: [String] = [], entries: [String: Entry] = [:]) {
        self.origins = origins
        self.entries = entries
    }

    /// Whether some subscription offered licensing.
    public var offered: Bool { !origins.isEmpty }

    /// `scheme://host:port`, lowercased; nil for a URL without a host or one that is not http(s).
    public static func origin(of url: URL) -> String? {
        guard let scheme = url.scheme?.lowercased(), scheme == "https" || scheme == "http",
              let host = url.host?.lowercased(), !host.isEmpty
        else {
            return nil
        }
        let port = url.port ?? (scheme == "https" ? 443 : 80)
        return "\(scheme)://\(host):\(port)"
    }

    /// The origin of a subscription link, nil for pasted text or a link that is not http(s).
    public static func origin(ofSubscription link: String?) -> String? {
        link.flatMap { URL(string: $0.trimmingCharacters(in: .whitespacesAndNewlines)) }.flatMap(origin(of:))
    }

    /// The `X-AnixOps-Device` value for a request to `url`: only for an origin that offered licensing, and only its
    /// own key.
    public func requestHeader(for url: URL) -> String? {
        guard let origin = Self.origin(of: url), origins.contains(origin) else {
            return nil
        }
        return entries[origin]?.deviceHeader
    }

    /// Takes in what a subscription response said. Answers true when `url`'s origin offers licensing for the first
    /// time (or has no key yet), so the caller fetches once more with the device header.
    public mutating func record(url: URL, header: (String) -> String?) -> Bool {
        guard let origin = Self.origin(of: url) else {
            return false
        }
        let ca = header(Self.headerCA)?.trimmingCharacters(in: .whitespaces) ?? ""
        guard !ca.isEmpty else {
            origins.removeAll { $0 == origin }
            entries[origin] = nil
            return false
        }
        // New to this origin, or a key it has not seen yet (an origin migrated without one): fetch once more.
        let fresh = !origins.contains(origin) || entries[origin] == nil
        if !origins.contains(origin) {
            origins.insert(origin, at: 0)
            for dropped in origins.dropFirst(Self.maxOrigins) {
                entries[dropped] = nil
            }
            origins = Array(origins.prefix(Self.maxOrigins))
        }
        var entry = entries[origin] ?? .generate()
        if let value = header(Self.headerLicense)?.trimmingCharacters(in: .whitespaces).lowercased(),
           Self.isHex(value)
        {
            entry.license = value
        }
        if let value = header(Self.headerStatus)?.trimmingCharacters(in: .whitespaces), !value.isEmpty {
            entry.status = String(value.prefix(32))
        }
        entries[origin] = entry
        return fresh
    }

    /// The file name of `origin`'s license in the packet tunnel's container.
    public static func licenseFileName(origin: String) -> String {
        let digest = SHA256.hash(data: Data(origin.utf8))
        return "anx-license-" + hex(Data(digest.prefix(8))) + ".cert"
    }

    public func keychainValue() throws -> String {
        String(decoding: try JSONEncoder().encode(self), as: UTF8.self)
    }

    /// The state from the Keychain. A state saved before 2026-11-19 had one key for every origin: it stays with the
    /// newest origin, which keeps its seat, and every other origin gets a key of its own (anx spec §2.4.1).
    public static func fromKeychainValue(_ value: String) throws -> AnxLicense {
        let data = Data(value.utf8)
        let state: AnxLicense
        if let legacy = try? JSONDecoder().decode(Legacy.self, from: data) {
            state = legacy.migrated()
        } else {
            state = try JSONDecoder().decode(AnxLicense.self, from: data)
        }
        guard state.entries.values.allSatisfy({ $0.deviceKey.count == 32 }) else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "device key is not 32 bytes"))
        }
        return state
    }

    /// The single-key state of clients before 2026-11-19; `origins` were kept oldest first.
    struct Legacy: Decodable {
        var deviceKey: Data
        var origins: [String]
        var license: String?
        var status: String?

        /// The newest origin keeps the key, license and status. The others have no entry yet: `record` makes their
        /// own key at their next response, and the request after that names it.
        func migrated() -> AnxLicense {
            let newestFirst = Array(origins.reversed().prefix(AnxLicense.maxOrigins))
            var entries: [String: Entry] = [:]
            if let newest = newestFirst.first {
                entries[newest] = Entry(deviceKey: deviceKey, license: license, status: status)
            }
            return AnxLicense(origins: newestFirst, entries: entries)
        }
    }

    static func isHex(_ value: String) -> Bool {
        !value.isEmpty && value.count % 2 == 0 && value.count <= maxLicenseHex
            && value.allSatisfy { $0.isHexDigit && $0.isASCII }
    }

    static func hex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }
}

/// What the packet tunnel gives the ANX outbounds of one config: each origin's fields, and which origin each
/// outbound's node came from. An outbound whose origin has no fields, or whose node names no origin, gets none, never
/// another origin's (anx spec §2.4.1).
public struct OutboundLicenses: Equatable {
    /// `device_key`, `device_alg` and `license_cert_path` by origin.
    public var fieldsByOrigin: [String: [String: JSONValue]]
    /// The origin of each routed outbound, by outbound id (`NativeImportReport.Node.id`).
    public var originByOutbound: [String: String]
    /// The origin of each ANX node by `server:port`, for the one outbound a config without routing runs. A server
    /// and port two origins share maps to no origin.
    public var originByEndpoint: [String: String]

    public init(
        fieldsByOrigin: [String: [String: JSONValue]],
        originByOutbound: [String: String] = [:],
        originByEndpoint: [String: String] = [:]
    ) {
        self.fieldsByOrigin = fieldsByOrigin
        self.originByOutbound = originByOutbound
        self.originByEndpoint = originByEndpoint
    }

    /// The outbound map of `nodes` (a library's, with `licenseOrigin` set by `NodeLibrary.markingLicenseOrigins`).
    public init(fieldsByOrigin: [String: [String: JSONValue]], nodes: [NativeImportReport.Node]) {
        var byOutbound: [String: String] = [:]
        var byEndpoint: [String: String?] = [:]
        for node in nodes where node.protocol == "anx" {
            guard let origin = node.licenseOrigin else {
                continue
            }
            byOutbound[node.id] = origin
            let key = Self.endpoint(node.server, node.port)
            if let seen = byEndpoint[key], seen != origin {
                byEndpoint[key] = .some(nil)
            } else {
                byEndpoint[key] = origin
            }
        }
        self.init(
            fieldsByOrigin: fieldsByOrigin,
            originByOutbound: byOutbound,
            originByEndpoint: byEndpoint.compactMapValues { $0 }
        )
    }

    static func endpoint(_ server: String, _ port: UInt16) -> String {
        "\(server.lowercased()):\(port)"
    }

    func fields(outbound id: String) -> [String: JSONValue]? {
        originByOutbound[id].flatMap { fieldsByOrigin[$0] }
    }

    func fields(server: String, port: UInt16) -> [String: JSONValue]? {
        originByEndpoint[Self.endpoint(server, port)].flatMap { fieldsByOrigin[$0] }
    }
}
