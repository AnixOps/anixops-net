import Foundation
import NetworkCoreBridge
import XCTest

final class DnsSettingsTests: XCTestCase {
    /// A user's own split, as they might set it up: the app ships no such preset (docs/no-presets.md).
    private let split = DnsSettings(
        servers: [
            DnsSettings.Server(name: "cloudflare", url: "https://1.1.1.1/dns-query", serverName: "cloudflare-dns.com"),
            DnsSettings.Server(name: "alidns", url: "https://223.5.5.5/dns-query", serverName: "dns.alidns.com", via: .direct),
        ],
        defaultServers: ["cloudflare"],
        rules: [
            DnsSettings.Rule(domainSuffix: ["cn"], server: "alidns"),
            DnsSettings.Rule(ruleSet: "geosite-cn", server: "alidns"),
        ]
    )

    func testEncodesTheEngineSection() throws {
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(split)) as? [String: Any])
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
        let settings = DnsSettings(defaultServers: ["gone"], fallbackServer: "gone", hosts: ["nas.lan": ["192.168.1.10"]], rules: [DnsSettings.Rule(domain: ["a.example"], server: "gone")])
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(settings)) as? [String: Any])
        XCTAssertEqual(Set(json.keys), ["hosts"])
        XCTAssertNil(DnsSettings.fromProfileValue(try DnsSettings().profileValue()))
        XCTAssertEqual(DnsSettings.fromProfileValue(try settings.profileValue())?.hosts, settings.hosts)
    }

    func testChecksServers() {
        for url in ["1.1.1.1", "1.1.1.1:53", "tcp://dns.example", "tls://9.9.9.9", "https://dns.google/dns-query", "https://[2606:4700::1111]/dns-query", "2606:4700::1111", "udp://1.1.1.1", "quic://dns.alidns.com", "quic://223.5.5.5:853", "system"] {
            XCTAssertTrue(DnsSettings.isUpstream(url), url)
        }
        for url in ["dns.google", "ftp://1.1.1.1", "quic://1.1.1.1/dns-query", "System", "system://1.1.1.1", "tls://1.1.1.1/path", "https://", "1.1.1.1:0", "https://a b/dns-query"] {
            XCTAssertFalse(DnsSettings.isUpstream(url), url)
        }
        XCTAssertThrowsError(try DnsSettings.check(DnsSettings.Server(name: "a b", url: "1.1.1.1"))) { error in
            XCTAssertEqual(error as? DnsSettingsError, .invalidName)
        }
    }

    func testRenamingAServerKeepsItsRulesAndDefault() throws {
        var settings = try DnsSettings().saving(DnsSettings.Server(name: "home", url: "192.168.1.1", via: .direct))
        settings = try settings.addingRule("lan\nDOMAIN,router.example", server: "home")
        settings = try settings.settingDefault("home").settingFallback("home")
        let renamed = try settings.saving(DnsSettings.Server(name: "router", url: "192.168.1.1", via: .direct), replacing: "home")
        XCTAssertEqual(renamed.rules.first?.server, "router")
        XCTAssertEqual(renamed.defaultServers, ["router"])
        XCTAssertEqual(renamed.fallbackServer, "router")
        XCTAssertEqual(renamed.rules.first?.domainSuffix, ["lan"])
        XCTAssertEqual(renamed.rules.first?.domain, ["router.example"])

        XCTAssertThrowsError(try renamed.saving(DnsSettings.Server(name: "router", url: "1.1.1.1"))) { error in
            XCTAssertEqual(error as? DnsSettingsError, .duplicateName)
        }
        let removed = renamed.removingServer("router")
        XCTAssertTrue(removed.isEmpty)
        XCTAssertTrue(removed.rules.isEmpty)
        XCTAssertTrue(removed.defaultServers.isEmpty)
        XCTAssertNil(removed.fallbackServer)
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

    /// The engine refuses the whole config when the DNS rules list more than 4096 domains, suffixes and keywords in
    /// all; rule sets do not count. Adding over it is refused with the reason, and settings saved over it by an
    /// earlier version reach the tunnel with their last rules left out.
    func testDnsRulesStayWithinTheEnginesDomainLimit() throws {
        XCTAssertEqual(DnsSettings.maxRuleDomains, 4096)
        func lines(_ count: Int, from start: Int = 0) -> String {
            (start..<(start + count)).map { "DOMAIN-SUFFIX,host-\($0).example" }.joined(separator: "\n")
        }
        let base = split
        XCTAssertEqual(base.ruleDomainCount, 1)

        // Exactly at the limit is fine, one more is refused, and a refusal changes nothing.
        let full = try base.addingRule(lines(4095), server: "alidns")
        XCTAssertEqual(full.ruleDomainCount, 4096)
        XCTAssertThrowsError(try full.addingRule("DOMAIN,one-more.example", server: "alidns")) { error in
            XCTAssertEqual(error as? DnsSettingsError, .tooManyDomains)
        }
        XCTAssertThrowsError(try base.addingRule(lines(4096), server: "alidns")) { error in
            XCTAssertEqual(error as? DnsSettingsError, .tooManyDomains)
        }
        XCTAssertThrowsError(try full.addingRule("DOMAIN-KEYWORD,ads", server: "alidns"))
        // Rule sets do not count.
        let withSet = try full.addingRule("GEOSITE,cn", server: "cloudflare")
        XCTAssertEqual(withSet.ruleDomainCount, 4096)
        XCTAssertEqual(withSet.rules.count, full.rules.count + 1)

        // Over the limit, as an earlier version could save it: the tunnel keeps the rules that fit, in order.
        var over = full
        over.rules.insert(DnsSettings.Rule(domain: ["early.example"], server: "alidns"), at: 0)
        over.rules.append(DnsSettings.Rule(ruleSet: "geosite-cn", server: "cloudflare"))
        XCTAssertEqual(over.ruleDomainCount, 4097)
        let limited = over.withinDomainLimit()
        XCTAssertEqual(limited.droppedRules, 1)
        XCTAssertEqual(limited.settings.ruleDomainCount, 2)
        XCTAssertEqual(limited.settings.rules.first?.domain, ["early.example"])
        XCTAssertEqual(limited.settings.rules.last?.ruleSet, "geosite-cn")
        XCTAssertEqual(full.withinDomainLimit().settings, full)
        XCTAssertEqual(full.withinDomainLimit().droppedRules, 0)
    }

    func testRuntimeConfigCarriesDnsAndItsRuleSets() throws {
        let routing = EmbeddedRuntimeConfig.Routing(outbounds: ["a": [:]], rules: ["FINAL,a"], groups: [])
        let config = EmbeddedRuntimeConfig(
            outbound: .shadowsocks(server: "198.51.100.1", port: 8388, method: "2022-blake3-aes-128-gcm", password: "secret"),
            profileID: "test"
        )
        .withRouting(routing)
        .withDns(split)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: config.encoded()) as? [String: Any])
        let dns = try XCTUnwrap(json["dns"] as? [String: Any])
        XCTAssertEqual(dns["default"] as? String, "cloudflare")
        let sets = try XCTUnwrap((json["routing"] as? [String: Any])?["rule_sets"] as? [String: Any])
        XCTAssertNotNil(sets["geosite-cn"])
        XCTAssertNil(config.withoutSecret().dns)
        XCTAssertNil(config.withDns(DnsSettings()).dns)
    }

    func testFakeIPIsWrittenOnlyWhenOn() throws {
        let off = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(split)) as? [String: Any])
        XCTAssertNil(off["fake_ip"])
        let on = try DnsSettings().settingFakeIP(true)
        XCTAssertFalse(on.isEmpty)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(on)) as? [String: Any])
        XCTAssertEqual(Set(json.keys), ["fake_ip"])
        XCTAssertEqual((json["fake_ip"] as? [String: Any])?["enabled"] as? Bool, true)
        XCTAssertEqual((json["fake_ip"] as? [String: Any])?["range6"] as? String, "fc00::/18")
        XCTAssertEqual(DnsSettings.fromProfileValue(try on.profileValue())?.fakeIP, true)
        XCTAssertNil(DnsSettings.fromProfileValue(try on.settingFakeIP(false).profileValue()))
    }

    func testFakeIPRefusesHostsInItsRange() throws {
        XCTAssertTrue(DnsSettings.isFakeIPAddress("198.18.0.2"))
        XCTAssertTrue(DnsSettings.isFakeIPAddress("198.19.255.255"))
        XCTAssertFalse(DnsSettings.isFakeIPAddress("198.20.0.1"))
        XCTAssertFalse(DnsSettings.isFakeIPAddress("fd00::1"))
        XCTAssertTrue(DnsSettings.isFakeIPAddress("fc00::1"))
        XCTAssertTrue(DnsSettings.isFakeIPAddress("fc00:3fff::1"))
        XCTAssertFalse(DnsSettings.isFakeIPAddress("fc00:4000::1"))
        XCTAssertThrowsError(try DnsSettings().settingFakeIP(true).settingHost("nas.lan", addresses: "fc00::9")) { error in
            XCTAssertEqual(error as? DnsSettingsError, .fakeIPRange)
        }
        let withHost = try DnsSettings().settingHost("nas.lan", addresses: "198.18.5.5")
        XCTAssertThrowsError(try withHost.settingFakeIP(true)) { error in
            XCTAssertEqual(error as? DnsSettingsError, .fakeIPRange)
        }
        let on = try DnsSettings().settingFakeIP(true)
        XCTAssertThrowsError(try on.settingHost("nas.lan", addresses: "198.19.0.9")) { error in
            XCTAssertEqual(error as? DnsSettingsError, .fakeIPRange)
        }
        XCTAssertEqual(try on.settingHost("nas.lan", addresses: "192.168.1.2").hosts["nas.lan"], ["192.168.1.2"])
    }

    func testDefaultServersRaceAndFallBack() throws {
        var settings = DnsSettings()
        for (name, url) in [("a", "udp://10.0.0.1"), ("b", "quic://223.5.5.5"), ("c", "tls://9.9.9.9"), ("d", "1.1.1.1"), ("e", "8.8.8.8")] {
            settings = try settings.saving(DnsSettings.Server(name: name, url: url))
        }
        settings = try settings.saving(DnsSettings.Server(name: "local", url: "system", serverName: "ignored.example", via: .proxy))
        let local = try XCTUnwrap(settings.servers.last)
        XCTAssertEqual(local.via, .direct)
        XCTAssertNil(local.serverName)
        XCTAssertEqual(local.kind, String(localized: "系统"))
        XCTAssertEqual(settings.servers.first?.kind, "UDP")
        XCTAssertEqual(settings.servers[1].kind, "DoQ")

        // Without a choice the first server is the default; racing adds to it, up to four.
        XCTAssertEqual(settings.effectiveDefaults, ["a"])
        settings = try settings.settingRacing("b", true).settingRacing("c", true).settingRacing("d", true)
        XCTAssertEqual(settings.defaultServers, ["a", "b", "c", "d"])
        XCTAssertThrowsError(try settings.settingRacing("e", true)) { error in
            XCTAssertEqual(error as? DnsSettingsError, .tooManyDefaults)
        }
        settings = try settings.settingFallback("local")
        var json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(settings)) as? [String: Any])
        XCTAssertEqual(json["default"] as? [String], ["a", "b", "c", "d"])
        XCTAssertEqual(json["fallback"] as? String, "local")
        let servers = try XCTUnwrap(json["servers"] as? [[String: Any]])
        XCTAssertEqual(servers.last?["url"] as? String, "system")
        XCTAssertEqual(DnsSettings.fromProfileValue(try settings.profileValue()), settings)

        // The last default stays; one left is written as a plain name.
        settings = try settings.settingRacing("b", false).settingRacing("c", false).settingRacing("d", false).settingRacing("a", false)
        XCTAssertEqual(settings.defaultServers, ["a"])
        json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(settings)) as? [String: Any])
        XCTAssertEqual(json["default"] as? String, "a")
        XCTAssertThrowsError(try settings.settingFallback("missing"))
        XCTAssertNil(try settings.settingFallback(nil).fallbackServer)
        XCTAssertEqual(try settings.settingDefault("e").effectiveDefaults, ["e"])
    }
}
