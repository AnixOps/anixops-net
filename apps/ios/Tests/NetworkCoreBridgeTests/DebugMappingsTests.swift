@testable import NetworkCoreBridge
import XCTest

final class DebugMappingsTests: XCTestCase {
    func testURLMatchKeepsTheQueryForMapRemote() throws {
        let entry = DebugMappings.Entry(
            kind: .remote,
            match: .url,
            pattern: "https://api.example.com/v1/users",
            target: "https://staging.example.com/v1/users"
        )
        try entry.validate()
        XCTAssertEqual(entry.regex, #"^https://api\.example\.com/v1/users(\?.*)?$"#)
        XCTAssertEqual(
            entry.line,
            #"^https://api\.example\.com/v1/users(\?.*)?$ 307 https://staging.example.com/v1/users$1"#
        )
        XCTAssertEqual(entry.host, "api.example.com")
        let regex = try NSRegularExpression(pattern: entry.regex)
        let url = "https://api.example.com/v1/users?page=2"
        XCTAssertNotNil(regex.firstMatch(in: url, range: NSRange(url.startIndex..., in: url)))
        let other = "https://api.example.com/v1/users/7"
        XCTAssertNil(regex.firstMatch(in: other, range: NSRange(other.startIndex..., in: other)))
    }

    func testPrefixMatchCarriesTheRestOfThePath() {
        let entry = DebugMappings.Entry(
            kind: .remote,
            match: .prefix,
            pattern: "https://api.example.com/v1/",
            target: "http://192.168.1.20:8080/v1/"
        )
        XCTAssertEqual(entry.regex, #"^https://api\.example\.com/v1/(.*)$"#)
        XCTAssertTrue(entry.line.hasSuffix(" 307 http://192.168.1.20:8080/v1/$1"))
    }

    func testMapLocalLineCarriesBodyStatusAndHeaders() throws {
        let body = Data(#"{"ok": true}"#.utf8)
        let entry = DebugMappings.Entry(
            kind: .local,
            pattern: "https://api.example.com/config",
            status: 503,
            headers: [.init(name: "Content-Type", value: "application/json"), .init(name: "X-Note", value: "a \"b\"")],
            body: body
        )
        try entry.validate()
        XCTAssertEqual(
            entry.line,
            #"^https://api\.example\.com/config(\?.*)?$ data-type=base64 data="\#(body.base64EncodedString())" status-code=503 header="Content-Type:application/json|X-Note:a \"b\"""#
        )
    }

    func testPluginTextHasEachSectionAndTheHostsOnce() throws {
        var mappings = DebugMappings()
        mappings = try mappings.saving(.init(kind: .local, pattern: "https://a.example.com/x", body: Data("x".utf8)))
        mappings = try mappings.saving(.init(kind: .remote, match: .prefix, pattern: "https://a.example.com/y/", target: "https://b.example.com/y/"))
        mappings = try mappings.saving(.init(enabled: false, kind: .local, pattern: "https://c.example.com/"))
        let text = try XCTUnwrap(mappings.pluginText)
        XCTAssertTrue(text.hasPrefix("#!name=\(DebugMappings.pluginName)\n"))
        XCTAssertTrue(text.contains("\n[Rewrite]\n^https://a\\.example\\.com/y/(.*)$ 307 https://b.example.com/y/$1\n"))
        XCTAssertTrue(text.contains("\n[Map Local]\n^https://a\\.example\\.com/x(\\?.*)?$ data-type=base64"))
        XCTAssertTrue(text.hasSuffix("\n[MITM]\nhostname = %APPEND% a.example.com\n"))
        XCTAssertFalse(text.contains("c.example.com"))
    }

    func testNothingEnabledMeansNoPlugin() {
        XCTAssertNil(DebugMappings().pluginText)
        let off = DebugMappings(entries: [.init(enabled: false, kind: .local, pattern: "https://a.example.com/")])
        XCTAssertNil(off.pluginText)
    }

    func testRegexHostOnlyWhenLiteral() {
        XCTAssertEqual(DebugMappings.regexHost(#"^https://api\.example\.com/v1/.*"#), "api.example.com")
        XCTAssertEqual(DebugMappings.regexHost(#"^https?:\/\/api\.example\.com:8443/"#), "api.example.com")
        XCTAssertNil(DebugMappings.regexHost(#"^https://api.example.com/"#))
        XCTAssertNil(DebugMappings.regexHost(#"^https://(a|b)\.example\.com/"#))
        XCTAssertNil(DebugMappings.regexHost("example"))
    }

    func testValidationRefusesWhatTheEngineCannotUse() {
        func error(_ entry: DebugMappings.Entry) -> DebugMappingsError? {
            do {
                try entry.validate()
                return nil
            } catch {
                return error as? DebugMappingsError
            }
        }
        XCTAssertEqual(error(.init(kind: .local, pattern: " ")), .emptyPattern)
        XCTAssertEqual(error(.init(kind: .local, pattern: "ftp://a.example.com/")), .invalidURL)
        XCTAssertEqual(error(.init(kind: .local, match: .regex, pattern: "^https://(a")), .invalidRegex)
        XCTAssertEqual(error(.init(kind: .local, pattern: "https://a.example.com/", status: 99)), .invalidStatus)
        XCTAssertEqual(
            error(.init(kind: .local, pattern: "https://a.example.com/", headers: [.init(name: "X", value: "a|b")])),
            .invalidHeader
        )
        XCTAssertEqual(
            error(.init(kind: .local, pattern: "https://a.example.com/", body: Data(count: DebugMappings.maxBodyBytes + 1))),
            .bodyTooLarge
        )
        XCTAssertEqual(error(.init(kind: .remote, pattern: "https://a.example.com/", target: "not a url")), .invalidTarget)
    }

    func testFromACapturedRequest() {
        let entry = DebugMappings.local(
            url: "https://a.example.com/v1/list?page=1",
            status: 200,
            headers: [.init(name: "Content-Type", value: "application/json"), .init(name: "Content-Length", value: "5")],
            body: Data("hello".utf8)
        )
        XCTAssertEqual(entry.pattern, "https://a.example.com/v1/list")
        XCTAssertEqual(entry.headers, [.init(name: "Content-Type", value: "application/json")])
        XCTAssertEqual(entry.bodyText, "hello")
        let remote = DebugMappings.remote(url: "https://a.example.com/v1/list?page=1#top")
        XCTAssertEqual(remote.pattern, "https://a.example.com/v1/list")
        XCTAssertEqual(remote.target, "https://a.example.com/v1/list")
    }

    func testStorageRoundTrip() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".json")
        defer { try? FileManager.default.removeItem(at: url) }
        XCTAssertEqual(try DebugMappings.load(from: url), DebugMappings())
        let mappings = DebugMappings(entries: [.init(kind: .remote, pattern: "https://a.example.com/", target: "https://b.example.com/")])
        try mappings.save(to: url)
        XCTAssertEqual(try DebugMappings.load(from: url), mappings)
    }
}
