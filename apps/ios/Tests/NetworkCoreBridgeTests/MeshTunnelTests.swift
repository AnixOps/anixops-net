@testable import NetworkCoreBridge
import XCTest

/// A tunnel running for the mesh alone (networkcore_anixops docs/architecture/mesh-status.md, 隧道生命周期).
final class MeshTunnelTests: XCTestCase {
    func testReadsTheTunnelSectionOfTheSnapshot() throws {
        let status = Data(#"""
        {"statusVersion":1,"tunnel":{"routes":["10.144.144.0/24","192.168.20.0/24"],"routesVersion":3,
        "dns":{"servers":["198.18.0.2"],"matchDomains":["office.internal"]}}}
        """#.utf8)
        let tunnel = try XCTUnwrap(MeshTunnel(status: status))
        XCTAssertEqual(tunnel.routes, ["10.144.144.0/24", "192.168.20.0/24"])
        XCTAssertEqual(tunnel.routesVersion, 3)
        XCTAssertEqual(tunnel.dnsServers, ["198.18.0.2"])
        XCTAssertEqual(tunnel.matchDomains, ["office.internal"])

        let noDns = try XCTUnwrap(MeshTunnel(status: Data(#"{"tunnel":{"routes":[],"routesVersion":1,"dns":null}}"#.utf8)))
        XCTAssertEqual(noDns.dnsServers, [])
        XCTAssertNil(MeshTunnel(status: Data(#"{"ok":false,"error":{"code":"unsupported"}}"#.utf8)))
        XCTAssertNil(MeshTunnel(status: nil))
    }

    func testTheTunnelNeverCarriesARouteWiderThanAMesh() throws {
        let wide = ["0.0.0.0/0", "0.0.0.0/1", "128.0.0.0/1", "0.0.0.0/7", "::/0", "::/1", "8000::/1", "2000::/3", "fc00::/6"]
        let narrow = ["10.0.0.0/8", "fc00::/7", "10.144.144.0/24", "198.18.0.2", "fd00::1"]
        let routes = (wide + narrow).map { "\"\($0)\"" }.joined(separator: ",")
        let json = #"{"tunnel":{"routes":["# + routes + #"],"routesVersion":1}}"#
        let tunnel = try XCTUnwrap(MeshTunnel(status: Data(json.utf8)))
        XCTAssertEqual(tunnel.routes, narrow)
        let built = MeshTunnel(routes: wide + narrow, routesVersion: 1)
        XCTAssertEqual(MeshTunnel.interface(built).includedRoutes, narrow)
        XCTAssertFalse(MeshTunnel.tooWide("not a route"))
        XCTAssertFalse(MeshTunnel.tooWide("10.0.0.0/x"))
    }

    func testTheInterfaceRoutesOnlyTheMesh() {
        let tunnel = MeshTunnel(
            routes: ["10.144.144.0/24"],
            routesVersion: 1,
            dnsServers: ["198.18.0.2"],
            matchDomains: ["office.internal"]
        )
        let interface = MeshTunnel.interface(tunnel)
        XCTAssertEqual(interface.addresses, TunnelInterface.proxy.addresses)
        XCTAssertEqual(interface.includedRoutes, ["10.144.144.0/24", "198.18.0.2"])
        XCTAssertEqual(interface.dnsServers, ["198.18.0.2"])
        XCTAssertFalse(interface.includedRoutes.contains("0.0.0.0/0"))

        // Without suffixes the device keeps its own DNS.
        var bare = tunnel
        bare.matchDomains = []
        XCTAssertEqual(MeshTunnel.interface(bare).dnsServers, [])
        XCTAssertEqual(MeshTunnel.interface(bare).includedRoutes, ["10.144.144.0/24"])

        // Before the engine answers, nothing is routed.
        XCTAssertEqual(MeshTunnel.interface(nil).includedRoutes, [])
    }

    func testTheMeshOnlyConfigHasOnlyTheMeshAndDirect() throws {
        let config = EmbeddedRuntimeConfig.meshOnly(["mesh-office": ["type": .string("mesh")]], profileID: "mesh-only")
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: config.encoded()) as? [String: Any])
        XCTAssertNil(object["outbound"])
        XCTAssertEqual(object["mode"] as? String, "rule")
        let routing = try XCTUnwrap(object["routing"] as? [String: Any])
        XCTAssertEqual((routing["outbounds"] as? [String: Any])?.keys.sorted(), ["mesh-office"])
        XCTAssertEqual(routing["rules"] as? [String], ["FINAL,DIRECT"])
    }

    func testSavedNetworksNameTheirEncryption() {
        XCTAssertEqual(
            MeshNetwork(name: "a", config: #"{"network":{"secure_mode":true}}"#, joined: true).security,
            "forward_secrecy"
        )
        XCTAssertEqual(
            MeshNetwork(name: "a", config: #"{"network":{"secure_mode":false}}"#, joined: true).security,
            "standard"
        )
        XCTAssertEqual(MeshNetwork(name: "a", config: #"{"network":{"cipher":"none"}}"#, joined: true).security, "none")
        XCTAssertNil(MeshNetwork(name: "a", config: "[]", joined: true).security)
    }

    func testMeshNeedsTheReleaseGateAndTheEnginesCapability() {
        // An engine that runs mesh (a probe that answers so) turns it on only with the release gate open, in either build;
        // `MeshGateTests` runs both values of the gate through `available(enabled:)`.
        MeshSupport.installCapabilitiesProbe { Data(#"{"capabilities":{"mesh":true}}"#.utf8) }
        defer { MeshSupport.installCapabilitiesProbe { nil } }
        XCTAssertEqual(MeshSupport.askEngine(), true)
        XCTAssertEqual(MeshSupport.available, MeshSupport.enabled)
        XCTAssertTrue(MeshSupport.available(enabled: true))
        XCTAssertFalse(MeshSupport.available(enabled: false))
    }
}
