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
/// up itself after the engine starts (`setCaptureStorage`), so the key never crosses to the app or leaves the
/// Keychain except into the engine, and sessions live in the extension's own container.
public enum CaptureSessionStorage {
    public static let retentionDays = 7
    static let service = "networkcore.capture-session-key"
    static let account = "default"

    /// The folder for session files in the calling process's Application Support.
    public static func directory() throws -> URL {
        let folder = try FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("AnixOps-Captures", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    /// The 32-byte session key, made once and kept in the Keychain on this device only.
    public static func key() throws -> Data {
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
        var bytes = [UInt8](repeating: 0, count: 32)
        let random = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        guard random == errSecSuccess else {
            throw NodeSecretStore.Failure(status: random)
        }
        let key = Data(bytes)
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
