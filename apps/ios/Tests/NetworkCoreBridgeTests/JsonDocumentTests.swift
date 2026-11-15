import Foundation
@testable import NetworkCoreBridge
import XCTest

final class JsonDocumentTests: XCTestCase {
    func testKeepsTheDigitsOfNumbersADoubleWouldRound() {
        XCTAssertEqual(
            JsonDocument.parse(#"{"id": 9007199254740993, "price": 1.10, "big": 1e400}"#),
            .object([
                JsonMember(name: "id", value: .number("9007199254740993")),
                JsonMember(name: "price", value: .number("1.10")),
                JsonMember(name: "big", value: .number("1e400")),
            ])
        )
    }

    func testKeepsKeyOrderAndRepeatedKeys() throws {
        guard case let .object(members) = try XCTUnwrap(JsonDocument.parse(#"{"b":1,"a":2,"b":3}"#)) else {
            return XCTFail("not an object")
        }
        XCTAssertEqual(members.map(\.name), ["b", "a", "b"])
    }

    func testReadsEscapesSurrogatePairsAndAByteOrderMark() {
        XCTAssertEqual(JsonDocument.parse("\u{FEFF}\"a\\n\\u4e2d\\ud83d\\ude00\\/\""), .string("a\n中😀/"))
        // A lone surrogate is not a character; the rest of the string still reads.
        XCTAssertEqual(JsonDocument.parse(#""a\ud83db""#), .string("a\u{FFFD}b"))
    }

    func testReadsScalarsEmptyContainersAndSurroundingWhitespace() {
        XCTAssertEqual(JsonDocument.parse(" true "), .bool(true))
        XCTAssertEqual(JsonDocument.parse("null"), .null)
        XCTAssertEqual(JsonDocument.parse("[ ]"), .array([]))
        XCTAssertEqual(JsonDocument.parse("{ }"), .object([]))
        XCTAssertEqual(JsonDocument.parse("-0.5e-3"), .number("-0.5e-3"))
    }

    func testRefusesWhatIsNotOneCompleteValue() {
        let bad = [
            "", "{", #"{"a":1"#, #"{"a":1,}"#, "[1,]", "{'a':1}", #"{"a" 1}"#, "01", "1.", "tru", "\"a", #""\x""#, "\"\n\"",
            #"{"a":1}{"b":2}"#, "[1] x", "NaN", "undefined", "-", "1e", #""\u12g4""#,
        ]
        for text in bad {
            XCTAssertNil(JsonDocument.parse(text), text)
        }
    }

    func testRefusesNestingTooDeepToShow() {
        XCTAssertNil(JsonDocument.parse(String(repeating: "[", count: 5000) + String(repeating: "]", count: 5000)))
        XCTAssertNotNil(JsonDocument.parse(String(repeating: "[", count: 100) + String(repeating: "]", count: 100)))
    }

    private func sample() throws -> JsonNode {
        try XCTUnwrap(JsonDocument.parse(#"{"items":[{"id":1,"odd key":"x"},{"id":2}],"total":2,"empty":{}}"#))
    }

    func testRowsListEveryValueWithItsDepthNameAndPath() throws {
        let rows = JsonDocument.rows(try sample(), collapsed: [])
        XCTAssertEqual(rows.map(\.path), [
            "$", "$.items", "$.items[0]", "$.items[0].id", #"$.items[0]["odd key"]"#, "$.items[1]", "$.items[1].id", "$.total", "$.empty",
        ])
        XCTAssertEqual(rows.map(\.depth), [0, 1, 2, 3, 3, 2, 3, 1, 1])
        XCTAssertEqual(rows[1].name, "items")
        XCTAssertEqual(rows[2].index, 0)
        XCTAssertNil(rows[2].name)
        XCTAssertEqual(rows[0].open, true)
        // An empty container has nothing to fold; a scalar has no fold state either.
        XCTAssertNil(rows[8].open)
        XCTAssertNil(rows[7].open)
    }

    func testRowsHideWhatIsUnderAFoldedContainer() throws {
        let root = try sample()
        let items = try XCTUnwrap(JsonDocument.rows(root, collapsed: []).first { $0.path == "$.items" })
        let rows = JsonDocument.rows(root, collapsed: [items.id])
        XCTAssertEqual(rows.map(\.path), ["$", "$.items", "$.total", "$.empty"])
        XCTAssertEqual(rows[1].open, false)
    }

    func testARepeatedKeyGetsItsOwnRow() throws {
        let rows = JsonDocument.rows(try XCTUnwrap(JsonDocument.parse(#"{"a":1,"a":2}"#)), collapsed: [])
        XCTAssertEqual(Set(rows.map(\.id)).count, 3)
        XCTAssertEqual(rows[1].parent, rows[0].id)
        XCTAssertNil(rows[0].parent)
    }

    func testFolding() throws {
        let root = try XCTUnwrap(JsonDocument.parse(#"{"a":{"b":{"c":[1]}},"d":[],"e":[{}]}"#))
        XCTAssertEqual(JsonDocument.containerIDs(root, fromDepth: 0).count, 5)
        XCTAssertEqual(JsonDocument.containerIDs(root).count, 4)
        XCTAssertEqual(JsonDocument.containerIDs(root, fromDepth: 2).count, 2)

        XCTAssertTrue(JsonDocument.initiallyCollapsed(try XCTUnwrap(JsonDocument.parse(#"{"a":{"b":[1,2,3]}}"#))).isEmpty)
        let items = (0..<300).map { #"{"id":\#($0),"tags":["x"]}"# }.joined(separator: ",")
        let large = try XCTUnwrap(JsonDocument.parse(#"{"rows":[\#(items)]}"#))
        XCTAssertGreaterThan(JsonDocument.nodeCount(large), 400)
        let rows = JsonDocument.rows(large, collapsed: JsonDocument.initiallyCollapsed(large))
        // The root, "rows" and its 300 items, each folded.
        XCTAssertEqual(rows.count, 302)
        XCTAssertEqual(rows[2].open, false)
        XCTAssertEqual(JsonDocument.nodeCount(large, limit: 10), 10)
    }

    func testPrettyPrinting() throws {
        let node = try XCTUnwrap(JsonDocument.parse(#"{"id":9007199254740993,"tags":["a","b"],"none":null,"ok":true,"e":{},"l":[]}"#))
        XCTAssertEqual(
            JsonDocument.pretty(node),
            "{\n  \"id\": 9007199254740993,\n  \"tags\": [\n    \"a\",\n    \"b\"\n  ],\n  \"none\": null,\n  \"ok\": true,\n  \"e\": {},\n  \"l\": []\n}"
        )
        // What it writes reads back as the same document.
        let tricky = try XCTUnwrap(JsonDocument.parse(#"{"text":"quote \" and \\ and \n newline \u0001","unicode":"中文 😀"}"#))
        XCTAssertEqual(JsonDocument.parse(JsonDocument.pretty(tricky)), tricky)
    }

    func testCopyText() throws {
        XCTAssertEqual(JsonDocument.copyText(.string("a\nb")), "a\nb")
        XCTAssertEqual(JsonDocument.copyText(try XCTUnwrap(JsonDocument.parse("[1,2]"))), "[\n  1,\n  2\n]")
        XCTAssertEqual(JsonDocument.copyText(.number("1.10")), "1.10")
        XCTAssertEqual(JsonDocument.scalarText(.string("a\"b")), #""a\"b""#)
        XCTAssertNil(JsonDocument.scalarText(.array([])))
    }
}

final class CaptureBodyTests: XCTestCase {
    func testMediaType() {
        XCTAssertEqual(CaptureBody.mediaType("Application/JSON; charset=utf-8"), "application/json")
        XCTAssertEqual(CaptureBody.mediaType(nil), "")
        XCTAssertTrue(CaptureBody.isImage("image/png"))
        XCTAssertFalse(CaptureBody.isImage("image/svg+xml"))
        XCTAssertFalse(CaptureBody.isImage("text/html"))
    }

    func testKind() {
        XCTAssertEqual(CaptureBody.kind(contentType: "application/json", text: "[1]"), .json(.array([.number("1")])))
        // JSON sent as plain text is still shown as JSON; a cut JSON body falls back to text.
        XCTAssertEqual(CaptureBody.kind(contentType: "text/plain", text: " {\"a\":null}"), .json(.object([JsonMember(name: "a", value: .null)])))
        XCTAssertEqual(CaptureBody.kind(contentType: "application/json", text: "{\"a\":"), .text)
        XCTAssertEqual(CaptureBody.kind(contentType: "text/plain", text: "42"), .text)
        XCTAssertEqual(CaptureBody.kind(contentType: "image/png", text: nil), .image)
        XCTAssertEqual(CaptureBody.kind(contentType: "application/octet-stream", text: nil), .binary)
        XCTAssertEqual(
            CaptureBody.kind(contentType: "application/x-www-form-urlencoded", text: "a=1&b=two+words"),
            .form([.init(name: "a", value: "1"), .init(name: "b", value: "two words")])
        )
    }

    func testModes() {
        XCTAssertEqual(CaptureBody.modes(.json(.null)), [.tree, .pretty, .raw, .hex])
        XCTAssertEqual(CaptureBody.modes(.binary), [.hex])
        XCTAssertEqual(CaptureBody.chosenMode(.json(.null), preferred: .raw), .raw)
        XCTAssertEqual(CaptureBody.chosenMode(.image, preferred: .raw), .preview)
        XCTAssertEqual(CaptureBody.chosenMode(.text, preferred: nil), .raw)
    }

    func testPairs() {
        XCTAssertEqual(CaptureBody.formPairs("a=1&&flag&b=%E4%B8%AD=x"), [
            .init(name: "a", value: "1"), .init(name: "flag", value: ""), .init(name: "b", value: "中=x"),
        ])
        XCTAssertEqual(CaptureBody.queryPairs("https://a.test/p?q=trail+shoes&page=2#top?x=1"), [
            .init(name: "q", value: "trail shoes"), .init(name: "page", value: "2"),
        ])
        XCTAssertEqual(CaptureBody.queryPairs("https://a.test/p#frag?x=1"), [])
        XCTAssertEqual(CaptureBody.queryPairs("https://a.test/p"), [])
    }

    func testWaterfall() {
        let start = Date(timeIntervalSince1970: 1_700_000_000)
        let bars = CaptureBody.waterfall([
            (start: start, end: start.addingTimeInterval(1)),
            (start: start.addingTimeInterval(1), end: start.addingTimeInterval(2)),
            (start: start.addingTimeInterval(3), end: nil),
        ], now: start.addingTimeInterval(4))
        XCTAssertEqual(bars.map(\.left), [0, 0.25, 0.75])
        XCTAssertEqual(bars.map(\.width), [0.25, 0.25, 0.25])
        XCTAssertEqual(bars.map(\.millis), [1000, 1000, nil])
        XCTAssertEqual(CaptureBody.waterfall([], now: start), [])
        // One instant request still gets a place on the axis.
        XCTAssertEqual(CaptureBody.waterfall([(start: start, end: start)], now: start).first?.left, 0)
    }
}
