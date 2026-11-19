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

    private let origin = "https://panel.example.com:443"

    func testDeviceKeyHeaderAndPKCS8() {
        let entry = AnxLicense.Entry(deviceKey: seed)
        XCTAssertEqual(entry.publicKeyHex, "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        XCTAssertEqual(entry.deviceHeader, "ed25519:d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        // PKCS#8 v2 as ring's `Ed25519KeyPair::from_pkcs8` (the ANX outbound) requires: version 1, the seed, then
        // `[1]` with the public key. A v1 key (version 0, no public key) made every ANX outbound refuse to build.
        XCTAssertEqual(
            entry.pkcs8Hex,
            "3051020101300506032b657004220420"
                + "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"
                + "812100"
                + "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
        )
        XCTAssertEqual(entry.pkcs8Hex.count, 83 * 2)
        XCTAssertEqual(AnxLicense.Entry.generate().deviceKey.count, 32)
    }

    func testDeviceIsNamedOnlyToOriginsThatOfferLicensing() {
        var license = AnxLicense()
        XCTAssertNil(license.requestHeader(for: url))
        XCTAssertFalse(license.record(url: url) { _ in nil })
        XCTAssertFalse(license.offered)

        let headers = [AnxLicense.headerCA: ca, AnxLicense.headerLicense: "01AB", AnxLicense.headerStatus: "ok"]
        XCTAssertTrue(license.record(url: url) { headers[$0] })
        let header = license.entries[origin]!.deviceHeader
        XCTAssertEqual(license.requestHeader(for: url), header)
        XCTAssertEqual(license.requestHeader(for: URL(string: "https://PANEL.example.com:443/other")!), header)
        XCTAssertNil(license.requestHeader(for: URL(string: "https://other.example.com/s/token")!))
        XCTAssertNil(license.requestHeader(for: URL(string: "http://panel.example.com/s/token")!))
        XCTAssertEqual(license.entries[origin]?.license, "01ab")
        XCTAssertEqual(license.entries[origin]?.status, "ok")
        // The second answer from the same origin is not new.
        XCTAssertFalse(license.record(url: url) { headers[$0] })
    }

    func testEachPanelGetsItsOwnKeyAndLicense() {
        var license = AnxLicense()
        let other = URL(string: "https://other.example.com/s/2")!
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abcd"][$0] }
        _ = license.record(url: other) { [AnxLicense.headerCA: self.ca, AnxLicense.headerStatus: "no_seat"][$0] }
        XCTAssertEqual(license.origins, ["https://other.example.com:443", origin])
        XCTAssertNotEqual(license.requestHeader(for: url), license.requestHeader(for: other))
        // The second panel's answer did not replace the first one's license.
        XCTAssertEqual(license.entries[origin]?.license, "abcd")
        XCTAssertNil(license.entries["https://other.example.com:443"]?.license)
        XCTAssertNotEqual(AnxLicense.licenseFileName(origin: origin), AnxLicense.licenseFileName(origin: "https://other.example.com:443"))
    }

    func testMalformedLicenseIsIgnoredAndStatusKept() {
        var license = AnxLicense()
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abcd"][$0] }
        _ = license.record(url: url) {
            [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "xyz", AnxLicense.headerStatus: "no_seat"][$0]
        }
        XCTAssertEqual(license.entries[origin]?.license, "abcd")
        XCTAssertEqual(license.entries[origin]?.status, "no_seat")
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abc"][$0] }
        XCTAssertEqual(license.entries[origin]?.license, "abcd")
    }

    func testOriginIsForgottenWhenLicensingIsTurnedOff() {
        var license = AnxLicense()
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abcd"][$0] }
        XCTAssertNotNil(license.entries[origin])
        _ = license.record(url: url) { _ in nil }
        XCTAssertFalse(license.offered)
        XCTAssertNil(license.entries[origin])
        XCTAssertNil(license.requestHeader(for: url))
    }

    func testKeychainRoundTrip() throws {
        var license = AnxLicense()
        _ = license.record(url: url) { [AnxLicense.headerCA: self.ca, AnxLicense.headerLicense: "abcd"][$0] }
        XCTAssertEqual(try AnxLicense.fromKeychainValue(try license.keychainValue()), license)
        XCTAssertThrowsError(try AnxLicense.fromKeychainValue(#"{"origins":["a"],"entries":{"a":{"deviceKey":"AAEC"}}}"#))
    }

    func testSingleKeyOfOlderVersionsStaysWithTheNewestOrigin() throws {
        let legacy = """
        {"deviceKey":"\(seed.base64EncodedString())","origins":["https://old.example:443","\(origin)"],\
        "license":"abcd","status":"ok"}
        """
        var license = try AnxLicense.fromKeychainValue(legacy)
        XCTAssertEqual(license.origins, [origin, "https://old.example:443"])
        XCTAssertEqual(license.entries[origin], AnxLicense.Entry(deviceKey: seed, license: "abcd", status: "ok"))
        // The other origin gets a key of its own at its next answer, and is asked again with it at once.
        let old = URL(string: "https://old.example/s/1")!
        XCTAssertNil(license.requestHeader(for: old))
        XCTAssertTrue(license.record(url: old) { [AnxLicense.headerCA: self.ca][$0] })
        XCTAssertNotNil(license.requestHeader(for: old))
        XCTAssertNotEqual(license.requestHeader(for: old), license.requestHeader(for: url))
    }

    func testLibraryMarksAnxNodesWithItsOrigin() throws {
        let anx = NativeImportReport.Node(
            id: "a", name: "A", protocol: "anx", server: "203.0.113.1", port: 443, outbound: ["type": .string("anx")],
            licenseOrigin: "https://claimed.example:443"
        )
        let ss = NativeImportReport.Node(
            id: "s", name: "S", protocol: "shadowsocks", server: "203.0.113.2", port: 443, outbound: [:]
        )
        let library = NodeLibrary(subscriptionURL: "https://Panel.Example.com/s/token", nodes: [anx, ss], selectedID: nil, updatedAt: Date())
        let marked = library.markingLicenseOrigins()
        XCTAssertEqual(marked.nodes.map(\.licenseOrigin), [origin, nil])
        let pasted = NodeLibrary(subscriptionURL: nil, nodes: [anx], selectedID: nil, updatedAt: Date())
        XCTAssertNil(pasted.markingLicenseOrigins().nodes[0].licenseOrigin)
    }

    func testEachAnxOutboundGetsItsOwnOriginsFields() {
        let fieldsA: [String: JSONValue] = ["device_key": .string("aa"), "license_cert_path": .string("/data/a.cert")]
        let fieldsB: [String: JSONValue] = ["device_key": .string("bb"), "license_cert_path": .string("/data/b.cert")]
        let node = { (id: String, server: String, origin: String?) in
            NativeImportReport.Node(
                id: id, name: id, protocol: "anx", server: server, port: 443, outbound: [:], licenseOrigin: origin
            )
        }
        let licenses = OutboundLicenses(
            fieldsByOrigin: ["https://a.example:443": fieldsA, "https://b.example:443": fieldsB],
            nodes: [
                node("a", "203.0.113.1", "https://a.example:443"),
                node("b", "203.0.113.2", "https://b.example:443"),
                node("c", "203.0.113.3", "https://c.example:443"),
                node("p", "203.0.113.4", nil),
            ]
        )

        let selected = EmbeddedRuntimeConfig.NativeOutbound(
            type: "anx", server: "203.0.113.2", port: 443, settings: ["type": .string("anx"), "user_key": .string("k")]
        )
        let config = EmbeddedRuntimeConfig(outbound: .native(selected), profileID: "p").withRouting(
            EmbeddedRuntimeConfig.Routing(
                outbounds: [
                    "a": ["type": .string("anx"), "server": .string("203.0.113.1")],
                    "b": ["type": .string("anx"), "server": .string("203.0.113.2")],
                    "c": ["type": .string("anx"), "server": .string("203.0.113.3")],
                    "p": ["type": .string("anx"), "server": .string("203.0.113.4")],
                    "s": ["type": .string("shadowsocks"), "server": .string("203.0.113.5")],
                ],
                rules: [],
                groups: []
            )
        )
        let licensed = config.withAnxLicense(licenses)
        guard case let .native(native) = licensed.outbound else {
            return XCTFail("the selected node is no longer native")
        }
        XCTAssertEqual(native.settings["device_key"], .string("bb"))
        XCTAssertEqual(native.settings["user_key"], .string("k"))
        XCTAssertEqual(licensed.routing?.outbounds["a"]?["device_key"], .string("aa"))
        XCTAssertEqual(licensed.routing?.outbounds["b"]?["license_cert_path"], .string("/data/b.cert"))
        // An origin without a license, a node without an origin, and other protocols get nothing.
        XCTAssertNil(licensed.routing?.outbounds["c"]?["device_key"])
        XCTAssertNil(licensed.routing?.outbounds["p"]?["device_key"])
        XCTAssertNil(licensed.routing?.outbounds["s"]?["device_key"])
    }
}
