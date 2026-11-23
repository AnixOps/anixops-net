@testable import NetworkCoreBridge
import XCTest

final class TunnelStartSecretsTests: XCTestCase {
    func testOptionsRoundTrip() {
        let secrets = TunnelStartSecrets(
            nodeKey: "key", nodeLibrary: "{}", mitmAuthority: Data([1, 2, 3]), mitmBundleChunks: ["a", "b"],
            anxLicense: "{}"
        )
        XCTAssertEqual(TunnelStartSecrets(options: secrets.options), secrets)
        XCTAssertFalse(secrets.isEmpty)
    }

    func testNodeKeyPairsOnlyWithItsOwnProfileVersion() {
        // Third-party review F05: a key handed over for one profile never runs another profile's template.
        let versioned = TunnelStartSecrets(nodeKey: "key-b", nodeKeyVersion: "B")
        XCTAssertEqual(versioned.nodeKey(forVersion: "B"), "key-b")
        XCTAssertNil(versioned.nodeKey(forVersion: "A"))
        XCTAssertNil(versioned.nodeKey(forVersion: nil))
        let legacy = TunnelStartSecrets(nodeKey: "key")
        XCTAssertEqual(legacy.nodeKey(forVersion: nil), "key")
        XCTAssertNil(legacy.nodeKey(forVersion: "A"))
        XCTAssertEqual(TunnelStartSecrets(options: versioned.options), versioned)
        XCTAssertEqual(NodeSecretStore.versionedAccount("A"), "default@A")
    }

    func testNoOptionsMeansNothingHandedOver() {
        XCTAssertNil(TunnelStartSecrets(options: nil))
        XCTAssertNil(TunnelStartSecrets(options: ["other": NSString(string: "x")]))
        XCTAssertTrue(TunnelStartSecrets().isEmpty)
        XCTAssertFalse(TunnelStartSecrets(anxLicense: "{}").isEmpty)
    }

    func testSavedCopyIsOwnerOnly() throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
            .appendingPathComponent(TunnelStartSecrets.fileName)
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let secrets = TunnelStartSecrets(nodeKey: "key")
        try secrets.save(to: url)
        XCTAssertEqual(TunnelStartSecrets.load(from: url), secrets)
        let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber
        XCTAssertEqual(mode?.intValue, 0o600)
    }

    func testHandOverMessageRoundTrips() throws {
        // apple-extensions-mac-01: the app hands the copy for the profile saved now to the running System Extension.
        let secrets = TunnelStartSecrets(
            nodeKey: "key-b", nodeLibrary: "{}", mitmBundleChunks: ["a"], anxLicense: "{}", nodeKeyVersion: "B"
        )
        let payload = try XCTUnwrap(secrets.encoded())
        let message = TunnelAppMessage.startSecrets(payload)
        XCTAssertEqual(TunnelAppMessage.decode(message.encoded), message)
        XCTAssertEqual(TunnelStartSecrets.decoded(from: payload), secrets)
        XCTAssertNil(TunnelStartSecrets.decoded(from: Data("not json".utf8)))
        // Other messages stay what they were.
        XCTAssertEqual(TunnelAppMessage.decode(Data("control:{}".utf8)), .control(Data("{}".utf8)))
        XCTAssertEqual(TunnelAppMessage.decode(Data()), .runtimeStatus)
    }

    func testOnlyTheExtensionsOkCountsAsKept() {
        // An extension older than the message answers the runtime status, or nothing.
        XCTAssertTrue(TunnelStartSecrets.handOverAccepted(Data("ok".utf8)))
        XCTAssertFalse(TunnelStartSecrets.handOverAccepted(nil))
        XCTAssertFalse(TunnelStartSecrets.handOverAccepted(Data(#"{"running":true}"#.utf8)))
        XCTAssertFalse(TunnelStartSecrets.handOverAccepted(Data()))
    }

    func testCopyForAnOlderProfileHasItsOwnFailureReason() {
        let older = TunnelStartSecrets(nodeKey: "key-a", nodeKeyVersion: "A")
        let stale = TunnelStartSecrets.nodeKeyFailureReason(status: -25300, handedOver: older, version: "B")
        XCTAssertEqual(stale, TunnelStartSecrets.staleCopyReason)
        XCTAssertTrue(TunnelStartSecrets.isStaleCopyFailure("隧道已停止：x\n\(stale) (networkcore.packet-tunnel 1)"))
        // No copy (iOS), a copy without a key, or a profile from before key versions: a Keychain failure.
        let keychain = "node key is unavailable (keychain status -25300)"
        XCTAssertEqual(TunnelStartSecrets.nodeKeyFailureReason(status: -25300, handedOver: nil, version: "B"), keychain)
        XCTAssertEqual(
            TunnelStartSecrets.nodeKeyFailureReason(
                status: -25300, handedOver: TunnelStartSecrets(nodeLibrary: "{}", nodeKeyVersion: "A"), version: "B"
            ),
            keychain
        )
        XCTAssertEqual(TunnelStartSecrets.nodeKeyFailureReason(status: -25300, handedOver: older, version: nil), keychain)
        XCTAssertFalse(TunnelStartSecrets.isStaleCopyFailure(keychain))
        XCTAssertFalse(TunnelStartSecrets.isStaleCopyFailure(nil))
    }

    func testStaleCopyStartsAgainOnceForEachProfile() {
        let failure = "隧道已停止：x\n\(TunnelStartSecrets.staleCopyReason)"
        XCTAssertTrue(TunnelStartSecrets.retriesStaleCopy(failure: failure, profileVersion: "B", retriedVersion: nil))
        XCTAssertTrue(TunnelStartSecrets.retriesStaleCopy(failure: failure, profileVersion: "B", retriedVersion: "A"))
        // Tried for this profile already: the start failed again, so it is not a stale copy any more.
        XCTAssertFalse(TunnelStartSecrets.retriesStaleCopy(failure: failure, profileVersion: "B", retriedVersion: "B"))
        XCTAssertFalse(TunnelStartSecrets.retriesStaleCopy(failure: failure, profileVersion: nil, retriedVersion: nil))
        XCTAssertFalse(TunnelStartSecrets.retriesStaleCopy(
            failure: "node key is unavailable (keychain status -25300)", profileVersion: "B", retriedVersion: nil
        ))
        XCTAssertFalse(TunnelStartSecrets.retriesStaleCopy(failure: nil, profileVersion: "B", retriedVersion: nil))
    }

    func testHandedOverCopyReplacesTheKeptOne() throws {
        // The kept copy follows the profile saved again, so a start without options finds its key.
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
            .appendingPathComponent(TunnelStartSecrets.fileName)
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        try TunnelStartSecrets(nodeKey: "key-a", nodeKeyVersion: "A").save(to: url)
        let handed = try XCTUnwrap(TunnelStartSecrets(nodeKey: "key-b", nodeKeyVersion: "B").encoded())
        try XCTUnwrap(TunnelStartSecrets.decoded(from: handed)).save(to: url)
        let kept = try XCTUnwrap(TunnelStartSecrets.load(from: url))
        XCTAssertEqual(kept.nodeKey(forVersion: "B"), "key-b")
        XCTAssertNil(kept.nodeKey(forVersion: "A"))
    }

    func testBundleReadsHandedOverChunks() throws {
        let bundle = MitmProfileBundle(plugins: ["[Rule]\nDOMAIN,example.com,REJECT\n"])
        let packed = try bundle.packed()
        let chunks = MitmProfileBundle.chunks(of: packed).map { $0.base64EncodedString() }
        let value = try bundle.profileValue()
        XCTAssertEqual(MitmProfileBundle.load(fromProfileValue: value, chunks: chunks), bundle)
        XCTAssertNil(MitmProfileBundle.load(fromProfileValue: value, chunks: []))
    }
}
