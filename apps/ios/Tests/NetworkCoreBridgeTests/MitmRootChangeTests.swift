import Foundation
@testable import NetworkCoreBridge
import XCTest

/// A regenerated or replaced MITM root (ios-bridge-features-07, ios-app-tools-03): a start without the app's values
/// fails on an older kept root, as on an older payload, and the app can tell when the running tunnel uses another
/// root.
final class MitmRootChangeTests: XCTestCase {
    func testKeptRootMustMatchTheProfileFingerprint() throws {
        let old = try MitmCertificateAuthority.generate()
        let new = try MitmCertificateAuthority.generate()
        let kept = TunnelStartSecrets(mitmAuthority: try MitmCertificateStore.encode(old))
        XCTAssertEqual(TunnelStartSecrets.rootFingerprint(ofStoredValue: try MitmCertificateStore.encode(old)), old.sha256Fingerprint)
        XCTAssertTrue(kept.mitmRootMatches(profileFingerprint: old.sha256Fingerprint))
        XCTAssertFalse(kept.mitmRootMatches(profileFingerprint: new.sha256Fingerprint))
        // A root generated after the last start: the copy has none.
        XCTAssertFalse(TunnelStartSecrets(nodeKey: "key").mitmRootMatches(profileFingerprint: new.sha256Fingerprint))
        XCTAssertFalse(TunnelStartSecrets(mitmAuthority: Data([1, 2])).mitmRootMatches(profileFingerprint: new.sha256Fingerprint))
        // Profiles saved before the fingerprint existed check nothing.
        XCTAssertTrue(kept.mitmRootMatches(profileFingerprint: nil))
        XCTAssertTrue(TunnelStartSecrets().mitmRootMatches(profileFingerprint: nil))
    }

    func testKeptCopyWithAnOlderRootFailsAsStale() throws {
        // The root was generated or replaced while the tunnel was down, then the system started it without options:
        // fail with the stale-copy reason, as for an older payload, so the app starts once more with options.
        let old = try MitmCertificateAuthority.generate()
        let new = try MitmCertificateAuthority.generate()
        let kept = TunnelStartSecrets(nodeKey: "key", mitmAuthority: try MitmCertificateStore.encode(old), nodeKeyVersion: "B")
        let reason = TunnelStartSecrets.mitmRootFailureReason(
            profileFingerprint: new.sha256Fingerprint, decrypts: true, keptCopy: kept
        )
        XCTAssertEqual(reason, TunnelStartSecrets.staleCopyReason)
        XCTAssertTrue(TunnelStartSecrets.isStaleCopyFailure(reason))
        // A copy from before any root.
        XCTAssertEqual(
            TunnelStartSecrets.mitmRootFailureReason(
                profileFingerprint: new.sha256Fingerprint, decrypts: true, keptCopy: TunnelStartSecrets(nodeKey: "key")
            ),
            TunnelStartSecrets.staleCopyReason
        )
        // The same root, a profile that decrypts nothing, one saved before the fingerprint, or fresh values in this
        // start's options (no kept copy, and always on iOS).
        XCTAssertNil(TunnelStartSecrets.mitmRootFailureReason(
            profileFingerprint: old.sha256Fingerprint, decrypts: true, keptCopy: kept
        ))
        XCTAssertNil(TunnelStartSecrets.mitmRootFailureReason(
            profileFingerprint: new.sha256Fingerprint, decrypts: false, keptCopy: kept
        ))
        XCTAssertNil(TunnelStartSecrets.mitmRootFailureReason(profileFingerprint: nil, decrypts: true, keptCopy: kept))
        XCTAssertNil(TunnelStartSecrets.mitmRootFailureReason(
            profileFingerprint: new.sha256Fingerprint, decrypts: true, keptCopy: nil
        ))
    }

    func testNewRootCountsAsAnotherProfileForTheRetry() throws {
        // A retry already made for this profile is made again after the root changed: only the fingerprint differs.
        let bundle = try MitmProfileBundle(plugins: ["[MITM]\nhostname = %APPEND% example.com\n"]).profileValue()
        let before = TunnelStartSecrets.staleCopyProfileStamp(nodeKeyVersion: "B", mitmBundle: bundle, mitmRoot: "AA")
        let after = TunnelStartSecrets.staleCopyProfileStamp(nodeKeyVersion: "B", mitmBundle: bundle, mitmRoot: "BB")
        XCTAssertNotEqual(before, after)
        XCTAssertTrue(TunnelStartSecrets.retriesStaleCopy(
            failure: TunnelStartSecrets.staleCopyReason, profile: after, retriedProfile: before
        ))
        // A profile without a fingerprint keeps the stamp it had.
        XCTAssertEqual(
            TunnelStartSecrets.staleCopyProfileStamp(nodeKeyVersion: "B", mitmBundle: bundle, mitmRoot: nil),
            TunnelStartSecrets.staleCopyProfileStamp(nodeKeyVersion: "B", mitmBundle: bundle)
        )
        XCTAssertNotNil(TunnelStartSecrets.staleCopyProfileStamp(nodeKeyVersion: nil, mitmBundle: nil, mitmRoot: "AA"))
    }

    func testProfileReferencesPayloadWhetherOrNotItReads() throws {
        let bundle = MitmProfileBundle(plugins: ["[MITM]\nhostname = %APPEND% example.com\n"])
        let configuration: [String: Any] = [TunnelProfileKeys.mitmBundle: try bundle.profileValue()]
        XCTAssertTrue(MitmProfileBundle.referencesPayload(configuration))
        // Chunks of another payload: nothing loads, but the profile still asks for decryption.
        XCTAssertTrue(MitmProfileBundle.fromProfile(configuration, chunks: ["AAAA"]).isEmpty)
        XCTAssertFalse(MitmProfileBundle.referencesPayload([:]))
        XCTAssertFalse(MitmProfileBundle.referencesPayload(nil))
        XCTAssertFalse(MitmProfileBundle.referencesPayload([TunnelProfileKeys.mitmBundle: Data("x".utf8)]))
    }

    func testDecryptionStateCarriesTheRoot() throws {
        let on = HttpsDecryptionState(status: .on, plugins: 2, rootFingerprint: "AA:BB")
        XCTAssertEqual(HttpsDecryptionState.decoded(from: on.encoded()), on)
        // Both optional fields in one state, and an unknown reason next to a fingerprint.
        let both = HttpsDecryptionState(status: .off, reason: .tooLarge, plugins: 3, bytes: 70_000, rootFingerprint: "AA:BB")
        XCTAssertEqual(HttpsDecryptionState.decoded(from: both.encoded()), both)
        let future = try XCTUnwrap(HttpsDecryptionState.decoded(
            from: Data(#"{"status":"on","reason":"later","plugins":1,"rootFingerprint":"AA:BB"}"#.utf8)
        ))
        XCTAssertNil(future.reason)
        XCTAssertEqual(future.rootFingerprint, "AA:BB")
        // An older extension sends no fingerprint.
        XCTAssertNil(HttpsDecryptionState.decoded(from: Data(#"{"status":"on","plugins":1}"#.utf8))?.rootFingerprint)

        XCTAssertTrue(on.usesOtherRoot(than: "CC:DD"))
        XCTAssertFalse(on.usesOtherRoot(than: "AA:BB"))
        XCTAssertFalse(on.usesOtherRoot(than: nil))
        XCTAssertFalse(HttpsDecryptionState(status: .on, plugins: 1).usesOtherRoot(than: "CC:DD"))
        XCTAssertFalse(HttpsDecryptionState(status: .off, reason: .noRoot, rootFingerprint: "AA:BB").usesOtherRoot(than: "CC:DD"))
    }
}

#if os(macOS)
/// Replacing the root on a Mac takes the old one's trust back, not only its keychain item (apple-extensions-mac-06).
final class MitmRootRevocationTests: XCTestCase {
    private final class FakeStore: MitmRootTrustStore {
        var calls: [String] = []
        var trustAnswer: OSStatus = errSecSuccess
        var deleteAnswer: OSStatus = errSecSuccess
        var trustedAfterwards = false

        func removeUserTrust(_ certificateDER: Data) -> OSStatus {
            calls.append("trust")
            return trustAnswer
        }

        func removeFromKeychain(_ certificateDER: Data) -> OSStatus {
            calls.append("item")
            return deleteAnswer
        }

        func isTrusted(_ certificateDER: Data) -> Bool {
            calls.append("check")
            return trustedAfterwards
        }
    }

    private let old = Data([1])
    private let new = Data([2])

    func testRemovesTheTrustBeforeTheItem() {
        let store = FakeStore()
        XCTAssertEqual(MitmRootRevocation.revoke(previous: old, current: new, store: store), .revoked)
        XCTAssertEqual(store.calls, ["trust", "item", "check"])
    }

    func testNoTrustOrNoItemCountsAsDone() {
        let store = FakeStore()
        store.trustAnswer = errSecItemNotFound
        store.deleteAnswer = errSecItemNotFound
        XCTAssertEqual(MitmRootRevocation.revoke(previous: old, current: new, store: store), .revoked)
        let none = FakeStore()
        none.trustAnswer = errSecNoTrustSettings
        XCTAssertEqual(MitmRootRevocation.revoke(previous: old, current: new, store: none), .revoked)
    }

    func testCancelledPromptKeepsTheItem() {
        for answer in [errAuthorizationCanceled, errSecAuthFailed, errSecUserCanceled] {
            let store = FakeStore()
            store.trustAnswer = answer
            XCTAssertEqual(MitmRootRevocation.revoke(previous: old, current: new, store: store), .cancelled)
            XCTAssertEqual(store.calls, ["trust"])
        }
    }

    func testOtherFailureKeepsTheItem() {
        let store = FakeStore()
        store.trustAnswer = errSecInternalComponent
        XCTAssertEqual(MitmRootRevocation.revoke(previous: old, current: new, store: store), .failed(errSecInternalComponent))
        XCTAssertEqual(store.calls, ["trust"])
    }

    func testTrustLeftElsewhereIsReported() {
        let store = FakeStore()
        store.trustedAfterwards = true
        XCTAssertEqual(MitmRootRevocation.revoke(previous: old, current: new, store: store), .stillTrusted)
    }

    func testItemThatStaysIsReported() {
        let store = FakeStore()
        store.deleteAnswer = errSecInteractionNotAllowed
        XCTAssertEqual(MitmRootRevocation.revoke(previous: old, current: new, store: store), .itemKept(errSecInteractionNotAllowed))
    }

    func testSameCertificateRevokesNothing() {
        let store = FakeStore()
        XCTAssertNil(MitmRootRevocation.revoke(previous: old, current: old, store: store))
        XCTAssertTrue(store.calls.isEmpty)
    }
}
#endif
