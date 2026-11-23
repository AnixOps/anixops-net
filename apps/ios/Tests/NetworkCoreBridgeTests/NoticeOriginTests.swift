@testable import NetworkCoreBridge
import XCTest

final class NoticeOriginTests: XCTestCase {
    func testStopNoticeGoesOnceTheTunnelIsBack() {
        XCTAssertFalse(NoticeOrigin.tunnelStopped.applies(whileConnected: true))
        XCTAssertTrue(NoticeOrigin.tunnelStopped.applies(whileConnected: false))
    }

    func testOtherNoticesStayWhileConnected() {
        // A rejected node, a failed subscription update or a failed save is not fixed by reconnecting.
        XCTAssertTrue(NoticeOrigin.other.applies(whileConnected: true))
        XCTAssertTrue(NoticeOrigin.other.applies(whileConnected: false))
    }
}
