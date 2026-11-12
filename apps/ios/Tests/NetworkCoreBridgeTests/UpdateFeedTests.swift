import Foundation
import NetworkCoreBridge
import XCTest

final class UpdateFeedTests: XCTestCase {
    private let product = "anixops-net-macos"
    private let macOS14 = OperatingSystemVersion(majorVersion: 14, minorVersion: 0, patchVersion: 0)

    // MARK: Feed

    func testSignedFeedVerifies() throws {
        let (signer, verifier) = try pair()
        let data = try signer.sign(feed(sequence: 5, items: [item(build: 7, signer: signer)]))

        let verified = try verifier.verifyFeed(data, lastAccepted: 5)

        XCTAssertEqual(verified.sequence, 5)
        XCTAssertEqual(verified.items.map(\.build), [7])
    }

    func testChangedPayloadIsRefused() throws {
        let (signer, verifier) = try pair()
        let data = try signer.sign(feed(sequence: 5, items: [item(build: 7, signer: signer)]))
        var envelope = try JSONDecoder().decode(SignedUpdateFeed.self, from: data)
        // Point the download somewhere else and keep the old signature.
        let payload = try XCTUnwrap(Data(base64Encoded: envelope.payload))
        let changed = try XCTUnwrap(String(data: payload, encoding: .utf8))
            .replacingOccurrences(of: "updates.example.com", with: "evil.example.net")
        XCTAssertNotEqual(changed.utf8.count, 0)
        envelope.payload = Data(changed.utf8).base64EncodedString()

        XCTAssertThrowsError(try verifier.verifyFeed(JSONEncoder().encode(envelope))) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .badFeedSignature)
        }
    }

    func testEveryFlippedPayloadByteIsRefused() throws {
        let (signer, verifier) = try pair()
        let data = try signer.sign(feed(sequence: 1, items: [item(build: 2, signer: signer)]))
        let envelope = try JSONDecoder().decode(SignedUpdateFeed.self, from: data)
        let payload = try XCTUnwrap(Data(base64Encoded: envelope.payload))
        for index in stride(from: 0, to: payload.count, by: max(1, payload.count / 64)) {
            var bytes = payload
            bytes[index] ^= 0x01
            let tampered = SignedUpdateFeed(payload: bytes.base64EncodedString(), signature: envelope.signature)
            XCTAssertThrowsError(try verifier.verifyFeed(JSONEncoder().encode(tampered)), "byte \(index)")
        }
    }

    func testChangedSignatureIsRefused() throws {
        let (signer, verifier) = try pair()
        let data = try signer.sign(feed(sequence: 1, items: []))
        var envelope = try JSONDecoder().decode(SignedUpdateFeed.self, from: data)
        var signature = try XCTUnwrap(Data(base64Encoded: envelope.signature))
        signature[10] ^= 0x80
        envelope.signature = signature.base64EncodedString()

        XCTAssertThrowsError(try verifier.verifyFeed(JSONEncoder().encode(envelope))) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .badFeedSignature)
        }
    }

    func testFeedOfAnotherKeyIsRefused() throws {
        let (_, verifier) = try pair()
        let stranger = try UpdateSigner(privateKey: UpdateSigner.generate().privateKey)
        let data = try stranger.sign(feed(sequence: 1, items: [item(build: 7, signer: stranger)]))

        XCTAssertThrowsError(try verifier.verifyFeed(data)) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .badFeedSignature)
        }
    }

    func testRotatedKeysAreBothTrusted() throws {
        let old = try UpdateSigner(privateKey: UpdateSigner.generate().privateKey)
        let new = try UpdateSigner(privateKey: UpdateSigner.generate().privateKey)
        let verifier = try UpdateVerifier(product: product, publicKeys: [old.publicKey, new.publicKey])

        XCTAssertNoThrow(try verifier.verifyFeed(old.sign(feed(sequence: 1, items: []))))
        XCTAssertNoThrow(try verifier.verifyFeed(new.sign(feed(sequence: 2, items: []))))
    }

    func testOlderFeedIsRefusedAsReplay() throws {
        let (signer, verifier) = try pair()
        let old = try signer.sign(feed(sequence: 4, items: []))

        XCTAssertThrowsError(try verifier.verifyFeed(old, lastAccepted: 5)) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .staleFeed(sequence: 4, lastAccepted: 5))
        }
    }

    func testFeedOfAnotherProductIsRefused() throws {
        let (signer, verifier) = try pair()
        var other = feed(sequence: 1, items: [])
        other.product = "anixops-net-windows"

        XCTAssertThrowsError(try verifier.verifyFeed(signer.sign(other))) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .wrongProduct("anixops-net-windows"))
        }
    }

    func testPlainHTTPDownloadIsRefused() throws {
        let (signer, verifier) = try pair()
        var plain = try item(build: 7, signer: signer)
        plain.url = try XCTUnwrap(URL(string: "http://updates.example.com/AnixOps-Net.pkg"))

        XCTAssertThrowsError(try verifier.verifyFeed(signer.sign(feed(sequence: 1, items: [plain])))) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .insecureURL)
        }
    }

    func testGarbageAndUnknownFormatAreRefused() throws {
        let (_, verifier) = try pair()
        XCTAssertThrowsError(try verifier.verifyFeed(Data("<rss/>".utf8))) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .malformedFeed)
        }
        let future = SignedUpdateFeed(format: "anixops-appcast-v9", payload: "", signature: "")
        XCTAssertThrowsError(try verifier.verifyFeed(JSONEncoder().encode(future))) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .unsupportedFormat("anixops-appcast-v9"))
        }
    }

    func testVerifierNeedsValidKeys() {
        XCTAssertThrowsError(try UpdateVerifier(product: product, publicKeys: [])) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .noTrustedKeys)
        }
        XCTAssertThrowsError(try UpdateVerifier(product: product, publicKeys: ["bm90IGEga2V5"])) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .invalidPublicKey)
        }
    }

    // MARK: Picking an update

    func testNewestUpdateSkipsOlderBuildsAndNewerSystems() throws {
        let (signer, verifier) = try pair()
        var needsSequoia = try item(build: 9, signer: signer)
        needsSequoia.minimumSystemVersion = "15.0"
        let items = try [item(build: 3, signer: signer), item(build: 8, signer: signer), needsSequoia]
        let verified = try verifier.verifyFeed(signer.sign(feed(sequence: 1, items: items)))

        XCTAssertEqual(verifier.newestUpdate(in: verified, currentBuild: 5, systemVersion: macOS14)?.build, 8)
        XCTAssertEqual(
            verifier.newestUpdate(
                in: verified,
                currentBuild: 5,
                systemVersion: OperatingSystemVersion(majorVersion: 15, minorVersion: 1, patchVersion: 0)
            )?.build,
            9
        )
        // Never a downgrade, never the running build again.
        XCTAssertNil(verifier.newestUpdate(in: verified, currentBuild: 9, systemVersion: macOS14))
        XCTAssertNil(verifier.newestUpdate(in: verified, currentBuild: 8, systemVersion: macOS14))
    }

    // MARK: Archive

    func testArchiveVerifies() throws {
        let (signer, verifier) = try pair()
        let file = try archive(Data(repeating: 0x5A, count: 3 * (1 << 20) + 17))
        let signed = try item(build: 7, signer: signer, archive: file)

        XCTAssertNoThrow(try verifier.verifyArchive(at: file, item: signed))
    }

    func testChangedArchiveIsRefused() throws {
        let (signer, verifier) = try pair()
        let file = try archive(Data("installer package".utf8))
        let signed = try item(build: 7, signer: signer, archive: file)
        var bytes = try Data(contentsOf: file)
        bytes[3] ^= 0x01
        try bytes.write(to: file)

        XCTAssertThrowsError(try verifier.verifyArchive(at: file, item: signed)) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .digestMismatch)
        }
    }

    func testTruncatedArchiveIsRefused() throws {
        let (signer, verifier) = try pair()
        let file = try archive(Data("installer package".utf8))
        let signed = try item(build: 7, signer: signer, archive: file)
        try Data("installer".utf8).write(to: file)

        XCTAssertThrowsError(try verifier.verifyArchive(at: file, item: signed)) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .sizeMismatch(expected: 17, actual: 9))
        }
    }

    func testArchiveSignatureOfAnotherBuildIsRefused() throws {
        let (signer, verifier) = try pair()
        let file = try archive(Data("installer package".utf8))
        var signed = try item(build: 7, signer: signer, archive: file)
        // The digest matches, but the signature was made for build 7.
        signed.build = 8

        XCTAssertThrowsError(try verifier.verifyArchive(at: file, item: signed)) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .badArchiveSignature)
        }
    }

    func testArchiveSignedByAnotherKeyIsRefused() throws {
        let (_, verifier) = try pair()
        let stranger = try UpdateSigner(privateKey: UpdateSigner.generate().privateKey)
        let file = try archive(Data("installer package".utf8))
        let signed = try item(build: 7, signer: stranger, archive: file)

        XCTAssertThrowsError(try verifier.verifyArchive(at: file, item: signed)) { error in
            XCTAssertEqual(error as? UpdateVerificationError, .badArchiveSignature)
        }
    }

    // MARK: Interoperability

    /// A feed and archive signed outside CryptoKit (Python `cryptography`, RFC 8032 test key 1), so any standard
    /// Ed25519 tool can sign releases.
    func testFeedSignedByAnotherEd25519ImplementationVerifies() throws {
        let verifier = try UpdateVerifier(
            product: product,
            publicKeys: ["11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo="]
        )
        let feed = try verifier.verifyFeed(Data(Self.foreignFeed.utf8), lastAccepted: 3)
        let item = try XCTUnwrap(feed.items.first)
        XCTAssertEqual(item.version, "0.3.1")
        XCTAssertEqual(item.url.host, "updates.example.com")

        let file = try archive(Data("AnixOps Net test archive\n".utf8))
        XCTAssertNoThrow(try verifier.verifyArchive(at: file, item: item))
    }

    func testSignerAcceptsAStandardSeed() throws {
        // RFC 8032 section 7.1, test 1.
        let signer = try UpdateSigner(privateKey: "nWGxne/9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A=")
        XCTAssertEqual(signer.publicKey, "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=")
    }

    /// The anix-kit (`anixsign`) key formats: PKCS#8 PEM private keys, PEM SPKI public keys, 16-digit key ids.
    func testAnixKitKeyFormats() throws {
        let pkcs8 = """
        -----BEGIN PRIVATE KEY-----
        MC4CAQAwBQYDK2VwBCIEIJ1hsZ3v/VpguoRK9JLsLMREScVpezJpGXA7rAMcrn9g
        -----END PRIVATE KEY-----

        """
        let spki = """
        -----BEGIN PUBLIC KEY-----
        MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=
        -----END PUBLIC KEY-----
        """
        let signer = try UpdateSigner(privateKey: pkcs8)
        XCTAssertEqual(signer.publicKey, "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=")
        XCTAssertEqual(signer.keyID, "21fe31dfa154a261")
        XCTAssertEqual(try UpdateSigner.pem(privateKey: "nWGxne/9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A="), pkcs8)

        let verifier = try UpdateVerifier(product: product, publicKeys: [spki])
        XCTAssertEqual(verifier.keyIDs, ["21fe31dfa154a261"])
        XCTAssertNoThrow(try verifier.verifyFeed(Data(Self.foreignFeed.utf8)))

        // A PEM of another algorithm or a truncated one is not a key.
        XCTAssertThrowsError(try UpdateSigner(privateKey: "-----BEGIN PRIVATE KEY-----\nMC4CAQAw\n-----END PRIVATE KEY-----"))
        XCTAssertThrowsError(try UpdateVerifier(product: product, publicKeys: ["-----BEGIN PUBLIC KEY-----\nMCow\n-----END PUBLIC KEY-----"]))
    }

    // MARK: Helpers

    private func pair() throws -> (UpdateSigner, UpdateVerifier) {
        let signer = try UpdateSigner(privateKey: UpdateSigner.generate().privateKey)
        return (signer, try UpdateVerifier(product: product, publicKeys: [signer.publicKey]))
    }

    private func feed(sequence: Int64, items: [UpdateItem]) -> UpdateFeed {
        UpdateFeed(
            product: product,
            channel: "stable",
            sequence: sequence,
            generatedAt: Date(timeIntervalSince1970: 1_800_000_000),
            items: items
        )
    }

    private func item(build: Int, signer: UpdateSigner, archive file: URL? = nil) throws -> UpdateItem {
        var length: Int64 = 1024
        var sha256 = String(repeating: "ab", count: 32)
        if let file {
            (length, sha256) = try UpdateSigning.digest(of: file)
        }
        return UpdateItem(
            version: "0.3.\(build)",
            build: build,
            minimumSystemVersion: "14.0",
            url: URL(string: "https://updates.example.com/AnixOps-Net-\(build).pkg")!,
            length: length,
            sha256: sha256,
            signature: try signer.archiveSignature(product: product, build: build, length: length, sha256: sha256),
            notes: "Fixes."
        )
    }

    private func archive(_ data: Data) throws -> URL {
        let file = FileManager.default.temporaryDirectory
            .appendingPathComponent("update-\(UUID().uuidString).pkg")
        try data.write(to: file)
        addTeardownBlock {
            try? FileManager.default.removeItem(at: file)
        }
        return file
    }

    private static let foreignFeed = """
    {"format": "anixops-appcast-v1", "payload": "eyJzY2hlbWEiOjEsInByb2R1Y3QiOiJhbml4b3BzLW5ldC1tYWNvcyIsImNoYW5uZWwiOiJzdGFibGUiLCJzZXF1ZW5jZSI6MywiZ2VuZXJhdGVkQXQiOiIyMDI2LTExLTEyVDAwOjAwOjAwWiIsIml0ZW1zIjpbeyJ2ZXJzaW9uIjoiMC4zLjEiLCJidWlsZCI6NywibWluaW11bVN5c3RlbVZlcnNpb24iOiIxNC4wIiwidXJsIjoiaHR0cHM6Ly91cGRhdGVzLmV4YW1wbGUuY29tL0FuaXhPcHMtTmV0LTAuMy4xLnBrZyIsImxlbmd0aCI6MjUsInNoYTI1NiI6ImY3YzRkZDk2ZDRjMjlkNjY1NDViZTBkMmU3NjNkMzM2MTQzYjFhYmUxNzE5MTU5MDVlYmY2MjBjNGI1Y2RiNGIiLCJzaWduYXR1cmUiOiJZVUc5Nk9iVEtRUko1Q1EvOFhPWWpaY2gxcEE3WFVNVHR4YUs2ZmJjVU4zZ2d3L2d0VlBFcmNDcXdKUjIxMVErWDZNUDAzY1dGUmZaZ0xDRWhkcWtDZz09Iiwibm90ZXMiOiJGaXh0dXJlIn1dfQ==", "signature": "HBMWShgz+pwjkGGCirsHxQVRmWGL0XGPGp9UiIfSi0fK7iaS+siOKzJh0nclL40Bn2ohA065mTHDLGKsobMEBg=="}
    """
}
