import NetworkCoreBridge
import XCTest

final class TunnelRemoteAddressTests: XCTestCase {
    func testNumericServersAreKept() {
        XCTAssertEqual(IPAddressText.tunnelRemoteAddress(for: "203.0.113.7"), "203.0.113.7")
        XCTAssertEqual(IPAddressText.tunnelRemoteAddress(for: "2001:db8::1"), "2001:db8::1")
        XCTAssertEqual(IPAddressText.tunnelRemoteAddress(for: "[2001:db8::1]"), "2001:db8::1")
        XCTAssertEqual(IPAddressText.tunnelRemoteAddress(for: " 198.51.100.2 "), "198.51.100.2")
    }

    func testHostNamesAndMissingServersFallBackToLoopback() {
        XCTAssertEqual(IPAddressText.tunnelRemoteAddress(for: "jp1.example.com"), "127.0.0.1")
        XCTAssertEqual(IPAddressText.tunnelRemoteAddress(for: ""), "127.0.0.1")
        XCTAssertEqual(IPAddressText.tunnelRemoteAddress(for: nil), "127.0.0.1")
    }
}
