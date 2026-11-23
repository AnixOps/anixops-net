@testable import NetworkCoreBridge
import XCTest

/// Every way of starting HTTPS decryption asks for the confirmation first (TERMS.md section 4), and the tunnel
/// profile never carries a mappings plugin that decrypts without it.
final class DecryptionConsentTests: XCTestCase {
    private let confirmed = "2.1.2"

    func testAddingADomainToDecryptAsksUntilConfirmed() throws {
        let current = DebugMappings()
        let next = try current.addingHost("api.example.com")
        XCTAssertTrue(DecryptionConsent.isRequired(from: current, to: next, accepted: nil))
        XCTAssertTrue(DecryptionConsent.isRequired(from: current, to: next, accepted: ""))
        XCTAssertFalse(DecryptionConsent.isRequired(from: current, to: next, accepted: confirmed))
    }

    func testOnlyANewlyDecryptedDomainAsks() throws {
        let current = try DebugMappings().addingHost("api.example.com")
        // A domain already decrypted asks nothing again, nor does a change that decrypts less.
        XCTAssertFalse(DecryptionConsent.isRequired(from: current, to: current, accepted: nil))
        XCTAssertFalse(DecryptionConsent.isRequired(
            from: current,
            to: current.settingHost("api.example.com", enabled: false),
            accepted: nil
        ))
        XCTAssertFalse(DecryptionConsent.isRequired(
            from: current,
            to: current.removingHost("api.example.com"),
            accepted: nil
        ))
        XCTAssertFalse(DecryptionConsent.isRequired(
            from: current,
            to: try current.addingHost("api.example.com", exclude: true),
            accepted: nil
        ))
        XCTAssertTrue(DecryptionConsent.isRequired(
            from: current,
            to: try current.addingHost("*.cdn.example.com"),
            accepted: nil
        ))
    }

    func testExclusionsAloneNeverAsk() throws {
        let next = try DebugMappings().addingHost("bank.example.com", exclude: true)
        XCTAssertTrue(next.decryptedPatterns.isEmpty)
        XCTAssertFalse(DecryptionConsent.isRequired(from: DebugMappings(), to: next, accepted: nil))
        XCTAssertFalse(DecryptionConsent.isRequired(
            from: DebugMappings(),
            to: try DebugMappings().addingHost("-bank.example.com"),
            accepted: nil
        ))
    }

    func testTurningAPausedLineOnAsks() {
        let paused = DebugMappings(domains: [.init(pattern: "api.example.com", enabled: false)])
        XCTAssertTrue(DecryptionConsent.isRequired(
            from: paused,
            to: paused.settingHost("api.example.com", enabled: true),
            accepted: nil
        ))
    }

    /// The row menu's "改为解密" turns an exclusion into a decrypted line.
    func testTurningAnExclusionIntoADecryptedLineAsks() throws {
        let excluded = DebugMappings(domains: [.init(pattern: "api.example.com", exclude: true)])
        let next = try excluded.addingHost("api.example.com", exclude: false)
        XCTAssertEqual(next.decryptedPatterns, ["api.example.com"])
        XCTAssertTrue(DecryptionConsent.isRequired(from: excluded, to: next, accepted: nil))
        XCTAssertTrue(DecryptionConsent.isRequired(
            from: excluded,
            to: try excluded.editingHost("api.example.com", to: "api.example.com", exclude: false),
            accepted: nil
        ))
    }

    /// A Map Local / Map Remote entry decrypts its host, an http:// one included: the host joins `[MITM] hostname`.
    func testSavingOrTurningOnAMappingWithAHostAsks() throws {
        let entry = DebugMappings.Entry(kind: .local, pattern: "http://api.example.com/config")
        let saved = try DebugMappings().saving(entry)
        XCTAssertEqual(saved.decryptedPatterns, ["api.example.com"])
        XCTAssertTrue(DecryptionConsent.isRequired(from: DebugMappings(), to: saved, accepted: nil))
        XCTAssertFalse(DecryptionConsent.isRequired(from: DebugMappings(), to: saved, accepted: confirmed))

        var off = entry
        off.enabled = false
        let paused = try DebugMappings().saving(off)
        XCTAssertFalse(DecryptionConsent.isRequired(from: DebugMappings(), to: paused, accepted: nil))
        XCTAssertTrue(DecryptionConsent.isRequired(from: paused, to: try paused.saving(entry), accepted: nil))

        // Editing the response of a mapping whose host is decrypted already asks nothing.
        var edited = entry
        edited.status = 404
        XCTAssertFalse(DecryptionConsent.isRequired(from: saved, to: try saved.saving(edited), accepted: nil))
    }

    func testAMappingWithoutAHostNeverAsks() throws {
        let entry = DebugMappings.Entry(kind: .local, match: .regex, pattern: #"^https?://.*\.example\.com/x"#)
        XCTAssertNil(entry.host)
        let next = try DebugMappings().saving(entry)
        XCTAssertFalse(DecryptionConsent.isRequired(from: DebugMappings(), to: next, accepted: nil))
    }

    // MARK: - The profile

    func testProfileLeavesOutAPluginThatDecryptsUntilConfirmed() throws {
        let mappings = try DebugMappings()
            .addingHost("api.example.com")
            .saving(.init(kind: .remote, pattern: "https://a.example.com/v1", target: "https://b.example.com/v1"))
        XCTAssertNotNil(mappings.pluginText)
        XCTAssertNil(DecryptionConsent.profilePlugin(for: mappings, accepted: nil))
        XCTAssertNil(DecryptionConsent.profilePlugin(for: mappings, accepted: ""))
        XCTAssertEqual(DecryptionConsent.profilePlugin(for: mappings, accepted: confirmed), mappings.pluginText)
    }

    func testProfileKeepsAPluginThatDecryptsNothing() throws {
        XCTAssertNil(DecryptionConsent.profilePlugin(for: DebugMappings(), accepted: nil))
        let exclusions = try DebugMappings().addingHost("bank.example.com", exclude: true)
        XCTAssertEqual(DecryptionConsent.profilePlugin(for: exclusions, accepted: nil), exclusions.pluginText)
        XCTAssertNotNil(exclusions.pluginText)
        let hostless = try DebugMappings().saving(
            .init(kind: .local, match: .regex, pattern: #"^https?://.*\.example\.com/x"#)
        )
        XCTAssertEqual(DecryptionConsent.profilePlugin(for: hostless, accepted: nil), hostless.pluginText)
    }

    /// `decryptedPatterns` is exactly what the plugin's `[MITM] hostname` line decrypts.
    func testDecryptedPatternsMatchThePluginLine() throws {
        let mappings = try DebugMappings()
            .saving(.init(kind: .local, pattern: "https://a.example.com/x"))
            .addingHost("a.example.com")
            .addingHost("*.cdn.example.com")
            .addingHost("pay.example.com", exclude: true)
        XCTAssertEqual(mappings.decryptedPatterns, ["a.example.com", "*.cdn.example.com"])
        XCTAssertTrue(try XCTUnwrap(mappings.pluginText).hasSuffix(
            "hostname = %APPEND% a.example.com, *.cdn.example.com, -pay.example.com\n"
        ))
    }
}
