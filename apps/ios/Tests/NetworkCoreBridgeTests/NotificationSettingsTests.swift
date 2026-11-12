import Foundation
import NetworkCoreBridge
import XCTest

final class NotificationSettingsTests: XCTestCase {
    func testDefaultsAreOnAndRoundTrip() {
        XCTAssertEqual(NotificationSettings.fromProfileValue(nil), NotificationSettings())
        XCTAssertEqual(NotificationSettings.fromProfileValue(Data("garbage".utf8)), NotificationSettings())
        let settings = NotificationSettings(scripts: false, connection: true, subscriptions: false)
        XCTAssertEqual(NotificationSettings.fromProfileValue(settings.profileValue), settings)
    }

    func testMissingKeysStayOn() {
        let decoded = NotificationSettings.fromProfileValue(Data(#"{"scripts":false,"future":1}"#.utf8))
        XCTAssertFalse(decoded.scripts)
        XCTAssertTrue(decoded.connection)
        XCTAssertTrue(decoded.subscriptions)
    }

    func testOnlyUnexpectedStopsNotify() {
        // providerFailed, noNetworkAvailable, connectionFailed
        XCTAssertTrue(NotificationSettings.notifiesStop(reasonCode: 2))
        XCTAssertTrue(NotificationSettings.notifiesStop(reasonCode: 3))
        XCTAssertTrue(NotificationSettings.notifiesStop(reasonCode: 14))
        // none, userInitiated, configurationDisabled, sleep, appUpdate
        for quiet in [0, 1, 9, 15, 16] {
            XCTAssertFalse(NotificationSettings.notifiesStop(reasonCode: quiet), "\(quiet)")
        }
    }

    func testTravelsAsAnAppMessage() {
        let payload = NotificationSettings(scripts: false).profileValue
        let message = TunnelAppMessage.notificationSettings(payload)
        XCTAssertEqual(TunnelAppMessage.decode(message.encoded), message)
        // Older extensions read it as a status request, which is harmless.
        XCTAssertNotEqual(TunnelAppMessage.decode(Data("control:{}".utf8)), message)
    }
}
