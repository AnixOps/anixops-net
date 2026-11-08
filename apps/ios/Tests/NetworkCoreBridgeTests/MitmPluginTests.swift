@testable import NetworkCoreBridge
import XCTest

final class MitmPluginTests: XCTestCase {
    private let adBlock = """
    #!name = Example Ad Block
    #!desc = Rejects ad endpoints

    [Rule]
    DOMAIN,ads.example.com,REJECT

    [Rewrite]
    ^https?://api\\.example\\.com/v1/ad reject-dict
    ^https?://old\\.example\\.com/(.*) 302 https://new.example.com/$1

    [Script]
    http-response ^https?://api\\.example\\.com/v1/feed script-path=feed.js, requires-body=true, tag=feed

    [MITM]
    hostname = %APPEND% api.example.com, Old.Example.com, -login.example.com
    """

    func testSummaryReadsNameHostsAndSectionCounts() throws {
        let plugin = try MitmPlugin(text: adBlock)

        XCTAssertEqual(plugin.name, "Example Ad Block")
        XCTAssertTrue(plugin.isEnabled)
        let summary = plugin.summary
        XCTAssertEqual(summary.hostnames, ["api.example.com", "old.example.com"])
        XCTAssertEqual(summary.rewriteLines, 2)
        XCTAssertEqual(summary.scriptLines, 1)
        XCTAssertEqual(summary.ruleLines, 1)
    }

    func testRejectsEmptyOversizedAndInertText() {
        XCTAssertThrowsError(try MitmPlugin(text: "  \n")) { XCTAssertEqual($0 as? MitmPluginError, .empty) }
        XCTAssertThrowsError(try MitmPlugin(text: String(repeating: "#", count: MitmPlugin.maxBytes + 1))) {
            XCTAssertEqual($0 as? MitmPluginError, .tooLarge)
        }
        XCTAssertThrowsError(try MitmPlugin(text: "[Rule]\nDOMAIN,a.example,REJECT\n")) {
            XCTAssertEqual($0 as? MitmPluginError, .nothingToApply)
        }
        XCTAssertEqual(try MitmPlugin(text: "[MITM]\nhostname = a.example\n", fallbackName: "file").name, "file")
    }

    func testLibraryHandsOnlyEnabledPluginsToTheTunnelWhileSwitchedOn() throws {
        var library = MitmPluginLibrary()
        let first = try MitmPlugin(text: adBlock)
        let second = try MitmPlugin(text: "[MITM]\nhostname = b.example\n", fallbackName: "second")
        try library.add(first)
        try library.add(second)

        XCTAssertEqual(library.activeTexts, [], "the master switch starts off")
        library.isEnabled = true
        XCTAssertEqual(library.activeTexts, [first.text, second.text])
        try library.setEnabled(false, for: first.id)
        XCTAssertEqual(library.activeTexts, [second.text])
        library.remove(second.id)
        XCTAssertEqual(library.activeTexts, [])
        XCTAssertEqual(library.plugins.map(\.id), [first.id])
    }

    func testLibraryCapsTheEnabledTotal() throws {
        var library = MitmPluginLibrary()
        let body = "[MITM]\nhostname = a.example\n" + String(repeating: "#", count: 200 * 1024)
        try library.add(MitmPlugin(text: body))
        try library.add(MitmPlugin(text: body))
        XCTAssertThrowsError(try library.add(MitmPlugin(text: body))) {
            XCTAssertEqual($0 as? MitmPluginError, .libraryFull)
        }
        XCTAssertEqual(library.plugins.count, 2)
    }

    func testLibraryAndProfileValueRoundTrip() throws {
        var library = MitmPluginLibrary()
        library.isEnabled = true
        try library.add(MitmPlugin(text: adBlock))
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("mitm-plugins-\(UUID()).json")
        defer { try? FileManager.default.removeItem(at: url) }

        try library.save(to: url)
        XCTAssertEqual(try MitmPluginLibrary.load(from: url), library)
        XCTAssertEqual(
            try MitmPluginLibrary.load(from: url.appendingPathExtension("missing")),
            MitmPluginLibrary()
        )
        let value = try MitmPluginLibrary.profileValue(library.activeTexts)
        XCTAssertEqual(MitmPluginLibrary.texts(fromProfileValue: value), [adBlock])
        XCTAssertEqual(MitmPluginLibrary.texts(fromProfileValue: nil), [])
    }

    func testLargePluginsAreCompressedInTheProfile() throws {
        var text = "#!name = Large\n[Rule]\n"
        var index = 0
        while text.utf8.count < 2 * 1024 * 1024 {
            text += "DOMAIN-SUFFIX,ads\(index).example.com,REJECT\n"
            index += 1
        }
        text += "[MITM]\nhostname = large.example.com\n"
        let plugin = try MitmPlugin(text: text)
        var library = MitmPluginLibrary()
        library.isEnabled = true
        try library.add(plugin)

        let value = try MitmPluginLibrary.profileValue(library.activeTexts)
        XCTAssertLessThan(value.count, text.utf8.count / 4)
        XCTAssertEqual(MitmPluginLibrary.texts(fromProfileValue: value), [text])
        // A profile written before compression is plain JSON.
        let legacy = try JSONEncoder().encode([text])
        XCTAssertEqual(MitmPluginLibrary.texts(fromProfileValue: legacy), [text])
    }

    func testRuntimeConfigCarriesMitmOnlyWhenAddedAndNeverInTheProfile() throws {
        let node = ShadowsocksNode(
            host: "203.0.113.7",
            port: 8388,
            method: "2022-blake3-aes-128-gcm",
            password: "YWJjZGVmZ2hpamtsbW5vcA==",
            name: "node"
        )
        let authority = try MitmCertificateAuthority.generate()
        let config = EmbeddedRuntimeConfig(node: node, profileID: "default")

        XCTAssertNil(try json(config)["mitm"])
        let decrypting = try config.withMitm(plugins: [adBlock], authority: authority)
        let mitm = try XCTUnwrap(try json(decrypting)["mitm"] as? [String: Any])
        XCTAssertEqual(mitm["plugins"] as? [String], [adBlock])
        XCTAssertEqual(mitm["ca_certificate"] as? String, authority.certificateDER.base64EncodedString())
        XCTAssertEqual(
            mitm["ca_private_key"] as? String,
            try authority.privateKeyRepresentation().base64EncodedString()
        )
        XCTAssertEqual(try EmbeddedRuntimeConfig.decoded(from: decrypting.encoded()), decrypting)
        XCTAssertNil(decrypting.withoutSecret().mitm, "the root key never goes into the tunnel profile")
        XCTAssertNil(decrypting.withoutMitm().mitm)
        XCTAssertNil(try config.withMitm(plugins: [], authority: authority).mitm)
    }

    private func json(_ config: EmbeddedRuntimeConfig) throws -> [String: Any] {
        try XCTUnwrap(JSONSerialization.jsonObject(with: config.encoded()) as? [String: Any])
    }

    // MARK: - Kernel inspection

    private let quantumultSnippet = """
    #!name = QX Snippet
    ^https?://a\\.example\\.com/s url script-response-body https://cdn.example.com/s.js
    ^https?://a\\.example\\.com/e url echo-response text/json echo-response https://cdn.example.com/e.json
    hostname = a.example.com
    """

    private var quantumultInspection: MitmPluginInspection {
        MitmPluginInspection(
            name: "QX Snippet",
            level: "full",
            hostnames: ["a.example.com"],
            lines: .init(supported: 3),
            resources: [
                .init(kind: "script", url: "https://cdn.example.com/s.js", line: 2),
                .init(kind: "map_local_data", url: "https://cdn.example.com/e.json", line: 3),
            ]
        )
    }

    func testKernelInspectionAcceptsSnippetsWithoutLoonSections() throws {
        XCTAssertThrowsError(try MitmPlugin(text: quantumultSnippet)) {
            XCTAssertEqual($0 as? MitmPluginError, .nothingToApply)
        }
        let plugin = try MitmPlugin(text: quantumultSnippet, inspection: quantumultInspection)
        XCTAssertEqual(plugin.name, "QX Snippet")
        XCTAssertEqual(plugin.scriptReferences.map(\.path), ["https://cdn.example.com/s.js"])
        XCTAssertEqual(plugin.dataReferences.map(\.url), [URL(string: "https://cdn.example.com/e.json")])

        let nothing = MitmPluginInspection(level: "unsupported", lines: .init(unsupported: 2))
        XCTAssertThrowsError(try MitmPlugin(text: adBlock, inspection: nothing)) {
            XCTAssertEqual($0 as? MitmPluginError, .nothingToApply)
        }
    }

    func testInspectionDecodesTheKernelJSONAndOldPluginsAreInspectedOnce() throws {
        let json = Data(#"""
        {"name":"QX Snippet","level":"partial","hostnames":["a.example.com"],"excluded_hostnames":["b.example.com"],
         "lines":{"supported":2,"partial":0,"unsupported":1,"deferred":0},
         "resources":[{"url":"https://cdn.example.com/e.json","kind":"map_local_data","line":3}],"report":"r"}
        """#.utf8)
        let decoded = try XCTUnwrap(MitmPluginInspection.decode(json))
        XCTAssertEqual(decoded.excludedHostnames, ["b.example.com"])
        XCTAssertEqual(decoded.lines.unsupported, 1)
        XCTAssertEqual(decoded.resources.first?.kind, MitmPluginInspection.Resource.mapLocalData)

        var library = MitmPluginLibrary()
        library.isEnabled = true
        try library.add(MitmPlugin(text: adBlock))
        XCTAssertNil(library.plugins[0].inspection)
        XCTAssertTrue(library.inspectMissing { _ in decoded })
        XCTAssertFalse(library.inspectMissing { _ in decoded })
        XCTAssertEqual(library.activeDataReferences.map(\.path), ["https://cdn.example.com/e.json"])
        XCTAssertEqual(library.allDataPaths, ["https://cdn.example.com/e.json"])
    }

    func testResourceLibrarySelectsTheEnabledBodiesWithinTheBudget() throws {
        var resources = MitmResourceLibrary()
        resources.store(Data("{}".utf8), for: "https://cdn.example.com/a.json")
        resources.store(Data(count: MitmResourceLibrary.maxBytes + 1), for: "https://cdn.example.com/big.bin")
        XCTAssertNil(resources.items["https://cdn.example.com/big.bin"])
        XCTAssertEqual(
            resources.data(for: ["https://cdn.example.com/a.json", "https://cdn.example.com/missing.json"]),
            ["https://cdn.example.com/a.json": Data("{}".utf8)]
        )
        let value = try MitmResourceLibrary.profileValue(resources.data(for: ["https://cdn.example.com/a.json"]))
        XCTAssertEqual(MitmResourceLibrary.data(fromProfileValue: value), ["https://cdn.example.com/a.json": Data("{}".utf8)])
        resources.keep(only: [])
        XCTAssertTrue(resources.items.isEmpty)

        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let assets = try MitmResourceLibrary.writeAssets(["https://cdn.example.com/a.json": Data("{}".utf8)], to: directory)
        let path = try XCTUnwrap(assets["https://cdn.example.com/a.json"])
        XCTAssertEqual(try Data(contentsOf: URL(fileURLWithPath: path)), Data("{}".utf8))
    }
}
