import Foundation
@testable import NetworkCoreBridge
import XCTest

/// The same cases as `binary-view.test.ts` on the desktop and `CaptureBinaryTest.kt` on Android: the three must print
/// the same text.
final class CaptureBinaryTests: XCTestCase {
    private func bytes(_ parts: Any...) -> Data {
        var out: [UInt8] = []
        for part in parts {
            switch part {
            case let text as String: out += Array(text.utf8)
            case let list as [UInt8]: out += list
            case let list as [Int]: out += list.map { UInt8($0) }
            case let value as Int: out.append(UInt8(value))
            default: XCTFail("unexpected part \(part)")
            }
        }
        return Data(out)
    }

    private let labels = CaptureBinary.Labels(
        message: { index, size in "message \(index) (\(size))" },
        compressed: "compressed",
        trailers: "trailers",
        undecoded: "not protobuf",
        more: "…"
    )

    private func decoded(_ data: Data, cut: Bool = false) -> String? {
        CaptureBinary.decodeProtobuf(data, cut: cut).map { CaptureBinary.protoText($0, more: "…") }
    }

    private var sample: Data {
        bytes(
            [0x08, 0x96, 0x01],
            [0x12, 0x07], "testing",
            [0x1A, 0x03, 0x08, 0x96, 0x01],
            [0x21, 0x18, 0x2D, 0x44, 0x54, 0xFB, 0x21, 0x09, 0x40],
            [0x2D, 0x00, 0x00, 0x80, 0x3F],
            [0x32, 0x03, 0x00, 0xFF, 0x10],
            [0x38, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
            [0x42, 0x00],
            [0x4A, 0x05], "a\"b\n\\"
        )
    }

    func testPrintsEveryWireTypeLikeDecodeRaw() {
        XCTAssertEqual(decoded(sample), [
            "1: 150",
            "2: \"testing\"",
            "3 {",
            "  1: 150",
            "}",
            "4: 0x400921fb54442d18 (fixed64)",
            "5: 0x3f800000 (fixed32)",
            "6: <00 ff 10> (3 B)",
            "7: 18446744073709551615 (-1)",
            "8: \"\"",
            "9: \"a\\\"b\\n\\\\\"",
        ].joined(separator: "\n"))
    }

    func testShowsLongBytesByTheirStartAndSize() {
        let blob = (0..<40).map { $0 == 0 ? 0 : 0x80 + $0 }
        let head = blob.prefix(32).map { String(format: "%02lx", $0) }.joined(separator: " ")
        XCTAssertEqual(decoded(bytes([0x0A, 40], blob)), "1: <\(head) …> (40 B)")
    }

    func testRefusesBytesThatAreNotAWholeMessage() {
        XCTAssertNil(CaptureBinary.decodeProtobuf(bytes(0x08)))
        XCTAssertNil(CaptureBinary.decodeProtobuf(bytes(0x0A, 0x05, 0x01)))
        XCTAssertNil(CaptureBinary.decodeProtobuf(bytes(0x0F, 0x00)))
        XCTAssertNil(CaptureBinary.decodeProtobuf(bytes(0x0B, 0x0C)))
        XCTAssertNil(CaptureBinary.decodeProtobuf(bytes(0x00, 0x01)))
        XCTAssertNil(CaptureBinary.decodeProtobuf(bytes([0x08], [Int](repeating: 0xFF, count: 11), [0x01])))
        XCTAssertNil(CaptureBinary.decodeProtobuf(bytes([0x08], [Int](repeating: 0xFF, count: 9), [0x02])))
    }

    func testKeepsTheFieldsBeforeTheCutOfACutBody() {
        XCTAssertNil(decoded(bytes([0x08, 0x96, 0x01, 0x12, 0x07], "tes")))
        XCTAssertEqual(decoded(bytes([0x08, 0x96, 0x01, 0x12, 0x07], "tes"), cut: true), "1: 150\n…")
    }

    func testEmptyIsAnEmptyMessageButNeverOfferedForUnknownBytes() {
        XCTAssertEqual(CaptureBinary.decodeProtobuf(Data()), CaptureBinary.ProtoMessage(fields: [], cut: false))
        XCTAssertNil(CaptureBinary.looksLikeProtobuf(Data()))
        XCTAssertNotNil(CaptureBinary.looksLikeProtobuf(sample))
        XCTAssertNil(CaptureBinary.looksLikeProtobuf(bytes(0xFF, 0xFE)))
    }

    func testOpensMessagesInsideMessagesOnlySoDeep() {
        var inner = bytes(0x08, 0x01)
        for _ in 0..<(CaptureBinary.maxDepth + 4) {
            inner = bytes([0x0A, inner.count], [UInt8](inner))
        }
        let text = decoded(inner) ?? ""
        XCTAssertEqual(text.split(separator: "\n").filter { $0.hasSuffix("{") }.count, CaptureBinary.maxDepth - 1)
        XCTAssertTrue(text.contains("B)"))
    }

    func testStopsAtTheFieldLimitAndSaysSo() {
        let many = [UInt8]((0..<(CaptureBinary.maxFields + 500)).flatMap { _ in [UInt8(0x08), 0x01] })
        let message = CaptureBinary.decodeProtobuf(Data(many))
        XCTAssertEqual(message?.fields.count, CaptureBinary.maxFields)
        XCTAssertEqual(message?.cut, true)
        let nested = bytes([0x0A, 0xA0, 0x9C, 0x01], Array(many.prefix(20000)), [0x10, 0x01])
        XCTAssertEqual(CaptureBinary.decodeProtobuf(nested)?.cut, false)
        let over = bytes([0x0A, 0xD0, 0xB8, 0x02], Array(many.prefix(40016)))
        XCTAssertEqual(CaptureBinary.decodeProtobuf(over)?.cut, true)
    }

    func testKnowsTheMediaTypes() {
        XCTAssertTrue(CaptureBinary.isProtobufType("application/x-protobuf"))
        XCTAssertTrue(CaptureBinary.isProtobufType("application/vnd.foo+proto"))
        XCTAssertFalse(CaptureBinary.isProtobufType("application/grpc+proto"))
        XCTAssertTrue(CaptureBinary.isGrpcType("application/grpc"))
        XCTAssertTrue(CaptureBinary.isGrpcType("application/grpc-web+proto"))
        XCTAssertFalse(CaptureBinary.isGrpcType("application/grpc-web-text"))
    }

    private func frame(_ flag: Int, _ data: Data) -> [UInt8] {
        [UInt8(flag), 0, 0, UInt8((data.count >> 8) & 0xFF), UInt8(data.count & 0xFF)] + [UInt8](data)
    }

    func testDecodesGrpcMessagesAndShowsTrailers() throws {
        let body = bytes(frame(0, bytes(0x08, 0x96, 0x01)), frame(1, bytes(1, 2, 3)), frame(0x80, bytes("grpc-status:0\r\n")))
        let frames = try XCTUnwrap(CaptureBinary.grpcFrames(body))
        XCTAssertEqual(frames.frames.count, 3)
        XCTAssertEqual(
            CaptureBinary.grpcText(frames, labels: labels),
            "# message 1 (3)\n1: 150\n\n# message 2 (3)\ncompressed\n\n# trailers\ngrpc-status:0"
        )
    }

    func testKeepsACutGrpcMessageAndRefusesWhatIsNotFramed() throws {
        let cut = Data(frame(0, bytes(0x08, 0x96, 0x01, 0x10, 0x02)).prefix(9))
        XCTAssertEqual(CaptureBinary.grpcText(try XCTUnwrap(CaptureBinary.grpcFrames(cut)), labels: labels), "# message 1 (5)\n1: 150\n…\n\n…")
        XCTAssertNil(CaptureBinary.grpcFrames(bytes(0x02, 0, 0, 0, 0)))
        XCTAssertNil(CaptureBinary.grpcFrames(bytes(0, 0, 0)))
        XCTAssertEqual(
            CaptureBinary.grpcText(try XCTUnwrap(CaptureBinary.grpcFrames(Data(frame(0, bytes(0xFF))))), labels: labels),
            "# message 1 (1)\nnot protobuf"
        )
    }

    func testRecognisesPicturesByTheirFirstBytes() {
        XCTAssertEqual(CaptureBinary.sniffImage(bytes([0x89], "PNG\r\n\u{1a}\n", [0, 0, 0, 13], "IHDR")), "image/png")
        XCTAssertEqual(CaptureBinary.sniffImage(bytes(0xFF, 0xD8, 0xFF, 0xE0)), "image/jpeg")
        XCTAssertEqual(CaptureBinary.sniffImage(bytes("GIF89a")), "image/gif")
        XCTAssertEqual(CaptureBinary.sniffImage(bytes("RIFF", [0, 0, 0, 0], "WEBPVP8 ")), "image/webp")
        XCTAssertEqual(CaptureBinary.sniffImage(bytes([0, 0, 0, 0x1C], "ftypavif")), "image/avif")
        XCTAssertEqual(CaptureBinary.sniffImage(bytes("BM", [Int](repeating: 0, count: 12), [40, 0, 0, 0], [Int](repeating: 0, count: 8))), "image/bmp")
        XCTAssertNil(CaptureBinary.sniffImage(bytes("BMx")))
        XCTAssertNil(CaptureBinary.sniffImage(bytes("hello")))
    }

    func testBodyKindsForBinaryData() {
        XCTAssertEqual(CaptureBody.modes(CaptureBody.kind(contentType: "application/x-protobuf", text: nil, data: sample)), [.protobuf, .hex])
        guard case .protobuf(.grpc, labelled: true) = CaptureBody.kind(contentType: "application/grpc", text: nil, data: bytes(0, 0, 0, 0, 3, 0x08, 0x96, 0x01)) else {
            return XCTFail("gRPC not recognised")
        }
        XCTAssertEqual(CaptureBody.modes(CaptureBody.kind(contentType: "application/octet-stream", text: nil, data: sample)), [.hex, .protobuf])
        XCTAssertEqual(CaptureBody.kind(contentType: "application/x-protobuf", text: nil, data: bytes(0xFF, 0xFF)), .binary)
        XCTAssertEqual(CaptureBody.kind(contentType: "application/octet-stream", text: nil, data: bytes(0xFF, 0xD8, 0xFF, 0xE0)), .image)
        XCTAssertEqual(CaptureBody.kind(contentType: nil, text: nil, data: bytes("GIF89a", [0, 0x80])), .image)
    }

    func testOpensOnlyTypesThatRunNoCode() {
        XCTAssertEqual(CaptureBinary.fileType("image/png").fileExtension, "png")
        XCTAssertEqual(CaptureBinary.fileType("application/problem+json; charset=utf-8").fileExtension, "json")
        XCTAssertEqual(CaptureBinary.fileType("application/x-protobuf").fileExtension, "bin")
        for opens in ["image/png", "image/jpeg", "application/pdf", "text/plain; charset=utf-8", "application/json", "audio/mpeg"] {
            XCTAssertTrue(CaptureBinary.fileType(opens).opens, opens)
        }
        for saved in ["image/svg+xml", "text/html", "text/javascript", "application/xml", "text/csv", "application/octet-stream", ""] {
            XCTAssertFalse(CaptureBinary.fileType(saved).opens, saved)
        }
        XCTAssertFalse(CaptureBinary.fileType(nil).opens)
    }

    /// Random and damaged input, from a fixed seed so a failure can be run again.
    func testNeverFailsOnUntrustedBytes() {
        var state: UInt64 = 20_261_122
        func next() -> UInt64 {
            state = state &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
            return state >> 33
        }
        let seeds: [Data] = [
            sample,
            bytes(0, 0, 0, 0, 3, 0x08, 0x96, 0x01, 0x80, 0, 0, 0, 2, 0x61, 0x62),
            bytes([0x89], "PNG\r\n\u{1a}\n", [0, 0, 0, 13], "IHDR"),
            bytes("RIFF", [0, 0, 0, 0], "WEBPVP8L", [Int](repeating: 0xFF, count: 12)),
        ]
        for round in 0..<3000 {
            var data: [UInt8]
            if round % 2 == 0 {
                data = (0..<Int(next() % 600)).map { _ in UInt8(next() & 0xFF) }
            } else {
                data = [UInt8](seeds[round % seeds.count])
                for _ in 0..<(1 + Int(next() % 4)) where !data.isEmpty {
                    data[Int(next() % UInt64(data.count))] = UInt8(next() & 0xFF)
                }
                if next() % 10 < 3 {
                    data = Array(data.prefix(Int(next() % UInt64(data.count + 1))))
                }
                if next() % 10 < 2 {
                    data += data
                }
            }
            let body = Data(data)
            if let message = CaptureBinary.decodeProtobuf(body, cut: next() % 2 == 0) {
                XCTAssertLessThan(CaptureBinary.protoText(message, more: "…").count, 200 * (data.count + 1), "round \(round)")
            }
            _ = CaptureBinary.looksLikeProtobuf(body)
            if let frames = CaptureBinary.grpcFrames(body) {
                _ = CaptureBinary.grpcText(frames, labels: labels)
            }
            _ = CaptureBinary.sniffImage(body)
            _ = CaptureBody.kind(contentType: "application/octet-stream", text: nil, data: body, cut: true)
            _ = CaptureBody.kind(contentType: "application/grpc", text: nil, data: body, cut: true)
        }
    }
}
