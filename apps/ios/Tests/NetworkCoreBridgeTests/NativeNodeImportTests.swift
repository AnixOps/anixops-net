import Foundation
import NetworkCoreBridge
import XCTest

final class NativeNodeImportTests: XCTestCase {
    /// What crates/node-import returns for a Trojan-over-WebSocket link and an unsupported node.
    private let trojanReport = """
    {"nodes":[{"id":"n1","name":"HK Trojan","protocol":"trojan","server":"edge.example.com","port":443,
    "outbound":{"type":"trojan","server":"edge.example.com","port":443,"password":"trojan-secret",
    "server_name":"cdn.example.com","alpn":["h2","http/1.1"],"insecure":false,
    "transport":{"type":"ws","path":"/ws","host":"cdn.example.com"}}}],
    "unsupported":[{"name":"Old","protocol":"hysteria","reason":"protocol is not implemented by the embedded engine yet"}]}
    """

    private func importer(_ json: String) -> NativeNodeImporter {
        { _ in Data(json.utf8) }
    }

    func testOtherProtocolsBecomeANativeOutboundWithTheirSettingsInTheKeychain() throws {
        let profile = try TunnelNodeImport.parse(
            "trojan://trojan-secret@edge.example.com:443?type=ws",
            profileID: "default",
            native: importer(trojanReport)
        )
        XCTAssertEqual(profile.displayName, "HK Trojan")
        XCTAssertEqual(profile.serverAddress, "edge.example.com")
        XCTAssertEqual(profile.interface, .proxy)
        XCTAssertEqual(profile.config.outboundKind, "trojan")

        let full = try XCTUnwrap(JSONSerialization.jsonObject(with: profile.config.encoded()) as? [String: Any])
        let outbound = try XCTUnwrap(full["outbound"] as? [String: Any])
        XCTAssertEqual(outbound["type"] as? String, "trojan")
        XCTAssertEqual(outbound["port"] as? Int, 443)
        XCTAssertEqual(outbound["password"] as? String, "trojan-secret")
        XCTAssertEqual(outbound["insecure"] as? Bool, false)
        XCTAssertEqual(outbound["alpn"] as? [String], ["h2", "http/1.1"])
        XCTAssertEqual((outbound["transport"] as? [String: Any])?["path"] as? String, "/ws")

        let stored = try profile.config.withoutSecret().encoded()
        let storedText = String(decoding: stored, as: UTF8.self)
        XCTAssertFalse(storedText.contains("trojan-secret"))
        let storedOutbound = try XCTUnwrap(
            (JSONSerialization.jsonObject(with: stored) as? [String: Any])?["outbound"] as? [String: Any]
        )
        XCTAssertEqual(Set(storedOutbound.keys), ["type", "server", "port"])

        let secret = NodeSecret.fromKeychainValue(try profile.config.secret.keychainValue())
        let restored = try EmbeddedRuntimeConfig.decoded(from: stored).withSecret(secret)
        XCTAssertEqual(restored, profile.config)
    }

    func testShadowsocksAndRealityFromTheImporterKeepTheirOwnCases() throws {
        let report = """
        {"nodes":[{"id":"n1","name":"","protocol":"shadowsocks","server":"198.51.100.1","port":8388,
        "outbound":{"type":"shadowsocks","server":"198.51.100.1","port":8388,"method":"aes-256-gcm","password":"pw"}}],
        "unsupported":[]}
        """
        let profile = try TunnelNodeImport.parse("ss://subscription", profileID: "default", native: importer(report))
        XCTAssertEqual(profile.config.outboundKind, "shadowsocks (aes-256-gcm)")
        XCTAssertEqual(profile.displayName, "198.51.100.1")
        XCTAssertEqual(profile.config.secret, NodeSecret(password: "pw"))
    }

    func testVlessOverTlsIsNativeButRealityIsBuiltIn() throws {
        let tls = Data(#"{"type":"vless","server":"a.example","port":443,"uuid":"u","tls":{"server_name":"a.example"}}"#.utf8)
        let outbound = try JSONDecoder().decode(EmbeddedRuntimeConfig.Outbound.self, from: tls)
        guard case let .native(native) = outbound else {
            return XCTFail("expected a native outbound")
        }
        XCTAssertEqual(native.type, "vless")
        XCTAssertEqual(native.settings["uuid"], .string("u"))
        // The profile copy (no settings) still reads back as the same native outbound.
        let bare = try JSONDecoder().decode(
            EmbeddedRuntimeConfig.Outbound.self,
            from: Data(#"{"type":"vless","server":"a.example","port":443}"#.utf8)
        )
        guard case .native = bare else {
            return XCTFail("expected a native outbound")
        }
    }

    // MARK: - Every imported protocol through the tunnel profile

    private let realityUUID = "b831381d-6324-4d53-ad4f-8cda48b30811"
    private let reality = #"{"server_name":"www.example.com","public_key":"realityPublicKey","short_id":"6ba85179e30d4fc2","fingerprint":"auto"}"#
    /// A line node inlined as the `via` of the node behind it (`NativeImportReport.Node.standalone(among:)`).
    private let line = #"{"type":"iwan","server":"203.0.113.10","port":4567,"username":"line-user","password":"line-secret"}"#
    private let wireGuardPrivateKey = Data(repeating: 7, count: 32).base64EncodedString()
    private let wireGuardPeerKey = Data(repeating: 9, count: 32).base64EncodedString()
    private let wireGuardPresharedKey = Data(repeating: 3, count: 32).base64EncodedString()

    /// A WireGuard node of a subscription or `wireguard://` link, with the address the server gave this device.
    private var wireGuardOutbound: String {
        #"{"type":"wireguard","server":"wg.example","port":51820,"private_key":"\#(wireGuardPrivateKey)","peer_public_key":"\#(wireGuardPeerKey)","local_address":["172.16.0.2/32","fd01::2/128"],"preshared_key":"\#(wireGuardPresharedKey)","allowed_ips":["0.0.0.0/0","::/0"],"mtu":1280,"reserved":[1,2,3]}"#
    }

    private var realityXhttpOutbound: String {
        #"{"type":"vless","server":"x.example","port":443,"uuid":"\#(realityUUID)","flow":"","reality":\#(reality),"transport":{"type":"xhttp","path":"/up","host":"x.example","mode":"auto"}}"#
    }

    /// An outbound as crates/node-import writes it, what the profile copy must leave out (the secrets, and for a
    /// native outbound every setting), and whether the node keeps a dedicated case.
    private struct Imported {
        let name: String
        let outbound: String
        let keptOut: [String]
        var dedicated = false
    }

    private var importedSamples: [Imported] {
        [
            Imported(
                name: "shadowsocks",
                outbound: #"{"type":"shadowsocks","server":"198.51.100.1","port":8388,"method":"2022-blake3-aes-128-gcm","password":"ss-secret"}"#,
                keptOut: ["ss-secret"],
                dedicated: true
            ),
            Imported(
                name: "shadowsocks behind a line",
                outbound: #"{"type":"shadowsocks","server":"198.51.100.2","port":8388,"method":"2022-blake3-aes-128-gcm","password":"land-secret","via":\#(line)}"#,
                keptOut: ["land-secret", "via", "line-user", "line-secret"]
            ),
            Imported(
                name: "shadowsocks with multiplexing",
                outbound: #"{"type":"shadowsocks","server":"198.51.100.3","port":8388,"method":"aes-128-gcm","password":"mux-secret","multiplex":{"enabled":true,"protocol":"h2mux","max_connections":2,"min_streams":8,"padding":true}}"#,
                keptOut: ["mux-secret", "multiplex"]
            ),
            Imported(
                name: "vless reality",
                outbound: #"{"type":"vless","server":"r.example","port":443,"uuid":"\#(realityUUID)","flow":"xtls-rprx-vision","reality":\#(reality)}"#,
                keptOut: [realityUUID],
                dedicated: true
            ),
            Imported(name: "vless reality over xhttp", outbound: realityXhttpOutbound, keptOut: [realityUUID, "xhttp"]),
            Imported(
                name: "vless reality behind a line",
                outbound: #"{"type":"vless","server":"rl.example","port":443,"uuid":"\#(realityUUID)","flow":"","reality":\#(reality),"via":\#(line)}"#,
                keptOut: [realityUUID, "via", "line-secret"]
            ),
            Imported(
                name: "vless over tls and websocket",
                outbound: #"{"type":"vless","server":"t.example","port":443,"uuid":"vless-secret","tls":{"server_name":"t.example","alpn":["h2"],"insecure":false},"transport":{"type":"ws","path":"/ws","host":"t.example"}}"#,
                keptOut: ["vless-secret"]
            ),
            Imported(
                name: "trojan",
                outbound: #"{"type":"trojan","server":"edge.example.com","port":443,"password":"trojan-secret","server_name":"cdn.example.com","alpn":["h2","http/1.1"],"insecure":false,"transport":null}"#,
                keptOut: ["trojan-secret"]
            ),
            Imported(
                name: "vmess",
                outbound: #"{"type":"vmess","server":"v.example","port":443,"uuid":"vmess-secret","security":"auto","tls":null,"transport":{"type":"grpc","service_name":"svc"}}"#,
                keptOut: ["vmess-secret"]
            ),
            Imported(
                name: "hysteria2",
                outbound: #"{"type":"hysteria2","server":"h.example","port":8443,"password":"hy2-secret","server_name":"h.example","alpn":["h3"],"insecure":false,"obfs":{"type":"salamander","password":"obfs-secret"}}"#,
                keptOut: ["hy2-secret", "obfs-secret"]
            ),
            Imported(
                name: "tuic",
                outbound: #"{"type":"tuic","server":"tu.example","port":8443,"uuid":"tuic-user","password":"tuic-secret","server_name":null,"alpn":[],"insecure":false,"congestion_control":"bbr"}"#,
                keptOut: ["tuic-user", "tuic-secret"]
            ),
            Imported(
                name: "snell",
                outbound: #"{"type":"snell","server":"sn.example","port":8000,"psk":"snell-secret","version":4,"mode":"default","obfs":"http","obfs_host":"obfs.example","obfs_uri":null,"reuse":false,"userkey":null}"#,
                keptOut: ["snell-secret"]
            ),
            Imported(
                name: "mieru",
                outbound: #"{"type":"mieru","server":"m.example","port":2999,"username":"mieru-user","password":"mieru-secret","port_range":"2999-3010","transport":"TCP"}"#,
                keptOut: ["mieru-user", "mieru-secret"]
            ),
            Imported(
                name: "iwan",
                outbound: #"{"type":"iwan","server":"203.0.113.10","port":4567,"username":"iwan-user","password":"iwan-secret","mtu":1400,"encrypt":true}"#,
                keptOut: ["iwan-user", "iwan-secret"]
            ),
            Imported(
                name: "anx",
                outbound: #"{"type":"anx","server":"a.example","port":443,"public_key":"anx-public","user_key":"anx-user-secret","server_name":"a.example","alpn":["h2"]}"#,
                keptOut: ["anx-user-secret"]
            ),
            Imported(
                name: "wireguard",
                outbound: wireGuardOutbound,
                keptOut: [wireGuardPrivateKey, wireGuardPresharedKey, "172.16.0.2"]
            ),
        ]
    }

    private struct MissingField: Error {}

    /// The importer's node for an outbound object.
    private func importedNode(_ outbound: String, id: String = "n1") throws -> NativeImportReport.Node {
        let fields = try JSONDecoder().decode([String: JSONValue].self, from: Data(outbound.utf8))
        guard case let .string(type)? = fields["type"], case let .string(server)? = fields["server"],
              case let .int(port)? = fields["port"]
        else {
            throw MissingField()
        }
        let node = #"{"id":"\#(id)","name":"","protocol":"\#(type)","server":"\#(server)","port":\#(port),"outbound":\#(outbound)}"#
        return try JSONDecoder().decode(NativeImportReport.Node.self, from: Data(node.utf8))
    }

    /// The `outbound` object of an encoded config, as the runtime reads it.
    private func encodedOutbound(_ config: EmbeddedRuntimeConfig) throws -> [String: JSONValue] {
        let envelope = try JSONDecoder().decode([String: JSONValue].self, from: config.encoded())
        guard case let .object(outbound)? = envelope["outbound"] else {
            throw MissingField()
        }
        return outbound
    }

    /// What the app saves and the extension reads back: the profile copy without secrets (`withoutSecret()`), then
    /// the secrets from the Keychain value.
    private func roundTrip(_ config: EmbeddedRuntimeConfig) throws -> EmbeddedRuntimeConfig {
        let secret = NodeSecret.fromKeychainValue(try config.secret.keychainValue())
        return try EmbeddedRuntimeConfig.decoded(from: config.withoutSecret().encoded()).withSecret(secret)
    }

    /// A profile copy as an earlier version saved it.
    private func savedProfile(_ outbound: String) throws -> EmbeddedRuntimeConfig {
        try EmbeddedRuntimeConfig.decoded(
            from: Data(#"{"schema_version":1,"profile_id":"default","mtu":1500,"outbound":\#(outbound)}"#.utf8)
        )
    }

    /// The profile of every protocol the importer writes keeps every field it wrote (a `via`, a transport,
    /// multiplexing, a WireGuard address), keeps the secrets out of the profile copy, and comes back whole from that
    /// copy and the Keychain value, as the extension reads it.
    func testEveryImportedProtocolRoundTripsThroughTheProfile() throws {
        for sample in importedSamples {
            let node = try importedNode(sample.outbound)
            let profile = try TunnelProfile(node: node, profileID: "default")
            XCTAssertEqual(profile.interface, .proxy, sample.name)
            XCTAssertEqual(try encodedOutbound(profile.config), node.outbound, sample.name)
            let native: Bool
            if case .native = profile.config.outbound {
                native = true
            } else {
                native = false
            }
            XCTAssertEqual(native, !sample.dedicated, sample.name)

            let stored = try profile.config.withoutSecret().encoded()
            let storedText = String(decoding: stored, as: UTF8.self)
            for text in sample.keptOut {
                XCTAssertFalse(storedText.contains(text), "\(sample.name): the profile copy holds a setting it must leave out")
            }
            if native {
                let template = try JSONDecoder().decode([String: JSONValue].self, from: stored)
                guard case let .object(outbound)? = template["outbound"] else {
                    return XCTFail("\(sample.name): the profile copy has no outbound")
                }
                XCTAssertEqual(Set(outbound.keys), ["type", "server", "port"], sample.name)
            }

            let restored = try roundTrip(profile.config)
            XCTAssertEqual(restored, profile.config, sample.name)
            XCTAssertEqual(try encodedOutbound(restored), node.outbound, sample.name)
        }
        // The direct profile has no settings and comes back the same.
        let direct = TunnelProfile.direct(profileID: "default", displayName: "直连").config
        XCTAssertEqual(try roundTrip(direct), direct)
    }

    /// Profiles saved by earlier versions still load. The dedicated cases keep their case with the secret from the
    /// Keychain (a bare Shadowsocks password from before secrets were JSON). The copy of a Shadowsocks node behind a
    /// line held only `type`, `server` and `port`, and the tunnel failed to start on it; its settings, `via`
    /// included, come back from the Keychain.
    func testProfilesSavedByEarlierVersionsStillLoad() throws {
        let shadowsocks = try savedProfile(
            #"{"type":"shadowsocks","server":"198.51.100.1","port":8388,"method":"aes-256-gcm","password":""}"#
        ).withSecret(NodeSecret.fromKeychainValue("bare-password"))
        guard case let .shadowsocks(_, _, method, password) = shadowsocks.outbound else {
            return XCTFail("expected the Shadowsocks case")
        }
        XCTAssertEqual(method, "aes-256-gcm")
        XCTAssertEqual(password, "bare-password")

        let vless = try savedProfile(
            #"{"type":"vless","server":"r.example","port":443,"uuid":"","flow":"xtls-rprx-vision","reality":\#(reality)}"#
        ).withSecret(NodeSecret(uuid: realityUUID))
        guard case let .vless(restoredVless) = vless.outbound else {
            return XCTFail("expected the VLESS REALITY case")
        }
        XCTAssertEqual(restoredVless.uuid, realityUUID)
        XCTAssertEqual(restoredVless.flow, "xtls-rprx-vision")

        let wireGuard = try savedProfile(
            #"{"type":"wireguard","server":"vpn.example.com","port":51820,"private_key":"","peer_public_key":"\#(wireGuardPeerKey)","persistent_keepalive_secs":25}"#
        ).withSecret(NodeSecret(privateKey: wireGuardPrivateKey))
        guard case let .wireguard(peer) = wireGuard.outbound else {
            return XCTFail("expected the WireGuard case")
        }
        XCTAssertEqual(peer.privateKey, wireGuardPrivateKey)
        XCTAssertEqual(peer.persistentKeepaliveSecs, 25)

        let settings = #"{"method":"2022-blake3-aes-128-gcm","password":"land-secret","via":\#(line)}"#
        let land = try savedProfile(#"{"type":"shadowsocks","server":"land.example","port":8388}"#)
            .withSecret(NodeSecret(outbound: settings))
        guard case let .native(native) = land.outbound else {
            return XCTFail("expected a native outbound")
        }
        XCTAssertEqual(native.type, "shadowsocks")
        XCTAssertEqual(native.settings["password"], .string("land-secret"))
        XCTAssertEqual(native.settings["via"], try JSONDecoder().decode(JSONValue.self, from: Data(line.utf8)))
    }

    /// Running alone, a WireGuard node from the importer is the only node of a routing section, so the engine runs
    /// it in userspace with its own address behind the proxy interface. A pasted WireGuard config stays the
    /// whole-device tunnel, and any other node is the top-level outbound.
    func testAWireGuardNodeFromTheImporterRunsAloneAsARoutedNode() throws {
        let node = try importedNode(wireGuardOutbound)
        let profile = try TunnelProfile(node: node, profileID: "default")
        let alone = profile.config.selectedNodeAlone()
        let routing = try XCTUnwrap(alone.routing)
        XCTAssertEqual(routing.outbounds, ["wireguard": node.outbound])
        XCTAssertEqual(routing.rules, ["FINAL,wireguard"])
        XCTAssertEqual(routing.groups, [])
        XCTAssertEqual(routing.global, "wireguard")
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: alone.encoded()) as? [String: Any])
        XCTAssertNil(json["outbound"])
        XCTAssertNotNil(json["routing"])
        // The same when the engine refuses the subscription's rules.
        let other = try importedNode(importedSamples[0].outbound, id: "n2")
        let subscription = EmbeddedRuntimeConfig.Routing(
            outbounds: ["n1": node.outbound, "n2": other.outbound],
            rules: ["FINAL,n1"],
            groups: [],
            global: "n1"
        )
        XCTAssertEqual(profile.config.withRouting(subscription).selectedNodeAlone(), alone)

        let pasted = try savedProfile(
            #"{"type":"wireguard","server":"vpn.example.com","port":51820,"private_key":"\#(wireGuardPrivateKey)","peer_public_key":"\#(wireGuardPeerKey)"}"#
        )
        XCTAssertEqual(pasted.selectedNodeAlone(), pasted)
        let trojan = try TunnelProfile(node: importedNode(importedSamples[7].outbound), profileID: "default").config
        XCTAssertEqual(trojan.withRouting(subscription).selectedNodeAlone(), trojan)
    }

    /// Versions before 1.0 saved a shorter copy of a subscription's VLESS REALITY or WireGuard node, without the
    /// xhttp transport or the WireGuard address. When the engine refuses the subscription's rules, the selected node
    /// runs whole from the routing section; another node there leaves the saved copy as it is.
    func testTheSelectedNodeRunsWholeWhenAnEarlierProfileKeptAShorterCopy() throws {
        let xhttp = try importedNode(realityXhttpOutbound)
        let savedVless = try savedProfile(
            #"{"type":"vless","server":"x.example","port":443,"uuid":"","flow":"","reality":\#(reality)}"#
        ).withSecret(NodeSecret(uuid: realityUUID))
        guard case .vless = savedVless.outbound else {
            return XCTFail("expected the VLESS REALITY case")
        }
        let routing = EmbeddedRuntimeConfig.Routing(outbounds: ["n1": xhttp.outbound], rules: ["FINAL,n1"], groups: [], global: "n1")
        let alone = savedVless.withRouting(routing).selectedNodeAlone()
        XCTAssertNil(alone.routing)
        XCTAssertEqual(try encodedOutbound(alone), xhttp.outbound)

        var anotherUser = xhttp.outbound
        anotherUser["uuid"] = .string("another-user")
        let another = EmbeddedRuntimeConfig.Routing(outbounds: ["n1": anotherUser], rules: ["FINAL,n1"], groups: [], global: "n1")
        XCTAssertEqual(savedVless.withRouting(another).selectedNodeAlone(), savedVless)
        XCTAssertEqual(savedVless.selectedNodeAlone(), savedVless)

        let wireGuard = try importedNode(wireGuardOutbound)
        let savedWireGuard = try savedProfile(
            #"{"type":"wireguard","server":"wg.example","port":51820,"private_key":"","peer_public_key":"\#(wireGuardPeerKey)"}"#
        ).withSecret(NodeSecret(privateKey: wireGuardPrivateKey, presharedKey: wireGuardPresharedKey))
        let wireGuardRouting = EmbeddedRuntimeConfig.Routing(
            outbounds: ["n2": wireGuard.outbound],
            rules: ["FINAL,n2"],
            groups: [],
            global: "n2"
        )
        let wireGuardAlone = savedWireGuard.withRouting(wireGuardRouting).selectedNodeAlone()
        XCTAssertEqual(wireGuardAlone.routing?.outbounds, ["wireguard": wireGuard.outbound])
        XCTAssertEqual(wireGuardAlone.routing?.global, "wireguard")
    }

    func testFailuresKeepTheMostUsefulError() {
        let unsupportedOnly = """
        {"nodes":[],"unsupported":[{"name":"x","protocol":"mieru","reason":"r"}]}
        """
        XCTAssertThrowsError(
            try TunnelNodeImport.parse("mieru://x", profileID: "p", native: importer(unsupportedOnly))
        ) { error in
            XCTAssertEqual(
                error as? TunnelNodeImportError,
                .native(.noSupportedNode(protocols: ["mieru"], reasons: ["r"]))
            )
        }

        let unrecognized: NativeNodeImporter = { _ in throw NativeImportFailure.unrecognized }
        XCTAssertThrowsError(try TunnelNodeImport.parse("hello", profileID: "p", native: unrecognized)) { error in
            XCTAssertEqual(error as? TunnelNodeImportError, .native(.unrecognized))
        }
        // A link Swift recognizes keeps its own error when the importer cannot help.
        XCTAssertThrowsError(
            try TunnelNodeImport.parse("ss://aes-256-gcm:secret@example.com:8388", profileID: "p", native: unrecognized)
        ) { error in
            guard case .shadowsocks = error as? TunnelNodeImportError else {
                return XCTFail("expected the Shadowsocks error, got \(error)")
            }
        }
        // Without the importer, unknown text stays unrecognized.
        XCTAssertThrowsError(try TunnelNodeImport.parse("hello", profileID: "p")) { error in
            XCTAssertEqual(error as? TunnelNodeImportError, .unrecognized)
        }
    }

    func testJSONValuesRoundTrip() throws {
        let text = #"{"a":null,"b":true,"c":-3,"d":1.5,"e":"s","f":[1,"x"],"g":{"h":false}}"#
        let value = try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8))
        XCTAssertEqual(
            value,
            .object([
                "a": .null, "b": .bool(true), "c": .int(-3), "d": .double(1.5), "e": .string("s"),
                "f": .array([.int(1), .string("x")]), "g": .object(["h": .bool(false)]),
            ])
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        XCTAssertEqual(try JSONDecoder().decode(JSONValue.self, from: encoder.encode(value)), value)
    }
}
