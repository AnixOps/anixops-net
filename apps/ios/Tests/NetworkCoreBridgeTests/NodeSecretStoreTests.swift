@testable import NetworkCoreBridge
import XCTest
import Security

final class NodeSecretStoreTests: XCTestCase {
    func testCleanUpKeepsTheSavedKeyAndTheKeysOfSavesOnTheirWay() {
        let saved = NodeSecretStore.versionedAccount("saved")
        let inFlight = NodeSecretStore.versionedAccount("in-flight")
        let older = [NodeSecretStore.versionedAccount("old-1"), NodeSecretStore.versionedAccount("old-2")]
        // The fixed account and the store's other values are no key versions.
        let others = [
            NodeSecretStore.tunnelProfileAccount,
            SubscriptionGroups.keychainAccount,
            NodeLibrary.keychainAccount,
            AnxLicense.keychainAccount,
            MitmProfileBundle.accountPrefix + "0",
        ]
        let accounts = others + [older[0], saved, inFlight, older[1]]
        XCTAssertEqual(NodeSecretStore.staleNodeKeyAccounts(accounts, keeping: ["saved", "in-flight"]), older)
    }

    func testKeyOfTheProfileSavedLastSurvivesTheCleanUp() {
        // 0.3.0-beta.11: two saves ran at once and each kept only the key it wrote itself, so the clean-up of the
        // save that finished first removed the key of the profile that stayed in the system settings. The key the
        // saved profile names, and that of a save still on its way, stay.
        let first = NodeSecretStore.versionedAccount("first")
        let second = NodeSecretStore.versionedAccount("second")
        XCTAssertEqual(NodeSecretStore.staleNodeKeyAccounts([first, second], keeping: ["first", "second"]), [])
        XCTAssertEqual(NodeSecretStore.staleNodeKeyAccounts([first, second], keeping: ["second"]), [first])
        XCTAssertEqual(NodeSecretStore.staleNodeKeyAccounts([first, second], keeping: []), [first, second])
    }

    func testOnlyALockedKeychainCountsAsReadableLater() {
        // The app reads its subscription groups again after the first unlock and refuses to overwrite them until
        // then; a failure that repeats on every read must not block every write for good.
        XCTAssertTrue(NodeSecretStore.Failure(status: errSecInteractionNotAllowed).isLockedUntilFirstUnlock)
        XCTAssertFalse(NodeSecretStore.Failure(status: errSecDecode).isLockedUntilFirstUnlock)
        XCTAssertFalse(NodeSecretStore.Failure(status: errSecMissingEntitlement).isLockedUntilFirstUnlock)
    }
}
