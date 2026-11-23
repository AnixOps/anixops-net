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

    /// Plugins within the 8 MB decryption budget but over the config limit as JSON strings are flagged before
    /// connecting; escaping counts, since the config carries the texts escaped.
    func testDecryptionThatCannotFitTheConfigIsFlagged() {
        XCTAssertFalse(EngineBudget.decryptionExceedsConfig([]))
        XCTAssertFalse(EngineBudget.decryptionExceedsConfig(["[MITM]\nhostname = a.example\n"]))
        let half = String(repeating: "a", count: EngineBudget.maxConfigBytes / 2)
        XCTAssertFalse(EngineBudget.decryptionExceedsConfig([half, String(half.dropLast(8))]))
        XCTAssertTrue(EngineBudget.decryptionExceedsConfig([half, half, "a"]))
        // Raw bytes within the limit, but each line break is two bytes in JSON.
        let lines = String(repeating: "\n", count: EngineBudget.maxConfigBytes / 2 + 1)
        XCTAssertLessThanOrEqual(EngineBudget.decryptionBytes([lines]), EngineBudget.maxConfigBytes)
        XCTAssertTrue(EngineBudget.decryptionExceedsConfig([lines]))
        // A 100 KB ad-block plugin is far within the decryption budget and still cannot fit.
        let adBlock = "[Rule]\n" + String(repeating: "DOMAIN-SUFFIX,ads.example.com,REJECT\n", count: 2800)
        XCTAssertNoThrow(try EngineBudget.checkDecryption([adBlock]))
        XCTAssertTrue(EngineBudget.decryptionExceedsConfig([adBlock]))
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
