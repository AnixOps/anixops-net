@testable import NetworkCoreBridge
import XCTest

final class TunnelStartSecretsTests: XCTestCase {
    func testOptionsRoundTrip() {
        let secrets = TunnelStartSecrets(
            nodeKey: "key", nodeLibrary: "{}", mitmAuthority: Data([1, 2, 3]), mitmBundleChunks: ["a", "b"]
        )
        XCTAssertEqual(TunnelStartSecrets(options: secrets.options), secrets)
        XCTAssertFalse(secrets.isEmpty)
    }

    func testNoOptionsMeansNothingHandedOver() {
        XCTAssertNil(TunnelStartSecrets(options: nil))
        XCTAssertNil(TunnelStartSecrets(options: ["other": NSString(string: "x")]))
        XCTAssertTrue(TunnelStartSecrets().isEmpty)
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

    func testBundleReadsHandedOverChunks() throws {
        let bundle = MitmProfileBundle(plugins: ["[Rule]\nDOMAIN,example.com,REJECT\n"])
        let packed = try bundle.packed()
        let chunks = MitmProfileBundle.chunks(of: packed).map { $0.base64EncodedString() }
        let value = try bundle.profileValue()
        XCTAssertEqual(MitmProfileBundle.load(fromProfileValue: value, chunks: chunks), bundle)
        XCTAssertNil(MitmProfileBundle.load(fromProfileValue: value, chunks: []))
    }
}
