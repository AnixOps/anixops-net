import Foundation
import NetworkCoreBridge
import XCTest

final class TrafficStatsTests: XCTestCase {
    private func list(_ rows: [(id: Int, host: String, chain: String, up: Int, down: Int)], closed: Bool = false) throws -> RuntimeControl.Connections {
        let items = rows.map { row in
            """
            {"id":\(row.id),"network":"tcp","host":"\(row.host)","destination":"198.51.100.1:443","rule":"FINAL",
             "chain":["\(row.chain)"],"outbound":"hk-1","started_at_ms":1,"ended_at_ms":\(closed ? "2" : "null"),
             "upload":\(row.up),"download":\(row.down),"error":null}
            """
        }.joined(separator: ",")
        let json = """
        {"traffic":{"upload_total":0,"download_total":0,"upload_rate":0,"download_rate":0,"active":0},
         "active":[\(closed ? "" : items)],"closed":[\(closed ? items : "")]}
        """
        return try XCTUnwrap(RuntimeControl.decodeConnections(Data(json.utf8)))
    }

    func testCounterAddsOnlyNewBytes() throws {
        var stats = TrafficStats()
        var counter = TrafficStatsCounter()
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        counter.ingest(try list([(1, "a.example", "Proxy", 10, 100)]), into: &stats, at: now)
        counter.ingest(try list([(1, "a.example", "Proxy", 15, 300), (2, "b.example", "Media", 0, 50)]), into: &stats, at: now)
        // The first connection closes with a few more bytes.
        counter.ingest(try list([(1, "a.example", "Proxy", 15, 310)], closed: true), into: &stats, at: now)
        XCTAssertEqual(stats.hours.count, 1)
        let hour = try XCTUnwrap(stats.hours.first)
        XCTAssertEqual(hour.upload, 15)
        XCTAssertEqual(hour.download, 360)
        XCTAssertEqual(hour.hosts, ["a.example": 325, "b.example": 50])
        XCTAssertEqual(hour.policies, ["Proxy": 325, "Media": 50])
        XCTAssertEqual(TrafficStats.hosts(stats.hours).map(\.name), ["a.example", "b.example"])
    }

    func testRestartedEngineCountsFromZero() throws {
        var stats = TrafficStats()
        var counter = TrafficStatsCounter()
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        counter.ingest(try list([(1, "a.example", "Proxy", 0, 500)]), into: &stats, at: now)
        counter.ingest(try list([(1, "a.example", "Proxy", 0, 20)]), into: &stats, at: now)
        XCTAssertEqual(TrafficStats.total(stats.hours), 520)
    }

    func testHoursArePrunedAfterSevenDaysAndHostsAreCapped() {
        var stats = TrafficStats()
        let start = TrafficStats.hourStart(of: Date(timeIntervalSince1970: 1_800_000_000))
        stats.add(host: "old.example", policy: "P", upload: 1, download: 0, at: start)
        for index in 0..<(TrafficStats.hostsPerHour + 5) {
            stats.add(host: "h\(index).example", policy: "P", upload: 0, download: 1, at: start.addingTimeInterval(3600))
        }
        XCTAssertEqual(stats.hours.count, 2)
        let busy = stats.hours[1]
        XCTAssertEqual(busy.hosts.count, TrafficStats.hostsPerHour + 1)
        XCTAssertEqual(busy.hosts[TrafficStats.otherHosts], 5)
        stats.add(host: "new.example", policy: "P", upload: 0, download: 1, at: start.addingTimeInterval(7 * 24 * 3600))
        XCTAssertFalse(stats.hours.contains { $0.start == start })
        XCTAssertEqual(stats.hours(since: start.addingTimeInterval(7 * 24 * 3600)).count, 1)
    }

    func testRoundTripsAndMessages() throws {
        var stats = TrafficStats()
        stats.add(host: "a.example", policy: "P", upload: 1, download: 2, at: Date(timeIntervalSince1970: 1_800_000_000))
        XCTAssertEqual(try TrafficStats.decoded(from: stats.encoded()), stats)
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.trafficStats.encoded), .trafficStats)
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.clearTrafficStats.encoded), .clearTrafficStats)
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.diagnostics.encoded), .diagnostics)
        XCTAssertEqual(TunnelAppMessage.decode(Data()), .runtimeStatus)
    }
}
