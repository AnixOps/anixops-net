import Foundation
import NetworkCoreBridge
import XCTest

/// The LAN share's proxy credentials (`LanShareAuth`): made with the system's CSPRNG, typeable on a phone, required by
/// default, and read back from what an older build or another client kept.
final class LanShareAuthTests: XCTestCase {
    func testGeneratedCredentialsAreRandomTypeableAndRequired() throws {
        let auth = try LanShareAuth.generate()
        XCTAssertTrue(auth.required)
        XCTAssertTrue(auth.isValid)
        XCTAssertTrue(auth.username.hasPrefix("anx-"))
        XCTAssertEqual(auth.username.count, 10)
        XCTAssertEqual(auth.password.count, LanShareAuth.passwordLength)
        XCTAssertTrue(auth.password.utf8.allSatisfy { LanShareAuth.alphabet.contains($0) })
        for ambiguous in ["0", "o", "1", "l", "i"] {
            XCTAssertFalse(auth.password.contains(ambiguous))
        }
        let other = try LanShareAuth.generate()
        XCTAssertNotEqual(auth.password, other.password)
        XCTAssertNotEqual(auth.username, other.username)
    }

    func testRegeneratingKeepsTheSwitch() throws {
        var auth = try LanShareAuth.generate()
        auth.required = false
        let fresh = try auth.regenerated()
        XCTAssertFalse(fresh.required)
        XCTAssertNotEqual(fresh.password, auth.password)
    }

    func testKeptFormReadsBackAndIsRequiredWithoutTheSwitch() throws {
        let auth = try LanShareAuth.generate()
        XCTAssertEqual(try JSONDecoder().decode(LanShareAuth.self, from: JSONEncoder().encode(auth)), auth)
        let old = try JSONDecoder().decode(LanShareAuth.self, from: Data(#"{"username":"anx-abc","password":"secret"}"#.utf8))
        XCTAssertTrue(old.required)
        XCTAssertFalse(LanShareAuth(username: "a:b", password: "secret").isValid)
        XCTAssertFalse(LanShareAuth(username: "anx-abc", password: "").isValid)
    }
}
