import Foundation
import NetworkCoreBridge
import XCTest

final class AppURLCommandTests: XCTestCase {
    private func parse(_ string: String) -> Result<AppURLCommand, AppURLCommand.Failure> {
        AppURLCommand.parse(URL(string: string)!)
    }

    func testSixActions() {
        XCTAssertEqual(parse("anixops://connect"), .success(.connect))
        XCTAssertEqual(parse("anixops://disconnect"), .success(.disconnect))
        XCTAssertEqual(parse("anixops://toggle"), .success(.toggle))
        XCTAssertEqual(parse("anixops://mode?set=direct"), .success(.mode(.direct)))
        XCTAssertEqual(parse("anixops://select?node=Tokyo"), .success(.selectNode(name: "Tokyo", group: nil)))
        XCTAssertEqual(
            parse("anixops://select?node=%E4%B8%9C%E4%BA%AC%2001&group=Work"),
            .success(.selectNode(name: "东京 01", group: "Work"))
        )
        XCTAssertEqual(
            parse("anixops://select?policy=Proxy&node=Auto"),
            .success(.selectMember(policy: "Proxy", member: "Auto"))
        )
        XCTAssertEqual(
            parse("anixops://import?url=https%3A%2F%2Fsub.example%2Fs%3Ftoken%3Dabc%26flag%3Dclash"),
            .success(.importLink("https://sub.example/s?token=abc&flag=clash"))
        )
    }

    func testAliasesAndSpellings() {
        XCTAssertEqual(parse("ANIXOPS://Connect"), .success(.connect))
        XCTAssertEqual(parse("anixops:///stop"), .success(.disconnect))
        XCTAssertEqual(parse("anixops://x-callback-url/toggle?x-success=shortcuts://"), .success(.toggle))
        XCTAssertEqual(parse("anixops://mode?mode=Rule"), .success(.mode(.rule)))
        XCTAssertEqual(parse("anixops://install-config?url=ss%3A%2F%2Fabc"), .success(.importLink("ss://abc")))
    }

    func testRefusals() {
        XCTAssertEqual(parse("https://connect"), .failure(.notOurScheme))
        XCTAssertEqual(parse("anixops://"), .failure(.missing("action")))
        XCTAssertEqual(parse("anixops://reboot"), .failure(.unknownAction("reboot")))
        XCTAssertEqual(parse("anixops://mode"), .failure(.missing("set")))
        XCTAssertEqual(parse("anixops://mode?set=turbo"), .failure(.invalid("set")))
        XCTAssertEqual(parse("anixops://select?group=Work"), .failure(.missing("node")))
        XCTAssertEqual(parse("anixops://select?node=%20%20"), .failure(.missing("node")))
        XCTAssertEqual(parse("anixops://import"), .failure(.missing("url")))
        let huge = String(repeating: "a", count: AppURLCommand.maxLinkLength + 1)
        XCTAssertEqual(parse("anixops://import?url=\(huge)"), .failure(.invalid("url")))
    }

    func testResolveNodeByNameGroupAndID() throws {
        let json = """
        {"nodes":[
        {"id":"a","name":"Tokyo","protocol":"trojan","server":"a.example","port":443,
         "outbound":{"type":"trojan","server":"a.example","port":443,"password":"pa"}},
        {"id":"b","name":"Osaka","protocol":"trojan","server":"b.example","port":443,
         "outbound":{"type":"trojan","server":"b.example","port":443,"password":"pb"}}],
        "unsupported":[]}
        """
        let report = try NativeImportReport.decoded(from: Data(json.utf8))
        let home = SubscriptionGroup(
            id: "g1", name: "Home", library: NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        )
        let work = SubscriptionGroup(
            id: "g2", name: "Work", library: NodeLibrary(report: report, subscriptionURL: nil, keeping: nil)
        )
        let groups = SubscriptionGroups(groups: [home, work], activeID: "g1")

        let active = try XCTUnwrap(AppURLCommand.resolveNode(name: "Osaka", group: nil, in: groups))
        XCTAssertEqual(active.groupID, "g1")
        XCTAssertEqual(active.nodeID, "b")
        let byName = try XCTUnwrap(AppURLCommand.resolveNode(name: "tokyo", group: "work", in: groups))
        XCTAssertEqual(byName.groupID, "g2")
        XCTAssertEqual(byName.nodeID, "a")
        let byID = try XCTUnwrap(AppURLCommand.resolveNode(name: "b", group: "g2", in: groups))
        XCTAssertEqual(byID.nodeID, "b")
        XCTAssertNil(AppURLCommand.resolveNode(name: "Paris", group: nil, in: groups))
        XCTAssertNil(AppURLCommand.resolveNode(name: "Tokyo", group: "Nowhere", in: groups))
    }
}
