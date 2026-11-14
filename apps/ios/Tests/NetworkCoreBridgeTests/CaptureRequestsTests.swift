import Foundation
@testable import NetworkCoreBridge
import XCTest

final class CaptureRequestsTests: XCTestCase {
    private func row(_ seq: UInt64, url: String = "https://api.test/v1/items?id=1", type: String? = "application/json; charset=utf-8") -> String {
        let contentType = type.map { "\"\($0)\"" } ?? "null"
        return """
        {"seq":\(seq),"connection_id":7,"index":\(seq - 1),"method":"GET","url":"\(url)","host":"api.test",
         "status":200,"note":null,"http_version":"HTTP/2","started_at_ms":1700000000000,"ended_at_ms":1700000000250,
         "request_size":0,"response_size":42,"response_content_type":\(contentType)}
        """
    }

    private func answer(_ rows: [String], last: UInt64, dropped: UInt64 = 0) throws -> PacketCapture.RequestsAnswer {
        try XCTUnwrap(RuntimeControl.decode(PacketCapture.RequestsAnswer.self, from: Data("""
        {"requests":[\(rows.joined(separator: ","))],"last_seq":\(last),"dropped_before_seq":\(dropped)}
        """.utf8)))
    }

    func testRequestCommand() {
        XCTAssertEqual(
            String(decoding: RuntimeControl.captureRequests(sinceSeq: 3, limit: 500, filter: "type:json").request, as: UTF8.self),
            #"{"cmd":"capture_requests","filter":"type:json","limit":500,"since_seq":3}"#
        )
        XCTAssertEqual(
            String(decoding: RuntimeControl.captureRequests(sinceSeq: 0, limit: 10, filter: "").request, as: UTF8.self),
            #"{"cmd":"capture_requests","limit":10,"since_seq":0}"#
        )
    }

    func testDecodesRowsAndError() throws {
        let decoded = try answer([row(1)], last: 1)
        let first = try XCTUnwrap(decoded.requests.first)
        XCTAssertEqual(first.connectionID, 7)
        XCTAssertEqual(first.path, "/v1/items?id=1")
        XCTAssertEqual(first.kind, "JSON")
        XCTAssertTrue(first.isHTTP2)
        XCTAssertEqual(try XCTUnwrap(first.duration), 0.25, accuracy: 0.001)
        XCTAssertNil(decoded.invalidFilterAt)

        let failed = try XCTUnwrap(RuntimeControl.decode(
            PacketCapture.RequestsAnswer.self,
            from: Data(#"{"error":{"code":"invalid_filter","at":5}}"#.utf8)
        ))
        XCTAssertEqual(failed.invalidFilterAt, 5)
        XCTAssertNil(RuntimeControl.decode(PacketCapture.RequestsAnswer.self, from: Data(#"{"error":{"code":"other"}}"#.utf8)))
    }

    func testRowKindAndPath() throws {
        let image = try XCTUnwrap(answer([row(1, url: "https://cdn.test", type: "image/png")], last: 1).requests.first)
        XCTAssertEqual(image.kind, "IMG")
        XCTAssertEqual(image.path, "/")
        let none = try XCTUnwrap(answer([row(1, type: nil)], last: 1).requests.first)
        XCTAssertNil(none.kind)
    }

    func testComposeAddsChipsToEveryGroup() {
        XCTAssertEqual(CaptureFilter.compose(chips: [], text: ""), "")
        XCTAssertEqual(CaptureFilter.compose(chips: [], text: "  api  status:4xx "), "api status:4xx")
        XCTAssertEqual(CaptureFilter.compose(chips: [.json], text: "a OR b"), "type:json a OR type:json b")
        XCTAssertEqual(
            CaptureFilter.compose(chips: [.errors, .http2], text: "host:x"),
            "status:>=400 version:h2 host:x OR status:none version:h2 host:x"
        )
        XCTAssertEqual(CaptureFilter.compose(chips: [], text: #"body:"a OR b""#), #"body:"a OR b""#)
    }

    func testTextPosition() {
        XCTAssertEqual(CaptureFilter.textPosition(of: 3, chips: [], text: "ab status:>"), 3)
        XCTAssertEqual(CaptureFilter.textPosition(of: 12, chips: [.json], text: "status:?"), 2)
        XCTAssertNil(CaptureFilter.textPosition(of: 1, chips: [.json], text: "x"))
        XCTAssertNil(CaptureFilter.textPosition(of: 3, chips: [.errors], text: "x"))
        XCTAssertNil(CaptureFilter.textPosition(of: 3, chips: [], text: "a OR b"))
    }

    func testLogAppendsDropsAndPages() throws {
        var log = CaptureRequestLog()
        XCTAssertFalse(log.apply(try answer([row(1), row(2)], last: 2)))
        XCTAssertEqual(log.rows.map(\.seq), [1, 2])
        XCTAssertEqual(log.nextSeq, 2)

        // A repeated row is not added twice; a filtered answer moves on to last_seq.
        log.apply(try answer([row(2), row(5)], last: 9))
        XCTAssertEqual(log.rows.map(\.seq), [1, 2, 5])
        XCTAssertEqual(log.nextSeq, 9)

        // A full page carries on from its last row, not last_seq.
        XCTAssertTrue(log.apply(try answer([row(10), row(11)], last: 20), limit: 2))
        XCTAssertEqual(log.nextSeq, 11)

        log.apply(try answer([], last: 20, dropped: 5))
        XCTAssertEqual(log.rows.map(\.seq), [10, 11])
        XCTAssertEqual(log.nextSeq, 20)

        log.reset()
        XCTAssertTrue(log.rows.isEmpty)
        XCTAssertEqual(log.nextSeq, 0)
    }
}
