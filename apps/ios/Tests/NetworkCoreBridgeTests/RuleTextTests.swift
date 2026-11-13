import Foundation
import NetworkCoreBridge
import XCTest

final class RuleTextTests: XCTestCase {
    private func substring(_ text: String, _ range: NSRange) -> String {
        (text as NSString).substring(with: range)
    }

    func testTextRoundTripsRulesAndKeepsTheirIds() throws {
        var rules = LocalRules()
        rules.rules = [
            LocalRules.Rule(kind: "DOMAIN-SUFFIX", value: "example.com", policy: .direct),
            LocalRules.Rule(kind: "IP-CIDR", value: "10.0.0.0/8", policy: .proxy, noResolve: true, enabled: false),
        ]
        let text = RuleText.text(of: rules)
        XCTAssertEqual(text, "DOMAIN-SUFFIX,example.com,DIRECT\n# IP-CIDR,10.0.0.0/8,PROXY,no-resolve")
        XCTAssertEqual(RuleText.issues(in: text), [])

        let edited = text + "\n# just a note\nDOMAIN-KEYWORD,ads,REJECT\n"
        let next = try RuleText.apply(edited, to: rules)
        XCTAssertEqual(next.rules.map(\.text), [
            "DOMAIN-SUFFIX,example.com,DIRECT", "IP-CIDR,10.0.0.0/8,PROXY,no-resolve", "DOMAIN-KEYWORD,ads,REJECT",
        ])
        XCTAssertEqual(next.rules.map(\.enabled), [true, false, true])
        XCTAssertEqual(next.rules[0].id, rules.rules[0].id)
        XCTAssertEqual(next.rules[1].id, rules.rules[1].id)
        XCTAssertEqual(next.preset, rules.preset)
    }

    func testIssuesPointAtTheBadField() {
        let text = "DOMAIN,ok.example,DIRECT\r\nBOGUS,x,DIRECT\nIP-CIDR,10.0.0.0/99,DIRECT\nDOMAIN\n  FINAL,DIRECT\n域名,中文,DIRECT"
        let issues = RuleText.issues(in: text)
        XCTAssertEqual(issues.map(\.line), [1, 2, 3, 4, 5])
        XCTAssertEqual(issues[0].problem, .unsupportedKind("BOGUS"))
        XCTAssertEqual(substring(text, issues[0].range), "BOGUS")
        XCTAssertEqual(issues[1].problem, .invalidValue)
        XCTAssertEqual(substring(text, issues[1].range), "10.0.0.0/99")
        XCTAssertEqual(issues[2].problem, .missingValue)
        XCTAssertEqual(issues[3].problem, .final)
        XCTAssertEqual(substring(text, issues[3].range), "FINAL,DIRECT")
        XCTAssertEqual(issues[4].problem, .unsupportedKind("域名"))
        XCTAssertEqual(substring(text, issues[4].range), "域名")
        XCTAssertThrowsError(try RuleText.apply(text, to: LocalRules())) { error in
            XCTAssertEqual((error as? RuleText.Rejected)?.issues, issues)
        }
        let line2 = RuleText.range(ofLine: 2, in: text)
        XCTAssertEqual(line2.map { substring(text, $0) }, "IP-CIDR,10.0.0.0/99,DIRECT")
    }

    func testTokensColorEachField() {
        let text = "# off\nIP-CIDR, 10.0.0.0/8 ,Proxy,no-resolve"
        let tokens = RuleText.tokens(in: text)
        let named = tokens.filter { $0.kind != .separator }.map { ($0.kind, substring(text, $0.range)) }
        XCTAssertEqual(named.map { $0.0 }, [.comment, .kind, .value, .policy, .option])
        XCTAssertEqual(named.map { $0.1 }, ["# off", "IP-CIDR", "10.0.0.0/8", "Proxy", "no-resolve"])
        XCTAssertEqual(tokens.filter { $0.kind == .separator }.count, 3)
    }

    func testTooManyRulesAreMarked() {
        let text = (0...LocalRules.maxRules).map { "DOMAIN,host\($0).example,DIRECT" }.joined(separator: "\n")
        let issues = RuleText.issues(in: text)
        XCTAssertEqual(issues.map(\.problem), [.tooMany])
        XCTAssertEqual(issues.first?.line, LocalRules.maxRules)
    }

    func testReloadRequestAndAnswer() throws {
        let request = RuntimeControl.reloadRules(["DOMAIN,a.example,DIRECT", "FINAL,proxy"], dryRun: false).request
        XCTAssertEqual(
            String(decoding: request, as: UTF8.self),
            #"{"cmd":"reload_rules","dry_run":false,"rules":["DOMAIN,a.example,DIRECT","FINAL,proxy"]}"#
        )
        let done = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.ReloadAnswer.self, from: Data(#"{"reloaded":true,"rules":2,"generation":3}"#.utf8)))
        XCTAssertEqual(done, .init(reloaded: true, rules: 2, generation: 3))
        let refused = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.ReloadAnswer.self, from: Data(#"{"reloaded":false,"error":{"line":4,"code":"syntax"}}"#.utf8)))
        XCTAssertEqual(refused.error, .init(line: 4, code: "syntax"))
        let value = Data(#"{"rules":[]}"#.utf8)
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.reloadLocalRules(value).encoded), .reloadLocalRules(value))
    }

    func testOnlyRuleLineChangesReloadInPlace() throws {
        let node = ShadowsocksNode(host: "198.51.100.1", port: 8388, method: "2022-blake3-aes-128-gcm", password: "secret", name: "a")
        let base = EmbeddedRuntimeConfig(node: node, profileID: "test")
        let outbounds: [String: [String: JSONValue]] = ["a": ["type": .string("shadowsocks")]]
        let running = base.withRouting(.init(outbounds: outbounds, rules: ["DOMAIN,a.example,DIRECT", "FINAL,a"], groups: []))
        let edited = base.withRouting(.init(outbounds: outbounds, rules: ["DOMAIN,b.example,REJECT", "FINAL,a"], groups: []))
        XCTAssertEqual(running.reloadableRules(to: edited), ["DOMAIN,b.example,REJECT", "FINAL,a"])
        XCTAssertEqual(running.reloadableRules(to: running), running.routing?.rules)

        let newSet = base.withRouting(.init(
            outbounds: outbounds,
            rules: ["RULE-SET,local-1,DIRECT", "FINAL,a"],
            groups: [],
            ruleSets: ["local-1": .init(url: "https://example.com/list.txt", via: "a")]
        ))
        XCTAssertNil(running.reloadableRules(to: newSet), "a new rule set needs a restart")
        XCTAssertNil(running.reloadableRules(to: edited.withMode(.global)), "anything beyond the lines needs a restart")
        XCTAssertNil(base.reloadableRules(to: edited), "no routing to swap")
    }
}
