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

final class NotificationPermissionTests: XCTestCase {
    private let allOn = NotificationSettings()
    private let allOff = NotificationSettings(scripts: false, connection: false, subscriptions: false)

    func testAsksOnceInFrontWhileAKindIsOn() {
        XCTAssertTrue(NotificationPermission.shouldAsk(status: .notDetermined, settings: allOn, appActive: true, demo: false))
        // Already answered: the system would not show the prompt again.
        XCTAssertFalse(NotificationPermission.shouldAsk(status: .denied, settings: allOn, appActive: true, demo: false))
        XCTAssertFalse(NotificationPermission.shouldAsk(status: .allowed, settings: allOn, appActive: true, demo: false))
        // A tunnel brought up by On Demand or the widget while the app is in the background does not ask.
        XCTAssertFalse(NotificationPermission.shouldAsk(status: .notDetermined, settings: allOn, appActive: false, demo: false))
        // The screenshots' demo never shows the prompt.
        XCTAssertFalse(NotificationPermission.shouldAsk(status: .notDetermined, settings: allOn, appActive: true, demo: true))
        XCTAssertFalse(NotificationPermission.shouldAsk(status: .notDetermined, settings: allOff, appActive: true, demo: false))
        let one = NotificationSettings(scripts: false, connection: false, subscriptions: true)
        XCTAssertTrue(NotificationPermission.shouldAsk(status: .notDetermined, settings: one, appActive: true, demo: false))
    }

    func testSettingsOfferFollowsTheSystemAnswer() {
        XCTAssertEqual(NotificationPermission.offer(status: .notDetermined, settings: allOn), .ask)
        XCTAssertEqual(NotificationPermission.offer(status: .denied, settings: allOn), .openSystemSettings)
        XCTAssertEqual(NotificationPermission.offer(status: .allowed, settings: allOn), .nothing)
        XCTAssertEqual(NotificationPermission.offer(status: .denied, settings: allOff), .nothing)
    }

    func testTappedNotificationsOpenOnlyWebLinks() {
        XCTAssertEqual(NotificationPermission.openableURL("https://example.com/a?b=1")?.host, "example.com")
        XCTAssertEqual(NotificationPermission.openableURL("HTTP://example.com")?.host, "example.com")
        // A script must not run the app's own commands behind a tap.
        XCTAssertNil(NotificationPermission.openableURL("anixops://mode?set=direct"))
        XCTAssertNil(NotificationPermission.openableURL("anixops://disconnect"))
        XCTAssertNil(NotificationPermission.openableURL("ANIXOPS://select?node=a"))
        XCTAssertNil(NotificationPermission.openableURL("javascript:alert(1)"))
        XCTAssertNil(NotificationPermission.openableURL("tel:123"))
        XCTAssertNil(NotificationPermission.openableURL("https://"))
        XCTAssertNil(NotificationPermission.openableURL(""))
        XCTAssertNil(NotificationPermission.openableURL(nil))
    }
}
