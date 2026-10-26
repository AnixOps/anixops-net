import Foundation
import NetworkCoreBridge
import XCTest

final class RuntimeControlTests: XCTestCase {
    func testRequestsAreTheEngineCommands() throws {
        XCTAssertEqual(String(decoding: RuntimeControl.traffic.request, as: UTF8.self), #"{"cmd":"traffic"}"#)
        XCTAssertEqual(String(decoding: RuntimeControl.closeAll.request, as: UTF8.self), #"{"cmd":"close_all"}"#)
        XCTAssertEqual(
            String(decoding: RuntimeControl.closeConnection(7).request, as: UTF8.self),
            #"{"cmd":"close_connection","id":7}"#
        )
    }

    func testControlMessagesRoundTripThroughTheExtension() {
        let request = RuntimeControl.connections.request
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.control(request).encoded), .control(request))
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.diagnostics.encoded), .diagnostics)
    }

    func testDecodesTheConnectionList() throws {
        let json = """
        {"traffic":{"upload_total":10,"download_total":20,"upload_rate":1,"download_rate":2,"active":1},
         "active":[{"id":1,"network":"tcp","host":"www.google.com","destination":"142.250.1.1:443",
           "rule":"DOMAIN-SUFFIX,google.com","chain":["Proxy","hk-1"],"outbound":"hk-1",
           "started_at_ms":1700000000000,"ended_at_ms":null,"upload":5,"download":6,"error":null}],
         "closed":[{"id":2,"network":"udp","host":null,"destination":"example.com:53",
           "rule":"FINAL","chain":[],"outbound":"DIRECT",
           "started_at_ms":1700000000000,"ended_at_ms":1700000001500,"upload":1,"download":2,"error":"connect_failed"}]}
        """
        let list = try XCTUnwrap(RuntimeControl.decodeConnections(Data(json.utf8)))
        XCTAssertEqual(list.traffic.downloadRate, 2)
        let active = try XCTUnwrap(list.active.first)
        XCTAssertEqual(active.target, "www.google.com:443")
        XCTAssertTrue(active.isActive)
        XCTAssertEqual(active.startedAt, Date(timeIntervalSince1970: 1_700_000_000))
        XCTAssertTrue(active.matches("google HK-1"))
        XCTAssertFalse(active.matches("direct"))
        let closed = try XCTUnwrap(list.closed.first)
        XCTAssertEqual(closed.target, "example.com:53")
        XCTAssertFalse(closed.isActive)
        XCTAssertEqual(closed.endedAt, Date(timeIntervalSince1970: 1_700_000_001.5))
        XCTAssertEqual(closed.error, "connect_failed")
    }

    func testDecodesTraffic() {
        let traffic = RuntimeControl.decodeTraffic(Data(#"{"upload_total":1,"download_total":2,"upload_rate":3,"download_rate":4,"active":5}"#.utf8))
        XCTAssertEqual(traffic, RuntimeControl.Traffic(uploadTotal: 1, downloadTotal: 2, uploadRate: 3, downloadRate: 4, active: 5))
        XCTAssertNil(RuntimeControl.decodeTraffic(Data("not json".utf8)))
    }
}
