import Foundation
@testable import NetworkCoreBridge
import XCTest

/// The macOS capture session key (ios-bridge-features-01): the System Extension cannot use the data protection
/// keychain, so the key is a root-only file that is made once and never replaced.
final class CaptureSessionKeyFileTests: XCTestCase {
    private var folder: URL!

    override func setUpWithError() throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: folder)
    }

    private var url: URL {
        folder.appendingPathComponent(CaptureSessionKeyFile.fileName)
    }

    private func key(_ byte: UInt8) -> Data {
        Data(repeating: byte, count: CaptureSessionKeyFile.length)
    }

    func testMakesTheKeyOnceAndReadsItBack() throws {
        var made = 0
        let first = try CaptureSessionKeyFile.loadOrCreate(at: url) {
            made += 1
            return key(7)
        }
        XCTAssertEqual(first, key(7))
        let second = try CaptureSessionKeyFile.loadOrCreate(at: url) {
            made += 1
            return key(9)
        }
        XCTAssertEqual(second, key(7))
        XCTAssertEqual(made, 1)
        XCTAssertEqual(try Data(contentsOf: url), key(7))
    }

    func testKeyFileIsOwnerOnlyAndLeavesNoTemporaryFile() throws {
        _ = try CaptureSessionKeyFile.loadOrCreate(at: url) { key(1) }
        let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber
        XCTAssertEqual(mode?.intValue, 0o600)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: folder.path), [CaptureSessionKeyFile.fileName])
    }

    func testMalformedFileIsAnErrorAndIsNeverReplaced() throws {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data([1, 2, 3]).write(to: url)
        XCTAssertThrowsError(try CaptureSessionKeyFile.loadOrCreate(at: url) { self.key(5) }) { error in
            XCTAssertEqual(error as? CaptureSessionKeyFile.Failure, .malformed(3))
        }
        // A new key would make every saved session unreadable.
        XCTAssertEqual(try Data(contentsOf: url), Data([1, 2, 3]))
    }

    func testUnreadableFileIsAnError() throws {
        // A folder where the key file should be cannot be read as one.
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        XCTAssertThrowsError(try CaptureSessionKeyFile.loadOrCreate(at: url) { self.key(5) }) { error in
            XCTAssertEqual(error as? CaptureSessionKeyFile.Failure, .unreadable)
        }
    }

    func testRefusesANewKeyOfTheWrongLength() {
        XCTAssertThrowsError(try CaptureSessionKeyFile.loadOrCreate(at: url) { Data([1]) }) { error in
            XCTAssertEqual(error as? CaptureSessionKeyFile.Failure, .malformed(1))
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
    }

    func testRandomKeyHasTheRightLength() throws {
        XCTAssertEqual(try CaptureSessionStorage.randomKey().count, CaptureSessionKeyFile.length)
        XCTAssertEqual(try CaptureSessionStorage.keyFileURL().lastPathComponent, CaptureSessionKeyFile.fileName)
    }

    func testStopNoticeFollowsWhetherStorageWorked() {
        XCTAssertNil(CaptureStopNotice.afterStop(captured: false, storageAvailable: true))
        XCTAssertNil(CaptureStopNotice.afterStop(captured: false, storageAvailable: false))
        XCTAssertEqual(CaptureStopNotice.afterStop(captured: true, storageAvailable: true), .saved)
        // Before the fix the page said "saved" whatever happened; without storage nothing was.
        XCTAssertEqual(CaptureStopNotice.afterStop(captured: true, storageAvailable: false), .notSaved)
        XCTAssertEqual(CaptureStopNotice.afterStop(captured: true, storageAvailable: nil), .unknown)
    }
}
