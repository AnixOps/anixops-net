import Foundation
import NetworkCoreBridge
import XCTest

final class NodeLibraryTests: XCTestCase {
    private let report = """
    {"nodes":[
    {"id":"a","name":"Tokyo","protocol":"trojan","server":"a.example","port":443,
     "outbound":{"type":"trojan","server":"a.example","port":443,"password":"pa"}},
    {"id":"b","name":"","protocol":"hysteria2","server":"b.example","port":8443,
     "outbound":{"type":"hysteria2","server":"b.example","port":8443,"password":"pb"}}],
    "unsupported":[]}
    """

    private func decoded() throws -> NativeImportReport {
        try NativeImportReport.decoded(from: Data(report.utf8))
    }

    func testSubscriptionTextKeepsEveryNodeAndSelectsTheFirst() throws {
        let imported = try TunnelNodeImport.parseImport("trojan://x", profileID: "p") { _ in Data(self.report.utf8) }
        XCTAssertEqual(imported.profile.displayName, "Tokyo")
        let report = try XCTUnwrap(imported.report)
        let library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        XCTAssertEqual(library.nodes.map(\.id), ["a", "b"])
        XCTAssertEqual(library.selectedID, "a")
        XCTAssertNil(try TunnelNodeImport.parseImport(
            "ss://2022-blake3-aes-128-gcm:AAECAwQFBgcICQoLDA0ODw%3D%3D@example.com:8388#Tokyo%201",
            profileID: "p"
        ).report)
    }

    func testRulesTheImportLeftOutAreKept() throws {
        XCTAssertEqual(NodeLibrary(report: try decoded(), subscriptionURL: nil, keeping: nil).routingWarnings, [])
        let withWarnings = report.replacingOccurrences(
            of: #""unsupported":[]}"#,
            with: #""unsupported":[],"routing_warnings":["unsupported rule types were skipped: IP-ASN x2"]}"#
        )
        let warned = try NativeImportReport.decoded(from: Data(withWarnings.utf8))
        XCTAssertEqual(warned.routingWarnings, ["unsupported rule types were skipped: IP-ASN x2"])
        let library = NodeLibrary(report: warned, subscriptionURL: nil, keeping: nil)
        XCTAssertEqual(library.routingWarnings, ["unsupported rule types were skipped: IP-ASN x2"])
        let saved = try JSONDecoder().decode(NodeLibrary.self, from: JSONEncoder().encode(library))
        XCTAssertEqual(saved.routingWarnings, library.routingWarnings)
        // A library saved before the field existed has none.
        var old = try JSONSerialization.jsonObject(with: JSONEncoder().encode(library)) as? [String: Any] ?? [:]
        old.removeValue(forKey: "routingWarnings")
        let decodedOld = try JSONDecoder().decode(NodeLibrary.self, from: JSONSerialization.data(withJSONObject: old))
        XCTAssertEqual(decodedOld.routingWarnings, [])
    }

    func testRefreshKeepsTheSelectedNodeWhenItIsStillListed() throws {
        let report = try decoded()
        XCTAssertEqual(NodeLibrary(report: report, subscriptionURL: "https://s.example/x", keeping: "b").selectedID, "b")
        XCTAssertEqual(NodeLibrary(report: report, subscriptionURL: nil, keeping: "gone").selectedID, "a")
    }

    private let trunkReport = """
    {"nodes":[
    {"id":"direct","name":"JP direct","protocol":"trojan","server":"jp.example","port":443,
     "outbound":{"type":"trojan","server":"jp.example","port":443,"password":"pa"}},
    {"id":"trunk","name":"SH CN2","protocol":"trojan","server":"sh.example","port":443,"path":"trunk",
     "outbound":{"type":"trojan","server":"sh.example","port":443,"password":"pb"}}],
    "unsupported":[]}
    """

    /// The first trunk node is the default pick (anx F-51); a node the user picked survives a refresh, and a
    /// removed selection falls back to the trunk node too.
    func testTrunkNodeIsTheDefaultPick() throws {
        let report = try NativeImportReport.decoded(from: Data(trunkReport.utf8))
        XCTAssertEqual(report.nodes.map(\.isTrunk), [false, true])
        XCTAssertEqual(NodeLibrary(report: report, subscriptionURL: nil, keeping: nil).selectedID, "trunk")
        XCTAssertEqual(NodeLibrary(report: report, subscriptionURL: nil, keeping: "gone").selectedID, "trunk")
        XCTAssertEqual(NodeLibrary(report: report, subscriptionURL: nil, keeping: "direct").selectedID, "direct")
        let picked = NodeLibrary(report: report, subscriptionURL: nil, keeping: "trunk")
        XCTAssertEqual(picked.removing("trunk").selectedID, "direct")
        let imported = try TunnelNodeImport.parseImport("trojan://x", profileID: "p") { _ in Data(self.trunkReport.utf8) }
        XCTAssertEqual(imported.profile.serverAddress, "sh.example")
        // Without a mark the first node stays the default.
        XCTAssertEqual(NodeLibrary(report: try decoded(), subscriptionURL: nil, keeping: nil).selectedID, "a")
        // The mark survives the keychain round trip.
        let saved = try JSONEncoder().encode(picked)
        XCTAssertEqual(try JSONDecoder().decode(NodeLibrary.self, from: saved).node(id: "trunk")?.isTrunk, true)
    }

    func testSwitchingBuildsTheProfileForThatNode() throws {
        let library = NodeLibrary(report: try decoded(), subscriptionURL: nil, keeping: nil)
        let node = try XCTUnwrap(library.node(id: "b"))
        let profile = try TunnelProfile(node: node, profileID: "default")
        XCTAssertEqual(profile.serverAddress, "b.example")
        // A node without a name is shown by its server.
        XCTAssertEqual(profile.displayName, "b.example")
        XCTAssertEqual(profile.config.outboundKind, "hysteria2")
        XCTAssertTrue(NodeLibrary.usesUDP(node.protocol))
        XCTAssertFalse(NodeLibrary.usesUDP("trojan"))
    }

    private let lineReport = """
    {"nodes":[
    {"id":"line","name":"iWAN HK","protocol":"iwan","server":"203.0.113.10","port":4567,
     "outbound":{"type":"iwan","server":"203.0.113.10","port":4567,"username":"u","password":"p"}},
    {"id":"land","name":"US","protocol":"shadowsocks","server":"land.example","port":8388,
     "outbound":{"type":"shadowsocks","server":"land.example","port":8388,
     "method":"2022-blake3-aes-128-gcm","password":"AAECAwQFBgcICQoLDA0ODw==","via":"line"}},
    {"id":"loop","name":"","protocol":"trojan","server":"l.example","port":443,
     "outbound":{"type":"trojan","server":"l.example","port":443,"password":"pl","via":"loop"}}],
    "unsupported":[]}
    """

    func testANodeBehindALineRunsOnItsOwnWithTheLineInline() throws {
        let library = NodeLibrary(
            report: try NativeImportReport.decoded(from: Data(lineReport.utf8)),
            subscriptionURL: nil,
            keeping: "land"
        )
        let land = try XCTUnwrap(library.selected)
        XCTAssertEqual(land.via, "line")
        let line = try XCTUnwrap(library.node(id: "line"))
        let standalone = land.standalone(among: library.nodes)
        XCTAssertEqual(standalone.outbound["via"], .object(line.outbound))
        XCTAssertEqual(line.standalone(among: library.nodes), line)

        // Shadowsocks with a via stays a native outbound, so the via reaches the engine.
        let profile = try TunnelProfile(node: standalone, profileID: "default")
        guard case let .native(native) = profile.config.outbound else {
            return XCTFail("expected a native outbound")
        }
        XCTAssertEqual(native.type, "shadowsocks")
        XCTAssertEqual(native.settings["via"], .object(line.outbound))
        // The profile copy keeps neither the via nor a secret, and the extension gets the node back whole from it
        // and the Keychain value. Before 1.0 that copy, its type and address alone, did not load and the tunnel
        // failed to start.
        let stored = try profile.config.withoutSecret().encoded()
        let storedText = String(decoding: stored, as: UTF8.self)
        for setting in ["AAECAwQFBgcICQoLDA0ODw==", "2022-blake3", "via", "iwan"] {
            XCTAssertFalse(storedText.contains(setting), setting)
        }
        let secret = NodeSecret.fromKeychainValue(try profile.config.secret.keychainValue())
        XCTAssertEqual(try EmbeddedRuntimeConfig.decoded(from: stored).withSecret(secret), profile.config)

        // A loop is left for the engine to refuse rather than dropped.
        let loop = try XCTUnwrap(library.node(id: "loop"))
        XCTAssertEqual(loop.standalone(among: library.nodes).outbound["via"], .string("loop"))
    }

    func testKeychainValueRoundTrips() throws {
        let library = NodeLibrary(
            report: try decoded(),
            subscriptionURL: "https://s.example/x?token=t",
            keeping: "b",
            at: Date(timeIntervalSince1970: 1_700_000_000)
        )
        XCTAssertEqual(try NodeLibrary.fromKeychainValue(library.keychainValue()), library)
    }

    func testSubscriptionLinksAreSingleHttpUrls() {
        XCTAssertEqual(SubscriptionLink.url(from: "  https://sub.example/api?token=x \n")?.host, "sub.example")
        XCTAssertNotNil(SubscriptionLink.url(from: "http://sub.example/a"))
        // Recognized, so the import can refuse it with the reason (ATS) rather than as an unknown node link.
        XCTAssertTrue(SubscriptionLink.isPlainHTTP("http://sub.example/a"))
        XCTAssertTrue(SubscriptionLink.isPlainHTTP(" HTTP://sub.example/a?token=t \n"))
        XCTAssertFalse(SubscriptionLink.isPlainHTTP("https://sub.example/a"))
        XCTAssertFalse(SubscriptionLink.isPlainHTTP("ss://abc@host:1#name"))
        XCTAssertNil(SubscriptionLink.url(from: "trojan://p@a.example:443"))
        XCTAssertNil(SubscriptionLink.url(from: "https://a.example/x\nhttps://b.example/y"))
        XCTAssertNil(SubscriptionLink.url(from: "https://"))
        XCTAssertEqual(SubscriptionLink.displayHost("https://sub.example/api?token=x"), "sub.example")
    }

    private let routedReport = """
    {"nodes":[
    {"id":"a","name":"Tokyo","protocol":"trojan","server":"a.example","port":443,
     "outbound":{"type":"trojan","server":"a.example","port":443,"password":"pa"}},
    {"id":"b","name":"","protocol":"hysteria2","server":"b.example","port":8443,
     "outbound":{"type":"hysteria2","server":"b.example","port":8443,"password":"pb"}}],
    "unsupported":[],
    "routing":{"rules":["DOMAIN-SUFFIX,google.com,Proxy","FINAL,Proxy"],
    "groups":[{"id":"Proxy","type":"select","members":["Auto","a","DIRECT"],"default":null},
    {"id":"Auto","type":"url-test","members":["a","b"],"url":"http://www.gstatic.com/generate_204","interval_secs":300}]}}
    """

    func testSubscriptionRulesBecomeTheRuntimeRoutingSection() throws {
        let report = try NativeImportReport.decoded(from: Data(routedReport.utf8))
        var library = NodeLibrary(report: report, subscriptionURL: "https://s.example/x", keeping: nil)
        XCTAssertTrue(library.routesByRules)
        let proxy = try XCTUnwrap(library.routing?.groups.first)
        XCTAssertTrue(proxy.isSelectable)
        XCTAssertEqual(library.selection(in: proxy), "Auto")
        XCTAssertEqual(library.displayName(ofMember: "b"), "b.example")
        XCTAssertEqual(library.displayName(ofMember: "Auto"), "Auto")

        library.groupSelections["Proxy"] = "a"
        let routing = try XCTUnwrap(library.runtimeRouting())
        XCTAssertEqual(routing.groups.map { $0.default }, ["a", nil])
        XCTAssertEqual(routing.outbounds["b"]?["password"], .string("pb"))

        let profile = try TunnelProfile(node: try XCTUnwrap(library.selected), profileID: "default")
        let routed = profile.config.withRouting(routing)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: routed.encoded()) as? [String: Any])
        XCTAssertNil(json["outbound"])
        let section = try XCTUnwrap(json["routing"] as? [String: Any])
        XCTAssertEqual(section["rules"] as? [String], ["DOMAIN-SUFFIX,google.com,Proxy", "FINAL,Proxy"])
        let groups = try XCTUnwrap(section["groups"] as? [[String: Any]])
        XCTAssertEqual(groups.first?["default"] as? String, "a")
        XCTAssertEqual(groups.last?["interval_secs"] as? Int, 300)
        // The profile copy never carries the other nodes' credentials.
        let stored = String(decoding: try routed.withoutSecret().encoded(), as: UTF8.self)
        XCTAssertFalse(stored.contains("routing"))
        XCTAssertFalse(stored.contains("pb"))

        library.followsRules = false
        XCTAssertNil(library.runtimeRouting())
        XCTAssertFalse(library.routesByRules)
    }

    func testRefreshKeepsTheRulesSwitchAndValidGroupPicks() throws {
        let report = try NativeImportReport.decoded(from: Data(routedReport.utf8))
        var library = NodeLibrary(report: report, subscriptionURL: "https://s.example/x", keeping: nil)
        library.followsRules = false
        library.groupSelections = ["Proxy": "DIRECT", "Gone": "a"]
        library.selectedID = "b"
        let refreshed = library.refreshed(with: report)
        XCTAssertFalse(refreshed.followsRules)
        XCTAssertEqual(refreshed.groupSelections, ["Proxy": "DIRECT"])
        XCTAssertEqual(refreshed.selectedID, "b")
        XCTAssertEqual(refreshed.subscriptionURL, "https://s.example/x")
        // A subscription that gains rules follows them.
        let plain = NodeLibrary(report: try decoded(), subscriptionURL: "https://s.example/x", keeping: nil)
        XCTAssertFalse(plain.routesByRules)
        XCTAssertTrue(plain.refreshed(with: report).routesByRules)
    }

    func testLibrariesSavedBeforeRulesStillLoad() throws {
        let saved = """
        {"nodes":[],"selectedID":null,"subscriptionURL":"https://s.example/x","updatedAt":1700000000000}
        """
        let library = try NodeLibrary.fromKeychainValue(saved)
        XCTAssertNil(library.routing)
        XCTAssertFalse(library.followsRules)
        XCTAssertEqual(library.groupSelections, [:])
        // Imported before certificate pins reached the outbounds.
        XCTAssertEqual(library.importRevision, 0)
        XCTAssertTrue(library.predatesCertificatePins)
    }

    /// A subscription's `subnet` group (no members, a default and network rules) and a health-checked group that
    /// waits for its first measurement, as the importer's `routing_json` writes them.
    private let networkGroupsReport = """
    {"nodes":[
    {"id":"a","name":"Tokyo","protocol":"trojan","server":"a.example","port":443,
     "outbound":{"type":"trojan","server":"a.example","port":443,"password":"pa"}},
    {"id":"b","name":"","protocol":"hysteria2","server":"b.example","port":8443,
     "outbound":{"type":"hysteria2","server":"b.example","port":8443,"password":"pb"}}],
    "unsupported":[],
    "routing":{"rules":["FINAL,Home"],
    "groups":[{"id":"Home","type":"subnet","members":[],"default":"Auto","url":null,"interval_secs":null,
    "tolerance_ms":null,"network_rules":[{"when":"SUBNET:192.168.1.0/24","target":"DIRECT"},
    {"when":"TYPE:CELLULAR","target":"b"}]},
    {"id":"Auto","type":"url-test","members":["a"],"default":null,"url":"http://www.gstatic.com/generate_204",
    "interval_secs":300,"tolerance_ms":null,"evaluate_before_use":true}]}}
    """

    /// Groups keep the importer's `network_rules` and `evaluate_before_use` through the Keychain copy into the
    /// engine's routing section, unchanged and in order; a subnet group keeps no members.
    func testGroupsKeepNetworkRulesAndEvaluateBeforeUse() throws {
        let report = try NativeImportReport.decoded(from: Data(networkGroupsReport.utf8))
        let home = try XCTUnwrap(report.routing?.groups.first)
        XCTAssertEqual(home.networkRules, [
            .init(when: "SUBNET:192.168.1.0/24", target: "DIRECT"),
            .init(when: "TYPE:CELLULAR", target: "b"),
        ])
        XCTAssertTrue(home.picksByNetwork)
        XCTAssertEqual(home.typeTitle, String(localized: "按网络选择"))
        XCTAssertEqual(home.displayMembers, ["Auto", "DIRECT", "b"])
        XCTAssertEqual(report.routing?.groups.last?.evaluateBeforeUse, true)

        let library = NodeLibrary(
            report: report,
            subscriptionURL: "https://s.example/x",
            keeping: nil,
            at: Date(timeIntervalSince1970: 1_700_000_000)
        )
        XCTAssertEqual(library.selection(in: home), "Auto")
        XCTAssertFalse(library.predatesNetworkRules)
        // Named only by the subnet group's rules: deleting it would leave a rule pointing at nothing.
        XCTAssertFalse(library.canRemove("b"))

        let stored = try NodeLibrary.fromKeychainValue(library.keychainValue())
        XCTAssertEqual(stored, library)
        let routing = try XCTUnwrap(stored.runtimeRouting())
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(routing)) as? [String: Any])
        let groups = try XCTUnwrap(json["groups"] as? [[String: Any]])
        XCTAssertEqual(groups.first?["members"] as? [String], [])
        XCTAssertEqual(groups.first?["default"] as? String, "Auto")
        let rules = try XCTUnwrap(groups.first?["network_rules"] as? [[String: String]])
        XCTAssertEqual(rules, [
            ["when": "SUBNET:192.168.1.0/24", "target": "DIRECT"],
            ["when": "TYPE:CELLULAR", "target": "b"],
        ])
        XCTAssertNil(groups.first?["evaluate_before_use"])
        XCTAssertEqual(groups.last?["evaluate_before_use"] as? Bool, true)
        XCTAssertNil(groups.last?["network_rules"])
    }

    /// A library saved before groups kept their network rules still loads; its subnet groups are flagged so the
    /// subscription is imported again, and a library without such a group is not.
    func testLibrariesSavedBeforeNetworkRulesLoadAndAreFlagged() throws {
        let report = try NativeImportReport.decoded(from: Data(networkGroupsReport.utf8))
        let library = NodeLibrary(
            report: report,
            subscriptionURL: "https://s.example/x",
            keeping: nil,
            at: Date(timeIntervalSince1970: 1_700_000_000)
        )
        var saved = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(library.keychainValue().utf8)) as? [String: Any]
        )
        var routing = try XCTUnwrap(saved["routing"] as? [String: Any])
        routing["groups"] = (routing["groups"] as? [[String: Any]] ?? []).map { group in
            group.filter { $0.key != "network_rules" && $0.key != "evaluate_before_use" }
        }
        saved["routing"] = routing
        saved["importRevision"] = 1
        let old = try NodeLibrary.fromKeychainValue(
            String(decoding: JSONSerialization.data(withJSONObject: saved), as: UTF8.self)
        )
        XCTAssertNil(old.routing?.groups.first?.networkRules)
        XCTAssertTrue(old.predatesNetworkRules)
        XCTAssertFalse(old.predatesCertificatePins)
        let group = SubscriptionGroup(name: "Old", library: old)
        XCTAssertTrue(group.needsNetworkRulesReimport)
        XCTAssertFalse(group.needsReimport)
        XCTAssertEqual(group.nextUpdate, old.updatedAt)

        var plain = NodeLibrary(report: try decoded(), subscriptionURL: "https://s.example/x", keeping: nil)
        plain.importRevision = 1
        XCTAssertFalse(plain.predatesNetworkRules)
    }
}
