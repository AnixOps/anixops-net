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

    func testDecryptedDomainsMakeAPluginOnTheirOwn() throws {
        var mappings = try DebugMappings().addingHost("API.example.com:443")
        mappings = try mappings.addingHost("*.cdn.example.com")
        mappings = try mappings.addingHost("https://api.example.com/v1")
        XCTAssertEqual(mappings.hosts, ["api.example.com", "*.cdn.example.com"])
        let text = try XCTUnwrap(mappings.pluginText)
        XCTAssertFalse(text.contains("[Rewrite]"))
        XCTAssertTrue(text.hasSuffix("\n[MITM]\nhostname = %APPEND% api.example.com, *.cdn.example.com\n"))
        XCTAssertTrue(mappings.decrypts("img.cdn.example.com"))
        XCTAssertFalse(mappings.decrypts("cdn.example.com"))
        XCTAssertNil(mappings.removingHost("api.example.com").removingHost("*.cdn.example.com").pluginText)
    }

    func testOnlyDomainNamesAreDecrypted() {
        XCTAssertEqual(DebugMappings.normalizeHost(" Example.com. "), "example.com")
        XCTAssertNil(DebugMappings.normalizeHost("localhost"))
        XCTAssertNil(DebugMappings.normalizeHost("10.0.0.1:443"))
        XCTAssertNil(DebugMappings.normalizeHost("https://1.2.3.4/x"))
        XCTAssertNil(DebugMappings.normalizeHost("bad host.com"))
        XCTAssertNil(DebugMappings.normalizeHost("-a.com"))
        XCTAssertThrowsError(try DebugMappings().addingHost("not a domain"))
    }

    func testTheListTakesWildcardsExclusionsAndPausedLines() throws {
        var mappings = try DebugMappings().addingHost("*.Example.com")
        mappings = try mappings.addingHost("img-?.cdn.*")
        mappings = try mappings.addingHost("-pay.example.com")
        mappings = try mappings.addingHost("ads.example.com", exclude: true)
        mappings = try mappings.addingHost("old.example.org")
        mappings = mappings.settingHost("old.example.org", enabled: false)
        XCTAssertEqual(mappings.hosts, ["*.example.com", "img-?.cdn.*"])
        XCTAssertEqual(mappings.excludedHosts, ["pay.example.com", "ads.example.com"])
        let text = try XCTUnwrap(mappings.pluginText)
        XCTAssertTrue(text.hasSuffix(
            "\n[MITM]\nhostname = %APPEND% *.example.com, img-?.cdn.*, -pay.example.com, -ads.example.com\n"
        ))
        XCTAssertFalse(text.contains("old.example.org"), "a paused line does not reach the tunnel")

        XCTAssertTrue(mappings.decrypts("api.example.com"))
        XCTAssertFalse(mappings.decrypts("pay.example.com"), "an exclusion wins over *.example.com")
        XCTAssertTrue(mappings.excludes("pay.example.com"))
        XCTAssertTrue(mappings.decrypts("img-3.cdn.test"))
        XCTAssertFalse(mappings.decrypts("img-33.cdn.test"))
        XCTAssertFalse(mappings.decrypts("old.example.org"))

        // Turning a line into the other kind keeps one line per pattern.
        let flipped = try mappings.addingHost("pay.example.com")
        XCTAssertTrue(flipped.decrypts("pay.example.com"))
        XCTAssertEqual(flipped.domains.filter { $0.pattern == "pay.example.com" }.count, 1)

        // Editing keeps the place and refuses a pattern another line has.
        let edited = try mappings.editingHost("img-?.cdn.*", to: "video.cdn.test", exclude: false)
        XCTAssertEqual(edited.domains[1], DebugMappings.Domain(pattern: "video.cdn.test"))
        XCTAssertThrowsError(try mappings.editingHost("img-?.cdn.*", to: "*.example.com", exclude: false)) { error in
            XCTAssertEqual(error as? DebugMappingsError, .duplicateHost)
        }
        XCTAssertNil(mappings.removingHost("*.example.com").removingHost("img-?.cdn.*")
            .removingHost("pay.example.com").removingHost("ads.example.com").pluginText)
    }

    func testAnExclusionAloneStillMakesAPlugin() throws {
        // It narrows what the imported plugins decrypt.
        let mappings = try DebugMappings().addingHost("-bank.example.com")
        XCTAssertTrue(try XCTUnwrap(mappings.pluginText).hasSuffix("hostname = %APPEND% -bank.example.com\n"))
    }

    func testPatternsKeepToHostNames() {
        XCTAssertEqual(DebugMappings.normalizePattern("*Spclient.spotify.com"), "*spclient.spotify.com")
        XCTAssertEqual(DebugMappings.normalizePattern("https://*.example.com:443/x"), "*.example.com")
        XCTAssertEqual(DebugMappings.normalizePattern("api.example.com"), "api.example.com")
        XCTAssertNil(DebugMappings.normalizePattern("*"), "everything is refused")
        XCTAssertNil(DebugMappings.normalizePattern("*.*"))
        XCTAssertNil(DebugMappings.normalizePattern("a b*.com"))
        XCTAssertNil(DebugMappings.normalizePattern("*..example.com"))
        XCTAssertThrowsError(try DebugMappings().addingHost("-"))
    }

    func testOlderFilesLoadAsLinesThatAreOn() throws {
        let old = try JSONDecoder().decode(
            DebugMappings.self,
            from: Data(#"{"entries":[],"hosts":["api.example.com","*.cdn.test"]}"#.utf8)
        )
        XCTAssertEqual(old.domains, [.init(pattern: "api.example.com"), .init(pattern: "*.cdn.test")])
        // New files still carry `hosts` for older builds.
        let paused = old.settingHost("*.cdn.test", enabled: false)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(paused)) as? [String: Any])
        XCTAssertEqual(json["hosts"] as? [String], ["api.example.com"])
        XCTAssertEqual((json["domains"] as? [Any])?.count, 2)
        XCTAssertEqual(try JSONDecoder().decode(DebugMappings.self, from: JSONEncoder().encode(paused)), paused)
    }

    func testDomainsSurviveStorageAndOldFilesStillLoad() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".json")
        defer { try? FileManager.default.removeItem(at: url) }
        let mappings = try DebugMappings().addingHost("api.example.com")
        try mappings.save(to: url)
        XCTAssertEqual(try DebugMappings.load(from: url), mappings)
        try Data(#"{"entries":[]}"#.utf8).write(to: url)
        XCTAssertEqual(try DebugMappings.load(from: url).hosts, [])
    }

    private func bigLocal(_ index: Int) -> DebugMappings.Entry {
        DebugMappings.Entry(
            kind: .local,
            pattern: "https://api.example.com/\(index)",
            body: Data(repeating: UInt8(ascii: "a"), count: DebugMappings.maxBodyBytes)
        )
    }

    /// The generated plugin carries each Map Local body in Base64 (4/3 of its size), so eleven full bodies fit the
    /// mappings' 4 MiB and the twelfth is refused, while editing to something smaller, turning off and deleting
    /// always work.
    func testTheGeneratedPluginStaysWithinItsBudget() throws {
        var mappings = DebugMappings()
        for index in 0..<11 {
            mappings = try mappings.saving(bigLocal(index))
        }
        XCTAssertLessThanOrEqual(mappings.pluginBytes, DebugMappings.maxPluginBytes)
        XCTAssertThrowsError(try mappings.saving(bigLocal(11))) { error in
            XCTAssertEqual(error as? DebugMappingsError, .totalTooLarge)
        }
        XCTAssertEqual(mappings.entries.count, 11)

        // Over the budget, as an earlier version could save it: shrinking, pausing and deleting still pass.
        var over = mappings
        over.entries.append(bigLocal(11))
        over.entries.append(bigLocal(12))
        XCTAssertGreaterThan(over.pluginBytes, DebugMappings.maxPluginBytes)
        var smaller = over.entries[0]
        smaller.body = Data("{}".utf8)
        XCTAssertNoThrow(try over.saving(smaller))
        var paused = over
        paused.entries[1].enabled = false
        XCTAssertNoThrow(try paused.checkBudget(replacing: over, importedBytes: 0))
        var deleted = over
        deleted.entries.removeLast()
        XCTAssertNoThrow(try deleted.checkBudget(replacing: over, importedBytes: 0))
        // Turning a paused entry back on makes the plugin larger again.
        XCTAssertThrowsError(try over.checkBudget(replacing: paused, importedBytes: 0)) { error in
            XCTAssertEqual(error as? DebugMappingsError, .totalTooLarge)
        }
    }

    /// With the enabled plugins, the mappings' plugin must fit the engine's 8 MiB decryption budget: one more mapping
    /// or one more decrypted domain is refused when the plugins already take nearly all of it.
    func testMappingsShareTheDecryptionBudgetWithThePlugins() throws {
        let mappings = try DebugMappings().saving(bigLocal(0))
        let nearlyFull = EngineBudget.maxDecryptionBytes - mappings.pluginBytes
        XCTAssertNoThrow(try mappings.checkBudget(replacing: DebugMappings(), importedBytes: nearlyFull))
        let more = try mappings.saving(bigLocal(1))
        XCTAssertThrowsError(try more.checkBudget(replacing: mappings, importedBytes: nearlyFull)) { error in
            XCTAssertEqual(error as? DebugMappingsError, .decryptionBudgetFull)
        }
        let host = try mappings.addingHost("cdn.example.net")
        XCTAssertThrowsError(try host.checkBudget(replacing: mappings, importedBytes: nearlyFull)) { error in
            XCTAssertEqual(error as? DebugMappingsError, .decryptionBudgetFull)
        }
        XCTAssertEqual(EngineBudget.decryptionBytes([mappings.pluginText ?? ""]), mappings.pluginBytes)
    }
}
