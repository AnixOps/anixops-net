@testable import NetworkCoreBridge
import XCTest

final class AppModeTests: XCTestCase {
    func testSavedModeWins() {
        XCTAssertEqual(AppMode.resolve(stored: "debug", hasNodes: true, onboardingDone: true), .debug)
        XCTAssertEqual(AppMode.resolve(stored: "proxy", hasNodes: false, onboardingDone: false), .proxy)
    }

    func testExistingUsersKeepTheProxyClient() {
        XCTAssertEqual(AppMode.resolve(stored: nil, hasNodes: true, onboardingDone: false), .proxy)
        XCTAssertEqual(AppMode.resolve(stored: "", hasNodes: false, onboardingDone: true), .proxy)
    }

    func testNewUsersAreAsked() {
        XCTAssertNil(AppMode.resolve(stored: nil, hasNodes: false, onboardingDone: false))
        XCTAssertNil(AppMode.resolve(stored: "something-else", hasNodes: false, onboardingDone: false))
    }

    func testDirectExitOnlyInDebugMode() {
        XCTAssertTrue(DebugExit.usesDirect(mode: .debug, exit: .direct, hasNodes: true))
        XCTAssertTrue(DebugExit.usesDirect(mode: .debug, exit: .node, hasNodes: false))
        XCTAssertFalse(DebugExit.usesDirect(mode: .debug, exit: .node, hasNodes: true))
        XCTAssertFalse(DebugExit.usesDirect(mode: .proxy, exit: .direct, hasNodes: false))
        XCTAssertFalse(DebugExit.usesDirect(mode: nil, exit: .direct, hasNodes: false))
    }

    func testDirectProfileRunsTheEngineDirectOutbound() throws {
        let profile = TunnelProfile.direct(profileID: "anixops", displayName: "直连")
        XCTAssertTrue(profile.isDirect)
        XCTAssertEqual(profile.displayName, "直连")
        XCTAssertEqual(IPAddressText.tunnelRemoteAddress(for: profile.serverAddress), "127.0.0.1")

        let encoded = try profile.config.withoutSecret().encoded()
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        let outbound = try XCTUnwrap(json["outbound"] as? [String: Any])
        XCTAssertEqual(outbound["type"] as? String, "direct")
        XCTAssertNil(json["routing"])

        // The profile keeps it through the round trip the extension makes.
        let decoded = try EmbeddedRuntimeConfig.decoded(from: encoded).withSecret(profile.config.secret)
        XCTAssertEqual(decoded.outboundKind, "direct")
    }

    func testNodeProfilesAreNotDirect() throws {
        let profile = try TunnelNodeImport.parse(
            "ss://2022-blake3-aes-128-gcm:AAECAwQFBgcICQoLDA0ODw%3D%3D@example.com:8388#Tokyo",
            profileID: "anixops"
        )
        XCTAssertFalse(profile.isDirect)
    }
}
