@testable import NetworkCoreBridge
import XCTest

final class MeshKeyStoreTests: XCTestCase {
    /// An in-memory storage with a shared-Keychain copy and an optional read failure.
    private final class Memory {
        var values: [String: String] = [:]
        var legacy: [String: String] = [:]
        var failReads = false
        var writes = 0

        var storage: MeshKeyStore.Storage {
            MeshKeyStore.Storage(
                read: { [self] account in
                    if failReads {
                        throw MeshKeyStore.Failure(.keychain(errSecInteractionNotAllowed))
                    }
                    return values[account]
                },
                write: { [self] account, value in
                    writes += 1
                    values[account] = value
                },
                delete: { [self] in values.removeValue(forKey: $0) },
                legacy: { [self] in legacy[$0] },
                deleteLegacy: { [self] in legacy.removeValue(forKey: $0) }
            )
        }
    }

    func testEnsureCreatesOnceAndKeepsIt() throws {
        let memory = Memory()
        let first = try MeshKeyStore.ensurePrivateKey("office", storage: memory.storage)
        XCTAssertTrue(MeshNetworkLibrary.isValidPrivateKey(first))
        XCTAssertEqual(try MeshKeyStore.ensurePrivateKey("office", storage: memory.storage), first)
        XCTAssertEqual(memory.writes, 1)
        XCTAssertNotEqual(try MeshKeyStore.ensurePrivateKey("home", storage: memory.storage), first)
    }

    func testReadErrorNeverRegenerates() throws {
        let memory = Memory()
        let key = try MeshKeyStore.ensurePrivateKey("office", storage: memory.storage)
        memory.failReads = true
        XCTAssertThrowsError(try MeshKeyStore.ensurePrivateKey("office", storage: memory.storage))
        XCTAssertThrowsError(try MeshKeyStore.ensureInstanceID(storage: memory.storage))
        memory.failReads = false
        XCTAssertEqual(try MeshKeyStore.ensurePrivateKey("office", storage: memory.storage), key)
    }

    func testInvalidStoredValuesAreErrorsAndStay() throws {
        let memory = Memory()
        memory.values[MeshNetworkLibrary.privateKeyAccount("office")] = "short"
        XCTAssertThrowsError(try MeshKeyStore.ensurePrivateKey("office", storage: memory.storage)) { error in
            XCTAssertEqual(error as? MeshKeyStore.Failure, MeshKeyStore.Failure(.invalidStoredValue))
        }
        XCTAssertEqual(memory.values[MeshNetworkLibrary.privateKeyAccount("office")], "short")
        memory.values[MeshKeyStore.instanceIDAccount] = "-bad"
        XCTAssertThrowsError(try MeshKeyStore.ensureInstanceID(storage: memory.storage))
        XCTAssertEqual(memory.values[MeshKeyStore.instanceIDAccount], "-bad")
    }

    func testInstanceIDIsCreatedOnceAndValid() throws {
        let memory = Memory()
        let id = try MeshKeyStore.ensureInstanceID(storage: memory.storage)
        XCTAssertTrue(MeshNetworkLibrary.isValidInstanceID(id))
        XCTAssertEqual(try MeshKeyStore.ensureInstanceID(storage: memory.storage), id)
    }

    func testSharedKeychainKeyMovesToTheExtensionStore() throws {
        let memory = Memory()
        let old = try MeshNetworkLibrary.newPrivateKey()
        memory.legacy["office"] = old
        XCTAssertEqual(try MeshKeyStore.ensurePrivateKey("office", storage: memory.storage), old)
        XCTAssertEqual(memory.values[MeshNetworkLibrary.privateKeyAccount("office")], old)
        XCTAssertNil(memory.legacy["office"])

        memory.legacy["home"] = "short"
        XCTAssertThrowsError(try MeshKeyStore.ensurePrivateKey("home", storage: memory.storage))
        XCTAssertNil(memory.values[MeshNetworkLibrary.privateKeyAccount("home")])
    }

    func testSavedNamesRoundTrip() {
        let networks = [MeshNetwork(name: "a", config: "{}", joined: true), MeshNetwork(name: "b", config: "{}", joined: false)]
        XCTAssertEqual(MeshNetworkLibrary.savedNames(fromProfileValue: MeshNetworkLibrary.savedNamesValue(networks)), ["a", "b"])
        XCTAssertEqual(MeshNetworkLibrary.savedNames(fromProfileValue: MeshNetworkLibrary.savedNamesValue([])), [])
        XCTAssertNil(MeshNetworkLibrary.savedNames(fromProfileValue: nil))
        XCTAssertNil(MeshNetworkLibrary.savedNames(fromProfileValue: Data("x".utf8)))
    }
}
