import Foundation
import NetworkCoreBridge
import XCTest

final class RuntimeControlTests: XCTestCase {
    func testRequestsAreTheEngineCommands() throws {
        XCTAssertEqual(String(decoding: RuntimeControl.traffic.request, as: UTF8.self), #"{"cmd":"traffic"}"#)
        XCTAssertEqual(String(decoding: RuntimeControl.closeAll.request, as: UTF8.self), #"{"cmd":"close_all"}"#)
        XCTAssertEqual(
            String(decoding: RuntimeControl.closeConnection(7).request, as: UTF8.self),
            #"{"cmd":"close_connection","id":7}"#
        )
    }

    func testLanShareCommandsAndAnswer() throws {
        XCTAssertEqual(String(decoding: RuntimeControl.lanShare.request, as: UTF8.self), #"{"cmd":"lan_share"}"#)
        XCTAssertEqual(
            String(decoding: RuntimeControl.startLanShare(port: 8899).request, as: UTF8.self),
            #"{"cmd":"start_lan_share","port":8899}"#
        )
        XCTAssertEqual(String(decoding: RuntimeControl.stopLanShare.request, as: UTF8.self), #"{"cmd":"stop_lan_share"}"#)
        let share = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.LanShare.self, from: Data("""
        {"running":true,"port":8899,"addresses":["192.168.1.5","10.0.0.2"],"url":"http://192.168.1.5:8899/",
         "page_host":"cert.anixops","https_decryption":true,"certificate":true,"credentials":false,
         "accepted":3,"rejected":0,"active":1,"pages":2}
        """.utf8)))
        XCTAssertEqual(share.addresses, ["192.168.1.5", "10.0.0.2"])
        XCTAssertEqual(share.setupURL(address: "10.0.0.2"), "http://10.0.0.2:8899/")
        XCTAssertTrue(share.httpsDecryption)
        let stopped = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.LanShare.self, from: Data("""
        {"running":false,"port":null,"addresses":[],"url":null,"page_host":"cert.anixops","https_decryption":false,
         "certificate":false,"credentials":false,"accepted":0,"rejected":0,"active":0,"pages":0}
        """.utf8)))
        XCTAssertNil(stopped.setupURL(address: "192.168.1.5"))
    }

    func testSystemProxyCommandsAndAnswer() throws {
        XCTAssertEqual(String(decoding: RuntimeControl.systemProxy.request, as: UTF8.self), #"{"cmd":"system_proxy"}"#)
        XCTAssertEqual(
            String(decoding: RuntimeControl.startSystemProxy(port: RuntimeControl.systemProxyPort).request, as: UTF8.self),
            #"{"cmd":"start_system_proxy","port":8890}"#
        )
        XCTAssertEqual(
            String(decoding: RuntimeControl.stopSystemProxy.request, as: UTF8.self), #"{"cmd":"stop_system_proxy"}"#
        )
        let running = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.SystemProxy.self, from: Data("""
        {"running":true,"host":"127.0.0.1","port":50123,"https_decryption":true,"accepted":4,"rejected":0,"active":1}
        """.utf8)))
        XCTAssertTrue(running.running)
        XCTAssertEqual(running.port, 50123)
        XCTAssertTrue(running.httpsDecryption)
        let stopped = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.SystemProxy.self, from: Data("""
        {"running":false,"host":null,"port":null,"https_decryption":false,"accepted":0,"rejected":0,"active":0}
        """.utf8)))
        XCTAssertNil(stopped.port)
    }

    func testScriptCommandsAndAnswers() throws {
        XCTAssertEqual(String(decoding: RuntimeControl.networkChanged.request, as: UTF8.self), #"{"cmd":"network_changed"}"#)
        XCTAssertEqual(
            String(decoding: RuntimeControl.scriptNotifications.request, as: UTF8.self),
            #"{"cmd":"script_notifications"}"#
        )
        XCTAssertEqual(
            String(decoding: RuntimeControl.runScript(tag: "checkin").request, as: UTF8.self),
            #"{"cmd":"run_script","tag":"checkin"}"#
        )
        let scripts = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.ScheduledScripts.self, from: Data("""
        {"scripts":[{"tag":"checkin","trigger":"cron","cron":"0 9 * * *",
          "next_run":"2026-11-09T09:00:00+08:00[Asia/Shanghai]","last_status":"done"},
         {"tag":"net","trigger":"network-changed","cron":null,"next_run":null,"last_status":null}]}
        """.utf8)))
        XCTAssertEqual(scripts.scripts.map(\.tag), ["checkin", "net"])
        XCTAssertEqual(scripts.scripts[0].lastStatus, "done")
        let notifications = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.ScriptNotifications.self, from: Data("""
        {"notifications":[{"title":"Check-in","subtitle":"ok","content":"+5","open_url":null}]}
        """.utf8)))
        XCTAssertEqual(notifications.notifications.first?.title, "Check-in")
    }

    func testControlMessagesRoundTripThroughTheExtension() {
        let request = RuntimeControl.connections.request
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.control(request).encoded), .control(request))
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.diagnostics.encoded), .diagnostics)
    }

    func testDecodesTheConnectionList() throws {
        let json = """
        {"traffic":{"upload_total":10,"download_total":20,"upload_rate":1,"download_rate":2,"active":1},
         "active":[{"id":1,"network":"tcp","host":"www.google.com","destination":"142.250.1.1:443",
           "rule":"DOMAIN-SUFFIX,google.com","chain":["Proxy","hk-1"],"outbound":"hk-1",
           "started_at_ms":1700000000000,"ended_at_ms":null,"upload":5,"download":6,"error":null}],
         "closed":[{"id":2,"network":"udp","host":null,"destination":"example.com:53",
           "rule":"FINAL","chain":[],"outbound":"DIRECT",
           "started_at_ms":1700000000000,"ended_at_ms":1700000001500,"upload":1,"download":2,"error":"connect_failed"}]}
        """
        let list = try XCTUnwrap(RuntimeControl.decodeConnections(Data(json.utf8)))
        XCTAssertEqual(list.traffic.downloadRate, 2)
        let active = try XCTUnwrap(list.active.first)
        XCTAssertEqual(active.target, "www.google.com:443")
        XCTAssertTrue(active.isActive)
        XCTAssertEqual(active.startedAt, Date(timeIntervalSince1970: 1_700_000_000))
        XCTAssertTrue(active.matches("google HK-1"))
        XCTAssertFalse(active.matches("direct"))
        let closed = try XCTUnwrap(list.closed.first)
        XCTAssertEqual(closed.target, "example.com:53")
        XCTAssertFalse(closed.isActive)
        XCTAssertEqual(closed.endedAt, Date(timeIntervalSince1970: 1_700_000_001.5))
        XCTAssertEqual(closed.error, "connect_failed")
    }

    func testDecodesTraffic() {
        let traffic = RuntimeControl.decodeTraffic(Data(#"{"upload_total":1,"download_total":2,"upload_rate":3,"download_rate":4,"active":5}"#.utf8))
        XCTAssertEqual(traffic, RuntimeControl.Traffic(uploadTotal: 1, downloadTotal: 2, uploadRate: 3, downloadRate: 4, active: 5))
        XCTAssertNil(RuntimeControl.decodeTraffic(Data("not json".utf8)))
    }
}

final class ProxyModeTests: XCTestCase {
    func testModeRequestsAndAnswers() {
        XCTAssertEqual(
            String(decoding: RuntimeControl.setMode(.direct).request, as: UTF8.self),
            #"{"cmd":"set_mode","mode":"direct"}"#
        )
        XCTAssertEqual(
            String(decoding: RuntimeControl.select(group: "Proxy", member: "hk-1").request, as: UTF8.self),
            #"{"cmd":"select","group":"Proxy","member":"hk-1"}"#
        )
        XCTAssertEqual(RuntimeControl.decode(RuntimeControl.ModeAnswer.self, from: Data(#"{"mode":"global"}"#.utf8))?.mode, .global)
        XCTAssertNil(RuntimeControl.decode(RuntimeControl.ModeAnswer.self, from: nil))
        let groups = RuntimeControl.decode(RuntimeControl.Groups.self, from: Data(
            #"{"groups":[{"id":"Proxy","type":"select","now":"hk-1","members":[{"name":"hk-1","latency_ms":120},{"name":"DIRECT","latency_ms":null}]}]}"#.utf8
        ))
        XCTAssertEqual(groups?.groups.first?.now, "hk-1")
        XCTAssertEqual(groups?.groups.first?.members.map(\.latencyMs), [120, nil])
    }

    func testBlockQuicRequestAndConfig() throws {
        XCTAssertEqual(
            String(decoding: RuntimeControl.setBlockQuic(true).request, as: UTF8.self),
            #"{"cmd":"set_block_quic","enabled":true}"#
        )
        XCTAssertEqual(
            RuntimeControl.decode(RuntimeControl.BlockQuicAnswer.self, from: Data(#"{"block_quic":true}"#.utf8))?.blockQuic,
            true
        )
        // An engine without the command answers nothing the app can read, so the app restarts instead.
        XCTAssertNil(RuntimeControl.decode(RuntimeControl.BlockQuicAnswer.self, from: nil))

        let node = ShadowsocksNode(host: "203.0.113.1", port: 8388, method: "2022-blake3-aes-128-gcm", password: "key", name: nil)
        let config = EmbeddedRuntimeConfig(node: node, profileID: "test")
        let off = try XCTUnwrap(JSONSerialization.jsonObject(with: config.withBlockQuic(false).encoded()) as? [String: Any])
        XCTAssertNil(off["block_quic"])
        let blocked = config.withBlockQuic(true)
        let on = try XCTUnwrap(JSONSerialization.jsonObject(with: blocked.encoded()) as? [String: Any])
        XCTAssertEqual(on["block_quic"] as? Bool, true)
        XCTAssertNil(blocked.withoutSecret().blockQuic)
    }

    func testSavedModeFallsBackToTheRulesFlag() {
        XCTAssertEqual(ProxyMode.saved(in: nil), .global)
        XCTAssertEqual(ProxyMode.saved(in: [TunnelProfileKeys.routing: true]), .rule)
        XCTAssertEqual(ProxyMode.saved(in: [TunnelProfileKeys.routing: true, TunnelProfileKeys.mode: "direct"]), .direct)
        XCTAssertEqual(ProxyMode.saved(in: [TunnelProfileKeys.mode: "bogus"]), .global)
    }

    func testNewProfileKeepsTheModeSavedElsewhere() {
        // Shortcuts or Control Center picked direct: another node or a refreshed subscription stays direct.
        XCTAssertEqual(ProxyMode.forNewProfile(current: .direct, routed: true, direct: false, localRuleList: false), .direct)
        // The node-less profile always runs direct.
        XCTAssertEqual(ProxyMode.forNewProfile(current: .rule, routed: false, direct: true, localRuleList: true), .direct)
        // With subscription rules the rules switch decides; it follows the saved mode (`followingSavedMode`).
        XCTAssertEqual(ProxyMode.forNewProfile(current: .global, routed: true, direct: false, localRuleList: false), .rule)
        XCTAssertEqual(ProxyMode.forNewProfile(current: .rule, routed: false, direct: false, localRuleList: false), .global)
        // A node list without subscription rules keeps the rule mode of its local rules, and only that.
        XCTAssertEqual(ProxyMode.forNewProfile(current: .rule, routed: false, direct: false, localRuleList: true), .rule)
        XCTAssertEqual(ProxyMode.forNewProfile(current: .global, routed: false, direct: false, localRuleList: true), .global)
    }

    func testConfigCarriesTheModeAndTheGlobalTarget() throws {
        let profile = try TunnelProfile(
            node: try JSONDecoder().decode(NativeImportReport.Node.self, from: Data(
                #"{"id":"a","name":"A","protocol":"trojan","server":"a.example","port":443,"outbound":{"type":"trojan","server":"a.example","port":443,"password":"p"}}"#.utf8
            )),
            profileID: "default"
        )
        let direct = try XCTUnwrap(JSONSerialization.jsonObject(with: profile.config.withMode(.direct).encoded()) as? [String: Any])
        XCTAssertEqual(direct["mode"] as? String, "direct")
        XCTAssertNotNil(direct["outbound"])
        XCTAssertNil(try XCTUnwrap(JSONSerialization.jsonObject(with: profile.config.encoded()) as? [String: Any])["mode"])
        let stored = String(decoding: try profile.config.withMode(.global).withoutSecret().encoded(), as: UTF8.self)
        XCTAssertFalse(stored.contains("mode"))

        let routing = EmbeddedRuntimeConfig.Routing(outbounds: ["a": [:]], rules: ["FINAL,a"], groups: [], global: "a")
        let routed = try XCTUnwrap(JSONSerialization.jsonObject(with: profile.config.withRouting(routing).encoded()) as? [String: Any])
        XCTAssertEqual((routed["routing"] as? [String: Any])?["global"] as? String, "a")
    }
}
