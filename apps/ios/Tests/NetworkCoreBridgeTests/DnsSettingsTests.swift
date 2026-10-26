import Foundation
import NetworkCoreBridge
import XCTest

final class DnsSettingsTests: XCTestCase {
    func testEncodesTheEngineSection() throws {
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(DnsSettings.split)) as? [String: Any])
        XCTAssertEqual(json["default"] as? String, "cloudflare")
        let servers = try XCTUnwrap(json["servers"] as? [[String: Any]])
        XCTAssertEqual(servers.first?["server_name"] as? String, "cloudflare-dns.com")
        XCTAssertEqual(servers.last?["via"] as? String, "direct")
        let rules = try XCTUnwrap(json["rules"] as? [[String: Any]])
        XCTAssertEqual(rules.first?["domain_suffix"] as? [String], ["cn"])
        XCTAssertEqual(rules.last?["rule_set"] as? String, "geosite-cn")
        XCTAssertNil(json["hosts"])
    }

    func testRulesAndDefaultNeedServers() throws {
        let settings = DnsSettings(defaultServer: "gone", hosts: ["nas.lan": ["192.168.1.10"]], rules: [DnsSettings.Rule(domain: ["a.example"], server: "gone")])
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(settings)) as? [String: Any])
        XCTAssertEqual(Set(json.keys), ["hosts"])
        XCTAssertNil(DnsSettings.fromProfileValue(try DnsSettings().profileValue()))
        XCTAssertEqual(DnsSettings.fromProfileValue(try settings.profileValue())?.hosts, settings.hosts)
    }

    func testChecksServers() {
        for url in ["1.1.1.1", "1.1.1.1:53", "tcp://dns.example", "tls://9.9.9.9", "https://dns.google/dns-query", "https://[2606:4700::1111]/dns-query", "2606:4700::1111"] {
            XCTAssertTrue(DnsSettings.isUpstream(url), url)
        }
        for url in ["dns.google", "udp://1.1.1.1", "tls://1.1.1.1/path", "https://", "1.1.1.1:0", "https://a b/dns-query"] {
            XCTAssertFalse(DnsSettings.isUpstream(url), url)
        }
        XCTAssertThrowsError(try DnsSettings.check(DnsSettings.Server(name: "a b", url: "1.1.1.1"))) { error in
            XCTAssertEqual(error as? DnsSettingsError, .invalidName)
        }
    }

    func testRenamingAServerKeepsItsRulesAndDefault() throws {
        var settings = try DnsSettings().saving(DnsSettings.Server(name: "home", url: "192.168.1.1", via: .direct))
        settings = try settings.addingRule("lan\nDOMAIN,router.example", server: "home")
        settings.defaultServer = "home"
        let renamed = try settings.saving(DnsSettings.Server(name: "router", url: "192.168.1.1", via: .direct), replacing: "home")
        XCTAssertEqual(renamed.rules.first?.server, "router")
        XCTAssertEqual(renamed.defaultServer, "router")
        XCTAssertEqual(renamed.rules.first?.domainSuffix, ["lan"])
        XCTAssertEqual(renamed.rules.first?.domain, ["router.example"])

        XCTAssertThrowsError(try renamed.saving(DnsSettings.Server(name: "router", url: "1.1.1.1"))) { error in
            XCTAssertEqual(error as? DnsSettingsError, .duplicateName)
        }
        let removed = renamed.removingServer("router")
        XCTAssertTrue(removed.isEmpty)
        XCTAssertTrue(removed.rules.isEmpty)
        XCTAssertNil(removed.defaultServer)
    }

    func testParsesRulesAndHosts() throws {
        let base = try DnsSettings().saving(DnsSettings.Server(name: "ali", url: "223.5.5.5", via: .direct))
        let rule = try XCTUnwrap(base.addingRule("GEOSITE,CN\nDOMAIN-KEYWORD,Taobao\n.qq.com", server: "ali").rules.first)
        XCTAssertEqual(rule.ruleSet, "geosite-cn")
        XCTAssertEqual(rule.domainKeyword, ["taobao"])
        XCTAssertEqual(rule.domainSuffix, ["qq.com"])
        XCTAssertThrowsError(try base.addingRule("IP-CIDR,1.0.0.0/8", server: "ali"))
        XCTAssertThrowsError(try base.addingRule("cn", server: "missing"))

        let hosts = try base.settingHost("*.Lan", addresses: "192.168.1.2, fd00::2")
        XCTAssertEqual(hosts.hosts["*.lan"], ["192.168.1.2", "fd00::2"])
        XCTAssertThrowsError(try base.settingHost("nas.lan", addresses: "nas"))
        XCTAssertThrowsError(try base.settingHost("bad name", addresses: "10.0.0.1"))
    }

    func testRuntimeConfigCarriesDnsAndItsRuleSets() throws {
        let routing = EmbeddedRuntimeConfig.Routing(outbounds: ["a": [:]], rules: ["FINAL,a"], groups: [])
        let config = EmbeddedRuntimeConfig(
            outbound: .shadowsocks(server: "198.51.100.1", port: 8388, method: "2022-blake3-aes-128-gcm", password: "secret"),
            profileID: "test"
        )
        .withRouting(routing)
        .withDns(DnsSettings.split)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: config.encoded()) as? [String: Any])
        let dns = try XCTUnwrap(json["dns"] as? [String: Any])
        XCTAssertEqual(dns["default"] as? String, "cloudflare")
        let sets = try XCTUnwrap((json["routing"] as? [String: Any])?["rule_sets"] as? [String: Any])
        XCTAssertNotNil(sets["geosite-cn"])
        XCTAssertNil(config.withoutSecret().dns)
        XCTAssertNil(config.withDns(DnsSettings()).dns)
    }
}
