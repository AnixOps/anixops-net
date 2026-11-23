@testable import NetworkCoreBridge
import XCTest

final class SubscriptionRefreshTests: XCTestCase {
    private func node(_ id: String, password: String = "p", via: String? = nil) -> NativeImportReport.Node {
        var outbound: [String: JSONValue] = ["type": .string("trojan"), "password": .string(password)]
        if let via {
            outbound["via"] = .string(via)
        }
        return NativeImportReport.Node(id: id, name: id, protocol: "trojan", server: "\(id).example", port: 443, outbound: outbound)
    }

    private func library(_ nodes: [NativeImportReport.Node], selected: String?, at seconds: TimeInterval = 0) -> NodeLibrary {
        NodeLibrary(
            subscriptionURL: "https://sub.example/x",
            nodes: nodes,
            selectedID: selected,
            updatedAt: Date(timeIntervalSince1970: seconds)
        )
    }

    /// An update that brought nothing new keeps the running tunnel: only the update time changed.
    func testSameContentDoesNotReconnect() {
        let before = library([node("a"), node("b")], selected: "a", at: 0)
        let after = library([node("a"), node("b")], selected: "a", at: 86_400)
        XCTAssertFalse(SubscriptionRefresh.changesRunningTunnel(
            previous: before, next: after, previousActiveID: "g", nextActiveID: "g"
        ))
        XCTAssertFalse(SubscriptionRefresh.changesRunningTunnel(
            previous: nil, next: nil, previousActiveID: nil, nextActiveID: nil
        ))
    }

    /// The provider removed the selected node: the selection moved, the tunnel must follow.
    func testRemovedSelectedNodeReconnects() {
        let before = library([node("a"), node("b")], selected: "a")
        let after = library([node("b")], selected: "b")
        XCTAssertTrue(SubscriptionRefresh.changesRunningTunnel(
            previous: before, next: after, previousActiveID: "g", nextActiveID: "g"
        ))
    }

    /// Same node id, new credentials (or a new upstream in its chain): the running outbound is out of date.
    func testChangedCredentialsReconnect() {
        let before = library([node("a"), node("u")], selected: "a")
        XCTAssertTrue(SubscriptionRefresh.changesRunningTunnel(
            previous: before, next: library([node("a", password: "new"), node("u")], selected: "a"),
            previousActiveID: "g", nextActiveID: "g"
        ))
        XCTAssertTrue(SubscriptionRefresh.changesRunningTunnel(
            previous: before, next: library([node("a"), node("u", password: "new")], selected: "a"),
            previousActiveID: "g", nextActiveID: "g"
        ))
    }

    /// Rules and groups changed while the selected node stayed: a tunnel running by the rules uses them.
    func testChangedRulesReconnect() throws {
        let before = library([node("a")], selected: "a")
        var after = before
        after.routing = try JSONDecoder().decode(
            NativeImportReport.Routing.self,
            from: Data(#"{"rules":["FINAL,G"],"groups":[{"id":"G","type":"select","members":["a"]}]}"#.utf8)
        )
        XCTAssertTrue(SubscriptionRefresh.changesRunningTunnel(
            previous: before, next: after, previousActiveID: "g", nextActiveID: "g"
        ))
    }

    /// An import makes another group active, or a single pasted node: always a different tunnel.
    func testAnotherActiveGroupReconnects() {
        let same = library([node("a")], selected: "a")
        XCTAssertTrue(SubscriptionRefresh.changesRunningTunnel(
            previous: same, next: same, previousActiveID: "g", nextActiveID: "h"
        ))
        XCTAssertTrue(SubscriptionRefresh.changesRunningTunnel(
            previous: nil, next: same, previousActiveID: nil, nextActiveID: "g"
        ))
    }

    func testBackgroundPlanKeepsTheChainWhileTheGroupsAreUnknown() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        // A background launch before `load()`, or a Keychain that cannot be read yet: never withdrawn.
        XCTAssertEqual(
            SubscriptionRefresh.backgroundPlan(groupsRead: false, nextUpdate: nil, now: now),
            .retry(now.addingTimeInterval(SubscriptionRefresh.unknownRetryDelay))
        )
        XCTAssertEqual(SubscriptionRefresh.backgroundPlan(groupsRead: true, nextUpdate: nil, now: now), .cancel)
        let later = now.addingTimeInterval(6 * 3600)
        XCTAssertEqual(SubscriptionRefresh.backgroundPlan(groupsRead: true, nextUpdate: later, now: now), .submit(later))
        // Overdue: no sooner than the system allows.
        XCTAssertEqual(
            SubscriptionRefresh.backgroundPlan(groupsRead: true, nextUpdate: now.addingTimeInterval(-60), now: now),
            .submit(now.addingTimeInterval(SubscriptionRefresh.minimumBackgroundDelay))
        )
    }

    /// A background update marks the connection that kept the old nodes; only that same connection reconnects later.
    func testPendingReconnectOnlyForTheMarkedConnection() {
        let up = Date(timeIntervalSince1970: 1_700_000_000.25)
        XCTAssertTrue(SubscriptionRefresh.pendingReconnectApplies(marked: up, connectedSince: up))
        // Stored as seconds since 1970: a round trip through a Double still matches.
        XCTAssertTrue(SubscriptionRefresh.pendingReconnectApplies(
            marked: Date(timeIntervalSince1970: up.timeIntervalSince1970), connectedSince: up
        ))
        // Reconnected since (On Demand, the widget): it already runs the saved profile.
        XCTAssertFalse(SubscriptionRefresh.pendingReconnectApplies(marked: up, connectedSince: up.addingTimeInterval(120)))
        XCTAssertFalse(SubscriptionRefresh.pendingReconnectApplies(marked: up, connectedSince: nil))
        XCTAssertFalse(SubscriptionRefresh.pendingReconnectApplies(marked: nil, connectedSince: up))
    }
}
