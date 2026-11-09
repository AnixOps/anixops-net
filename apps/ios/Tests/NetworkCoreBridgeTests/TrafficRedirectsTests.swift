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
