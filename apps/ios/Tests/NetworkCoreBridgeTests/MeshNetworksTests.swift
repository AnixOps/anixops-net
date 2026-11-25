@testable import NetworkCoreBridge
import XCTest

/// The 添加网络 form, the saved networks and their outbounds in the tunnel profile (docs/mesh-ui.md, 添加网络;
/// networkcore_anixops docs/architecture/mesh-module.md, 配置).
final class MeshNetworksTests: XCTestCase {
    private func draft(
        name: String = "office",
        secret: String = "s3cret",
        peers: String = "203.0.113.10:11010",
        address: String = ""
    ) -> MeshNetworkDraft {
        var draft = MeshNetworkDraft()
        draft.name = name
        draft.secret = secret
        draft.peers = peers
        draft.address = address
        return draft
    }

    private func object(_ text: String) throws -> [String: Any] {
        try XCTUnwrap(JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any])
    }

    func testDraftProblemsInOrder() {
        XCTAssertEqual(draft(name: " ").problem, .name)
        XCTAssertEqual(draft(name: String(repeating: "a", count: 65)).problem, .name)
        XCTAssertEqual(draft(name: "a/b").problem, .name)
        XCTAssertEqual(draft(secret: "").problem, .secret)
        XCTAssertEqual(draft(peers: " \n ").problem, .peers)
        XCTAssertEqual(draft(peers: "203.0.113.10:0").problem, .peer("203.0.113.10:0"))
        XCTAssertEqual(draft(peers: "host:70000").problem, .peer("host:70000"))
        XCTAssertEqual(draft(peers: "[2001:db8::1").problem, .peer("[2001:db8::1"))
        XCTAssertEqual(draft(peers: "a:b:c").problem, .peer("a:b:c"))
        XCTAssertEqual(draft(peers: "bad host").problem, .peer("bad host"))
        XCTAssertEqual(draft(address: "10.144.144.5").problem, .address)
        XCTAssertEqual(draft(address: "10.144.144.5/31").problem, .address)
        XCTAssertEqual(draft(address: "10.144.144.300/24").problem, .address)
        XCTAssertNil(draft(address: "10.144.144.5/24").problem)
        XCTAssertNil(draft(name: "办公室").problem)
    }

    func testPeerLines() throws {
        let plain = try XCTUnwrap(MeshNetworkDraft.peer("mesh.example.net"))
        XCTAssertEqual(plain.server, "mesh.example.net")
        XCTAssertEqual(plain.port, MeshNetworkDraft.defaultPort)
        XCTAssertEqual(plain.transport, "tcp")

        let udp = try XCTUnwrap(MeshNetworkDraft.peer("UDP://203.0.113.10:11011"))
        XCTAssertEqual(udp.server, "203.0.113.10")
        XCTAssertEqual(udp.port, 11011)
        XCTAssertEqual(udp.transport, "udp")

        let v6 = try XCTUnwrap(MeshNetworkDraft.peer("tcp://[2001:db8::1]:443"))
        XCTAssertEqual(v6.server, "2001:db8::1")
        XCTAssertEqual(v6.port, 443)

        let v6Default = try XCTUnwrap(MeshNetworkDraft.peer("[2001:db8::1]"))
        XCTAssertEqual(v6Default.port, MeshNetworkDraft.defaultPort)
        XCTAssertNil(MeshNetworkDraft.peer("[not-v6]:1"))
    }

    func testNetworkIsAnAccessNodeWithoutTheSecret() throws {
        var form = draft(name: " office ", peers: "203.0.113.10\n\nudp://[2001:db8::1]:11011", address: "10.144.144.5/24")
        form.security = .standard
        let network = try XCTUnwrap(form.network())
        XCTAssertEqual(network.name, "office")
        XCTAssertTrue(network.joined)
        XCTAssertFalse(network.config.contains("s3cret"))

        let config = try object(network.config)
        XCTAssertEqual(config["config_version"] as? Int, 1)
        let net = try XCTUnwrap(config["network"] as? [String: Any])
        XCTAssertEqual(net["name"] as? String, "office")
        XCTAssertEqual(net["kind"] as? String, "legacy")
        XCTAssertEqual(net["secure_mode"] as? Bool, false)
        XCTAssertNil(net["secret"])
        let node = try XCTUnwrap(config["node"] as? [String: Any])
        XCTAssertEqual(node["role"] as? String, "access")
        XCTAssertEqual(node["address"] as? String, "10.144.144.5/24")
        XCTAssertNil(node["listen_port"])
        let peers = try XCTUnwrap(config["peers"] as? [[String: Any]])
        XCTAssertEqual(peers.count, 2)
        XCTAssertEqual(peers[0]["server"] as? String, "203.0.113.10")
        XCTAssertEqual(peers[0]["port"] as? Int, 11010)
        XCTAssertEqual(peers[0]["transport"] as? String, "tcp")
        XCTAssertEqual(peers[1]["server"] as? String, "2001:db8::1")
        XCTAssertEqual(peers[1]["transport"] as? String, "udp")

        let managed = try object(try XCTUnwrap(draft().network()).config)
        let managedNetwork = try XCTUnwrap(managed["network"] as? [String: Any])
        XCTAssertEqual(managedNetwork["kind"] as? String, "managed")
        XCTAssertEqual(managedNetwork["secure_mode"] as? Bool, true)
        let auto = try XCTUnwrap(managed["node"] as? [String: Any])
        XCTAssertNil(auto["address"])

        XCTAssertNil(draft(secret: "").network())
    }

    func testRuntimeConfigAddsTheSecret() throws {
        let network = try XCTUnwrap(draft().network())
        let config = try object(try XCTUnwrap(network.runtimeConfigJSON(secret: "s3cret")))
        XCTAssertEqual((config["network"] as? [String: Any])?["secret"] as? String, "s3cret")

        let entry = try XCTUnwrap(network.outboundEntry(secret: "s3cret"))
        XCTAssertEqual(entry["type"], .string("mesh"))
        XCTAssertEqual(entry["config_version"], .int(1))
        guard case let .object(inner)? = entry["network"] else {
            return XCTFail("network is not an object")
        }
        XCTAssertEqual(inner["secret"], .string("s3cret"))

        XCTAssertNil(MeshNetwork(name: "x", config: "[]", joined: true).outboundEntry(secret: "s"))
    }

    func testOutboundKeysAreDistinctAndPlain() {
        XCTAssertEqual(MeshNetwork(name: "office-1.lab_a", config: "{}", joined: true).outboundKey, "mesh-office-1.lab_a")
        let spaced = MeshNetwork(name: "a b", config: "{}", joined: true).outboundKey
        let underscored = MeshNetwork(name: "a_b", config: "{}", joined: true).outboundKey
        XCTAssertEqual(spaced, "mesh-x612062")
        XCTAssertNotEqual(spaced, underscored)
        let chinese = MeshNetwork(name: "办公室", config: "{}", joined: true).outboundKey
        XCTAssertTrue(chinese.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || "._-".contains($0)) })
    }

    func testTheReleaseGateKeepsJoinedNetworksOutOfTheTunnel() throws {
        let joined = MeshNetwork(name: "a", config: #"{"config_version":1}"#, joined: true)
        XCTAssertNil(MeshNetworkLibrary.gatedProfileValue([joined], enabled: false))
        let value = try XCTUnwrap(MeshNetworkLibrary.gatedProfileValue([joined], enabled: true))
        XCTAssertEqual(MeshNetworkLibrary.gatedNetworks(fromProfileValue: value, enabled: false), [])
        XCTAssertEqual(MeshNetworkLibrary.gatedNetworks(fromProfileValue: value, enabled: true), [joined])
        #if !MESH_PREVIEW
        XCTAssertNil(MeshNetworkLibrary.gatedProfileValue([joined]))
        XCTAssertEqual(MeshNetworkLibrary.gatedNetworks(fromProfileValue: value), [])
        #endif
    }

    func testProfileValueKeepsJoinedNetworksOnly() throws {
        let joined = MeshNetwork(name: "a", config: #"{"config_version":1}"#, joined: true)
        let saved = MeshNetwork(name: "b", config: #"{"config_version":1}"#, joined: false)
        XCTAssertNil(MeshNetworkLibrary.profileValue([saved]))
        let value = try XCTUnwrap(MeshNetworkLibrary.profileValue([joined, saved]))
        XCTAssertFalse(String(decoding: value, as: UTF8.self).contains("secret"))
        XCTAssertEqual(MeshNetworkLibrary.fromProfileValue(value), [joined])
        XCTAssertEqual(MeshNetworkLibrary.fromProfileValue(nil), [])
        XCTAssertEqual(MeshNetworkLibrary.fromProfileValue(Data("{".utf8)), [])
    }

    func testOutboundsLeaveOutNetworksWithoutASecret() throws {
        let a = try XCTUnwrap(draft(name: "a").network())
        let b = try XCTUnwrap(draft(name: "b").network())
        let built = MeshNetworkLibrary.outbounds([a, b]) { $0 == "a" ? "k" : nil }
        XCTAssertEqual(Array(built.outbounds.keys), ["mesh-a"])
        XCTAssertEqual(built.missing, ["b"])
    }

    func testLibraryKeepsOneJoinedNetwork() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "MeshNetworksTests"))
        defaults.removePersistentDomain(forName: "MeshNetworksTests")
        defer {
            defaults.removePersistentDomain(forName: "MeshNetworksTests")
        }
        MeshNetworkLibrary.store(
            [
                MeshNetwork(name: "a", config: "{}", joined: true),
                MeshNetwork(name: "b", config: "{}", joined: false),
            ],
            defaults
        )
        MeshNetworkLibrary.setJoined("b", true, defaults)
        XCTAssertEqual(MeshNetworkLibrary.load(defaults).filter(\.joined).map(\.name), ["b"])
        MeshNetworkLibrary.setJoined("b", false, defaults)
        XCTAssertEqual(MeshNetworkLibrary.load(defaults).filter(\.joined).map(\.name), [])
    }

    func testWithMeshAddsOutboundsBesideTheRoutingSection() {
        let mesh: [String: [String: JSONValue]] = [
            "mesh-office": ["type": .string("mesh")],
            "n1": ["type": .string("mesh")],
        ]
        let routed = EmbeddedRuntimeConfig(
            outbound: .shadowsocks(server: "203.0.113.1", port: 8388, method: "aes-256-gcm", password: "p"),
            profileID: "p"
        ).withRouting(EmbeddedRuntimeConfig.Routing(
            outbounds: ["n1": ["type": .string("shadowsocks")]],
            rules: ["FINAL,n1"],
            groups: []
        ))
        let meshed = routed.withMesh(mesh)
        XCTAssertEqual(meshed.routing?.outbounds["mesh-office"], ["type": .string("mesh")])
        // A node of the same key stays.
        XCTAssertEqual(meshed.routing?.outbounds["n1"], ["type": .string("shadowsocks")])
        XCTAssertEqual(meshed.routing?.rules, ["FINAL,n1"])
        XCTAssertEqual(routed.withMesh([:]), routed)
    }

    func testWithMeshGivesTheSelectedNodeARoutingSection() throws {
        let alone = EmbeddedRuntimeConfig(
            outbound: .shadowsocks(server: "203.0.113.1", port: 8388, method: "aes-256-gcm", password: "p"),
            profileID: "p"
        )
        let meshed = alone.withMesh(["mesh-office": ["type": .string("mesh")]])
        let routing = try XCTUnwrap(meshed.routing)
        XCTAssertEqual(routing.rules, ["FINAL,node"])
        XCTAssertEqual(routing.global, "node")
        XCTAssertEqual(routing.outbounds["node"]?["type"], .string("shadowsocks"))
        XCTAssertEqual(routing.outbounds["node"]?["password"], .string("p"))
        XCTAssertEqual(routing.outbounds["mesh-office"], ["type": .string("mesh")])
    }

    func testWithMeshLeavesAWholeDeviceTunnelUnchanged() {
        let wireguard = EmbeddedRuntimeConfig(
            outbound: .wireguard(EmbeddedRuntimeConfig.WireGuardPeer(
                privateKey: "k", peerPublicKey: "p", presharedKey: nil, server: "203.0.113.1", port: 51820,
                persistentKeepaliveSecs: nil
            )),
            profileID: "p"
        )
        XCTAssertEqual(wireguard.withMesh(["mesh-office": ["type": .string("mesh")]]), wireguard)
    }

    func testStartSecretsCarryMeshSecrets() {
        let secrets = TunnelStartSecrets(meshSecrets: ["office": "k"])
        XCTAssertFalse(secrets.isEmpty)
        XCTAssertEqual(TunnelStartSecrets(options: secrets.options)?.meshSecrets, ["office": "k"])
    }
}
