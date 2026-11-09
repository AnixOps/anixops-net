import Foundation
import NetworkCoreBridge
import XCTest

final class LocalRulesTests: XCTestCase {
    private let plainReport = """
    {"nodes":[
    {"id":"a","name":"Tokyo","protocol":"trojan","server":"a.example","port":443,
     "outbound":{"type":"trojan","server":"a.example","port":443,"password":"pa"}},
    {"id":"b","name":"","protocol":"trojan","server":"b.example","port":443,
     "outbound":{"type":"trojan","server":"b.example","port":443,"password":"pb"}}],
    "unsupported":[]}
    """

    private let routedReport = """
    {"nodes":[
    {"id":"a","name":"Tokyo","protocol":"trojan","server":"a.example","port":443,
     "outbound":{"type":"trojan","server":"a.example","port":443,"password":"pa"}}],
    "unsupported":[],
    "routing":{"rules":["DOMAIN-SUFFIX,google.com,Proxy","FINAL,Proxy"],
    "groups":[{"id":"Proxy","type":"select","members":["a","DIRECT"],"default":null}]}}
    """

    func testPlainNodeListsRouteByThePresetToTheSelectedNode() throws {
        let report = try NativeImportReport.decoded(from: Data(plainReport.utf8))
        var library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        library.selectedID = "b"
        XCTAssertNil(library.routingForModes())

        let routing = try XCTUnwrap(library.routingForModes(localRules: LocalRules()))
        XCTAssertEqual(routing.global, "b")
        XCTAssertTrue(routing.groups.isEmpty)
        XCTAssertEqual(Set(routing.outbounds.keys), ["a", "b"])
        XCTAssertEqual(routing.rules.first, "DOMAIN-SUFFIX,local,DIRECT")
        XCTAssertEqual(Array(routing.rules.suffix(3)), ["GEOSITE,cn,DIRECT", "GEOIP,cn,DIRECT", "FINAL,b"])
        XCTAssertNotNil(routing.ruleSets?["geosite-cn"]?.url)
        XCTAssertNotNil(routing.ruleSets?["geoip-cn"]?.url)

        let gfw = try XCTUnwrap(library.routingForModes(localRules: LocalRules(preset: .gfwList, bypassLAN: false)))
        XCTAssertEqual(gfw.rules, ["GEOSITE,gfw,b", "FINAL,DIRECT"])
        XCTAssertEqual(gfw.ruleSets.map { Set($0.keys) }, ["geosite-gfw"])
    }

    func testUserRulesGoFirstWithProxyReplaced() throws {
        let report = try NativeImportReport.decoded(from: Data(plainReport.utf8))
        let library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        var rules = LocalRules(preset: .proxyAll, bypassLAN: false)
        rules = try rules.saving(LocalRules.rule(from: "DOMAIN-SUFFIX,Example.COM,Proxy"))
        rules = try rules.saving(LocalRules.rule(from: "IP-CIDR,203.0.113.0/24,DIRECT,no-resolve"))
        rules = try rules.saving(LocalRules.rule(from: "RULE-SET,https://lists.example/ads.list,REJECT-TINYGIF"))
        var disabled = try LocalRules.rule(from: "DOMAIN,off.example,DIRECT")
        disabled.enabled = false
        rules = try rules.saving(disabled)

        let routing = try XCTUnwrap(library.routingForModes(localRules: rules))
        XCTAssertEqual(routing.rules, [
            "DOMAIN-SUFFIX,example.com,a",
            "IP-CIDR,203.0.113.0/24,DIRECT,no-resolve",
            "RULE-SET,local-1,REJECT",
            "FINAL,a",
        ])
        XCTAssertEqual(routing.ruleSets?["local-1"]?.url, "https://lists.example/ads.list")
        XCTAssertEqual(routing.ruleSets?["local-1"]?.via, "a")
    }

    func testUserRulesGoInFrontOfSubscriptionRules() throws {
        let report = try NativeImportReport.decoded(from: Data(routedReport.utf8))
        let library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        let rules = try LocalRules().saving(LocalRules.rule(from: "DOMAIN-KEYWORD,video,PROXY"))
        let routing = try XCTUnwrap(library.routingForModes(localRules: rules))
        // The preset applies only without subscription rules.
        XCTAssertEqual(routing.rules, ["DOMAIN-KEYWORD,video,Proxy", "DOMAIN-SUFFIX,google.com,Proxy", "FINAL,Proxy"])
        XCTAssertEqual(library.routingForModes()?.rules, ["DOMAIN-SUFFIX,google.com,Proxy", "FINAL,Proxy"])
    }

    func testLinesTheEngineWouldRefuseAreNotKept() {
        let bad = [
            "DOMAIN-SUFFIX,,DIRECT",
            "IP-CIDR,10.0.0.0,DIRECT",
            "IP-CIDR,300.0.0.0/8,DIRECT",
            "GEOSITE,netflix,PROXY",
            "GEOIP,CHN,DIRECT",
            "DST-PORT,80-,DIRECT",
            "DST-PORT,90-80,DIRECT",
            "PROTOCOL,SCTP,DIRECT",
            "RULE-SET,SYSTEM,DIRECT",
            "RULE-SET,http://plain.example/a.list,DIRECT",
            "URL-REGEX,^https://a,REJECT",
            "PROCESS-NAME,Telegram,PROXY",
        ]
        for line in bad {
            XCTAssertThrowsError(try LocalRules.rule(from: line), line)
        }
        XCTAssertEqual(try LocalRules.rule(from: "DEST-PORT,6000-6100,DIRECT").text, "DST-PORT,6000-6100,DIRECT")
        XCTAssertEqual(try LocalRules.rule(from: "geoip,cn,DIRECT,no-resolve").text, "GEOIP,CN,DIRECT,no-resolve")
        XCTAssertEqual(try LocalRules.rule(from: "DOMAIN,a.example,DIRECT,no-resolve").text, "DOMAIN,a.example,DIRECT")
        XCTAssertEqual(try LocalRules.rule(from: "IP-CIDR6,2001:DB8::/32,🚀 节点选择").text, "IP-CIDR6,2001:db8::/32,PROXY")
        XCTAssertEqual(try LocalRules.rule(from: "DOMAIN-SUFFIX,example.com").policy, .proxy)
    }

    func testPastedProfilesKeepOnlyTheRuleSection() {
        let text = """
        [General]
        DOMAIN-SUFFIX,general.example,DIRECT
        [Rule]
        # comment
        DOMAIN-SUFFIX,apple.com,DIRECT
        DOMAIN-SUFFIX,apple.com,DIRECT
        IP-CIDR,192.168.0.0/16,DIRECT,no-resolve
        PROCESS-NAME,Telegram,Proxy
        FINAL,Proxy
        [MITM]
        hostname = a.example
        """
        let parsed = LocalRules.parse(text)
        XCTAssertEqual(parsed.rules.map(\.text), [
            "DOMAIN-SUFFIX,apple.com,DIRECT",
            "IP-CIDR,192.168.0.0/16,DIRECT,no-resolve",
        ])
        XCTAssertEqual(parsed.skipped, 2)

        let existing = LocalRules(rules: [parsed.rules[0]])
        let merged = existing.merging(parsed.rules)
        XCTAssertEqual(merged.added, 1)
        XCTAssertEqual(merged.rules.rules.count, 2)

        XCTAssertEqual(LocalRules.parse("DOMAIN,a.example,REJECT\nGEOIP,CN,DIRECT").rules.count, 2)
    }

    func testProfileValueRoundTripsAndOldProfilesGetTheDefault() throws {
        let rules = try LocalRules(preset: .returnChina, bypassLAN: false)
            .saving(LocalRules.rule(from: "DOMAIN,a.example,DIRECT"))
        XCTAssertEqual(LocalRules.fromProfileValue(try rules.profileValue()), rules)
        XCTAssertEqual(LocalRules.fromProfileValue(nil), LocalRules())
        XCTAssertEqual(LocalRules.fromProfileValue(Data("{}".utf8)).preset, .bypassChina)
    }

    func testRuleSetSubscriptionsMatchAfterTheUserRulesAndDownloadThroughTheProxy() throws {
        let report = try NativeImportReport.decoded(from: Data(plainReport.utf8))
        let library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        let ads = try XCTUnwrap(OpenRuleSets.entry(id: "bm7-advertising")).subscription()
        var off = try XCTUnwrap(OpenRuleSets.entry(id: "bm7-apple")).subscription()
        off.enabled = false
        var rules = LocalRules(preset: .proxyAll, bypassLAN: false)
        rules = try rules.saving(LocalRules.rule(from: "DOMAIN,a.example,DIRECT"))
        rules = try rules.saving(ads)
        rules = try rules.saving(off)

        let routing = try XCTUnwrap(library.routingForModes(localRules: rules))
        XCTAssertEqual(routing.rules, ["DOMAIN,a.example,DIRECT", "RULE-SET,\(ads.engineName),REJECT", "FINAL,a"])
        XCTAssertEqual(routing.ruleSets?[ads.engineName]?.url, ads.url)
        XCTAssertEqual(routing.ruleSets?[ads.engineName]?.via, "a")
        XCTAssertNil(routing.ruleSets?[off.engineName])
        XCTAssertTrue(ads.engineName.hasPrefix("sub-"))

        // Kept through a profile round trip, and old values without subscriptions still load.
        XCTAssertEqual(LocalRules.fromProfileValue(try rules.profileValue()).subscriptions, rules.subscriptions)
    }

    func testRuleSetSubscriptionsNeedHttpsAndAreCapped() throws {
        XCTAssertThrowsError(try LocalRules().saving(LocalRules.Subscription(name: "x", url: "http://a.example/x.list", policy: .proxy)))
        let named = try LocalRules().saving(LocalRules.Subscription(name: " ", url: "https://a.example/lists/Ads.list", policy: .reject))
        XCTAssertEqual(named.subscriptions.first?.name, "Ads")

        var full = LocalRules()
        for index in 0..<LocalRules.maxSubscriptions {
            full = try full.saving(LocalRules.Subscription(name: "\(index)", url: "https://a.example/\(index).list", policy: .proxy))
        }
        XCTAssertThrowsError(try full.saving(LocalRules.Subscription(name: "more", url: "https://a.example/more.list", policy: .proxy)))
    }

    func testTheEngineGetsAtMost32RuleSets() throws {
        let report = try NativeImportReport.decoded(from: Data(plainReport.utf8))
        let library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        var rules = LocalRules()
        for index in 0..<LocalRules.maxSubscriptions {
            rules = try rules.saving(LocalRules.Subscription(name: "\(index)", url: "https://a.example/\(index).list", policy: .proxy))
        }
        for index in 0..<15 {
            rules = try rules.saving(LocalRules.rule(from: "RULE-SET,https://b.example/\(index).list,DIRECT"))
        }
        let routing = try XCTUnwrap(library.routingForModes(localRules: rules))
        let sets = try XCTUnwrap(routing.ruleSets)
        XCTAssertEqual(sets.count, NodeLibrary.maxRuleSets)
        XCTAssertNotNil(sets["geosite-cn"])
        XCTAssertNotNil(sets["geoip-cn"])
        // Every remaining RULE-SET rule names a set the engine has.
        for line in routing.rules where line.hasPrefix("RULE-SET,") {
            XCTAssertNotNil(sets[line.split(separator: ",")[1].description], line)
        }
    }

    func testTheOpenRuleSetCatalogIsWellFormed() throws {
        XCTAssertEqual(Set(OpenRuleSets.entries.map(\.id)).count, OpenRuleSets.entries.count)
        for entry in OpenRuleSets.entries {
            XCTAssertNoThrow(try LocalRules().saving(entry.subscription()), entry.id)
            XCTAssertTrue(entry.url.hasPrefix("https://raw.githubusercontent.com/"), entry.id)
        }
        for category in OpenRuleSets.Category.allCases {
            XCTAssertFalse(OpenRuleSets.entries.filter { $0.category == category }.isEmpty, category.rawValue)
        }
    }
}
