import Foundation
import Security

/// The mesh static private keys (`node.private_key`, one per network) and the device instance id
/// (`node.instance_id`), made and kept by the network extension alone (core#406). The main app never calls this and
/// never holds a key: it reads only the public key from the engine snapshot.
///
/// - iOS: generic passwords in the keychain access group `$(AppIdentifierPrefix)networkcore.tunnel-private`, which
///   only the packet tunnel extension lists in its entitlements (the app and the widget do not).
/// - macOS: the extension is a system extension running as root. Its items go to the System keychain (the file
///   keychain, no data protection keychain), with an access that trusts only the calling extension. When that fails
///   at runtime, the same values go to a root-only (0600) file in the extension's data container. This follows
///   core#406.
///
/// Errors carry only a status, never a key. A value that cannot be read, or that is not valid, is an error and
/// never replaced: the panel lists the public key, and a new key would lock the device out.
public enum MeshKeyStore {
    public struct Failure: Error, Equatable {
        public enum Reason: Equatable {
            /// The keychain refused the call; carries the `OSStatus`.
            case keychain(OSStatus)
            /// The stored value is not a valid key or instance id.
            case invalidStoredValue
            /// The access group is not known (`AnixOpsKeychainPrefix` missing from the extension's Info.plist).
            case noAccessGroup
            /// The file fallback failed; carries `errno`.
            case file(Int32)
        }

        public let reason: Reason

        public init(_ reason: Reason) {
            self.reason = reason
        }
    }

    static let service = "networkcore.mesh-key"
    /// The account of the device instance id. Network keys use `MeshNetworkLibrary.privateKeyAccount(_:)`, whose
    /// prefix no instance id account shares.
    static let instanceIDAccount = "mesh-instance-id"
    static let accessGroupSuffix = "networkcore.tunnel-private"
    /// The Info.plist key holding `$(AppIdentifierPrefix)` (team id and a dot) in the extension targets.
    static let prefixInfoKey = "AnixOpsKeychainPrefix"
    static let fileName = "mesh-keys.json"

    // MARK: - Public (extension only)

    /// The network's private key: read; created and saved when there is none. A read error throws; a stored value
    /// that is not a valid key throws.
    public static func ensurePrivateKey(_ name: String) throws -> String {
        try ensurePrivateKey(name, storage: .system)
    }

    /// The device instance id: read; created (lowercase UUID v4) and saved on first use. The same value goes into
    /// every network's configuration.
    public static func ensureInstanceID() throws -> String {
        try ensureInstanceID(storage: .system)
    }

    /// Removes the key of `name`, if any (the network was removed).
    public static func removeKey(_ name: String) {
        try? delete(account: MeshNetworkLibrary.privateKeyAccount(name))
    }

    /// Removes every key whose network is not in `names`: the app removed the network while the extension was not
    /// running. The instance id stays.
    public static func prune(keeping names: Set<String>) {
        let prefix = MeshNetworkLibrary.privateKeyAccount("")
        for account in (try? accounts()) ?? [] where account.hasPrefix(prefix) {
            if !names.contains(String(account.dropFirst(prefix.count))) {
                try? delete(account: account)
            }
        }
    }

    // MARK: - Logic over a storage (testable)

    /// Where values live; the tests use memory.
    struct Storage {
        var read: (String) throws -> String?
        var write: (String, String) throws -> Void
        var delete: (String) throws -> Void
        /// The key a version before core#406 left in the shared Keychain, or nil. Only the extension reads it.
        var legacy: (String) throws -> String?
        var deleteLegacy: (String) -> Void

        static var system: Storage {
            Storage(
                read: { try read(account: $0) },
                write: { try write($1, account: $0) },
                delete: { try delete(account: $0) },
                legacy: { name in
                    #if os(macOS)
                    // The app handed keys to a macOS extension; none was ever readable in its keychain.
                    return nil
                    #else
                    return try NodeSecretStore.value(account: MeshNetworkLibrary.privateKeyAccount(name))
                    #endif
                },
                deleteLegacy: { try? NodeSecretStore.delete(account: MeshNetworkLibrary.privateKeyAccount($0)) }
            )
        }
    }

    static func ensurePrivateKey(_ name: String, storage: Storage) throws -> String {
        let account = MeshNetworkLibrary.privateKeyAccount(name)
        if let stored = try storage.read(account) {
            guard MeshNetworkLibrary.isValidPrivateKey(stored) else {
                throw Failure(.invalidStoredValue)
            }
            return stored
        }
        // A key a version before core#406 saved in the shared Keychain moves here, so the public key the panel
        // lists stays valid. An unreadable or invalid shared copy is an error as well.
        if let old = try storage.legacy(name) {
            guard MeshNetworkLibrary.isValidPrivateKey(old) else {
                throw Failure(.invalidStoredValue)
            }
            try storage.write(account, old)
            storage.deleteLegacy(name)
            return old
        }
        let key = try MeshNetworkLibrary.newPrivateKey()
        try storage.write(account, key)
        return key
    }

    static func ensureInstanceID(storage: Storage) throws -> String {
        if let stored = try storage.read(instanceIDAccount) {
            guard MeshNetworkLibrary.isValidInstanceID(stored) else {
                throw Failure(.invalidStoredValue)
            }
            return stored
        }
        let id = MeshNetworkLibrary.newInstanceID()
        try storage.write(instanceIDAccount, id)
        return id
    }

    // MARK: - Storage

    static func read(account: String) throws -> String? {
        #if os(macOS)
        let result = keychainRead(account: account)
        if case let .found(value) = result {
            return value
        }
        if let value = try FileStore.read()[account] {
            return value
        }
        if case let .failed(status) = result {
            throw Failure(.keychain(status))
        }
        return nil
        #else
        try requireGroup()
        return try keychainRead(account: account).value()
        #endif
    }

    static func write(_ value: String, account: String) throws {
        #if os(macOS)
        do {
            try keychainWrite(value, account: account)
        } catch {
            // The System keychain path is not available (not interactive, not implemented): the root-only file.
            try FileStore.write(value, account: account)
        }
        #else
        try requireGroup()
        try keychainWrite(value, account: account)
        #endif
    }

    private static func requireGroup() throws {
        #if !os(macOS)
        guard accessGroup() != nil else {
            throw Failure(.noAccessGroup)
        }
        #endif
    }

    static func delete(account: String) throws {
        let status = SecItemDelete(identity(account: account) as CFDictionary)
        #if os(macOS)
        try FileStore.remove(account: account)
        #endif
        guard status == errSecSuccess || status == errSecItemNotFound else {
            #if os(macOS)
            return
            #else
            throw Failure(.keychain(status))
            #endif
        }
    }

    static func accounts() throws -> [String] {
        var query = baseQuery()
        query[kSecReturnAttributes as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitAll
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        var names: [String] = []
        if status == errSecSuccess, let items = result as? [[String: Any]] {
            names = items.compactMap { $0[kSecAttrAccount as String] as? String }
        } else if status != errSecItemNotFound {
            #if !os(macOS)
            throw Failure(.keychain(status))
            #endif
        }
        #if os(macOS)
        names += Array(try FileStore.read().keys)
        #endif
        return names
    }

    enum KeychainRead {
        case found(String)
        case notFound
        case failed(OSStatus)

        func value() throws -> String? {
            switch self {
            case let .found(value): return value
            case .notFound: return nil
            case let .failed(status): throw Failure(.keychain(status))
            }
        }
    }

    static func keychainRead(account: String) -> KeychainRead {
        var query = identity(account: account)
        query[kSecReturnData as String] = true
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound {
            return .notFound
        }
        guard status == errSecSuccess else {
            return .failed(status)
        }
        guard let data = result as? Data, let value = String(data: data, encoding: .utf8) else {
            return .failed(errSecDecode)
        }
        return .found(value)
    }

    static func keychainWrite(_ value: String, account: String) throws {
        var attributes = identity(account: account)
        attributes[kSecValueData as String] = Data(value.utf8)
        #if os(macOS)
        attributes[kSecAttrAccess as String] = try selfOnlyAccess()
        #else
        // The tunnel can start on demand while the device is locked, after the first unlock since boot.
        attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        #endif
        let status = SecItemAdd(attributes as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw Failure(.keychain(status))
        }
    }

    private static func baseQuery() -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
        ]
        #if !os(macOS)
        // `read` and `write` refuse to run without the group: the default group would be the shared one.
        if let group = accessGroup() {
            query[kSecAttrAccessGroup as String] = group
        }
        query[kSecAttrSynchronizable as String] = false
        #endif
        return query
    }

    private static func identity(account: String) -> [String: Any] {
        var query = baseQuery()
        query[kSecAttrAccount as String] = account
        return query
    }

    /// `$(AppIdentifierPrefix)networkcore.tunnel-private`; nil when the prefix is missing from the Info.plist.
    static func accessGroup(bundle: Bundle = .main) -> String? {
        guard let prefix = bundle.object(forInfoDictionaryKey: prefixInfoKey) as? String, !prefix.isEmpty else {
            return nil
        }
        return prefix + accessGroupSuffix
    }

    #if os(macOS)
    /// An access that trusts only the calling extension (a nil path names the caller).
    private static func selfOnlyAccess() throws -> SecAccess {
        var app: SecTrustedApplication?
        var status = SecTrustedApplicationCreateFromPath(nil, &app)
        guard status == errSecSuccess, let app else {
            throw Failure(.keychain(status))
        }
        var access: SecAccess?
        status = SecAccessCreate("AnixOps Net mesh key" as CFString, [app] as CFArray, &access)
        guard status == errSecSuccess, let access else {
            throw Failure(.keychain(status))
        }
        return access
    }

    /// The file fallback: a JSON object of account to value in the extension's Application Support directory,
    /// root-only (directory 0700, file 0600), replaced through a temporary file and a rename.
    enum FileStore {
        static func url() throws -> URL {
            let base = try FileManager.default.url(
                for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
            )
            return base.appendingPathComponent(fileName)
        }

        static func read() throws -> [String: String] {
            let file = try url()
            guard let data = FileManager.default.contents(atPath: file.path) else {
                return [:]
            }
            guard let values = try? JSONDecoder().decode([String: String].self, from: data) else {
                throw Failure(.invalidStoredValue)
            }
            return values
        }

        static func write(_ value: String, account: String) throws {
            var values = try read()
            values[account] = value
            try save(values)
        }

        static func remove(account: String) throws {
            var values = try read()
            guard values.removeValue(forKey: account) != nil else {
                return
            }
            try save(values)
        }

        private static func save(_ values: [String: String]) throws {
            let file = try url()
            try FileManager.default.createDirectory(
                at: file.deletingLastPathComponent(), withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o700]
            )
            let temporary = file.deletingLastPathComponent().appendingPathComponent(".\(fileName).\(UUID().uuidString)")
            let descriptor = open(temporary.path, O_WRONLY | O_CREAT | O_EXCL, 0o600)
            guard descriptor >= 0 else {
                throw Failure(.file(errno))
            }
            let data = try JSONEncoder().encode(values)
            let written = data.withUnsafeBytes { Darwin.write(descriptor, $0.baseAddress, data.count) }
            let synced = fsync(descriptor)
            close(descriptor)
            guard written == data.count, synced == 0, rename(temporary.path, file.path) == 0 else {
                let code = errno
                unlink(temporary.path)
                throw Failure(.file(code))
            }
        }
    }
    #endif
}
