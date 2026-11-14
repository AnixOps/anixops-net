import Foundation
@testable import NetworkCoreBridge
import XCTest

final class RequestReplayTests: XCTestCase {
    func testCapturedDropsPseudoAndManagedHeaders() {
        let (replay, complete) = RequestReplay.captured(
            method: "POST",
            url: "https://api.example.com/v1/items?id=1",
            headers: [
                (":authority", "api.example.com"),
                ("Host", "api.example.com"),
                ("Content-Length", "2"),
                ("Content-Type", "application/json"),
                ("Authorization", "Bearer t"),
            ],
            body: Data("{}".utf8),
            bodyTruncated: false
        )
        XCTAssertTrue(complete)
        XCTAssertEqual(replay.headers, [
            .init(name: "Content-Type", value: "application/json"),
            .init(name: "Authorization", value: "Bearer t"),
        ])
        XCTAssertEqual(replay.body, Data("{}".utf8))
        XCTAssertEqual(replay.headerText, "Content-Type: application/json\nAuthorization: Bearer t")
    }

    func testTruncatedBodyIsNotComplete() {
        let (replay, complete) = RequestReplay.captured(
            method: "PUT", url: "https://example.com/", headers: [], body: Data(repeating: 1, count: 10), bodyTruncated: true
        )
        XCTAssertFalse(complete)
        XCTAssertTrue(replay.body.isEmpty)
    }

    func testHeaderTextParsing() throws {
        let headers = try RequestReplay.headers(from: "Accept: */*\n\n  X-Trace :  a:b  \n")
        XCTAssertEqual(headers, [.init(name: "Accept", value: "*/*"), .init(name: "X-Trace", value: "a:b")])
        XCTAssertThrowsError(try RequestReplay.headers(from: "no colon here"))
        XCTAssertThrowsError(try RequestReplay.headers(from: ": value"))
        XCTAssertThrowsError(try RequestReplay.headers(from: "Bad Name: value"))
    }

    func testURLRequest() throws {
        let replay = RequestReplay(
            method: "post",
            url: " https://example.com/a?b=1 ",
            headers: [.init(name: "X-A", value: "1"), .init(name: "Host", value: "evil")],
            body: Data("x".utf8)
        )
        let request = try replay.urlRequest()
        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(request.url?.absoluteString, "https://example.com/a?b=1")
        XCTAssertEqual(request.value(forHTTPHeaderField: "X-A"), "1")
        XCTAssertNil(request.value(forHTTPHeaderField: "Host"))
        XCTAssertEqual(request.httpBody, Data("x".utf8))
        XCTAssertFalse(request.httpShouldHandleCookies)
    }

    func testURLRequestErrors() {
        XCTAssertThrowsError(try RequestReplay(method: "GE T", url: "https://example.com").urlRequest()) {
            XCTAssertEqual($0 as? RequestReplayError, .invalidMethod)
        }
        XCTAssertThrowsError(try RequestReplay(url: "ftp://example.com").urlRequest()) {
            XCTAssertEqual($0 as? RequestReplayError, .invalidURL)
        }
        XCTAssertThrowsError(try RequestReplay(url: "example.com/path").urlRequest()) {
            XCTAssertEqual($0 as? RequestReplayError, .invalidURL)
        }
        let big = RequestReplay(method: "POST", url: "https://example.com", body: Data(count: RequestReplay.maxBodyBytes + 1))
        XCTAssertThrowsError(try big.urlRequest()) {
            XCTAssertEqual($0 as? RequestReplayError, .bodyTooLarge)
        }
    }

    func testResultFromResponse() throws {
        let response = try XCTUnwrap(HTTPURLResponse(
            url: URL(string: "https://example.com")!,
            statusCode: 201,
            httpVersion: "HTTP/1.1",
            headerFields: ["Content-Type": "text/plain", "A": "1"]
        ))
        let result = ReplayResult(response: response, body: Data("ok".utf8), duration: 0.5)
        XCTAssertEqual(result.status, 201)
        XCTAssertEqual(result.headers.map(\.name), ["A", "Content-Type"])
        XCTAssertEqual(result.bodyText, "ok")
    }
}
