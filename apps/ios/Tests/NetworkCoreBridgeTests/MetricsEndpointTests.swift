@testable import NetworkCoreBridge
import XCTest

final class MetricsEndpointTests: XCTestCase {
    private func head(_ text: String) -> Data {
        Data(text.utf8)
    }

    func testRoutesLikeTheDesktopEndpoint() {
        XCTAssertEqual(MetricsEndpoint.route(head("GET /metrics HTTP/1.1\r\nHost: 127.0.0.1:9477\r\n\r\n")), .metrics)
        XCTAssertEqual(MetricsEndpoint.route(head("GET /metrics?x=1 HTTP/1.0\r\nhost: localhost:9477\r\n\r\n")), .metrics)
        XCTAssertEqual(MetricsEndpoint.route(head("GET / HTTP/1.1\r\nHost: [::1]:9477\r\n\r\n")), .notFound)
        XCTAssertEqual(MetricsEndpoint.route(head("POST /metrics HTTP/1.1\r\nHost: 127.0.0.1:9477\r\n\r\n")), .methodNotAllowed)
        XCTAssertEqual(MetricsEndpoint.route(head("hello\r\n\r\n")), .badRequest)
        XCTAssertTrue(MetricsEndpoint.isComplete(head("GET /metrics HTTP/1.1\r\n\r\n")))
        XCTAssertFalse(MetricsEndpoint.isComplete(head("GET /metrics HTTP/1.1\r\n")))
    }

    func testRefusesOtherHosts() throws {
        // A web page that rebinds its own name to 127.0.0.1, another port, no port, or no Host at all.
        for host in ["rebound.example:9477", "127.0.0.1:9478", "127.0.0.1"] {
            XCTAssertEqual(MetricsEndpoint.route(head("GET /metrics HTTP/1.1\r\nHost: \(host)\r\n\r\n")), .forbidden, host)
        }
        XCTAssertEqual(MetricsEndpoint.route(head("GET /metrics HTTP/1.1\r\n\r\n")), .forbidden)
        XCTAssertEqual(MetricsEndpoint.route(head("GET /metrics HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n"), port: 1234), .metrics)
        let refused = try XCTUnwrap(String(data: MetricsEndpoint.response(.forbidden, body: ""), encoding: .utf8))
        XCTAssertTrue(refused.hasPrefix("HTTP/1.1 403 "))
    }

    func testResponseCarriesLengthAndType() throws {
        let data = MetricsEndpoint.response(.metrics, body: "a 1\n")
        let text = try XCTUnwrap(String(data: data, encoding: .utf8))
        XCTAssertTrue(text.hasPrefix("HTTP/1.1 200 OK\r\n"))
        XCTAssertTrue(text.contains("Content-Type: text/plain; version=0.0.4; charset=utf-8\r\n"))
        XCTAssertTrue(text.contains("Content-Length: 4\r\n"))
        XCTAssertTrue(text.hasSuffix("\r\n\r\na 1\n"))
        let refused = try XCTUnwrap(String(data: MetricsEndpoint.response(.methodNotAllowed, body: ""), encoding: .utf8))
        XCTAssertTrue(refused.hasPrefix("HTTP/1.1 405 "))
        XCTAssertTrue(refused.contains("Allow: GET\r\n"))
    }

    func testCombinesEngineTextWithClientGauges() {
        let client = MetricsEndpoint.clientGauges(
            platform: "macos",
            version: "0.3.0 (\"6\")",
            connected: true,
            crashReportsEnabled: false,
            crashReportsKept: 2,
            integrity: "verified"
        )
        XCTAssertTrue(client.contains("anixops_client_info{platform=\"macos\",version=\"0.3.0 (\\\"6\\\")\"} 1\n"))
        XCTAssertTrue(client.contains("anixops_client_connected 1\n"))
        XCTAssertTrue(client.contains("anixops_client_crash_reports_kept 2\n"))
        XCTAssertTrue(client.contains("anixops_client_integrity{state=\"verified\"} 1\n"))
        XCTAssertTrue(client.contains("anixops_client_integrity{state=\"damaged\"} 0\n"))

        let answer = Data(#"{"content_type":"text/plain","text":"networkcore_up 1"}"#.utf8)
        let engine = MetricsEndpoint.engineText(answer)
        XCTAssertEqual(engine, "networkcore_up 1")
        XCTAssertEqual(MetricsEndpoint.combine(engine: engine, client: "c 1\n"), "networkcore_up 1\nc 1\n")
        XCTAssertEqual(MetricsEndpoint.combine(engine: nil, client: "c 1\n"), "c 1\n")
        XCTAssertNil(MetricsEndpoint.engineText(nil))
    }
}
