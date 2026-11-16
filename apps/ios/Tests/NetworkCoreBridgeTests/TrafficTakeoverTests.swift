import Foundation
import NetworkCoreBridge
import XCTest

final class TrafficTakeoverTests: XCTestCase {
    func testChoicesPerPlatform() {
        XCTAssertEqual(TrafficTakeover.choices(macOS: false), [.tunnelAndProxy, .tunnel])
        XCTAssertEqual(TrafficTakeover.choices(macOS: true), [.systemProxy, .tunnelAndProxy, .tunnel])
        XCTAssertTrue(TrafficTakeover.tunnelAndProxy.routesTraffic)
        XCTAssertTrue(TrafficTakeover.tunnelAndProxy.usesSystemProxy)
        XCTAssertFalse(TrafficTakeover.tunnel.usesSystemProxy)
        XCTAssertFalse(TrafficTakeover.systemProxy.routesTraffic)
    }

    func testSavedModeFallsBackToTheDefault() {
        XCTAssertEqual(TrafficTakeover.saved(in: nil, macOS: false), .tunnelAndProxy)
        XCTAssertEqual(TrafficTakeover.saved(in: [TunnelProfileKeys.takeover: "tun"], macOS: false), .tunnel)
        XCTAssertEqual(TrafficTakeover.saved(in: [TunnelProfileKeys.takeover: "proxy"], macOS: true), .systemProxy)
        // iOS has no proxy-only mode, and an unknown value is the default.
        XCTAssertEqual(TrafficTakeover.saved(in: [TunnelProfileKeys.takeover: "proxy"], macOS: false), .tunnelAndProxy)
        XCTAssertEqual(TrafficTakeover.saved(in: [TunnelProfileKeys.takeover: "gateway"], macOS: true), .tunnelAndProxy)
    }

    func testExceptionEntries() {
        for valid in ["localhost", "*.local", "router.lan", "192.168.0.0/16", "10.0.0.1", "fe80::/10", "::1"] {
            XCTAssertTrue(ProxyExceptions.isValid(valid), valid)
        }
        for invalid in ["", "*", "0.0.0.0/0", "::/0", "a b", "http://x", ".local", "a..b", "例子.com", "10.0.0.0/40"] {
            XCTAssertFalse(ProxyExceptions.isValid(invalid), invalid)
        }
    }

    func testParsingKeepsValidLinesOnceAndReportsTheRest() {
        let (exceptions, rejected) = ProxyExceptions.parse("""
        # LAN
        localhost

        *.corp.example
        LOCALHOST
        not valid
        10.0.0.0/8
        """)
        XCTAssertEqual(exceptions.entries, ["localhost", "*.corp.example", "10.0.0.0/8"])
        XCTAssertEqual(rejected, [6])
        XCTAssertEqual(exceptions.text, "localhost\n*.corp.example\n10.0.0.0/8")
    }

    func testProfileValueRoundTripsAndDefaults() {
        XCTAssertEqual(ProxyExceptions.fromProfileValue(nil), ProxyExceptions())
        XCTAssertTrue(ProxyExceptions().isDefault)
        let custom = ProxyExceptions(entries: ["localhost", "*.example"])
        XCTAssertEqual(ProxyExceptions.fromProfileValue(custom.profileValue), custom)
        // Entries a newer or tampered profile carries that do not parse are dropped.
        let tampered = Data(#"["localhost","0.0.0.0/0"]"#.utf8)
        XCTAssertEqual(ProxyExceptions.fromProfileValue(tampered).entries, ["localhost"])
    }
}
