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

    private func liveRow(_ index: Int, rev: UInt64, seq: UInt64 = 0, status: Int? = nil, messages: UInt64 = 0) -> String {
        let state = seq == 0 ? "in_progress" : "done"
        let ended = seq == 0 ? "null" : "1700000000900"
        return """
        {"seq":\(seq),"connection_id":3,"index":\(index),"method":"GET","url":"https://ai.test/v1/stream","host":"ai.test",
         "status":\(status.map { String($0) } ?? "null"),"note":null,"http_version":"HTTP/1.1","started_at_ms":1700000000000,
         "ended_at_ms":\(ended),"request_size":0,"response_size":10,"response_content_type":"text/event-stream",
         "state":"\(state)","rev":\(rev),"rules_skipped":true,"message_count":\(messages)}
        """
    }

    private func liveAnswer(_ rows: [String], lastSeq: UInt64, lastRev: UInt64, dropped: UInt64 = 0) throws -> PacketCapture.RequestsAnswer {
        try XCTUnwrap(RuntimeControl.decode(PacketCapture.RequestsAnswer.self, from: Data("""
        {"requests":[\(rows.joined(separator: ","))],"last_seq":\(lastSeq),"last_rev":\(lastRev),"dropped_before_seq":\(dropped)}
        """.utf8)))
    }

    func testStreamCommands() {
        XCTAssertEqual(
            String(decoding: RuntimeControl.captureRequests(sinceSeq: 0, sinceRev: 7, limit: 500, filter: "").request, as: UTF8.self),
            #"{"cmd":"capture_requests","limit":500,"since_rev":7,"since_seq":0}"#
        )
        XCTAssertEqual(
            String(decoding: RuntimeControl.captureExchange(id: 3, index: 1, sinceMessage: 4).request, as: UTF8.self),
            #"{"cmd":"capture_exchange","id":3,"index":1,"since_message":4}"#
        )
        XCTAssertEqual(
            String(decoding: RuntimeControl.captureH2Frames(id: 3, since: 0).request, as: UTF8.self),
            #"{"cmd":"capture_h2_frames","id":3,"since":0}"#
        )
    }

    func testLogUpdatesRowsUnderWayInPlace() throws {
        var log = CaptureRequestLog()
        log.apply(try liveAnswer([liveRow(0, rev: 1)], lastSeq: 0, lastRev: 1))
        let first = try XCTUnwrap(log.rows.first)
        XCTAssertTrue(first.inProgress)
        XCTAssertTrue(first.rulesSkipped)
        XCTAssertEqual(first.streamKind, "SSE")
        XCTAssertEqual(log.nextRev, 1)

        // The same exchange again, now answered and with events: one row, updated where it was.
        log.apply(try liveAnswer([liveRow(1, rev: 2), liveRow(0, rev: 3, status: 200, messages: 4)], lastSeq: 0, lastRev: 3))
        XCTAssertEqual(log.rows.map(\.index), [0, 1])
        XCTAssertEqual(log.rows[0].status, 200)
        XCTAssertEqual(log.rows[0].messageCount, 4)
        XCTAssertEqual(log.nextRev, 3)

        // A full page carries on from its last row's rev.
        XCTAssertTrue(log.apply(try liveAnswer([liveRow(0, rev: 5, seq: 1, status: 200)], lastSeq: 1, lastRev: 9), limit: 1))
        XCTAssertEqual(log.nextRev, 5)
        XCTAssertFalse(log.rows[0].inProgress)

        // Dropping ended rows keeps the one still under way.
        log.apply(try liveAnswer([], lastSeq: 1, lastRev: 9, dropped: 1))
        XCTAssertEqual(log.rows.map(\.index), [1])
        XCTAssertEqual(log.nextRev, 9)

        log.reset()
        XCTAssertEqual(log.nextRev, 0)
        XCTAssertTrue(log.rows.isEmpty)
    }

    func testDecodesMessagesAndKeepsThemInStep() throws {
        let json = """
        {"exchange":{"seq":0,"started_at_ms":1700000000000,"ended_at_ms":null,"method":"GET","url":"wss://ws.test/",
         "status":101,"note":"streaming","state":"in_progress","rev":4,"rules_skipped":false,
         "messages":{"seen":5,"dropped":2,"kept":[
          {"index":2,"at_ms":1700000000100,"dir":"up","kind":"text","label":null,"id":null,"code":null,"size":5,
           "truncated":false,"text":"hello","data_base64":null},
          {"index":3,"at_ms":1700000000200,"dir":"down","kind":"binary","label":"permessage-deflate","id":null,"code":null,
           "size":9000,"truncated":true,"text":null,"data_base64":"AAEC"},
          {"index":4,"at_ms":1700000000300,"dir":"down","kind":"close","label":null,"id":null,"code":1000,"size":2,
           "truncated":false,"text":null,"data_base64":"A+g="}]},
         "request":{"headers":[],"body":{"size":0,"truncated":false,"data_base64":""}},
         "response":{"headers":[["Upgrade","websocket"]],"body":{"size":0,"truncated":false,"data_base64":""}}}}
        """
        let exchange = try XCTUnwrap(RuntimeControl.decode(PacketCapture.ExchangeAnswer.self, from: Data(json.utf8))?.exchange)
        XCTAssertTrue(exchange.inProgress)
        XCTAssertTrue(exchange.hasMessages)
        let messages = try XCTUnwrap(exchange.messages)
        XCTAssertEqual(messages.kept.map(\.title), ["Text", "Binary", "Close 1000"])
        XCTAssertTrue(messages.kept[0].isUpload)
        XCTAssertEqual(messages.kept[0].data, Data("hello".utf8))
        XCTAssertEqual(messages.kept[1].data, Data([0, 1, 2]))
        XCTAssertTrue(messages.kept[1].truncated)

        var log = PacketCapture.StreamMessageLog()
        log.apply(messages)
        XCTAssertEqual(log.messages.map(\.index), [2, 3, 4])
        XCTAssertEqual(log.missed, 2)
        XCTAssertEqual(log.nextIndex, 5)
        // A repeat adds nothing; later messages are appended.
        log.apply(messages)
        XCTAssertEqual(log.messages.count, 3)
        XCTAssertEqual(log.seen, 5)

        let none = try XCTUnwrap(RuntimeControl.decode(PacketCapture.ExchangeAnswer.self, from: Data(#"{"exchange":null}"#.utf8)))
        XCTAssertNil(none.exchange)
    }

    func testDecodesHTTP2Frames() throws {
        let json = """
        {"h2_frames":{"seen":3,"dropped":0,"frames":[
          {"index":0,"at_ms":1700000000000,"link":"app","dir":"down","type":"SETTINGS","type_code":4,"flags":0,
           "flag_names":[],"stream":0,"length":18,"error":null,"error_code":null,"value":3},
          {"index":1,"at_ms":1700000000001,"link":"origin","dir":"up","type":"WINDOW_UPDATE","type_code":8,"flags":0,
           "flag_names":[],"stream":1,"length":4,"error":null,"error_code":null,"value":65535},
          {"index":2,"at_ms":1700000000002,"link":"origin","dir":"down","type":"RST_STREAM","type_code":3,"flags":0,
           "flag_names":[],"stream":1,"length":4,"error":"CANCEL","error_code":8,"value":null}]}}
        """
        let frames = try XCTUnwrap(RuntimeControl.decode(PacketCapture.H2FramesAnswer.self, from: Data(json.utf8))?.frames)
        XCTAssertEqual(frames.frames.map(\.type), ["SETTINGS", "WINDOW_UPDATE", "RST_STREAM"])
        XCTAssertEqual(frames.frames.map(\.detail), ["3 settings", "+65535", "CANCEL"])
        XCTAssertEqual(frames.frames.map(\.isNotable), [false, false, true])
        XCTAssertEqual(frames.frames[1].link, .origin)
        XCTAssertTrue(frames.frames[1].isUpload)
        XCTAssertNil(try XCTUnwrap(RuntimeControl.decode(PacketCapture.H2FramesAnswer.self, from: Data(#"{"h2_frames":null}"#.utf8))).frames)
    }
}
