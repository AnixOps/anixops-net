import Foundation
import NetworkCoreBridge
import XCTest

final class SubscriptionGroupTests: XCTestCase {
    private let plainReport = """
    {"nodes":[
    {"id":"a","name":"Tokyo","protocol":"trojan","server":"a.example","port":443,
     "outbound":{"type":"trojan","server":"a.example","port":443,"password":"pa"}},
    {"id":"b","name":"","protocol":"hysteria2","server":"b.example","port":8443,
     "outbound":{"type":"hysteria2","server":"b.example","port":8443,"password":"pb"}}],
    "unsupported":[]}
    """

    private let routedReport = """
    {"nodes":[
    {"id":"a","name":"Tokyo","protocol":"trojan","server":"a.example","port":443,
     "outbound":{"type":"trojan","server":"a.example","port":443,"password":"pa"}}],
    "unsupported":[],
    "routing":{"rules":["FINAL,Proxy"],
    "groups":[{"id":"Proxy","type":"select","members":["Auto","a","DIRECT"],"default":null},
    {"id":"Auto","type":"url-test","members":["a"],"url":"http://www.gstatic.com/generate_204","interval_secs":300}]}}
    """

    private func report(_ json: String) throws -> NativeImportReport {
        try NativeImportReport.decoded(from: Data(json.utf8))
    }

    private let start = Date(timeIntervalSince1970: 1_700_000_000)

    func testUserInfoHeaderParses() throws {
        let info = try XCTUnwrap(SubscriptionUserInfo.parse(
            "upload=1073741824; download=2147483648; total=10737418240; expire=1798761600"
        ))
        XCTAssertEqual(info.used, 3_221_225_472)
        XCTAssertEqual(info.total, 10_737_418_240)
        XCTAssertEqual(info.expire, Date(timeIntervalSince1970: 1_798_761_600))
        XCTAssertEqual(try XCTUnwrap(info.usedFraction), 0.3, accuracy: 0.0001)
        // Some servers send floats, odd spacing and `expire=0` for no expiry.
        let loose = try XCTUnwrap(SubscriptionUserInfo.parse("Upload=1.0e3 ;download=0;total=0;expire=0"))
        XCTAssertEqual(loose.used, 1000)
        XCTAssertNil(loose.expire)
        XCTAssertNil(loose.usedFraction)
        XCTAssertNil(SubscriptionUserInfo.parse("nothing=1"))
        XCTAssertNil(SubscriptionUserInfo.parse(""))
    }

    func testResponseHeadersGiveTitleAndInterval() {
        let headers = [
            "subscription-userinfo": "upload=1; download=2; total=3",
            "profile-update-interval": "12",
            "content-disposition": "attachment; filename*=UTF-8''%E6%9C%BA%E5%9C%BA.yaml",
        ]
        let info = SubscriptionResponseInfo(header: { headers[$0] })
        XCTAssertEqual(info.userInfo?.total, 3)
        XCTAssertEqual(info.updateIntervalHours, 12)
        XCTAssertEqual(info.title, "机场")

        let titled = SubscriptionResponseInfo(header: { name in
            ["profile-title": "base64:5py65Zy6Qg==", "content-disposition": "attachment; filename=\"x.txt\""][name]
        })
        XCTAssertEqual(titled.title, "机场B")
        XCTAssertEqual(SubscriptionResponseInfo(header: { $0 == "content-disposition" ? "attachment; filename=\"Sub.txt\"" : nil }).title, "Sub")
        XCTAssertNil(SubscriptionResponseInfo(header: { $0 == "profile-update-interval" ? "0" : nil }).updateIntervalHours)
        XCTAssertEqual(SubscriptionResponseInfo(header: { _ in nil }), SubscriptionResponseInfo())
    }

    func testNewGroupIsNamedByTitleOrHostAndUpdatesAutomatically() throws {
        let url = "https://sub.example/api?token=t"
        let named = SubscriptionGroup(
            report: try report(plainReport),
            url: url,
            response: SubscriptionResponseInfo(updateIntervalHours: 6, title: "My"),
            at: start
        )
        XCTAssertEqual(named.name, "My")
        XCTAssertEqual(named.updateIntervalHours, 6)
        XCTAssertEqual(named.nextUpdate, start.addingTimeInterval(6 * 3600))
        XCTAssertFalse(named.isDue(at: start.addingTimeInterval(5 * 3600)))
        XCTAssertTrue(named.isDue(at: start.addingTimeInterval(6 * 3600)))

        let host = SubscriptionGroup(report: try report(plainReport), url: url, response: SubscriptionResponseInfo(), at: start)
        XCTAssertEqual(host.name, "sub.example")
        XCTAssertEqual(host.updateIntervalHours, SubscriptionGroup.defaultUpdateIntervalHours)

        var manual = host
        manual.updateIntervalHours = 0
        XCTAssertNil(manual.nextUpdate)
        let pasted = SubscriptionGroup(name: "Pasted", library: NodeLibrary(report: try report(plainReport), subscriptionURL: nil, keeping: nil))
        XCTAssertFalse(pasted.isUpdatable)
        XCTAssertNil(pasted.nextUpdate)
    }

    func testRefreshKeepsNameIntervalAndSelectionAndClearsTheFailure() throws {
        var group = SubscriptionGroup(
            report: try report(plainReport),
            url: "https://sub.example/x",
            response: SubscriptionResponseInfo(userInfo: SubscriptionUserInfo(total: 10)),
            at: start
        )
        group.name = "Renamed"
        group.updateIntervalHours = 3
        group.library.selectedID = "b"
        group.lastFailureAt = start
        let later = start.addingTimeInterval(3600)
        let refreshed = group.refreshed(with: try report(plainReport), response: SubscriptionResponseInfo(), at: later)
        XCTAssertEqual(refreshed.name, "Renamed")
        XCTAssertEqual(refreshed.updateIntervalHours, 3)
        XCTAssertEqual(refreshed.library.selectedID, "b")
        XCTAssertEqual(refreshed.library.updatedAt, later)
        XCTAssertNil(refreshed.lastFailureAt)
        // A response without the header keeps the last known traffic.
        XCTAssertEqual(refreshed.userInfo?.total, 10)
    }

    func testStoreFindsGroupsByLinkAndPicksTheNextActiveOnDelete() throws {
        let first = SubscriptionGroup(report: try report(plainReport), url: "https://one.example/s", response: .init(), at: start)
        let second = SubscriptionGroup(report: try report(plainReport), url: "https://two.example/s", response: .init(), at: start)
        var groups = SubscriptionGroups(groups: [first, second], activeID: first.id)
        XCTAssertEqual(groups.group(url: "https://two.example/s")?.id, second.id)
        XCTAssertEqual(groups.activeLibrary()?.nodes.map(\.id), ["a", "b"])
        XCTAssertEqual(groups.nextUpdate, start.addingTimeInterval(24 * 3600))
        XCTAssertEqual(groups.due(at: start.addingTimeInterval(25 * 3600)).count, 2)

        groups.remove(id: first.id)
        XCTAssertEqual(groups.activeID, second.id)
        groups.remove(id: second.id)
        XCTAssertNil(groups.activeID)
        XCTAssertNil(groups.activeLibrary())
    }

    func testOtherGroupsBecomePolicyGroupsOfTheActiveRules() throws {
        let routed = SubscriptionGroup(report: try report(routedReport), url: "https://rules.example/s", response: .init(), at: start)
        var other = SubscriptionGroup(report: try report(plainReport), url: "https://two.example/s", response: .init(), at: start)
        other.name = "Two, B"
        other.library.selectedID = "b"
        let groups = SubscriptionGroups(groups: [routed, other], activeID: routed.id)

        let library = try XCTUnwrap(groups.activeLibrary())
        XCTAssertTrue(library.routesByRules)
        // Node ids of the other group are prefixed, so its "a" does not clash with the active group's "a".
        XCTAssertEqual(library.nodes.map(\.id), ["a", "s1.a", "s1.b"])
        let routing = try XCTUnwrap(library.routing)
        let linked = try XCTUnwrap(routing.groups.last)
        XCTAssertEqual(linked.id, "订阅·Two  B")
        XCTAssertEqual(linked.type, "select")
        XCTAssertEqual(linked.members, ["s1.a", "s1.b"])
        XCTAssertEqual(linked.default, "s1.b")
        XCTAssertEqual(routing.groups.first?.members, ["Auto", "a", "DIRECT", "订阅·Two  B"])
        // url-test groups keep their own members.
        XCTAssertEqual(routing.groups[1].members, ["a"])
        XCTAssertEqual(library.displayName(ofMember: "s1.a"), "Tokyo")

        let runtime = try XCTUnwrap(library.runtimeRouting())
        XCTAssertEqual(runtime.outbounds["s1.b"]?["password"], .string("pb"))
        XCTAssertEqual(groups.linkedGroups().map(\.groupID), [other.id])

        // A plain active group runs only its own node.
        let plain = SubscriptionGroups(groups: [routed, other], activeID: other.id)
        XCTAssertEqual(plain.activeLibrary()?.nodes.map(\.id), ["a", "b"])
        XCTAssertNil(plain.activeLibrary()?.routing)
    }

    func testLinkedGroupsStayWithinTheEngineLimits() throws {
        let routed = SubscriptionGroup(report: try report(routedReport), url: "https://rules.example/s", response: .init(), at: start)
        let nodes = (0..<63).map { index in
            "{\"id\":\"n\(index)\",\"name\":\"N\(index)\",\"protocol\":\"trojan\",\"server\":\"n.example\",\"port\":443," +
                "\"outbound\":{\"type\":\"trojan\",\"server\":\"n.example\",\"port\":443,\"password\":\"p\"}}"
        }
        let big = SubscriptionGroup(
            report: try report("{\"nodes\":[\(nodes.joined(separator: ","))],\"unsupported\":[]}"),
            url: "https://big.example/s",
            response: .init(),
            at: start
        )
        let small = SubscriptionGroup(report: try report(plainReport), url: "https://small.example/s", response: .init(), at: start)
        // 1 + 63 nodes fit exactly; the small group would pass 64 and is left out.
        let groups = SubscriptionGroups(groups: [routed, big, small], activeID: routed.id)
        XCTAssertEqual(groups.linkedGroups().map(\.groupID), [big.id])
        XCTAssertEqual(groups.activeLibrary()?.nodes.count, 64)
    }

    func testLinkedNodesKeepTheirViaUnderThePrefix() throws {
        let routed = SubscriptionGroup(report: try report(routedReport), url: "https://rules.example/s", response: .init(), at: start)
        let lines = SubscriptionGroup(report: try report("""
        {"nodes":[
        {"id":"line","name":"iWAN","protocol":"iwan","server":"203.0.113.10","port":4567,
         "outbound":{"type":"iwan","server":"203.0.113.10","port":4567,"username":"u","password":"p"}},
        {"id":"land","name":"US","protocol":"trojan","server":"land.example","port":443,
         "outbound":{"type":"trojan","server":"land.example","port":443,"password":"pl","via":"line"}},
        {"id":"lost","name":"","protocol":"trojan","server":"x.example","port":443,
         "outbound":{"type":"trojan","server":"x.example","port":443,"password":"px","via":"bad id"}},
        {"id":"behind-lost","name":"","protocol":"trojan","server":"y.example","port":443,
         "outbound":{"type":"trojan","server":"y.example","port":443,"password":"py","via":"lost"}}],
        "unsupported":[]}
        """), url: "https://lines.example/s", response: .init(), at: start)
        let groups = SubscriptionGroups(groups: [routed, lines], activeID: routed.id)

        let library = try XCTUnwrap(groups.activeLibrary())
        // A node behind a node the engine cannot take is left out too, rather than connecting directly.
        XCTAssertEqual(library.nodes.map(\.id), ["a", "s1.line", "s1.land"])
        let runtime = try XCTUnwrap(library.runtimeRouting())
        XCTAssertEqual(runtime.outbounds["s1.land"]?["via"], .string("s1.line"))
        XCTAssertNil(runtime.outbounds["s1.line"]?["via"])
    }

    func testMigrationTurnsTheOldLibraryIntoTheActiveGroup() throws {
        let old = NodeLibrary(report: try report(plainReport), subscriptionURL: "https://sub.example/x", keeping: "b", at: start)
        let groups = SubscriptionGroups.migrating(old, name: "Imported")
        XCTAssertEqual(groups.groups.count, 1)
        XCTAssertEqual(groups.active?.name, "sub.example")
        XCTAssertEqual(groups.activeLibrary(), old)

        let pasted = NodeLibrary(report: try report(plainReport), subscriptionURL: nil, keeping: nil, at: start)
        let pastedGroups = SubscriptionGroups.migrating(pasted, name: "Imported")
        XCTAssertEqual(pastedGroups.active?.name, "Imported")
        XCTAssertEqual(pastedGroups.active?.updateIntervalHours, 0)
        XCTAssertEqual(SubscriptionGroups.migrating(nil, name: "Imported"), SubscriptionGroups())
    }

    func testKeychainValueRoundTrips() throws {
        let group = SubscriptionGroup(
            report: try report(plainReport),
            url: "https://sub.example/x",
            response: SubscriptionResponseInfo(
                userInfo: SubscriptionUserInfo(upload: 1, download: 2, total: 3, expire: start),
                updateIntervalHours: 12,
                title: "Sub"
            ),
            at: start
        )
        let groups = SubscriptionGroups(groups: [group], activeID: group.id)
        XCTAssertEqual(try SubscriptionGroups.fromKeychainValue(groups.keychainValue()), groups)
    }
}
