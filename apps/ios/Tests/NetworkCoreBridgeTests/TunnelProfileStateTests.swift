@testable import NetworkCoreBridge
import XCTest

final class TunnelProfileStateTests: XCTestCase {
    func testStateFollowsTheSavedConfiguration() {
        XCTAssertEqual(TunnelProfileState.of(hasConfiguration: false, isEnabled: false), .missing)
        // A configuration that is gone cannot be enabled; "missing" wins.
        XCTAssertEqual(TunnelProfileState.of(hasConfiguration: false, isEnabled: true), .missing)
        XCTAssertEqual(TunnelProfileState.of(hasConfiguration: true, isEnabled: false), .disabled)
        XCTAssertEqual(TunnelProfileState.of(hasConfiguration: true, isEnabled: true), .ready)
    }

    func testOffHintSaysWhatAConnectDoes() {
        XCTAssertEqual(TunnelProfileState.missing.offHint, .firstConnect)
        XCTAssertEqual(TunnelProfileState.disabled.offHint, .reenable)
        XCTAssertNil(TunnelProfileState.ready.offHint)
    }
}
