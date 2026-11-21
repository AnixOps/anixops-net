import Foundation
import Security

/// Keeps node keys in the Keychain so the tunnel profile only carries a persistent reference.
///
/// Items go to the default access group, which is the first entry of `keychain-access-groups`. Both the app and
/// the packet tunnel extension list the same shared group first, so the extension can read what the app saved.
/// Errors carry only the `OSStatus`, never the key.
public enum NodeSecretStore {
    public struct Failure: Error, Equatable {
        public let status: OSStatus

        public init(status: OSStatus) {
            self.status = status
        }
    }

    static let service = "networkcore.node-key"

    /// Account of the key behind the tunnel profile's `passwordReference`. The extension falls back to it when the
    /// reference no longer resolves. Profiles that carry `TunnelProfileKeys.nodeKeyVersion` read
    /// `versionedAccount(_:)` instead; this account then only follows the last profile saved.
    public static let tunnelProfileAccount = "default"

    /// The account of the node key saved with one tunnel profile. Each save writes a new one, so the profile in the
    /// system settings keeps its own key until the next profile is saved, whatever happens in between.
    public static func versionedAccount(_ version: String) -> String {
        "\(tunnelProfileAccount)@\(version)"
    }

    /// Removes the versioned node keys of every profile but `version`'s, once that profile is saved.
    public static func removeNodeKeys(keeping version: String) {
        let kept = versionedAccount(version)
        for account in (try? accounts()) ?? [] where account.hasPrefix(tunnelProfileAccount + "@") && account != kept {
            try? delete(account: account)
        }
    }

    /// Every account this store holds a value for.
    static func accounts() throws -> [String] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecReturnAttributes as String: true,
            kSecMatchLimit as String: kSecMatchLimitAll,
        ]
        query.merge(platformAttributes) { _, new in new }
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound {
            return []
        }
        guard status == errSecSuccess, let items = result as? [[String: Any]] else {
            throw Failure(status: status == errSecSuccess ? errSecDecode : status)
        }
        return items.compactMap { $0[kSecAttrAccount as String] as? String }
    }

    /// Saves the key for `account` and returns its persistent reference, for `NEVPNProtocol.passwordReference`.
    ///
    /// An existing item is updated in place, so its persistent reference stays the same. Its value changes, though, and
    /// anything that reads this account gets the new value: a tunnel profile keeps its own key by naming a
    /// `versionedAccount(_:)` that nothing else writes.
    public static func save(_ password: String, account: String) throws -> Data {
        var identity: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        identity.merge(platformAttributes) { _, new in new }
        let updated = SecItemUpdate(
            identity as CFDictionary,
            [kSecValueData as String: Data(password.utf8)] as CFDictionary
        )
        if updated == errSecSuccess {
            var query = identity
            query[kSecReturnPersistentRef as String] = true
            var result: CFTypeRef?
            let status = SecItemCopyMatching(query as CFDictionary, &result)
            guard status == errSecSuccess, let reference = result as? Data else {
                throw Failure(status: status == errSecSuccess ? errSecDecode : status)
            }
            return reference
        }
        guard updated == errSecItemNotFound else {
            throw Failure(status: updated)
        }

        var attributes = identity
        attributes[kSecValueData as String] = Data(password.utf8)
        // The tunnel can start on demand while the device is locked, after the first unlock since boot.
        attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        attributes[kSecReturnPersistentRef as String] = true
        var result: CFTypeRef?
        let status = SecItemAdd(attributes as CFDictionary, &result)
        guard status == errSecSuccess, let reference = result as? Data else {
            throw Failure(status: status)
        }
        return reference
    }

    /// Reads the value saved for `account`, or nil when there is none.
    public static func value(account: String) throws -> String? {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
        ]
        query.merge(platformAttributes) { _, new in new }
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound {
            return nil
        }
        guard status == errSecSuccess, let data = result as? Data, let value = String(data: data, encoding: .utf8)
        else {
            throw Failure(status: status == errSecSuccess ? errSecDecode : status)
        }
        return value
    }

    /// Removes the value saved for `account`, if any.
    public static func delete(account: String) throws {
        var identity: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        identity.merge(platformAttributes) { _, new in new }
        let status = SecItemDelete(identity as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw Failure(status: status)
        }
    }

    /// Reads the key behind a persistent reference returned by `save(_:account:)`.
    public static func password(for reference: Data) throws -> String {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecValuePersistentRef as String: reference,
            kSecReturnData as String: true,
        ]
        query.merge(platformAttributes) { _, new in new }
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data, let password = String(data: data, encoding: .utf8)
        else {
            throw Failure(status: status == errSecSuccess ? errSecDecode : status)
        }
        return password
    }

    /// On macOS only the data protection keychain honors `keychain-access-groups`, which the app and the tunnel
    /// extension share; the legacy file keychain would keep the key out of the extension's reach.
    static var platformAttributes: [String: Any] {
        #if os(macOS)
        return [kSecUseDataProtectionKeychain as String: true]
        #else
        return [:]
        #endif
    }
}
