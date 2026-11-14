import Foundation
import NetworkCoreBridge
import XCTest

final class PacketCaptureTests: XCTestCase {
    private func request(_ command: RuntimeControl) -> String {
        String(decoding: command.request, as: UTF8.self)
    }

    func testRequestsAreTheEngineCommands() {
        XCTAssertEqual(request(.captureStatus), #"{"cmd":"capture"}"#)
        XCTAssertEqual(request(.setCapture(true)), #"{"cmd":"set_capture","enabled":true}"#)
        XCTAssertEqual(request(.clearCapture), #"{"cmd":"clear_capture"}"#)
        XCTAssertEqual(request(.captureConnection(9)), #"{"cmd":"capture_connection","id":9}"#)
        XCTAssertEqual(request(.exportCapture(.har, id: nil)), #"{"cmd":"export_capture","format":"har"}"#)
        XCTAssertEqual(
            request(.exportCapture(.pcapng, id: 4)),
            #"{"cmd":"export_capture","format":"pcapng","id":4}"#
        )
    }

    func testDecodesStatusConnectionAndExport() throws {
        let status = try XCTUnwrap(RuntimeControl.decode(PacketCapture.Status.self, from: Data("""
        {"enabled":true,"max_total_bytes":2097152,"used_bytes":1024,"connections":3}
        """.utf8)))
        XCTAssertEqual(status, PacketCapture.Status(enabled: true, maxTotalBytes: 2_097_152, usedBytes: 1024, connections: 3))

        let hello = Data("hello".utf8).base64EncodedString()
        let answer = try XCTUnwrap(RuntimeControl.decode(PacketCapture.ConnectionAnswer.self, from: Data("""
        {"capture":{"id":9,"network":"tcp","host":"a.test","destination":"a.test:443","started_at_ms":1700000000000,
          "ended_at_ms":null,"decrypted":true,
          "upload":{"seen":10,"captured":0,"truncated":true},"download":{"seen":5,"captured":0,"truncated":true},
          "chunks":[{"dir":"down","at_ms":1700000000100,"data_base64":"\(hello)"}],
          "http":[{"started_at_ms":1700000000000,"ended_at_ms":1700000000200,"method":"GET",
            "url":"https://a.test/x","status":200,"note":null,
            "request":{"headers":[["Host","a.test"]],"body":{"size":0,"truncated":false,"data_base64":""}},
            "response":{"headers":[["Content-Type","text/plain"]],
              "body":{"size":5,"truncated":false,"data_base64":"\(hello)"}}}]}}
        """.utf8)))
        let capture = try XCTUnwrap(answer.capture)
        XCTAssertTrue(capture.decrypted)
        XCTAssertFalse(capture.chunks[0].isUpload)
        XCTAssertEqual(capture.chunks[0].data, Data("hello".utf8))
        XCTAssertEqual(capture.http[0].request.header("host"), "a.test")
        XCTAssertEqual(capture.http[0].response.body.data, Data("hello".utf8))
        XCTAssertEqual(capture.http[0].status, 200)

        let missing = try XCTUnwrap(RuntimeControl.decode(PacketCapture.ConnectionAnswer.self, from: Data(#"{"capture":null}"#.utf8)))
        XCTAssertNil(missing.capture)

        let export = try XCTUnwrap(RuntimeControl.decode(PacketCapture.ExportAnswer.self, from: Data("""
        {"format":"pcapng","file_name":"../anixops-capture-9.pcapng","content_type":"application/vnd.tcpdump.pcap",
         "size":5,"data_base64":"\(hello)"}
        """.utf8)))
        XCTAssertEqual(export.fileName, "anixops-capture-9.pcapng")
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let url = try PacketCapture.write(export, in: directory)
        XCTAssertEqual(try Data(contentsOf: url), Data("hello".utf8))
        try? FileManager.default.removeItem(at: directory)
    }

    func testConnectionsCarryTheCapturedFlag() throws {
        let json = """
        {"traffic":{"upload_total":0,"download_total":0,"upload_rate":0,"download_rate":0,"active":1},
         "active":[{"id":1,"network":"tcp","host":null,"destination":"192.0.2.1:80","rule":"FINAL","chain":[],
           "outbound":"DIRECT","started_at_ms":1,"ended_at_ms":null,"upload":0,"download":0,"error":null,"captured":true}],
         "closed":[{"id":2,"network":"tcp","host":null,"destination":"192.0.2.1:80","rule":"FINAL","chain":[],
           "outbound":"DIRECT","started_at_ms":1,"ended_at_ms":2,"upload":0,"download":0,"error":null}]}
        """
        let list = try XCTUnwrap(RuntimeControl.decodeConnections(Data(json.utf8)))
        XCTAssertTrue(list.active[0].captured)
        XCTAssertFalse(list.closed[0].captured)
    }

    func testShowsTextDecompressedOrHex() throws {
        let gzip = try XCTUnwrap(Data(base64Encoded: "H4sIAAAAAAAC/8tIzcnJV0hOLCgpLUoFABDhXhYNAAAA"))
        XCTAssertEqual(PacketCapture.text(gzip, contentEncoding: "gzip"), "hello capture")
        let zlib = try XCTUnwrap(Data(base64Encoded: "eJxLSU3LSSxJTVEoSa0oAQAjngUf"))
        XCTAssertEqual(PacketCapture.text(zlib, contentEncoding: "deflate"), "deflated text")
        XCTAssertEqual(PacketCapture.text(Data("GET / HTTP/1.1\r\n".utf8)), "GET / HTTP/1.1\r\n")
        XCTAssertNil(PacketCapture.text(Data([0x16, 0x03, 0x01, 0x00, 0x05, 0x01, 0x02, 0x00, 0x00])))
        XCTAssertEqual(
            PacketCapture.hexDump(Data("AB\u{0}".utf8)),
            "00000000  41 42 00" + String(repeating: " ", count: 39) + "  AB."
        )
    }
}
