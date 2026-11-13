import NetworkCoreBridge
import XCTest
#if canImport(NetworkExtension)
import NetworkExtension
#endif

final class OnDemandSettingsTests: XCTestCase {
    func testDefaultReconnectsEverywhere() {
        let steps = OnDemandSettings().steps
        XCTAssertEqual(steps?.map(\.action), [.connect, .connect, .connect])
        XCTAssertEqual(steps?.map(\.interface), [.wifi, .cellular, .any])
        XCTAssertEqual(steps?.first?.ssids, [])
    }

    func testOffHasNoRules() {
        XCTAssertNil(OnDemandSettings(enabled: false).steps)
    }

    func testNamedNetworksComeFirstGroupedByAction() {
        var settings = OnDemandSettings(otherWiFi: .ignore, cellular: .disconnect)
        XCTAssertTrue(settings.setRule(ssid: " Home ", action: .disconnect))
        XCTAssertTrue(settings.setRule(ssid: "Cafe", action: .connect))
        XCTAssertTrue(settings.setRule(ssid: "Office", action: .disconnect))
        let steps = settings.steps ?? []
        XCTAssertEqual(steps.count, 5)
        XCTAssertEqual(steps[0], .init(action: .disconnect, interface: .wifi, ssids: ["Home", "Office"]))
        XCTAssertEqual(steps[1], .init(action: .connect, interface: .wifi, ssids: ["Cafe"]))
        XCTAssertEqual(steps[2], .init(action: .ignore, interface: .wifi, ssids: []))
        XCTAssertEqual(steps[3], .init(action: .disconnect, interface: .cellular, ssids: []))
        XCTAssertEqual(steps[4], .init(action: .connect, interface: .any, ssids: []))
    }

    func testSetRuleReplacesAndRejectsBadNames() {
        var settings = OnDemandSettings()
        XCTAssertTrue(settings.setRule(ssid: "Home", action: .disconnect))
        XCTAssertTrue(settings.setRule(ssid: "Home", action: .connect))
        XCTAssertEqual(settings.wifi, [.init(ssid: "Home", action: .connect)])
        XCTAssertFalse(settings.setRule(ssid: "   ", action: .connect))
        XCTAssertFalse(settings.setRule(ssid: String(repeating: "a", count: 33), action: .connect))
        // 32 bytes is the SSID limit: 10 three-byte characters fit, 11 do not.
        XCTAssertTrue(settings.setRule(ssid: String(repeating: "网", count: 10), action: .connect))
        XCTAssertFalse(settings.setRule(ssid: String(repeating: "网", count: 11), action: .connect))
        settings.removeRule(ssid: "Home")
        XCTAssertEqual(settings.wifi.map(\.ssid), [String(repeating: "网", count: 10)])
    }

    func testListIsCapped() {
        var settings = OnDemandSettings()
        for index in 0..<OnDemandSettings.maxWiFiRules {
            XCTAssertTrue(settings.setRule(ssid: "net-\(index)", action: .disconnect))
        }
        XCTAssertFalse(settings.setRule(ssid: "one-more", action: .disconnect))
        XCTAssertTrue(settings.setRule(ssid: "net-0", action: .connect))
    }

    func testRoundTripsThroughDefaults() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "OnDemandSettingsTests"))
        defaults.removePersistentDomain(forName: "OnDemandSettingsTests")
        XCTAssertEqual(OnDemandSettings.load(from: defaults), OnDemandSettings())
        var settings = OnDemandSettings(cellular: .ignore)
        settings.setRule(ssid: "Home", action: .disconnect)
        settings.save(to: defaults)
        XCTAssertEqual(OnDemandSettings.load(from: defaults), settings)
    }

    #if canImport(NetworkExtension)
    func testRulesMatchTheSteps() throws {
        var settings = OnDemandSettings()
        settings.setRule(ssid: "Home", action: .disconnect)
        let rules = try XCTUnwrap(settings.rules)
        XCTAssertTrue(rules.first is NEOnDemandRuleDisconnect)
        XCTAssertEqual(rules.first?.ssidMatch, ["Home"])
        XCTAssertEqual(rules.first?.interfaceTypeMatch, .wiFi)
        XCTAssertTrue(rules.last is NEOnDemandRuleConnect)
        XCTAssertEqual(rules.last?.interfaceTypeMatch, .any)
        #if os(macOS)
        // No cellular rule on macOS: it would match every interface there.
        XCTAssertEqual(rules.count, 3)
        #else
        XCTAssertEqual(rules.count, 4)
        #endif
        XCTAssertNil(OnDemandSettings(enabled: false).rules)
    }
    #endif
}
