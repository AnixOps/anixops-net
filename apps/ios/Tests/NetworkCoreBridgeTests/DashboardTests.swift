import Foundation
import NetworkCoreBridge
import XCTest

final class DashboardLayoutTests: XCTestCase {
    func testDefaultShowsEveryCardInOrder() {
        let layout = DashboardLayout(storage: "")
        XCTAssertEqual(layout.order, DashboardCard.allCases)
        XCTAssertEqual(layout.visible, DashboardCard.allCases)
    }

    func testStorageRoundTripsOrderAndHiddenCards() {
        var layout = DashboardLayout()
        layout.setShown(.dns, false)
        layout.move(fromOffsets: IndexSet(integer: 7), toOffset: 0)
        let restored = DashboardLayout(storage: layout.storage)
        XCTAssertEqual(restored, layout)
        XCTAssertEqual(restored.order.first, .events)
        XCTAssertFalse(restored.isShown(.dns))
        XCTAssertFalse(restored.visible.contains(.dns))
    }

    func testUnknownCardsAreDroppedAndNewOnesAppended() {
        let layout = DashboardLayout(storage: "events, -speed,weather,events")
        XCTAssertEqual(layout.order.prefix(2), [.events, .speed])
        XCTAssertEqual(layout.order.count, DashboardCard.allCases.count)
        XCTAssertEqual(Set(layout.order), Set(DashboardCard.allCases))
        XCTAssertFalse(layout.isShown(.speed))
    }

    func testMoveMatchesListOnMove() {
        var layout = DashboardLayout()
        // Move the first card below the third: SwiftUI passes destination 3.
        layout.move(fromOffsets: IndexSet(integer: 0), toOffset: 3)
        XCTAssertEqual(Array(layout.order.prefix(3)), [.usage, .network, .speed])
        layout.move(fromOffsets: IndexSet(integer: 2), toOffset: 0)
        XCTAssertEqual(layout.order, DashboardCard.allCases)
    }

    func testRowsPairNarrowCards() {
        let layout = DashboardLayout(order: [.usage, .speed, .network, .dns, .exitIP, .groups], hidden: [.topHosts, .events])
        XCTAssertEqual(layout.rows, [[.usage], [.speed], [.network, .dns], [.exitIP], [.groups]])
    }

    func testWideRowsFillFourSlots() {
        let layout = DashboardLayout(order: [.usage, .speed, .network, .dns, .exitIP, .groups], hidden: [.topHosts, .events])
        XCTAssertEqual(layout.rows(columns: 4), [[.usage, .speed, .network], [.dns, .exitIP, .groups]])
        XCTAssertEqual(layout.rows(columns: 1), [[.usage], [.speed], [.network], [.dns], [.exitIP], [.groups]])
        XCTAssertEqual(DashboardLayout.span(of: .speed, columns: 4), 2)
        XCTAssertEqual(DashboardLayout.span(of: .speed, columns: 1), 1)
    }
}

final class DashboardDataTests: XCTestCase {
    func testTrafficHistoryKeepsTheLastWindow() {
        var history = TrafficHistory(capacity: 3)
        XCTAssertEqual(history.peak, 1)
        for rate in [10, 50, 20, 30] as [UInt64] {
            history.append(upload: rate / 10, download: rate)
        }
        XCTAssertEqual(history.samples.map(\.download), [50, 20, 30])
        XCTAssertEqual(history.peak, 50)
        history.reset()
        XCTAssertTrue(history.samples.isEmpty)
    }

    func testTopHostsSumsActiveAndClosedConnections() throws {
        let json = """
        {"traffic":{"upload_total":0,"download_total":0,"upload_rate":0,"download_rate":0,"active":2},
         "active":[
           {"id":1,"network":"tcp","host":"video.example.com","destination":"198.51.100.1:443","rule":"FINAL",
            "chain":["Proxy","hk-1"],"outbound":"hk-1","started_at_ms":1,"ended_at_ms":null,"upload":10,"download":990,"error":null},
           {"id":2,"network":"udp","host":null,"destination":"203.0.113.9:123","rule":"FINAL",
            "chain":[],"outbound":"DIRECT","started_at_ms":1,"ended_at_ms":null,"upload":1,"download":1,"error":null}],
         "closed":[
           {"id":3,"network":"tcp","host":"video.example.com","destination":"198.51.100.2:443","rule":"FINAL",
            "chain":["Proxy","jp-1"],"outbound":"jp-1","started_at_ms":1,"ended_at_ms":2,"upload":0,"download":200,"error":null},
           {"id":4,"network":"tcp","host":"api.example.org","destination":"[2001:db8::1]:443","rule":"FINAL",
            "chain":[],"outbound":"DIRECT","started_at_ms":1,"ended_at_ms":2,"upload":0,"download":0,"error":"connect_failed"}]}
        """
        let list = try XCTUnwrap(RuntimeControl.decodeConnections(Data(json.utf8)))
        let top = HostTraffic.top(list, limit: 5)
        XCTAssertEqual(top, [
            HostTraffic(host: "video.example.com", bytes: 1200, connections: 2, outbound: "hk-1"),
            HostTraffic(host: "203.0.113.9", bytes: 2, connections: 1, outbound: "DIRECT"),
        ])
        XCTAssertEqual(HostTraffic.top(list, limit: 1).count, 1)
    }

    func testParsesTheTraceAnswer() {
        let text = "fl=1f1\nh=www.example.com\nip=203.0.113.24\nts=1.0\nloc=hk\nwarp=off\n"
        XCTAssertEqual(ExitAddress.parseTrace(text), ExitAddress(ip: "203.0.113.24", region: "HK"))
        XCTAssertEqual(ExitAddress.parseTrace("ip=2001:db8::5\nloc=XX"), ExitAddress(ip: "2001:db8::5", region: nil))
        XCTAssertNil(ExitAddress.parseTrace("<html>blocked</html>"))
    }

    func testEventLogIsNewestFirstBoundedAndSkipsRepeats() throws {
        var log = DashboardEventLog()
        let start = Date(timeIntervalSince1970: 1_000)
        log.record(.connected, "a", at: start)
        log.record(.connected, "a", at: start.addingTimeInterval(30))
        XCTAssertEqual(log.events.count, 1)
        log.record(.connected, "a", at: start.addingTimeInterval(90))
        XCTAssertEqual(log.events.count, 2)
        for index in 0..<30 {
            log.record(.network, "n\(index)", at: start.addingTimeInterval(Double(100 + index)))
        }
        XCTAssertEqual(log.events.count, DashboardEventLog.capacity)
        XCTAssertEqual(log.events.first?.text, "n29")
        let decoded = try JSONDecoder().decode(DashboardEventLog.self, from: JSONEncoder().encode(log))
        XCTAssertEqual(decoded, log)
    }
}
