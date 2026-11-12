import CryptoKit
import Foundation

// The signed update feed ("appcast") of the macOS app, and the checks an update must pass before it is installed.
//
// The feed is a JSON envelope: the feed itself as base64 bytes plus one Ed25519 signature over those exact bytes,
// so no canonical JSON form is needed. The app ships the public keys; only the release pipeline holds a private key.
// Every check fails closed: a feed that does not verify against a trusted key, an older feed than the last one seen
// (replay), an archive whose length, SHA-256 or signature differ from the signed item, or a plain-HTTP download.
//
// This file depends on Foundation and CryptoKit only, so the release tool (apps/ios/Tools/appcast) compiles it
// as is and signs with exactly the code the app verifies with. Format: docs/macos-updates.md.

/// One signed release in the feed.
public struct UpdateItem: Codable, Equatable, Sendable {
    /// The marketing version shown to the user (`CFBundleShortVersionString`).
    public var version: String
    /// The build number (`CFBundleVersion`); an update must have a higher one than the running app.
    public var build: Int
    /// The oldest macOS the build runs on, for example "14.0".
    public var minimumSystemVersion: String
    /// Where the installer package is; HTTPS only.
    public var url: URL
    /// The archive's size in bytes.
    public var length: Int64
    /// The archive's SHA-256, 64 lowercase hex digits.
    public var sha256: String
    /// Base64 Ed25519 signature over `UpdateSigning.archiveMessage` of this item.
    public var signature: String
    /// Release notes, plain text.
    public var notes: String?
    public var publishedAt: Date?

    public init(
        version: String,
        build: Int,
        minimumSystemVersion: String,
        url: URL,
        length: Int64,
        sha256: String,
        signature: String,
        notes: String? = nil,
        publishedAt: Date? = nil
    ) {
        self.version = version
        self.build = build
        self.minimumSystemVersion = minimumSystemVersion
        self.url = url
        self.length = length
        self.sha256 = sha256
        self.signature = signature
        self.notes = notes
        self.publishedAt = publishedAt
    }
}

/// The feed inside the signed envelope.
public struct UpdateFeed: Codable, Equatable, Sendable {
    public static let currentSchema = 1

    public var schema: Int
    /// Which app the feed is for, so a feed of another product signed with the same key is refused.
    public var product: String
    /// For example "stable" or "beta"; informational, the feed URL selects the channel.
    public var channel: String
    /// Grows with every published feed. The app remembers the highest one it accepted and refuses older feeds, so an
    /// attacker cannot replay an old signed feed to hide a newer (security) release.
    public var sequence: Int64
    public var generatedAt: Date
    public var items: [UpdateItem]

    public init(
        schema: Int = UpdateFeed.currentSchema,
        product: String,
        channel: String,
        sequence: Int64,
        generatedAt: Date,
        items: [UpdateItem]
    ) {
        self.schema = schema
        self.product = product
        self.channel = channel
        self.sequence = sequence
        self.generatedAt = generatedAt
        self.items = items
    }
}

/// The JSON document served at the feed URL.
public struct SignedUpdateFeed: Codable, Equatable, Sendable {
    public static let currentFormat = "anixops-appcast-v1"

    public var format: String
    /// The `UpdateFeed` JSON, base64.
    public var payload: String
    /// Base64 Ed25519 signature over `UpdateSigning.feedMessage(payload bytes)`.
    public var signature: String

    public init(format: String = SignedUpdateFeed.currentFormat, payload: String, signature: String) {
        self.format = format
        self.payload = payload
        self.signature = signature
    }
}

public enum UpdateVerificationError: Error, Equatable, Sendable {
    case noTrustedKeys
    case invalidPublicKey
    case invalidPrivateKey
    /// The envelope or the feed inside it is not valid JSON of the expected shape.
    case malformedFeed
    case unsupportedFormat(String)
    /// No trusted key verifies the feed: it was changed after signing or signed by someone else.
    case badFeedSignature
    case unsupportedSchema(Int)
    case wrongProduct(String)
    /// The feed is older than one already accepted.
    case staleFeed(sequence: Int64, lastAccepted: Int64)
    case malformedItem(build: Int)
    case insecureURL
    case archiveUnreadable
    case sizeMismatch(expected: Int64, actual: Int64)
    case digestMismatch
    /// No trusted key verifies the archive signature.
    case badArchiveSignature
}

/// The exact bytes each signature covers. A distinct prefix per kind keeps a feed signature from being reused as an
/// archive signature and the other way round.
public enum UpdateSigning {
    public static let feedPrefix = "anixops-appcast-v1\n"
    public static let archivePrefix = "anixops-update-archive-v1\n"

    public static func feedMessage(_ payload: Data) -> Data {
        Data(feedPrefix.utf8) + payload
    }

    /// Binds the archive's digest and size to the product and build, so a signed archive of one build cannot be
    /// presented as another.
    public static func archiveMessage(product: String, build: Int, length: Int64, sha256: String) -> Data {
        Data("\(archivePrefix)\(product)\n\(build)\n\(length)\n\(sha256.lowercased())".utf8)
    }

    /// Size and SHA-256 of a file, read in 1 MiB chunks.
    public static func digest(of file: URL) throws -> (length: Int64, sha256: String) {
        guard let handle = try? FileHandle(forReadingFrom: file) else {
            throw UpdateVerificationError.archiveUnreadable
        }
        defer { try? handle.close() }
        var hasher = SHA256()
        var length: Int64 = 0
        while true {
            let chunk: Data
            do {
                chunk = try handle.read(upToCount: 1 << 20) ?? Data()
            } catch {
                throw UpdateVerificationError.archiveUnreadable
            }
            if chunk.isEmpty {
                break
            }
            hasher.update(data: chunk)
            length += Int64(chunk.count)
        }
        let hex = hasher.finalize().map { String(format: "%02x", $0) }.joined()
        return (length, hex)
    }

    static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }

    static func encoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        return encoder
    }
}

/// Checks feeds and archives against the public keys the app ships.
public struct UpdateVerifier: Sendable {
    public let product: String
    /// Raw 32-byte Ed25519 public keys. More than one lets a new key roll out before the old one is retired.
    private let keys: [Data]

    /// `publicKeys` are base64 raw Ed25519 keys; at least one is required.
    public init(product: String, publicKeys: [String]) throws {
        let keys = try publicKeys.map { text -> Data in
            guard let raw = Data(base64Encoded: text.trimmingCharacters(in: .whitespacesAndNewlines)),
                  (try? Curve25519.Signing.PublicKey(rawRepresentation: raw)) != nil
            else {
                throw UpdateVerificationError.invalidPublicKey
            }
            return raw
        }
        guard !keys.isEmpty else {
            throw UpdateVerificationError.noTrustedKeys
        }
        self.product = product
        self.keys = keys
    }

    /// Verifies a downloaded feed and returns it. `lastAccepted` is the highest `sequence` accepted before; an older
    /// feed is refused. The same sequence is accepted again, since the same feed is fetched on every check.
    public func verifyFeed(_ data: Data, lastAccepted: Int64? = nil) throws -> UpdateFeed {
        let decoder = UpdateSigning.decoder()
        guard let envelope = try? decoder.decode(SignedUpdateFeed.self, from: data) else {
            throw UpdateVerificationError.malformedFeed
        }
        guard envelope.format == SignedUpdateFeed.currentFormat else {
            throw UpdateVerificationError.unsupportedFormat(envelope.format)
        }
        guard let payload = Data(base64Encoded: envelope.payload),
              let signature = Data(base64Encoded: envelope.signature)
        else {
            throw UpdateVerificationError.malformedFeed
        }
        guard verify(signature, over: UpdateSigning.feedMessage(payload)) else {
            throw UpdateVerificationError.badFeedSignature
        }
        // Only signed bytes are parsed from here on.
        guard let feed = try? decoder.decode(UpdateFeed.self, from: payload) else {
            throw UpdateVerificationError.malformedFeed
        }
        guard feed.schema == UpdateFeed.currentSchema else {
            throw UpdateVerificationError.unsupportedSchema(feed.schema)
        }
        guard feed.product == product else {
            throw UpdateVerificationError.wrongProduct(feed.product)
        }
        if let lastAccepted, feed.sequence < lastAccepted {
            throw UpdateVerificationError.staleFeed(sequence: feed.sequence, lastAccepted: lastAccepted)
        }
        for item in feed.items {
            try Self.validate(item)
        }
        return feed
    }

    /// The newest item with a higher build than `currentBuild` that runs on `systemVersion`, or nil.
    public func newestUpdate(
        in feed: UpdateFeed,
        currentBuild: Int,
        systemVersion: OperatingSystemVersion
    ) -> UpdateItem? {
        feed.items
            .filter { $0.build > currentBuild }
            .filter { item in
                guard let minimum = Self.systemVersion(item.minimumSystemVersion) else {
                    return false
                }
                return !Self.isOlder(systemVersion, than: minimum)
            }
            .max { $0.build < $1.build }
    }

    /// Checks a downloaded archive against its signed item: size, SHA-256 and the archive signature. Call it on the
    /// file the installer will open, after the download finished and before anything opens it.
    public func verifyArchive(at file: URL, item: UpdateItem) throws {
        try Self.validate(item)
        let (length, sha256) = try UpdateSigning.digest(of: file)
        guard length == item.length else {
            throw UpdateVerificationError.sizeMismatch(expected: item.length, actual: length)
        }
        guard sha256 == item.sha256.lowercased() else {
            throw UpdateVerificationError.digestMismatch
        }
        guard let signature = Data(base64Encoded: item.signature),
              verify(signature, over: UpdateSigning.archiveMessage(
                  product: product, build: item.build, length: length, sha256: sha256
              ))
        else {
            throw UpdateVerificationError.badArchiveSignature
        }
    }

    private func verify(_ signature: Data, over message: Data) -> Bool {
        keys.contains { raw in
            guard let key = try? Curve25519.Signing.PublicKey(rawRepresentation: raw) else {
                return false
            }
            return key.isValidSignature(signature, for: message)
        }
    }

    static func validate(_ item: UpdateItem) throws {
        guard item.url.scheme?.lowercased() == "https", item.url.host?.isEmpty == false else {
            throw UpdateVerificationError.insecureURL
        }
        let hex = Set("0123456789abcdef")
        guard item.build > 0, item.length > 0,
              item.sha256.count == 64, item.sha256.lowercased().allSatisfy({ hex.contains($0) }),
              let signature = Data(base64Encoded: item.signature), signature.count == 64,
              systemVersion(item.minimumSystemVersion) != nil
        else {
            throw UpdateVerificationError.malformedItem(build: item.build)
        }
    }

    /// "14", "14.2" or "14.2.1".
    static func systemVersion(_ text: String) -> OperatingSystemVersion? {
        let parts = text.split(separator: ".", omittingEmptySubsequences: false).map { Int($0) }
        guard (1...3).contains(parts.count), parts.allSatisfy({ ($0 ?? -1) >= 0 }) else {
            return nil
        }
        let numbers = parts.compactMap { $0 } + [0, 0]
        return OperatingSystemVersion(majorVersion: numbers[0], minorVersion: numbers[1], patchVersion: numbers[2])
    }

    static func isOlder(_ lhs: OperatingSystemVersion, than rhs: OperatingSystemVersion) -> Bool {
        (lhs.majorVersion, lhs.minorVersion, lhs.patchVersion) < (rhs.majorVersion, rhs.minorVersion, rhs.patchVersion)
    }
}

/// Signs feeds and archives. Used by the release tool and the tests; the app never holds a private key.
public struct UpdateSigner {
    private let key: Curve25519.Signing.PrivateKey

    /// `privateKey` is the base64 raw 32-byte Ed25519 seed that `generate()` prints.
    public init(privateKey: String) throws {
        guard let raw = Data(base64Encoded: privateKey.trimmingCharacters(in: .whitespacesAndNewlines)),
              let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: raw)
        else {
            throw UpdateVerificationError.invalidPrivateKey
        }
        self.key = key
    }

    /// A new key pair: base64 private seed and base64 public key.
    public static func generate() -> (privateKey: String, publicKey: String) {
        let key = Curve25519.Signing.PrivateKey()
        return (key.rawRepresentation.base64EncodedString(), key.publicKey.rawRepresentation.base64EncodedString())
    }

    public var publicKey: String {
        key.publicKey.rawRepresentation.base64EncodedString()
    }

    /// The archive signature for an item of `product` with this build, size and digest.
    public func archiveSignature(product: String, build: Int, length: Int64, sha256: String) throws -> String {
        let message = UpdateSigning.archiveMessage(product: product, build: build, length: length, sha256: sha256)
        return try key.signature(for: message).base64EncodedString()
    }

    /// Encodes and signs a feed; the result is the document to serve at the feed URL.
    public func sign(_ feed: UpdateFeed) throws -> Data {
        try sign(payload: UpdateSigning.encoder().encode(feed))
    }

    /// Signs feed JSON bytes as they are.
    public func sign(payload: Data) throws -> Data {
        let signature = try key.signature(for: UpdateSigning.feedMessage(payload))
        let envelope = SignedUpdateFeed(
            payload: payload.base64EncodedString(),
            signature: signature.base64EncodedString()
        )
        return try UpdateSigning.encoder().encode(envelope)
    }
}
