import Foundation
import NetworkCoreBridge
import XCTest

final class RequestListTests: XCTestCase {
    private func connections() throws -> RuntimeControl.Connections {
        let json = """
        {"traffic":{"upload_total":0,"download_total":0,"upload_rate":0,"download_rate":0,"active":3},
         "active":[
           {"id":1,"network":"udp","host":"video.example.com","destination":"198.51.100.1:443","rule":"FINAL",
            "chain":["Media","jp-1"],"outbound":"jp-1","started_at_ms":1000,"ended_at_ms":null,"upload":10,"download":90,"error":null},
           {"id":2,"network":"tcp","host":"img.example.cn","destination":"203.0.113.8:80","rule":"GEOIP,CN",
            "chain":[],"outbound":"DIRECT","started_at_ms":1000,"ended_at_ms":null,"upload":1,"download":1,"error":null},
           {"id":3,"network":"udp","host":null,"destination":"203.0.113.40:53","rule":"FINAL",
            "chain":["hk-1"],"outbound":"hk-1","started_at_ms":1000,"ended_at_ms":null,"upload":0,"download":0,"error":null}],
         "closed":[
           {"id":4,"network":"tcp","host":"ads.example.net","destination":"198.51.100.90:443","rule":"RULE-SET,ads",
            "chain":[],"outbound":"REJECT","started_at_ms":1000,"ended_at_ms":1000,"upload":0,"download":0,"error":"rejected"},
           {"id":5,"network":"tcp","host":"chat.example.io","destination":"198.51.100.66:5222","rule":"FINAL",
            "chain":["Proxy","hk-1"],"outbound":"hk-1","started_at_ms":1000,"ended_at_ms":11000,"upload":4,"download":0,"error":"connect_failed"},
           {"id":6,"network":"tcp","host":"video.example.com","destination":"198.51.100.2:443","rule":"FINAL",
            "chain":["Media","jp-1"],"outbound":"jp-1","started_at_ms":1000,"ended_at_ms":4000,"upload":0,"download":200,"error":null}]}
        """
        return try XCTUnwrap(RuntimeControl.decodeConnections(Data(json.utf8)))
    }

    func testRouteStatusAndLabels() throws {
        let list = try connections()
        let all = list.active + list.closed
        XCTAssertEqual(all.map(\.route), [.proxy, .direct, .proxy, .reject, .proxy, .proxy])
        XCTAssertEqual(all.map(\.status), [.active, .active, .active, .rejected, .failed, .done])
        XCTAssertEqual(all.map(\.protocolLabel), ["QUIC", "HTTP", "DNS", "HTTPS", "TCP", "HTTPS"])
        XCTAssertEqual(all[0].policyLabel, "Media · jp-1")
        XCTAssertEqual(all[1].policyLabel, "DIRECT")
        XCTAssertEqual(all[2].policyLabel, "hk-1")
        XCTAssertEqual(all[2].hostName, "203.0.113.40")
        XCTAssertEqual(all[4].duration(), 10)
        XCTAssertEqual(all[0].duration(now: Date(timeIntervalSince1970: 6)), 5)
    }

    func testFilters() throws {
        let list = try connections()
        let all = list.active + list.closed
        func ids(_ filter: RequestFilter) -> [UInt64] {
            all.filter(filter.matches).map(\.id)
        }
        XCTAssertEqual(ids(.all), [1, 2, 3, 4, 5, 6])
        XCTAssertEqual(ids(.proxy), [1, 3, 5, 6])
        XCTAssertEqual(ids(.direct), [2])
        XCTAssertEqual(ids(.reject), [4])
        XCTAssertEqual(ids(.failed), [5])
        XCTAssertEqual(ids(.udp), [1, 3])
    }

    func testSummarizeKeepsQuietHostsLast() throws {
        let list = try connections()
        let hosts = HostTraffic.summarize(list.active + list.closed)
        XCTAssertEqual(hosts.first, HostTraffic(host: "video.example.com", bytes: 300, connections: 2, outbound: "jp-1"))
        XCTAssertEqual(hosts.map(\.host), [
            "video.example.com", "chat.example.io", "img.example.cn", "203.0.113.40", "ads.example.net",
        ])
        XCTAssertEqual(HostTraffic.top(list).map(\.host), ["video.example.com", "chat.example.io", "img.example.cn"])
    }

    func testMonogram() {
        XCTAssertEqual(HostMonogram.letter(for: "www.google.com"), "G")
        XCTAssertEqual(HostMonogram.letter(for: "video.example.com"), "V")
        XCTAssertEqual(HostMonogram.letter(for: "api.example.org"), "E")
        XCTAssertEqual(HostMonogram.letter(for: "localhost"), "L")
        XCTAssertEqual(HostMonogram.letter(for: "203.0.113.9"), "#")
        XCTAssertEqual(HostMonogram.letter(for: "2001:db8::1"), "#")
        let index = HostMonogram.colorIndex(for: "example.com", count: 5)
        XCTAssertTrue((0..<5).contains(index))
        XCTAssertEqual(index, HostMonogram.colorIndex(for: "EXAMPLE.com", count: 5))
        XCTAssertEqual(HostMonogram.colorIndex(for: "x", count: 0), 0)
    }
}
