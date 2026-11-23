import Foundation
import Security

extension PacketCapture {
    /// What one captured request exports to (`export_capture` with `id` and `exchange`).
    public enum RequestExport: Hashable, Identifiable, Sendable {
        case curl
        case code(CodeLanguage)
        case postman
        case insomnia
        case har

        public static let all: [RequestExport] = [.curl] + CodeLanguage.allCases.map { .code($0) } + [.postman, .insomnia, .har]

        public var id: String {
            switch self {
            case let .code(language):
                return "code-\(language.rawValue)"
            default:
                return format
            }
        }

        /// The engine's `format`.
        public var format: String {
            switch self {
            case .curl:
                return "curl"
            case .code:
                return "code"
            case .postman:
                return "postman"
            case .insomnia:
                return "insomnia"
            case .har:
                return "har"
            }
        }

        public var language: CodeLanguage? {
            if case let .code(language) = self {
                return language
            }
            return nil
        }
    }

    /// The languages `export_capture` writes code in.
    public enum CodeLanguage: String, CaseIterable, Identifiable, Sendable {
        /// requests
        case python
        /// fetch
        case javascript
        /// URLSession
        case swift
        /// OkHttp
        case kotlin
        /// net/http
        case go

        public var id: String {
            rawValue
        }

        public var title: String {
            switch self {
            case .python:
                return "Python (requests)"
            case .javascript:
                return "JavaScript (fetch)"
            case .swift:
                return "Swift (URLSession)"
            case .kotlin:
                return "Kotlin (OkHttp)"
            case .go:
                return "Go (net/http)"
            }
        }
    }

    /// One saved, encrypted capture session (`capture_sessions`).
    public struct Session: Decodable, Equatable, Identifiable, Sendable {
        public let id: String
        public let name: String?
        public let savedAt: Date
        public let expiresAt: Date?
        public let size: UInt64
        public let connections: Int?
        public let requests: Int?
        /// The file is there but could not be read (another key, or damaged).
        public let unreadable: Bool

        enum CodingKeys: String, CodingKey {
            case id = "session"
            case name
            case savedAtMs = "saved_at_ms"
            case expiresAtMs = "expires_at_ms"
            case size, connections, requests, unreadable
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            id = try container.decode(String.self, forKey: .id)
            name = try container.decodeIfPresent(String.self, forKey: .name)
            savedAt = PacketCapture.date(try container.decode(UInt64.self, forKey: .savedAtMs))
            expiresAt = try container.decodeIfPresent(UInt64.self, forKey: .expiresAtMs).map(PacketCapture.date)
            size = try container.decodeIfPresent(UInt64.self, forKey: .size) ?? 0
            connections = try container.decodeIfPresent(Int.self, forKey: .connections)
            requests = try container.decodeIfPresent(Int.self, forKey: .requests)
            unreadable = try container.decodeIfPresent(Bool.self, forKey: .unreadable) ?? false
        }
    }

    /// `{"sessions": [...]}`, newest first once sorted by the caller.
    public struct SessionsAnswer: Decodable, Equatable, Sendable {
        public let sessions: [Session]
    }

    /// `{"session": {...}}`, the `save_capture` answer.
    public struct SavedAnswer: Decodable, Equatable, Sendable {
        public let session: Session
    }
}

/// Where the tunnel keeps saved capture sessions and the key that seals them (plan F1/E11). The extension sets this
/// up itself after the engine starts (`setCaptureStorage`), so the key never crosses to the app and leaves its store
/// only into the engine, and sessions live in the extension's own container.
///
/// The key's store depends on the platform. iOS keeps it in the Keychain on this device only. A macOS Developer ID
/// build runs the tunnel as a System Extension: a root process outside any user login context, which cannot use
/// the data protection keychain (TN3137), so there the key is a file next to the sessions folder, readable by root
/// only (`CaptureSessionKeyFile`), like the start secrets the app hands over (`TunnelStartSecrets`).
public enum CaptureSessionStorage {
    public static let retentionDays = 7
    static let service = "networkcore.capture-session-key"
    static let account = "default"

    /// The folder for session files in the calling process's Application Support.
    public static func directory() throws -> URL {
        let folder = try applicationSupport().appendingPathComponent("AnixOps-Captures", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    /// The macOS key file in the calling process's Application Support, beside the sessions folder (the engine only
    /// reads `.anxcap` files from that folder).
    public static func keyFileURL() throws -> URL {
        try applicationSupport().appendingPathComponent(CaptureSessionKeyFile.fileName)
    }

    private static func applicationSupport() throws -> URL {
        try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
    }

    /// The 32-byte session key, made once on this device: in the Keychain on iOS, in a root-only file on macOS.
    public static func key() throws -> Data {
        #if os(macOS)
        return try CaptureSessionKeyFile.loadOrCreate(at: keyFileURL()) {
            // A key an earlier build managed to keep in the Keychain (only possible outside a System Extension)
            // still opens the sessions it sealed; otherwise a new one.
            try keychainKeyIfPresent() ?? randomKey()
        }
        #else
        return try keychainKey()
        #endif
    }

    static func randomKey() throws -> Data {
        var bytes = [UInt8](repeating: 0, count: CaptureSessionKeyFile.length)
        let random = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        guard random == errSecSuccess else {
            throw NodeSecretStore.Failure(status: random)
        }
        return Data(bytes)
    }

    #if os(macOS)
    /// The Keychain item, when this process can read one; nil on any failure (a System Extension gets
    /// `errSecNotAvailable` for the data protection keychain).
    private static func keychainKeyIfPresent() -> Data? {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
        ]
        query.merge(NodeSecretStore.platformAttributes) { _, new in new }
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data, data.count == CaptureSessionKeyFile.length
        else {
            return nil
        }
        return data
    }
    #endif

    /// The key in the Keychain on this device only, made on first use.
    private static func keychainKey() throws -> Data {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
        ]
        query.merge(NodeSecretStore.platformAttributes) { _, new in new }
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecSuccess, let data = result as? Data, data.count == 32 {
            return data
        }
        guard status == errSecItemNotFound || status == errSecSuccess else {
            throw NodeSecretStore.Failure(status: status)
        }
        let key = try randomKey()
        var attributes: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecValueData as String: key,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        attributes.merge(NodeSecretStore.platformAttributes) { _, new in new }
        if status == errSecSuccess {
            // A malformed item: replace it.
            var identity = attributes
            identity.removeValue(forKey: kSecValueData as String)
            identity.removeValue(forKey: kSecAttrAccessible as String)
            SecItemDelete(identity as CFDictionary)
        }
        let added = SecItemAdd(attributes as CFDictionary, nil)
        guard added == errSecSuccess else {
            throw NodeSecretStore.Failure(status: added)
        }
        return key
    }

    /// The `set_capture_storage` request for this process's folder and key.
    public static func setupRequest() throws -> RuntimeControl {
        .setCaptureStorage(dir: try directory().path, keyBase64: try key().base64EncodedString(), retentionDays: retentionDays)
    }
}

/// The capture session key as a file of exactly 32 bytes with mode 0600, for the macOS System Extension
/// (`CaptureSessionStorage.key()`).
///
/// A file that is there is never replaced: one that cannot be read or holds anything but one key is an error, since
/// a new key would make every saved session unreadable (the same rule as the desktop client, docs/debug-mode.md). A
/// new key is written to a private temporary file first and linked into place only when no file exists, so a crash
/// never leaves a partial key and two starts racing end up with the same one. The key is never logged.
public enum CaptureSessionKeyFile {
    public static let fileName = "capture-session.key"
    public static let length = 32

    public enum Failure: Error, Equatable {
        /// The file exists but could not be read.
        case unreadable
        /// The file (or a new key) is not 32 bytes long; it holds this many.
        case malformed(Int)
        /// A new key could not be written; the POSIX error.
        case notWritten(Int32)
    }

    /// The key in the file at `url`; when there is no file, `makeKey()` is written there and returned.
    public static func loadOrCreate(at url: URL, makeKey: () throws -> Data) throws -> Data {
        if let existing = try read(url) {
            return existing
        }
        let key = try makeKey()
        guard key.count == length else {
            throw Failure.malformed(key.count)
        }
        let folder = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let temporary = folder.appendingPathComponent(".\(fileName).\(UUID().uuidString)")
        try writeNew(key, to: temporary)
        defer {
            unlink(temporary.path)
        }
        // link(2) fails rather than replace an existing file.
        guard link(temporary.path, url.path) == 0 else {
            let code = errno
            if code == EEXIST, let existing = try read(url) {
                // Another start made it first: use that one.
                return existing
            }
            throw Failure.notWritten(code)
        }
        return key
    }

    /// The key in the file, or nil when there is no file.
    static func read(_ url: URL) throws -> Data? {
        let data: Data
        do {
            data = try Data(contentsOf: url)
        } catch let error as CocoaError where error.code == .fileReadNoSuchFile {
            return nil
        } catch {
            throw Failure.unreadable
        }
        guard data.count == length else {
            throw Failure.malformed(data.count)
        }
        return data
    }

    /// Creates `url` with mode 0600 (it must not exist) and writes `data` to disk.
    private static func writeNew(_ data: Data, to url: URL) throws {
        let descriptor = open(url.path, O_WRONLY | O_CREAT | O_EXCL, 0o600)
        guard descriptor >= 0 else {
            throw Failure.notWritten(errno)
        }
        let written = data.withUnsafeBytes { buffer in
            write(descriptor, buffer.baseAddress, buffer.count)
        }
        let code = errno
        let synced = fsync(descriptor) == 0
        close(descriptor)
        guard written == data.count, synced else {
            unlink(url.path)
            throw Failure.notWritten(written < 0 ? code : EIO)
        }
    }
}

/// What the request list says about the capture a disconnect ended. The extension seals it into a saved session on
/// stop (`saveCaptureOnStop`), which needs the session storage it set up at start; the app learns whether that
/// worked while still connected (`capture_sessions` answers only with storage set up).
public enum CaptureStopNotice: Equatable, Sendable {
    /// Saving sessions worked while connected: the extension sealed what was caught.
    case saved
    /// The tunnel could not save sessions: what was caught is gone with it.
    case notSaved
    /// The tunnel went down before the app knew.
    case unknown

    /// The notice after the tunnel went down; nil when the disconnect ended no capture with anything in it.
    public static func afterStop(captured: Bool, storageAvailable: Bool?) -> CaptureStopNotice? {
        guard captured else {
            return nil
        }
        switch storageAvailable {
        case true?:
            return .saved
        case false?:
            return .notSaved
        case nil:
            return .unknown
        }
    }
}
