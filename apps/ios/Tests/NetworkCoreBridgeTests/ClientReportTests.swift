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

    // MARK: - The setting

    private func freshDefaults(file: StaticString = #filePath, line: UInt = #line) throws -> UserDefaults {
        let name = "ClientReportTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: name), file: file, line: line)
        addTeardownBlock { UserDefaults(suiteName: name)?.removePersistentDomain(forName: name) }
        return defaults
    }

    private func health(okA: UInt64, okB: UInt64 = 7) throws -> OutboundHealth {
        let json = """
        {"outbounds":[
         {"node":"a","protocol":"anx","health":{"counts":{"ok":\(okA)},"consecutive_fallbacks":0,"stopped":false,
          "clock_skew_secs":null,"hint":null}},
         {"node":"b","protocol":"anx","health":{"counts":{"ok":\(okB)},"consecutive_fallbacks":0,"stopped":false,
          "clock_skew_secs":null,"hint":null}}]}
        """
        return try XCTUnwrap(RuntimeControl.decode(OutboundHealth.self, from: Data(json.utf8)))
    }

    func testReportIsOffByDefaultAndSendsNothing() throws {
        let defaults = try freshDefaults()
        let store = ClientReportStore(defaults: defaults)
        XCTAssertFalse(store.isEnabled)
        let start = Date(timeIntervalSince1970: 1_000_000)
        let planner = ClientReportPlanner(store: store, now: start, interval: { 3600 })
        let groups = try groups(url: "https://panel.example.com/s/tok9")
        for hours in [0.0, 1, 2, 30] {
            let step = planner.next(health: try health(okA: 100), groups: groups, network: "wifi", client: "c",
                                    now: start.addingTimeInterval(hours * 3600))
            XCTAssertEqual(step, .wait)
        }
        XCTAssertNil(defaults.object(forKey: ClientReportStore.snapshotKey), "nothing is kept while the report is off")
        XCTAssertNil(defaults.object(forKey: ClientReportStore.lastReportKey))
    }

    func testStateOfAnEarlierVersionIsDeletedWhileOff() throws {
        let defaults = try freshDefaults()
        // An earlier version reported without a setting and kept these.
        defaults.set(try JSONEncoder().encode(["a": OutboundHealth.Counts(ok: 3)]), forKey: ClientReportStore.snapshotKey)
        defaults.set(Date(timeIntervalSince1970: 5), forKey: ClientReportStore.lastReportKey)
        ClientReportStore(defaults: defaults).forgetIfDisabled()
        XCTAssertNil(defaults.object(forKey: ClientReportStore.snapshotKey))
        XCTAssertNil(defaults.object(forKey: ClientReportStore.lastReportKey))

        // While on, the state stays.
        let store = ClientReportStore(defaults: defaults)
        store.setEnabled(true, now: Date(timeIntervalSince1970: 10))
        store.forgetIfDisabled()
        XCTAssertEqual(store.lastReport, Date(timeIntervalSince1970: 10))
    }

    func testTurningOnStartsFromTheCurrentCounts() throws {
        let defaults = try freshDefaults()
        let store = ClientReportStore(defaults: defaults)
        let groups = try groups(url: "https://panel.example.com/s/tok9")
        let endpoint = try XCTUnwrap(URL(string: "https://panel.example.com/s/tok9/report"))
        let enabledAt = Date(timeIntervalSince1970: 2_000_000)
        store.setEnabled(true, now: enabledAt)
        XCTAssertTrue(store.isEnabled)
        XCTAssertNil(store.snapshot)
        XCTAssertEqual(store.lastReport, enabledAt)

        let planner = ClientReportPlanner(store: store, now: enabledAt, interval: { 3600 })
        // The counts from before the user turned the report on only become the baseline.
        XCTAssertEqual(
            planner.next(health: try health(okA: 500), groups: groups, network: "wifi", client: "c",
                         now: enabledAt.addingTimeInterval(5)),
            .baseline
        )
        XCTAssertEqual(store.snapshot?["a"], OutboundHealth.Counts(ok: 500))
        // Not due yet.
        XCTAssertEqual(
            planner.next(health: try health(okA: 510), groups: groups, network: "wifi", client: "c",
                         now: enabledAt.addingTimeInterval(1800)),
            .wait
        )
        // Due: only what happened since, over the window since it was turned on.
        let now = enabledAt.addingTimeInterval(3600)
        guard case let .send(bodies, counts) = planner.next(
            health: try health(okA: 520), groups: groups, network: "cellular", client: "c", platform: "macos", now: now
        ) else {
            return XCTFail("a report is due")
        }
        let body = try XCTUnwrap(bodies[endpoint])
        XCTAssertEqual(body.nodes.map(\.node), ["Tokyo"], "b has nothing new")
        XCTAssertEqual(body.nodes.first?.counts, OutboundHealth.Counts(ok: 20))
        XCTAssertEqual(body.windowSecs, 3600)
        XCTAssertEqual(body.platform, "macos")
        XCTAssertEqual(body.network, "cellular")
        planner.delivered(counts, at: now)
        XCTAssertEqual(store.snapshot?["a"], OutboundHealth.Counts(ok: 520))
        XCTAssertEqual(store.lastReport, now)
    }

    func testTurningOffDeletesTheStateAndOnAgainDoesNotBackFill() throws {
        let defaults = try freshDefaults()
        let store = ClientReportStore(defaults: defaults)
        let groups = try groups(url: "https://panel.example.com/s/tok9")
        let start = Date(timeIntervalSince1970: 3_000_000)
        store.setEnabled(true, now: start)
        let first = ClientReportPlanner(store: store, now: start, interval: { 3600 })
        XCTAssertEqual(first.next(health: try health(okA: 10), groups: groups, network: "wifi", client: "c", now: start), .baseline)

        store.setEnabled(false, now: start.addingTimeInterval(60))
        XCTAssertFalse(store.isEnabled)
        XCTAssertNil(defaults.object(forKey: ClientReportStore.snapshotKey))
        XCTAssertNil(defaults.object(forKey: ClientReportStore.lastReportKey))
        // A planner made before the switch stops too, and a report in flight no longer saves anything.
        XCTAssertEqual(
            first.next(health: try health(okA: 99), groups: groups, network: "wifi", client: "c",
                       now: start.addingTimeInterval(7200)),
            .wait
        )
        first.delivered(["a": OutboundHealth.Counts(ok: 99)], at: start.addingTimeInterval(7200))
        XCTAssertNil(defaults.object(forKey: ClientReportStore.snapshotKey))

        // Ten hours off, then on again: the counts from the off period are never sent.
        let again = start.addingTimeInterval(36_000)
        store.setEnabled(true, now: again)
        let second = ClientReportPlanner(store: store, now: again, interval: { 3600 })
        XCTAssertEqual(
            second.next(health: try health(okA: 900), groups: groups, network: "wifi", client: "c",
                        now: again.addingTimeInterval(1)),
            .baseline
        )
        guard case let .send(bodies, _) = second.next(
            health: try health(okA: 905), groups: groups, network: "wifi", client: "c", now: again.addingTimeInterval(3600)
        ) else {
            return XCTFail("a report is due")
        }
        let body = try XCTUnwrap(bodies.values.first)
        XCTAssertEqual(body.nodes.first?.counts, OutboundHealth.Counts(ok: 5))
        XCTAssertEqual(body.windowSecs, 3600)
    }

    func testFailedReportRetriesAfterThePanelMinimum() throws {
        let defaults = try freshDefaults()
        let store = ClientReportStore(defaults: defaults)
        let groups = try groups(url: "https://panel.example.com/s/tok9")
        let start = Date(timeIntervalSince1970: 4_000_000)
        store.setEnabled(true, now: start)
        let planner = ClientReportPlanner(store: store, now: start, interval: { 3600 })
        _ = planner.next(health: try health(okA: 1), groups: groups, network: "wifi", client: "c", now: start)
        let due = start.addingTimeInterval(3600)
        guard case .send = planner.next(health: try health(okA: 2), groups: groups, network: "wifi", client: "c", now: due) else {
            return XCTFail("a report is due")
        }
        planner.failed(at: due)
        XCTAssertEqual(store.lastReport, start, "a failed report keeps the window")
        XCTAssertEqual(
            planner.next(health: try health(okA: 2), groups: groups, network: "wifi", client: "c",
                         now: due.addingTimeInterval(ClientReport.minimumInterval - 1)),
            .wait
        )
        guard case let .send(bodies, _) = planner.next(
            health: try health(okA: 3), groups: groups, network: "wifi", client: "c",
            now: due.addingTimeInterval(ClientReport.minimumInterval)
        ) else {
            return XCTFail("retried after the panel's minimum")
        }
        XCTAssertEqual(bodies.values.first?.nodes.first?.counts, OutboundHealth.Counts(ok: 2))
    }

    func testPlatformNamesTheSystem() {
        #if os(macOS)
        XCTAssertEqual(ClientReport.platform, "macos")
        #else
        XCTAssertEqual(ClientReport.platform, "ios")
        #endif
    }
}
