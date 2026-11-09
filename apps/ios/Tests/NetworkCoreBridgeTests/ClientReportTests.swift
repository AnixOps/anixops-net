import Foundation
import NetworkCoreBridge
import XCTest

final class ClientReportTests: XCTestCase {
    private let healthJSON = """
    {"outbounds":[
     {"node":"a","protocol":"anx","health":{"counts":{"ok":5,"fallback":3,"timeout":1,"refused":0,"reset":2,
      "after_handshake":0,"keepalive_lost":1,"other":0},"consecutive_fallbacks":3,"stopped":true,
      "clock_skew_secs":-312,"hint":"clock"}},
     {"node":"b","protocol":"anx","health":{"counts":{"ok":7},"consecutive_fallbacks":0,"stopped":false,
      "clock_skew_secs":null,"hint":null}},
     {"node":"t","protocol":"trojan","health":{"counts":{"ok":1,"timeout":4},"consecutive_fallbacks":0,
      "stopped":false,"clock_skew_secs":null,"hint":null}}]}
    """

    private let report = """
    {"nodes":[
    {"id":"a","name":"Tokyo","protocol":"anx","server":"a.example","port":443,"outbound":{"type":"anx"}},
    {"id":"b","name":"","protocol":"anx","server":"b.example","port":8443,"outbound":{"type":"anx"}},
    {"id":"t","name":"Other","protocol":"trojan","server":"t.example","port":443,"outbound":{"type":"trojan"}}],
    "unsupported":[]}
    """

    private func groups(url: String) throws -> SubscriptionGroups {
        let imported = try NativeImportReport.decoded(from: Data(report.utf8))
        let group = SubscriptionGroup(
            id: "g1",
            name: "panel",
            library: NodeLibrary(report: imported, subscriptionURL: url, keeping: nil)
        )
        return SubscriptionGroups(groups: [group], activeID: "g1")
    }

    func testRequestsAndAnswerDecode() throws {
        XCTAssertEqual(String(decoding: RuntimeControl.outboundHealth.request, as: UTF8.self), #"{"cmd":"outbound_health"}"#)
        XCTAssertEqual(
            String(decoding: RuntimeControl.resumeOutbound(node: "a").request, as: UTF8.self),
            #"{"cmd":"resume_outbound","node":"a"}"#
        )
        let health = try XCTUnwrap(RuntimeControl.decode(OutboundHealth.self, from: Data(healthJSON.utf8)))
        XCTAssertEqual(health.outbounds.count, 3)
        XCTAssertEqual(health.stopped.map(\.node), ["a"])
        XCTAssertEqual(health.outbounds[0].health.counts.afterHandshake, 0)
        XCTAssertEqual(health.outbounds[0].health.counts.keepaliveLost, 1)
        XCTAssertEqual(health.outbounds[0].health.clockSkewSecs, -312)
        // Missing counters read as zero.
        XCTAssertEqual(health.outbounds[1].health.counts.fallback, 0)
    }

    func testEndpointIsTheSubscriptionPathPlusReport() {
        XCTAssertEqual(
            ClientReport.endpoint(forSubscription: "https://panel.example.com/s/abc123")?.absoluteString,
            "https://panel.example.com/s/abc123/report"
        )
        XCTAssertEqual(
            ClientReport.endpoint(forSubscription: "https://panel.example.com:8443/sub/abc123.yaml?flag=clash#x")?.absoluteString,
            "https://panel.example.com:8443/sub/abc123/report"
        )
        XCTAssertNil(ClientReport.endpoint(forSubscription: "https://panel.example.com/"))
        XCTAssertNil(ClientReport.endpoint(forSubscription: "ftp://panel.example.com/s/abc"))
        XCTAssertNil(ClientReport.endpoint(forSubscription: "not a link"))
    }

    func testCountsSinceTheLastReport() {
        let now = OutboundHealth.Counts(ok: 10, fallback: 3)
        XCTAssertEqual(now.since(nil), now)
        XCTAssertEqual(now.since(OutboundHealth.Counts(ok: 4, fallback: 3)), OutboundHealth.Counts(ok: 6))
        // The tunnel restarted: counters went down, so everything now is new.
        XCTAssertEqual(now.since(OutboundHealth.Counts(ok: 20)), now)
        XCTAssertTrue(OutboundHealth.Counts().isZero)
    }

    func testBodiesGroupAnxNodesByPanelLink() throws {
        let health = try XCTUnwrap(RuntimeControl.decode(OutboundHealth.self, from: Data(healthJSON.utf8)))
        let groups = try groups(url: "https://panel.example.com/s/tok9.txt?x=1")
        let bodies = ClientReport.bodies(
            health: health,
            groups: groups,
            previous: ["b": OutboundHealth.Counts(ok: 7)],
            window: 3600,
            client: "anixops-net/test",
            platform: "ios",
            network: "wifi"
        )
        let endpoint = try XCTUnwrap(URL(string: "https://panel.example.com/s/tok9/report"))
        let body = try XCTUnwrap(bodies[endpoint])
        XCTAssertEqual(bodies.count, 1)
        // `b` has nothing new, `t` is not ANX: only `a` goes out, under its subscription name.
        XCTAssertEqual(body.nodes.map(\.node), ["Tokyo"])
        XCTAssertEqual(body.windowSecs, 3600)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: body.encoded()) as? [String: Any])
        XCTAssertEqual(json["platform"] as? String, "ios")
        XCTAssertEqual(json["window_secs"] as? Int, 3600)
        let node = try XCTUnwrap((json["nodes"] as? [[String: Any]])?.first)
        XCTAssertEqual(node["server"] as? String, "a.example:443")
        XCTAssertEqual(node["proto"] as? String, "anx")
        XCTAssertEqual(node["fallback"] as? Int, 3)
        XCTAssertEqual(node["after_handshake"] as? Int, 0)
        XCTAssertEqual(node["keepalive_lost"] as? Int, 1)
        XCTAssertEqual(node["stopped"] as? Bool, true)
        XCTAssertEqual(node["clock_skew_secs"] as? Int, -312)
        XCTAssertNil(node["counts"], "counters are flat fields")

        // Windows outside the panel's range are clamped.
        let short = ClientReport.bodies(health: health, groups: groups, previous: [:], window: 5,
                                        client: "c", platform: "ios", network: "other")
        XCTAssertEqual(short[endpoint]?.windowSecs, 60)
    }

    func testSingleOutboundAndLinkedNodesFindTheirSubscription() throws {
        let groups = try groups(url: "https://panel.example.com/s/tok9")
        XCTAssertEqual(groups.origin(ofRuntimeNode: "proxy")?.node.id, "a", "the selected node")
        XCTAssertEqual(groups.origin(ofRuntimeNode: "b")?.node.server, "b.example")
        XCTAssertNil(groups.origin(ofRuntimeNode: "zz"))
    }

    func testStoppedTextNamesTheCause() {
        let clock = OutboundHealth.Health(counts: .init(fallback: 3), stopped: true, clockSkewSecs: 300, hint: "clock")
        XCTAssertTrue(ClientReport.stoppedText(node: "Tokyo", health: clock).contains("5"))
        let credentials = OutboundHealth.Health(counts: .init(fallback: 3), stopped: true, hint: "credentials_or_clock")
        XCTAssertTrue(ClientReport.stoppedText(node: "Tokyo", health: credentials).contains("Tokyo"))
    }
}
