import Foundation
import NetworkCoreBridge
import XCTest

final class RuleSetCatalogTests: XCTestCase {
    func testFindsTheSetsRulesName() {
        let rules = [
            "GEOIP,CN,DIRECT,no-resolve",
            "GEOSITE,cn,DIRECT",
            "RULE-SET,ads,REJECT",
            "AND,((GEOSITE,GFW),(DST-PORT,443)),Proxy",
            "DOMAIN-SUFFIX,google.com,Proxy",
            "FINAL,Proxy",
        ]
        XCTAssertEqual(RuleSetCatalog.referencedNames(in: rules), ["geoip-cn", "geosite-cn", "ads", "geosite-gfw"])
    }

    func testFillsKnownCategoriesAndKeepsTheSubscriptionsOwn() throws {
        let own = EmbeddedRuntimeConfig.RuleSet(url: "https://lists.example/cn.txt", format: "domain")
        let sets = try XCTUnwrap(RuleSetCatalog.ruleSets(
            for: ["GEOIP,CN,DIRECT", "GEOSITE,cn,DIRECT", "RULE-SET,ads,REJECT", "FINAL,Proxy"],
            defined: ["geosite-cn": own]
        ))
        XCTAssertEqual(sets["geosite-cn"], own)
        XCTAssertEqual(sets["geoip-cn"]?.url, RuleSetCatalog.known["geoip-cn"]?.url)
        // Unknown names are left to the engine, which treats a missing set as empty.
        XCTAssertNil(sets["ads"])
        XCTAssertNil(RuleSetCatalog.ruleSets(for: ["FINAL,DIRECT"], defined: nil))
    }

    func testRuntimeRoutingCarriesTheRuleSets() throws {
        let routing = EmbeddedRuntimeConfig.Routing(
            outbounds: ["a": [:]],
            rules: ["GEOIP,CN,DIRECT", "FINAL,a"],
            groups: [],
            ruleSets: RuleSetCatalog.ruleSets(for: ["GEOIP,CN,DIRECT"], defined: nil)
        )
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(routing)) as? [String: Any])
        let sets = try XCTUnwrap(json["rule_sets"] as? [String: [String: Any]])
        XCTAssertEqual(sets["geoip-cn"]?["url"] as? String, RuleSetCatalog.known["geoip-cn"]?.url)
        XCTAssertNil(sets["geoip-cn"]?["format"])
    }

    func testDecodesRuleSetStatus() throws {
        let answer = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.RuleSets.self, from: Data(
            #"{"rule_sets":[{"name":"geoip-cn","source":"remote","entries":8000,"updated_at_ms":1700000000000,"last_error":null}]}"#.utf8
        )))
        XCTAssertEqual(answer.ruleSets.first?.entries, 8000)
        XCTAssertEqual(answer.ruleSets.first?.updatedAt, Date(timeIntervalSince1970: 1_700_000_000))
        XCTAssertEqual(String(decoding: RuntimeControl.updateRuleSets.request, as: UTF8.self), #"{"cmd":"update_rule_sets"}"#)
        XCTAssertEqual(answer.ruleSets.first?.skipped, [:])
        XCTAssertEqual(answer.ruleSets.first?.skippedCount, 0)
    }

    func testDecodesTheLinesASetSkipped() throws {
        let answer = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.RuleSets.self, from: Data(
            #"{"rule_sets":[{"name":"telegram","source":"remote","entries":40,"skipped":{"SCRIPT":1,"IP-ASN":12,"GEOIP":1}}]}"#.utf8
        )))
        let status = try XCTUnwrap(answer.ruleSets.first)
        XCTAssertEqual(status.skippedCount, 14)
        XCTAssertEqual(status.skippedSummary, "IP-ASN ×12, GEOIP ×1, SCRIPT ×1")
    }

    func testTheBuiltInLanSetIsNeverMissing() {
        let rules = ["RULE-SET,LAN,DIRECT,no-resolve", "AND,((RULE-SET,LAN),(DST-PORT,445)),REJECT", "FINAL,Proxy"]
        XCTAssertEqual(RuleSetCatalog.referencedNames(in: rules), ["LAN"])
        XCTAssertEqual(RuleSetCatalog.missingSets(in: rules[0], sets: nil), [])
        XCTAssertEqual(RuleSetCatalog.runnableRules(rules, sets: nil), rules)
        // The catalog adds no list for it: the engine has it.
        XCTAssertNil(RuleSetCatalog.ruleSets(for: rules, defined: nil))
    }

    func testLeavesOutRulesNamingMissingSets() {
        let rules = [
            "GEOSITE,google,Proxy",
            "AND,((RULE-SET,ads),(DST-PORT,443)),REJECT",
            "GEOSITE,cn,DIRECT",
            "GEOIP,US,Proxy",
            "FINAL,Proxy",
        ]
        let sets = RuleSetCatalog.ruleSets(for: rules, defined: nil)
        XCTAssertEqual(RuleSetCatalog.runnableRules(rules, sets: sets), ["GEOSITE,cn,DIRECT", "GEOIP,US,Proxy", "FINAL,Proxy"])
        XCTAssertEqual(RuleSetCatalog.missingSets(in: rules[1], sets: sets), ["ads"])
    }

    func testRuleSetsAnswerNamesTheMissingData() throws {
        let answer = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.RuleSets.self, from: Data(
            #"{"rule_sets":[],"missing":["asn-13335","geoip-us"]}"#.utf8
        )))
        XCTAssertEqual(answer.missing, ["asn-13335", "geoip-us"])
        XCTAssertEqual(RuleSetCatalog.referencedNames(in: ["IP-ASN,AS13335,Proxy", "GEOIP,US,DIRECT"]), ["asn-13335", "geoip-us"])
        XCTAssertTrue(RuleSetCatalog.hasProcessRules(["AND,((PROCESS-NAME,curl),(DST-PORT,443)),DIRECT"]))
        XCTAssertFalse(RuleSetCatalog.hasProcessRules(["DOMAIN,process-name.test,DIRECT"]))
        XCTAssertTrue(RuleSetCatalog.hasRules(of: "DEVICE-NAME", in: ["OR,((DEVICE-NAME,TV),(DST-PORT,22)),DIRECT"]))
        XCTAssertFalse(RuleSetCatalog.hasRules(of: "CELLULAR-RADIO", in: ["DOMAIN,cellular-radio.test,DIRECT"]))
    }

    func testRuleHitsAndRouteTestsDecode() throws {
        let hits = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.RuleHits.self, from: Data(
            #"{"generation":2,"rules":[{"rule":"DOMAIN,a.test","target":"Proxy","hits":0},{"rule":"GEOIP,CN","target":"DIRECT","hits":7,"last_hit_at_ms":5}],"final":{"rule":"FINAL","target":"Proxy","hits":3}}"#.utf8
        )))
        XCTAssertEqual(hits.busiest().map { $0.hit.rule }, ["GEOIP,CN"])
        XCTAssertEqual(hits.busiest().first?.index, 1)
        XCTAssertEqual(hits.unused, 1)
        XCTAssertEqual(hits.finalRule.hits, 3)

        let test = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.RouteTestAnswer.self, from: Data(
            #"{"mode":"rule","test":{"rule_index":1,"rule":"GEOIP,CN","chain":["Proxy"],"outbound":"n1","needs_address":true}}"#.utf8
        )))
        XCTAssertTrue(test.test.needsAddress)
        XCTAssertEqual(test.test.path, "Proxy → n1")

        let request = try XCTUnwrap(JSONSerialization.jsonObject(
            with: RuntimeControl.testRoute(host: "a.test", port: 443, udp: true, address: " ").request
        ) as? [String: Any])
        XCTAssertEqual(request["cmd"] as? String, "test_route")
        XCTAssertEqual(request["network"] as? String, "udp")
        XCTAssertNil(request["address"])
    }
}
