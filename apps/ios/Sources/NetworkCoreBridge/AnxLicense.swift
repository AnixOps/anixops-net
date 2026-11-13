import CryptoKit
import Foundation

/// This device's ANX license (anx spec §2.4.1), the Apple side of `crates/anx-license`.
///
/// Only ANX needs a license; nothing else in the app is restricted, and nothing happens until a subscription server
/// announces a license CA (`X-AnixOps-Anx-License-Ca`):
///
/// - The device key is an Ed25519 key created the first time a server offers licensing. It lives in the Keychain
///   with this state (`keychainAccount`), never in a file the user can see.
/// - `X-AnixOps-Device` goes only to the origins that offered licensing, so other subscription servers never see a
///   stable device identifier. The first time an origin offers it the app fetches once more (`record` answers true)
///   to get the license at once.
/// - The license (`X-AnixOps-Anx-License`, hex) is kept with the panel's status word. The packet tunnel writes it to
///   its own container and gives every ANX outbound `device_key`, `device_alg` and `license_cert_path`
///   (`outboundFields(licensePath:)`); the outbound reads the file at each connection.
/// - When an origin stops announcing a CA it is forgotten; with no origin left the license is dropped.
public struct AnxLicense: Codable, Equatable {
    public static let keychainAccount = "anx-license"

    public static let headerDevice = "X-AnixOps-Device"
    public static let headerLicense = "X-AnixOps-Anx-License"
    public static let headerCA = "X-AnixOps-Anx-License-Ca"
    public static let headerStatus = "X-AnixOps-Anx-License-Status"

    /// The file name of the license in the packet tunnel's container.
    public static let licenseFileName = "anx-license.cert"

    /// PKCS#8 v1 for an Ed25519 key (RFC 8410): the DER before the 32-byte seed.
    static let pkcs8Prefix = Data([0x30, 0x2E, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20])

    /// The longest license accepted, in hex characters (a P-256 device certificate is 160 bytes).
    static let maxLicenseHex = 1024

    /// The device key's 32-byte Ed25519 seed.
    public var deviceKey: Data
    /// The origins (`scheme://host:port`) that offered licensing.
    public var origins: [String]
    /// The license, hex, as the panel sent it.
    public var license: String?
    /// The panel's last status word (`ok`, `not_entitled`, `no_seat`, `revoked`, ...).
    public var status: String?

    public init(deviceKey: Data, origins: [String] = [], license: String? = nil, status: String? = nil) {
        self.deviceKey = deviceKey
        self.origins = origins
        self.license = license
        self.status = status
    }

    /// A new state with a fresh device key.
    public static func generate() -> AnxLicense {
        AnxLicense(deviceKey: Curve25519.Signing.PrivateKey().rawRepresentation)
    }

    /// The hex of the device's public key.
    public var publicKeyHex: String {
        guard let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: deviceKey) else {
            return ""
        }
        return Self.hex(key.publicKey.rawRepresentation)
    }

    /// The `X-AnixOps-Device` value.
    public var deviceHeader: String { "ed25519:" + publicKeyHex }

    /// The device key as the ANX outbound takes it: PKCS#8 DER, hex.
    public var pkcs8Hex: String { Self.hex(Self.pkcs8Prefix + deviceKey) }

    /// Whether some subscription offered licensing.
    public var offered: Bool { !origins.isEmpty }

    /// `scheme://host:port`, lowercased; nil for a URL without a host.
    static func origin(of url: URL) -> String? {
        guard let scheme = url.scheme?.lowercased(), let host = url.host?.lowercased(), !host.isEmpty else {
            return nil
        }
        let port = url.port ?? (scheme == "https" ? 443 : scheme == "http" ? 80 : 0)
        return "\(scheme)://\(host):\(port)"
    }

    /// The `X-AnixOps-Device` value for a request to `url`: only for an origin that offered licensing.
    public func requestHeader(for url: URL) -> String? {
        guard let origin = Self.origin(of: url), origins.contains(origin) else {
            return nil
        }
        return deviceHeader
    }

    /// Takes in what a subscription response said. Answers true when `url`'s origin offers licensing for the first
    /// time, so the caller fetches once more with the device header.
    public mutating func record(url: URL, header: (String) -> String?) -> Bool {
        guard let origin = Self.origin(of: url) else {
            return false
        }
        let ca = header(Self.headerCA)?.trimmingCharacters(in: .whitespaces) ?? ""
        guard !ca.isEmpty else {
            if let index = origins.firstIndex(of: origin) {
                origins.remove(at: index)
                if origins.isEmpty {
                    license = nil
                    status = nil
                }
            }
            return false
        }
        let fresh = !origins.contains(origin)
        if fresh {
            origins.append(origin)
        }
        if let value = header(Self.headerLicense)?.trimmingCharacters(in: .whitespaces).lowercased(),
           Self.isHex(value)
        {
            license = value
        }
        if let value = header(Self.headerStatus)?.trimmingCharacters(in: .whitespaces), !value.isEmpty {
            status = String(value.prefix(32))
        }
        return fresh
    }

    /// The fields every ANX outbound gets, or nil while no subscription offered licensing.
    public func outboundFields(licensePath: String) -> [String: JSONValue]? {
        guard offered, !publicKeyHex.isEmpty else {
            return nil
        }
        return [
            "device_key": .string(pkcs8Hex),
            "device_alg": .string("ed25519"),
            "license_cert_path": .string(licensePath),
        ]
    }

    public func keychainValue() throws -> String {
        String(decoding: try JSONEncoder().encode(self), as: UTF8.self)
    }

    public static func fromKeychainValue(_ value: String) throws -> AnxLicense {
        let state = try JSONDecoder().decode(AnxLicense.self, from: Data(value.utf8))
        guard state.deviceKey.count == 32 else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "device key is not 32 bytes"))
        }
        return state
    }

    static func isHex(_ value: String) -> Bool {
        !value.isEmpty && value.count % 2 == 0 && value.count <= maxLicenseHex
            && value.allSatisfy { $0.isHexDigit && $0.isASCII }
    }

    static func hex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }
}

extension EmbeddedRuntimeConfig {
    /// The same config with `fields` set on every ANX outbound: the selected node and the subscription's nodes.
    public func withAnxLicense(_ fields: [String: JSONValue]) -> EmbeddedRuntimeConfig {
        var copy = self
        if case var .native(native) = outbound, native.type == "anx" {
            native.settings.merge(fields) { _, new in new }
            copy.outbound = .native(native)
        }
        if var routing = copy.routing {
            for (id, settings) in routing.outbounds where settings["type"] == .string("anx") {
                routing.outbounds[id] = settings.merging(fields) { _, new in new }
            }
            copy.routing = routing
        }
        return copy
    }
}
