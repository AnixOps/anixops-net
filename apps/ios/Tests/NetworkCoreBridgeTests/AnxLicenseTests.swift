@testable import NetworkCoreBridge
import XCTest

final class AnxLicenseTests: XCTestCase {
    /// RFC 8032 §7.1 test 1.
    private let seed = Data(
        stride(from: 0, to: 64, by: 2).map { offset -> UInt8 in
            let hex = Array("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
            return UInt8(String(hex[offset..<offset + 2]), radix: 16)!
        }
    )
    private let url = URL(string: "https://panel.example.com/s/token")!
    private let ca = "c83c22218d9d871e9eb6196777b1ee9715a34420d42646c09e6bbedf3687de26"

    func testDeviceKeyHeaderAndPKCS8() {
        let license = AnxLicense(deviceKey: seed)
        XCTAssertEqual(license.publicKeyHex, "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        XCTAssertEqual(license.deviceHeader, "ed25519:d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        XCTAssertEqual(license.pkcs8Hex, "302e020100300506032b657004220420" + "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
        XCTAssertEqual(AnxLicense.generate().deviceKey.count, 32)
    }

    func testDeviceIsNamedOnlyToOriginsThatOfferLicensing() {
        var license = AnxLicense(deviceKey: seed)
        XCTAssertNil(license.requestHeader(for: url))
        XCTAssertFalse(license.record(url: url) { _ in nil })
        XCTAssertFalse(license.offered)

        let headers = [AnxLicense.headerCA: ca, AnxLicense.headerLicense: "01AB", AnxLicense.headerStatus: "ok"]
        XCTAssertTrue(license.record(url: url) { headers[$0] })
        XCTAssertEqual(license.requestHeader(for: url), license.deviceHeader)
        XCTAssertEqual(license.requestHeader(for: URL(string: "https://PANEL.example.com:443/other")!), license.deviceHeader)
        XCTAssertNil(license.requestHeader(for: URL(string: "https://other.example.com/s/token")!))
        XCTAssertNil(license.requestHeader(for: URL(string: "http://panel.example.com/s/token")!))
        XCTAssertEqual(license.license, "01ab")
        XCTAssertEqual(license.status, "ok")
        // The second answer from the same origin is not new.
        XCTAssertFalse(license.record(url: url) { headers[$0] })
    }

    func testMalformedLicenseIsIgnoredAndStatusKept() {
        var license = AnxLicense(deviceKey: seed)
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abcd"][$0] }
        _ = license.record(url: url) {
            [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "xyz", AnxLicense.headerStatus: "no_seat"][$0]
        }
        XCTAssertEqual(license.license, "abcd")
        XCTAssertEqual(license.status, "no_seat")
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abc"][$0] }
        XCTAssertEqual(license.license, "abcd")
    }

    func testOriginIsForgottenWhenLicensingIsTurnedOff() {
        var license = AnxLicense(deviceKey: seed)
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abcd"][$0] }
        XCTAssertNotNil(license.outboundFields(licensePath: "/tmp/anx-license.cert"))
        _ = license.record(url: url) { _ in nil }
        XCTAssertFalse(license.offered)
        XCTAssertNil(license.license)
        XCTAssertNil(license.requestHeader(for: url))
        XCTAssertNil(license.outboundFields(licensePath: "/tmp/anx-license.cert"))
    }

    func testKeychainRoundTrip() throws {
        var license = AnxLicense(deviceKey: seed)
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abcd"][$0] }
        XCTAssertEqual(try AnxLicense.fromKeychainValue(try license.keychainValue()), license)
        XCTAssertThrowsError(try AnxLicense.fromKeychainValue(#"{"deviceKey":"AAEC","origins":[]}"#))
    }

    func testOnlyAnxOutboundsGetTheFields() {
        var license = AnxLicense(deviceKey: seed)
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca][$0] }
        let fields = license.outboundFields(licensePath: "/data/anx-license.cert")!
        XCTAssertEqual(fields["device_alg"], .string("ed25519"))
        XCTAssertEqual(fields["license_cert_path"], .string("/data/anx-license.cert"))

        let anx = EmbeddedRuntimeConfig.NativeOutbound(
            type: "anx", server: "203.0.113.1", port: 443, settings: ["type": .string("anx"), "user_key": .string("k")]
        )
        let config = EmbeddedRuntimeConfig(outbound: .native(anx), profileID: "p").withRouting(
            EmbeddedRuntimeConfig.Routing(
                outbounds: [
                    "a": ["type": .string("anx"), "server": .string("203.0.113.1")],
                    "s": ["type": .string("shadowsocks"), "server": .string("203.0.113.2")],
                ],
                rules: [],
                groups: []
            )
        )
        let licensed = config.withAnxLicense(fields)
        guard case let .native(native) = licensed.outbound else {
            return XCTFail("the selected node is no longer native")
        }
        XCTAssertEqual(native.settings["device_key"], fields["device_key"])
        XCTAssertEqual(native.settings["user_key"], .string("k"))
        XCTAssertEqual(licensed.routing?.outbounds["a"]?["license_cert_path"], .string("/data/anx-license.cert"))
        XCTAssertNil(licensed.routing?.outbounds["s"]?["device_key"])
    }
}
