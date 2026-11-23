@testable import NetworkCoreBridge
import XCTest

final class TermsConsentTests: XCTestCase {
    func testAsksUntilAcceptedAndAgainOnlyForANewMajorVersion() {
        XCTAssertTrue(TermsConsent.needsConsent(accepted: nil, current: "2.0.0"))
        XCTAssertTrue(TermsConsent.needsConsent(accepted: "", current: "2.0.0"))
        XCTAssertTrue(TermsConsent.needsConsent(accepted: "garbage", current: "2.0.0"))
        XCTAssertFalse(TermsConsent.needsConsent(accepted: "2.0.0", current: "2.0.0"))
        XCTAssertFalse(TermsConsent.needsConsent(accepted: "2.0.0", current: "2.3.1"))
        XCTAssertTrue(TermsConsent.needsConsent(accepted: "1.0.0", current: "2.0.0"))
    }

    func testReadsTheVersionLine() {
        XCTAssertEqual(TermsConsent.version(inTerms: "基准版本 2.1.0，来源：x"), "2.1.0")
        XCTAssertNil(TermsConsent.version(inTerms: "no version"))
    }

    /// The app asks for the version TERMS.md carries, so a new major version of the terms asks everyone again.
    func testVersionMatchesTermsFile() throws {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let terms = try String(contentsOf: root.appendingPathComponent("TERMS.md"), encoding: .utf8)
        XCTAssertEqual(TermsConsent.version(inTerms: terms), TermsConsent.version)
    }

    /// The privacy policy the app links to is the repository's PRIVACY.md, which exists and covers every platform.
    func testPrivacyPolicyLinksToThePolicyFile() throws {
        XCTAssertEqual(PrivacyPolicy.url.scheme, "https")
        XCTAssertEqual(PrivacyPolicy.url.host, "github.com")
        XCTAssertEqual(PrivacyPolicy.url.lastPathComponent, "PRIVACY.md")
        XCTAssertEqual(
            PrivacyPolicy.url.deletingLastPathComponent(), TermsConsent.url.deletingLastPathComponent(),
            "the policy and the agreement come from the same place"
        )
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let policy = try String(contentsOf: root.appendingPathComponent("PRIVACY.md"), encoding: .utf8)
        for platform in ["**Android**", "**iOS and iPadOS**", "**macOS**", "**Windows**", "**Linux**"] {
            XCTAssertTrue(policy.contains(platform), "PRIVACY.md says nothing about \(platform)")
        }
    }

    func testDecryptionIsConfirmedOnce() {
        XCTAssertTrue(DecryptionConsent.needsConsent(accepted: nil))
        XCTAssertTrue(DecryptionConsent.needsConsent(accepted: ""))
        XCTAssertFalse(DecryptionConsent.needsConsent(accepted: "2.1.0"))
    }
}
