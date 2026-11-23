import Foundation
import Security

/// The proxy username and password of the Mac's LAN share (`engine_embedded::lan_share`) and whether phones must
/// send them. Made with the system's CSPRNG the first time the share is used, kept in the Keychain on this Mac, and
/// put into every `start_lan_share` (`RuntimeControl.startLanShare`) while `required`: without them anyone else on
/// the same network (a café or office Wi-Fi, a CGNAT pool, a mesh VPN) could use the user's nodes through the Mac and
/// end up in its capture. The same rules as the Windows and Linux clients (`desktop_mitm::phone::LanShareAuth`).
public struct LanShareAuth: Codable, Equatable, Sendable {
    /// Phones must send the credentials; on unless the user turned it off.
    public var required: Bool
    public var username: String
    public var password: String

    /// Characters of the made-up username and password: lower-case letters and digits without the ones that are
    /// easy to mistake for each other (`0 o 1 l i`), since they are typed on a phone by hand.
    public static let alphabet = Array("abcdefghjkmnpqrstuvwxyz23456789".utf8)
    /// Random characters of the password: 20 of 31 is about 99 bits.
    public static let passwordLength = 20
    static let usernamePrefix = "anx-"
    static let usernameRandomLength = 6

    public init(required: Bool = true, username: String, password: String) {
        self.required = required
        self.username = username
        self.password = password
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        // Kept before the switch existed, or by another client: required.
        required = try container.decodeIfPresent(Bool.self, forKey: .required) ?? true
        username = try container.decode(String.self, forKey: .username)
        password = try container.decode(String.self, forKey: .password)
    }

    /// A new random username and password, required.
    public static func generate() throws -> LanShareAuth {
        let suffix = try randomText(length: usernameRandomLength)
        let password = try randomText(length: passwordLength)
        return LanShareAuth(required: true, username: usernamePrefix + suffix, password: password)
    }

    /// A new username and password; whether they are required stays as it was.
    public func regenerated() throws -> LanShareAuth {
        var fresh = try LanShareAuth.generate()
        fresh.required = required
        return fresh
    }

    /// Whether the engine takes these credentials: both set, at most 255 bytes (SOCKS5), and a username without the
    /// `:` that HTTP Basic splits on.
    public var isValid: Bool {
        let fits = { (text: String) in !text.isEmpty && text.utf8.count <= 255 }
        let control = (username + password).unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
        return fits(username) && fits(password) && !username.contains(":") && !control
    }

    /// `length` characters of `alphabet` from `SecRandomCopyBytes`, without modulo bias.
    static func randomText(length: Int) throws -> String {
        // The largest multiple of the alphabet's size below 256: bytes at or above it are drawn again.
        let limit = UInt8(256 / alphabet.count * alphabet.count)
        var text = [UInt8]()
        text.reserveCapacity(length)
        var buffer = [UInt8](repeating: 0, count: 32)
        while text.count < length {
            let status = SecRandomCopyBytes(kSecRandomDefault, buffer.count, &buffer)
            guard status == errSecSuccess else {
                throw NodeSecretStore.Failure(status: status)
            }
            for byte in buffer where byte < limit && text.count < length {
                text.append(alphabet[Int(byte) % alphabet.count])
            }
        }
        return String(decoding: text, as: UTF8.self)
    }
}

/// Keeps `LanShareAuth` in the app's Keychain on this Mac (the data protection keychain, like the node keys). Only the
/// app reads it: it sends the credentials to the tunnel with each `start_lan_share`. Credentials that cannot be read
/// back are replaced by new ones, never by none: the share must not start without a password because an item broke.
public enum LanShareAuthStore {
    static let service = "networkcore.lan-share-auth"
    static let account = "default"

    /// The kept credentials, made and kept on first use. Users who had the share on before it asked for a password
    /// get theirs here, the first time they start it, with the password required.
    public static func load() throws -> LanShareAuth {
        if let data = try read(), let auth = try? JSONDecoder().decode(LanShareAuth.self, from: data), auth.isValid {
            return auth
        }
        let auth = try LanShareAuth.generate()
        try save(auth)
        return auth
    }

    /// New credentials in place of the kept ones.
    public static func regenerate() throws -> LanShareAuth {
        let auth = try load().regenerated()
        try save(auth)
        return auth
    }

    /// Whether phones must send the credentials; the kept pair stays for when it is turned on again.
    public static func setRequired(_ required: Bool) throws -> LanShareAuth {
        var auth = try load()
        auth.required = required
        try save(auth)
        return auth
    }

    private static var identity: [String: Any] {
        var identity: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        identity.merge(NodeSecretStore.platformAttributes) { _, new in new }
        return identity
    }

    private static func read() throws -> Data? {
        var query = identity
        query[kSecReturnData as String] = true
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound {
            return nil
        }
        guard status == errSecSuccess else {
            throw NodeSecretStore.Failure(status: status)
        }
        return result as? Data
    }

    public static func save(_ auth: LanShareAuth) throws {
        let data = try JSONEncoder().encode(auth)
        let updated = SecItemUpdate(identity as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if updated == errSecSuccess {
            return
        }
        guard updated == errSecItemNotFound else {
            throw NodeSecretStore.Failure(status: updated)
        }
        var attributes = identity
        attributes[kSecValueData as String] = data
        attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let added = SecItemAdd(attributes as CFDictionary, nil)
        guard added == errSecSuccess else {
            throw NodeSecretStore.Failure(status: added)
        }
    }
}
