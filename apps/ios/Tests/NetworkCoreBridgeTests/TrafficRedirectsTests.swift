import Foundation
import NetworkCoreBridge
import XCTest

final class TrafficRedirectsTests: XCTestCase {
    func testFieldsBecomeEngineRules() throws {
        XCTAssertEqual(try TrafficRedirects.match(source: "203.0.113.10", ports: "", network: .any), "IP-CIDR,203.0.113.10/32")
        XCTAssertEqual(try TrafficRedirects.match(source: "203.0.113.0/24", ports: "", network: .any), "IP-CIDR,203.0.113.0/24")
        XCTAssertEqual(try TrafficRedirects.match(source: "fd00::5", ports: "", network: .any), "IP-CIDR6,fd00::5/128")
        XCTAssertEqual(try TrafficRedirects.match(source: "Game.Example.com", ports: "", network: .any), "DOMAIN-SUFFIX,game.example.com")
        XCTAssertEqual(
            try TrafficRedirects.match(source: "203.0.113.10", ports: "7000-7100", network: .udp),
            "AND,((IP-CIDR,203.0.113.10/32),(DST-PORT,7000-7100),(PROTOCOL,UDP))"
        )
        XCTAssertEqual(try TrafficRedirects.match(source: "", ports: "9000", network: .any), "DST-PORT,9000")
        XCTAssertEqual(
            try TrafficRedirects.match(source: "DOMAIN,login.example.com", ports: "", network: .tcp),
            "AND,((DOMAIN,login.example.com),(PROTOCOL,TCP))"
        )
    }

    func testRefusesWhatTheEngineWouldRefuse() {
        let sources = [
            "GEOIP,CN",
            "RULE-SET,games",
            "IP-CIDR,203.0.113.10/32,10.8.0.5",
            "AND,((DOMAIN,a.example)",
            "203.0.113.10/33",
            "not a host",
        ]
        for source in sources {
            XCTAssertThrowsError(try TrafficRedirects.match(source: source, ports: "", network: .any), source) { error in
                XCTAssertEqual(error as? TrafficRedirectsError, .invalidSource)
            }
        }
        for ports in ["0", "70000", "9000-8000", "a"] {
            XCTAssertThrowsError(try TrafficRedirects.match(source: "", ports: ports, network: .any), ports) { error in
                XCTAssertEqual(error as? TrafficRedirectsError, .invalidPort)
            }
        }
        XCTAssertThrowsError(try TrafficRedirects.match(source: "", ports: "", network: .any)) { error in
            XCTAssertEqual(error as? TrafficRedirectsError, .emptyMatch)
        }
    }

    /// Typed rules are checked as the engine parses them (config-core `parse_redirect_line`), down to each value and
    /// each condition inside `AND`, `OR` and `NOT`: one line the engine refuses would refuse the whole config.
    func testTypedRulesAreCheckedAsTheEngineParsesThem() throws {
        // `AND,((AND,((… (DOMAIN,a.com) …))))` with `levels` logical rules in all.
        func nested(_ levels: Int) -> String {
            var condition = "(DOMAIN,a.com)"
            for _ in 1..<levels {
                condition = "(AND,(\(condition)))"
            }
            return "AND,(\(condition))"
        }
        XCTAssertNoThrow(try TrafficRedirects.match(source: nested(8), ports: "", network: .any))
        XCTAssertThrowsError(try TrafficRedirects.match(source: nested(9), ports: "", network: .any))
        let refused = [
            "IP-CIDR,10.0.0.0/33",
            "IP-CIDR,203.0.113.10",
            "IP-CIDR6,fd00::/129",
            "DST-PORT,abc",
            "DST-PORT,80/443",
            "DST-PORT,9000-8000",
            "PROTOCOL,FOO",
            "DOMAIN-SUFFIX,*.example.com",
            "DOMAIN,..example.com",
            "DOMAIN-KEYWORD,.ads",
            "DOMAIN-WILDCARD,*",
            "AND,((GEOIP,CN),(DST-PORT,443))",
            "OR,((RULE-SET,x),(DOMAIN,a.com))",
            "AND,((DOMAIN,a.com),(GEOSITE,cn))",
            "NOT,((DOMAIN,a.com),(DOMAIN,b.com))",
            "AND,(DOMAIN,a.com)",
            "AND,((DOMAIN,a.com)),(PROTOCOL,UDP)",
            "AND,((DOMAIN,a.com) (PROTOCOL,UDP))",
            "AND,((DOMAIN,a.com),(DST-PORT,0-70000))",
            "AND,()",
        ]
        for source in refused {
            XCTAssertThrowsError(try TrafficRedirects.match(source: source, ports: "", network: .any), source) { error in
                XCTAssertEqual(error as? TrafficRedirectsError, .invalidSource, source)
            }
        }

        let accepted: [(String, String)] = [
            ("domain-suffix,Game.Example.com", "domain-suffix,Game.Example.com"),
            ("DEST-PORT,7000-7100", "DEST-PORT,7000-7100"),
            ("DOMAIN-WILDCARD,img-?.example.*", "DOMAIN-WILDCARD,img-?.example.*"),
            ("AND,((OR,((DOMAIN,a.example),(DOMAIN-SUFFIX,b.example))),(NOT,((PROTOCOL,quic))))",
             "AND,((OR,((DOMAIN,a.example),(DOMAIN-SUFFIX,b.example))),(NOT,((PROTOCOL,quic))))"),
            ("AND,((IP-CIDR,203.0.113.0/24,no-resolve),(DST-PORT,443))", "AND,((IP-CIDR,203.0.113.0/24,no-resolve),(DST-PORT,443))"),
        ]
        for (source, match) in accepted {
            XCTAssertEqual(try TrafficRedirects.match(source: source, ports: "", network: .any), match, source)
        }
        // A typed logical rule wrapped with the port and protocol fields stays one the engine takes.
        let wrapped = try TrafficRedirects.match(source: "OR,((DOMAIN,a.example),(DOMAIN,b.example))", ports: "443", network: .udp)
        XCTAssertEqual(wrapped, "AND,((OR,((DOMAIN,a.example),(DOMAIN,b.example))),(DST-PORT,443),(PROTOCOL,UDP))")
        XCTAssertTrue(TrafficRedirects.isValidLine(wrapped + ",10.8.0.5:7000"))
    }

    /// Entries an earlier version saved without the full check never reach the tunnel: the profile and the extension
    /// leave them out instead of letting the engine refuse the whole config, and the list flags them.
    func testInvalidSavedEntriesNeverReachTheTunnel() throws {
        let saved = Data(#"""
        {"entries":[
        {"id":"6F9619FF-8B86-D011-B42D-00C04FC964FF","match":"IP-CIDR,10.0.0.0/33","target":"10.8.0.5","enabled":true},
        {"id":"7F9619FF-8B86-D011-B42D-00C04FC964FF","match":"AND,((GEOIP,CN),(DST-PORT,443))","target":"10.8.0.5","enabled":true},
        {"id":"8F9619FF-8B86-D011-B42D-00C04FC964FF","match":"DST-PORT,9000","target":"dev.example.com","enabled":true}]}
        """#.utf8)
        let redirects = try JSONDecoder().decode(TrafficRedirects.self, from: saved)
        XCTAssertEqual(redirects.invalidEntries.count, 2)
        XCTAssertFalse(redirects.entries[0].isValid)
        XCTAssertTrue(redirects.entries[2].isValid)
        XCTAssertEqual(redirects.lines, ["DST-PORT,9000,dev.example.com"])
        let value = try XCTUnwrap(redirects.profileValue())
        XCTAssertEqual(TrafficRedirects.lines(fromProfileValue: value), ["DST-PORT,9000,dev.example.com"])

        // The extension filters a profile written by an earlier version the same way.
        let old = [
            "IP-CIDR,10.0.0.0/33,10.8.0.5",
            "DST-PORT,9000,dev.example.com",
            "DOMAIN,a.example,not a target",
            "FINAL,10.8.0.5",
            "AND,((DOMAIN,a.example),(PROTOCOL,UDP)),[fd00::5]:7000",
        ]
        XCTAssertEqual(TrafficRedirects.validLines(old), [
            "DST-PORT,9000,dev.example.com",
            "AND,((DOMAIN,a.example),(PROTOCOL,UDP)),[fd00::5]:7000",
        ])
    }

    func testTargets() throws {
        for target in ["10.8.0.5", "10.8.0.5:7000", "fd00::5", "[fd00::5]", "[fd00::5]:7000", ":9000", "dev.example.com:9000"] {
            XCTAssertEqual(try TrafficRedirects.target(target), target)
        }
        XCTAssertEqual(try TrafficRedirects.target(" Dev.Example.com "), "dev.example.com")
        for target in ["", ":0", "10.8.0.5:", "10.8.0.5:70000", "[10.8.0.5]:7000", "[fd00::5", "a b", "dev.example.com:0"] {
            XCTAssertThrowsError(try TrafficRedirects.target(target), target) { error in
                XCTAssertEqual(error as? TrafficRedirectsError, .invalidTarget)
            }
        }
    }

    func testOnlyEnabledEntriesReachTheProfile() throws {
        var redirects = try TrafficRedirects()
            .adding(source: "203.0.113.10", ports: "", network: .udp, target: "10.8.0.5:7777")
            .adding(source: "game.example.com", ports: "", network: .any, target: "10.8.0.5")
        XCTAssertEqual(redirects.lines, [
            "AND,((IP-CIDR,203.0.113.10/32),(PROTOCOL,UDP)),10.8.0.5:7777",
            "DOMAIN-SUFFIX,game.example.com,10.8.0.5",
        ])
        redirects.entries[0].enabled = false
        let value = try XCTUnwrap(redirects.profileValue())
        XCTAssertEqual(TrafficRedirects.lines(fromProfileValue: value), ["DOMAIN-SUFFIX,game.example.com,10.8.0.5"])

        redirects.entries[1].enabled = false
        XCTAssertNil(try redirects.profileValue())
        XCTAssertEqual(TrafficRedirects.lines(fromProfileValue: nil), [])

        let decoded = try JSONDecoder().decode(TrafficRedirects.self, from: JSONEncoder().encode(redirects))
        XCTAssertEqual(decoded, redirects)
    }

    func testEditingKeepsTheEntryAndItsFields() throws {
        let redirects = try TrafficRedirects()
            .adding(source: "203.0.113.10", ports: "7000-7100", network: .udp, target: "10.8.0.5")
        let entry = try XCTUnwrap(redirects.entries.first)
        let fields = try XCTUnwrap(TrafficRedirects.fields(of: entry))
        XCTAssertEqual(fields.source, "203.0.113.10")
        XCTAssertEqual(fields.ports, "7000-7100")
        XCTAssertEqual(fields.network, .udp)

        let edited = try redirects.adding(source: "203.0.113.0/24", ports: "", network: .any, target: ":9000", replacing: entry.id)
        XCTAssertEqual(edited.entries.count, 1)
        XCTAssertEqual(edited.entries[0].id, entry.id)
        XCTAssertEqual(edited.lines, ["IP-CIDR,203.0.113.0/24,:9000"])

        let typed = TrafficRedirects.Entry(match: "DOMAIN,login.example.com", target: "10.8.0.5")
        XCTAssertNil(TrafficRedirects.fields(of: typed))
    }

    func testTheRuntimeConfigCarriesTheLines() throws {
        let node = ShadowsocksNode(host: "203.0.113.1", port: 8388, method: "2022-blake3-aes-128-gcm", password: "key", name: nil)
        let config = EmbeddedRuntimeConfig(node: node, profileID: "test")
        let plain = try XCTUnwrap(JSONSerialization.jsonObject(with: config.withRedirects([]).encoded()) as? [String: Any])
        XCTAssertNil(plain["redirects"])
        let redirected = config.withRedirects(["IP-CIDR,203.0.113.10/32,10.8.0.5"])
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: redirected.encoded()) as? [String: Any])
        XCTAssertEqual(json["redirects"] as? [String], ["IP-CIDR,203.0.113.10/32,10.8.0.5"])
        XCTAssertNil(redirected.withoutSecret().redirects)
    }

    func testConnectionsShowTheRewrite() throws {
        let json = #"{"id":1,"network":"udp","host":null,"destination":"203.0.113.10:7000","redirect":{"destination":"10.8.0.5:7000","rule":"IP-CIDR,203.0.113.10/32,10.8.0.5"},"rule":"FINAL","chain":["DIRECT"],"outbound":"DIRECT","started_at_ms":1700000000000,"ended_at_ms":null,"upload":0,"download":0,"error":null}"#
        let connection = try JSONDecoder().decode(RuntimeControl.Connection.self, from: Data(json.utf8))
        XCTAssertEqual(connection.destination, "203.0.113.10:7000")
        XCTAssertEqual(connection.redirect?.destination, "10.8.0.5:7000")
        XCTAssertTrue(connection.matches("10.8.0.5"))
    }
}
