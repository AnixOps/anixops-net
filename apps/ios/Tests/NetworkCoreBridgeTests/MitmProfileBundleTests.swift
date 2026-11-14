@testable import NetworkCoreBridge
import XCTest

final class MitmProfileBundleTests: XCTestCase {
    func testScriptsAndBodiesNeedAPlugin() {
        let bundle = MitmProfileBundle(plugins: [], scripts: ["a.js": "1"], resources: ["https://x": Data([1])])
        XCTAssertTrue(bundle.isEmpty)
        XCTAssertEqual(bundle, MitmProfileBundle())
    }

    func testProfileValueIsSmallAndFollowsThePayload() throws {
        // A few MB of plugin text is what made iOS refuse the VPN configuration.
        let large = String(repeating: "[Rule]\nDOMAIN-SUFFIX,example.com,REJECT\n", count: 100_000)
        let bundle = MitmProfileBundle(plugins: [large], scripts: ["a.js": "console.log(1)"])
        let value = try bundle.profileValue()
        XCTAssertLessThan(value.count, 512)
        let reference = try JSONDecoder().decode(MitmProfileBundle.Reference.self, from: value)
        XCTAssertEqual(reference.chunks, MitmProfileBundle.chunks(of: try bundle.packed()).count)

        let changed = MitmProfileBundle(plugins: [large], scripts: ["a.js": "console.log(2)"])
        XCTAssertNotEqual(try changed.profileValue(), value)
    }

    func testChunksJoinBackToThePayload() throws {
        var bytes = Data(count: MitmProfileBundle.chunkBytes * 2 + 17)
        for index in bytes.indices {
            bytes[index] = UInt8(truncatingIfNeeded: index &* 31)
        }
        let chunks = MitmProfileBundle.chunks(of: bytes)
        XCTAssertEqual(chunks.count, 3)
        XCTAssertTrue(chunks.allSatisfy { $0.count <= MitmProfileBundle.chunkBytes })
        XCTAssertEqual(chunks.reduce(Data(), +), bytes)
    }

    func testOlderProfilesStillReadTheirPluginKeys() throws {
        let configuration: [String: Any] = [
            TunnelProfileKeys.mitmPlugins: try MitmPluginLibrary.profileValue(["#!name = A"]),
            TunnelProfileKeys.mitmScripts: try MitmScriptLibrary.profileValue(["a.js": "1"]),
        ]
        let bundle = MitmProfileBundle.fromProfile(configuration)
        XCTAssertEqual(bundle.plugins, ["#!name = A"])
        XCTAssertEqual(bundle.scripts, ["a.js": "1"])
        XCTAssertEqual(MitmProfileBundle.fromProfile(nil), MitmProfileBundle())
    }

    func testMalformedReferenceReadsAsNothing() {
        XCTAssertNil(MitmProfileBundle.load(fromProfileValue: Data("{}".utf8)))
        XCTAssertNil(MitmProfileBundle.load(fromProfileValue: nil))
    }
}
