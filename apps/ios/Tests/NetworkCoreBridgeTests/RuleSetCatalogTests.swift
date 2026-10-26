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
}
