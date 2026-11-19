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

    func testPlainNodeListsSendEverythingElseToTheSelectedNodeWithoutPresetLists() throws {
        let report = try NativeImportReport.decoded(from: Data(plainReport.utf8))
        var library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        library.selectedID = "b"
        XCTAssertNil(library.routingForModes())

        let routing = try XCTUnwrap(library.routingForModes(localRules: LocalRules()))
        XCTAssertEqual(routing.global, "b")
        XCTAssertTrue(routing.groups.isEmpty)
        XCTAssertEqual(Set(routing.outbounds.keys), ["a", "b"])
        // A fresh install: the private networks direct (the switch the user can turn off), then the node. No
        // country, GFW or ad list and no rule set at all.
        XCTAssertEqual(routing.rules.first, "DOMAIN-SUFFIX,local,DIRECT")
        XCTAssertEqual(routing.rules.last, "FINAL,b")
        XCTAssertTrue(routing.rules.dropLast().allSatisfy { $0.hasSuffix(",DIRECT") || $0.hasSuffix(",DIRECT,no-resolve") })
        XCTAssertFalse(routing.rules.contains { $0.hasPrefix("GEOSITE") || $0.hasPrefix("GEOIP") || $0.hasPrefix("RULE-SET") })
        XCTAssertNil(routing.ruleSets)

        let bare = try XCTUnwrap(library.routingForModes(localRules: LocalRules(bypassLAN: false)))
        XCTAssertEqual(bare.rules, ["FINAL,b"])
        XCTAssertNil(bare.ruleSets)
    }

    func testValuesSavedWithAnOldPresetLoadWithoutIt() throws {
        let old = Data(#"{"preset":"bypassChina","bypass_lan":false,"rules":[],"subscriptions":[{"id":"6F9619FF-8B86-D011-B42D-00C04FC964FF","name":"Ads","url":"https://lists.example/ads.list","policy":"REJECT","enabled":true,"catalogID":"bm7-advertising"}]}"#.utf8)
        let rules = LocalRules.fromProfileValue(old)
        XCTAssertFalse(rules.bypassLAN)
        // The list the user added stays: it is their own configuration.
        XCTAssertEqual(rules.subscriptions.map(\.url), ["https://lists.example/ads.list"])
        let lines = rules.engineLines(proxy: "a").lines
        XCTAssertEqual(lines.last, "FINAL,a")
        XCTAssertFalse(lines.contains { $0.hasPrefix("GEOSITE") || $0.hasPrefix("GEOIP") })
    }

    func testTheUsersOwnGeositeRulesStillGetTheirList() throws {
        let rules = try LocalRules(bypassLAN: false).saving(LocalRules.rule(from: "GEOSITE,cn,DIRECT"))
        let engine = rules.engineLines(proxy: "a")
        XCTAssertEqual(engine.lines, ["GEOSITE,cn,DIRECT", "FINAL,a"])
        XCTAssertNotNil(engine.ruleSets["geosite-cn"])
    }

    func testRejectVariantsKeepTheirAnswer() throws {
        let policy = { (line: String) in try LocalRules.rule(from: line).policy }
        XCTAssertEqual(try policy("DOMAIN,a.example,REJECT"), .reject)
        XCTAssertEqual(try policy("DOMAIN,a.example,reject-drop"), .rejectDrop)
        XCTAssertEqual(try policy("DOMAIN,a.example,REJECT-NO-DROP"), .rejectNoDrop)
        XCTAssertEqual(try policy("DOMAIN,a.example,REJECT-TINYGIF"), .rejectTinyGif)
        XCTAssertEqual(try policy("DOMAIN,a.example,REJECT-IMG"), .rejectTinyGif)
        // Answers that need decryption stay a plain REJECT.
        XCTAssertEqual(try policy("DOMAIN,a.example,REJECT-200"), .reject)
        XCTAssertEqual(try LocalRules.rule(from: "DOMAIN,a.example,REJECT-DROP").text, "DOMAIN,a.example,REJECT-DROP")
        XCTAssertTrue(LocalRules.Policy.rejectDrop.isReject)
        XCTAssertFalse(LocalRules.Policy.direct.isReject)
    }

    func testUserRulesGoFirstWithProxyReplaced() throws {
        let report = try NativeImportReport.decoded(from: Data(plainReport.utf8))
        let library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        var rules = LocalRules(bypassLAN: false)
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
            "RULE-SET,local-1,REJECT-TINYGIF",
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
        // The private networks and FINAL apply only without subscription rules.
        XCTAssertEqual(routing.rules, ["DOMAIN-KEYWORD,video,Proxy", "DOMAIN-SUFFIX,google.com,Proxy", "FINAL,Proxy"])
        XCTAssertEqual(library.routingForModes()?.rules, ["DOMAIN-SUFFIX,google.com,Proxy", "FINAL,Proxy"])
    }

    func testDataListsReplaceTheSubscriptionsSetAndDownloadThroughTheProxy() throws {
        let report = try NativeImportReport.decoded(from: Data(routedReport.utf8))
        let library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        var rules = try LocalRules().saving(LocalRules.rule(from: "IP-ASN,AS13335,PROXY,no-resolve"))
        rules = try rules.saving(dataSource: "AS13335", url: "https://lists.example/13335.txt")
        rules = try rules.saving(dataSource: "geoip,us", url: "https://lists.example/us.txt")
        XCTAssertEqual(rules.dataSources.map(\.name), ["asn-13335", "geoip-us"])
        let routing = try XCTUnwrap(library.routingForModes(localRules: rules))
        XCTAssertEqual(routing.rules.first, "IP-ASN,13335,Proxy,no-resolve")
        XCTAssertEqual(routing.ruleSets?["asn-13335"]?.url, "https://lists.example/13335.txt")
        XCTAssertEqual(routing.ruleSets?["asn-13335"]?.via, "Proxy")
        XCTAssertEqual(routing.ruleSets?["geoip-us"]?.url, "https://lists.example/us.txt")

        let replaced = try rules.saving(dataSource: "geoip-us", url: "https://lists.example/us-2.txt")
        XCTAssertEqual(replaced.dataSources.count, 2)
        XCTAssertEqual(replaced.removing(dataSource: "geoip-us").dataSources.map(\.name), ["asn-13335"])
        XCTAssertThrowsError(try rules.saving(dataSource: "geoip-us", url: "http://lists.example/us.txt"))
        XCTAssertThrowsError(try rules.saving(dataSource: "usa", url: "https://lists.example/us.txt"))

        // Saved and read back; values from before data lists load with none.
        let decoded = try JSONDecoder().decode(LocalRules.self, from: rules.profileValue())
        XCTAssertEqual(decoded.dataSources, rules.dataSources)
        XCTAssertEqual(LocalRules.fromProfileValue(Data(#"{"rules":[]}"#.utf8)).dataSources, [])
    }

    func testDataListNamesComeFromWhatTheUserTyped() {
        for text in ["us", "US", "GEOIP,US", "geoip-us", " GeoIP, us "] {
            XCTAssertEqual(LocalRules.DataSource.name(from: text), "geoip-us", text)
        }
        for text in ["13335", "AS13335", "IP-ASN,13335", "ip-asn,AS13335", "asn-13335"] {
            XCTAssertEqual(LocalRules.DataSource.name(from: text), "asn-13335", text)
        }
        for text in ["", "usa", "geoip-1a", "0", "AS013", "4294967296", "geosite-cn"] {
            XCTAssertNil(LocalRules.DataSource.name(from: text), text)
        }
        XCTAssertEqual(LocalRules.DataSource.rule(for: "asn-13335"), "IP-ASN,13335")
        XCTAssertEqual(LocalRules.DataSource.rule(for: "geoip-us"), "GEOIP,US")
        XCTAssertThrowsError(try LocalRules.rule(from: "IP-ASN,AS0,PROXY"))
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
        let rules = try LocalRules(bypassLAN: false)
            .saving(LocalRules.rule(from: "DOMAIN,a.example,DIRECT"))
        XCTAssertEqual(LocalRules.fromProfileValue(try rules.profileValue()), rules)
        XCTAssertEqual(LocalRules.fromProfileValue(nil), LocalRules())
        XCTAssertEqual(LocalRules.fromProfileValue(Data("{}".utf8)), LocalRules())
    }

    func testRuleSetSubscriptionsMatchAfterTheUserRulesAndDownloadThroughTheProxy() throws {
        let report = try NativeImportReport.decoded(from: Data(plainReport.utf8))
        let library = NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        let ads = LocalRules.Subscription(name: "Ads", url: "https://lists.example/ads.list", policy: .reject)
        var off = LocalRules.Subscription(name: "Apple", url: "https://lists.example/apple.list", policy: .direct)
        off.enabled = false
        var rules = LocalRules(bypassLAN: false)
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
        // Every remaining RULE-SET rule names a set the engine has.
        for line in routing.rules where line.hasPrefix("RULE-SET,") {
            XCTAssertNotNil(sets[line.split(separator: ",")[1].description], line)
        }
    }
}
