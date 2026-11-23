import Foundation
import NetworkCoreBridge
import XCTest

final class EngineBudgetTests: XCTestCase {
    private let base = EmbeddedRuntimeConfig(
        outbound: .shadowsocks(server: "198.51.100.1", port: 8388, method: "2022-blake3-aes-128-gcm", password: "secret"),
        profileID: "test"
    )

    /// The extension measures the config before the engine sees it: within the engine's `MAX_CONFIG_LEN` it is the
    /// encoded config itself, over it a `TooLarge` with the size, so decryption or the rules can be left out with
    /// that reason instead of a bare refusal.
    func testConfigsOverTheEnginesLimitAreMeasuredBeforeTheEngineSeesThem() throws {
        XCTAssertEqual(EngineBudget.maxConfigBytes, 64 * 1024)
        XCTAssertEqual(try EngineBudget.encoded(base), try base.encoded())

        // Two plugins of 40 KB: each fits the decryption budget, together the config does not fit.
        let plugin = "#!name=big\n[Rewrite]\n" + String(repeating: "^https://a\\.example/x reject\n", count: 1400)
        let authority = try MitmCertificateAuthority.generate()
        let decrypting = try base.withMitm(plugins: [plugin, plugin], authority: authority)
        XCTAssertThrowsError(try EngineBudget.encoded(decrypting)) { error in
            let tooLarge = error as? EngineBudget.TooLarge
            XCTAssertEqual(tooLarge?.limit, EngineBudget.maxConfigBytes)
            XCTAssertEqual(tooLarge?.bytes, try? decrypting.encoded().count)
        }
        XCTAssertNoThrow(try EngineBudget.encoded(decrypting.withoutMitm()))

        // A routing section of thousands of rules is measured the same way.
        let rules = (0..<4000).map { "DOMAIN-SUFFIX,host-\($0).example,DIRECT" } + ["FINAL,a"]
        let routed = base.withRouting(EmbeddedRuntimeConfig.Routing(outbounds: ["a": [:]], rules: rules, groups: []))
        XCTAssertThrowsError(try EngineBudget.encoded(routed)) { error in
            XCTAssertNotNil(error as? EngineBudget.TooLarge)
        }
    }

    /// The decryption budget counts UTF-8 bytes, as the kernel's `String::len` does.
    func testDecryptionBudgetCountsUtf8Bytes() throws {
        XCTAssertEqual(EngineBudget.decryptionBytes(["ab", "解密"]), 2 + 6)
        XCTAssertEqual(EngineBudget.maxDecryptionBytes, MitmPluginLibrary.maxActiveBytes)
        let half = String(repeating: "a", count: EngineBudget.maxDecryptionBytes / 2)
        XCTAssertNoThrow(try EngineBudget.checkDecryption([half, half]))
        XCTAssertThrowsError(try EngineBudget.checkDecryption([half, half, "a"])) { error in
            XCTAssertEqual(error as? EngineBudget.TooLarge,
                           EngineBudget.TooLarge(bytes: EngineBudget.maxDecryptionBytes + 1, limit: EngineBudget.maxDecryptionBytes))
        }
    }

    func testHttpsDecryptionStateSaysTooLargeAndToleratesUnknownReasons() throws {
        let tooLarge = HttpsDecryptionState(status: .off, reason: .tooLarge, plugins: 3, bytes: 70_000)
        XCTAssertEqual(HttpsDecryptionState.decoded(from: tooLarge.encoded()), tooLarge)
        XCTAssertEqual(
            HttpsDecryptionState.decoded(from: Data(#"{"status":"off","reason":"too_large","plugins":2}"#.utf8))?.reason,
            .tooLarge
        )
        // A reason from a newer extension reads as none rather than losing the state.
        let future = try XCTUnwrap(HttpsDecryptionState.decoded(from: Data(#"{"status":"off","reason":"later","plugins":1}"#.utf8)))
        XCTAssertEqual(future.status, .off)
        XCTAssertNil(future.reason)
    }

    func testRoutingStateRoundTripsAndSaysWhyTheRulesAreNotInForce() throws {
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.routing.encoded), .routing)
        XCTAssertNotEqual(TunnelAppMessage.routing.encoded, TunnelAppMessage.httpsDecryption.encoded)

        let off = RoutingState(status: .off, reason: .tooLarge, bytes: 90_000)
        XCTAssertEqual(RoutingState.decoded(from: off.encoded()), off)
        XCTAssertNotNil(off.notice)
        XCTAssertNotNil(RoutingState(status: .off, reason: .refused).notice)
        XCTAssertNil(RoutingState(status: .on).notice)
        XCTAssertNil(RoutingState(status: .unused).notice)

        let dropped = RoutingState(status: .on, droppedRuleSets: ["local-1", "geosite-cn"])
        XCTAssertEqual(RoutingState.decoded(from: dropped.encoded())?.droppedRuleSets, ["local-1", "geosite-cn"])
        XCTAssertTrue(dropped.notice?.contains("local-1, geosite-cn") == true)

        // The runtime status an older extension answers with does not read as a routing state.
        XCTAssertNil(RoutingState.decoded(from: Data(#"{"status":"running","outbound":"direct"}"#.utf8)))
        XCTAssertNil(RoutingState.decoded(from: Data(#"{"status":"off","reason":"later"}"#.utf8))?.reason)
    }
}
