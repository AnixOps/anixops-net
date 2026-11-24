@testable import NetworkCoreBridge
import XCTest

/// The app's interface text is Simplified Chinese looked up in Localizable.xcstrings; a message with no entry shows
/// Simplified Chinese to English and Traditional Chinese users. `.github/scripts/apple_strings.py` checks the
/// interface sources against the catalogue; these tests pin the messages this module builds with
/// `String(localized:)`, which reach the screen through a String (a test bundle has no catalogue, so the lookup
/// returns the key itself).
final class LocalizationCatalogueTests: XCTestCase {
    private static func catalogue() throws -> [String: Any] {
        let ios = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let url = ios.appendingPathComponent("Sources/NetworkCoreApp/Resources/Localizable.xcstrings")
        let object = try JSONSerialization.jsonObject(with: Data(contentsOf: url))
        let root = try XCTUnwrap(object as? [String: Any])
        XCTAssertEqual(root["sourceLanguage"] as? String, "zh-Hans")
        return try XCTUnwrap(root["strings"] as? [String: Any])
    }

    /// The translation of `key` into `language`, when it is there and marked translated.
    private static func translation(_ strings: [String: Any], _ key: String, _ language: String) -> String? {
        guard let entry = strings[key] as? [String: Any],
              let localizations = entry["localizations"] as? [String: Any],
              let localization = localizations[language] as? [String: Any],
              let unit = localization["stringUnit"] as? [String: Any],
              unit["state"] as? String == "translated"
        else {
            return nil
        }
        return unit["value"] as? String
    }

    private func assertTranslated(_ keys: [String], file: StaticString = #filePath, line: UInt = #line) throws {
        let strings = try Self.catalogue()
        for key in keys {
            for language in ["en", "zh-Hant"] {
                let value = Self.translation(strings, key, language)
                XCTAssertNotNil(value, "no \(language) translation for “\(key)”", file: file, line: line)
                if language == "en", let value {
                    XCTAssertNil(value.range(of: #"\p{Han}"#, options: .regularExpression),
                                 "the English for “\(key)” is still Chinese", file: file, line: line)
                }
            }
        }
    }

    func testEveryEntryHasEnglishAndTraditionalChinese() throws {
        let strings = try Self.catalogue()
        for key in strings.keys {
            XCTAssertNotNil(Self.translation(strings, key, "en"), "no en translation for “\(key)”")
            XCTAssertNotNil(Self.translation(strings, key, "zh-Hant"), "no zh-Hant translation for “\(key)”")
        }
    }

    func testMappingValidationMessagesAreTranslated() throws {
        let errors: [DebugMappingsError] = [
            .emptyPattern, .invalidURL, .invalidRegex, .invalidStatus, .invalidHeader, .bodyTooLarge, .invalidTarget,
        ]
        try assertTranslated(errors.map(\.message))
    }

    func testReplayValidationMessagesAreTranslated() throws {
        let errors: [RequestReplayError] = [.invalidMethod, .invalidURL, .bodyTooLarge, .notHTTP]
        try assertTranslated(errors.map(\.message))
    }

    func testRedirectValidationMessagesAreTranslated() throws {
        let errors: [TrafficRedirectsError] = [.invalidSource, .invalidPort, .invalidTarget, .emptyMatch]
        try assertTranslated(errors.map(\.message))
    }

    /// The first screen after the terms: every word of it is looked up (AppModeViews.swift).
    func testFirstLaunchModeChoiceIsTranslated() throws {
        try assertTranslated([
            "选择使用方式",
            "调试网络",
            "抓包、解密 HTTPS、改写和重放请求。无需节点，流量默认直连。",
            "代理",
            "导入订阅或节点，按规则把流量交给代理或直连。",
            "继续",
            "可在设置中切换，不会删除订阅、节点和插件。",
        ])
    }

    /// The fixed keyboard shortcut titles are catalogue keys the shortcut list looks up.
    func testShortcutTitlesAreCatalogueKeys() throws {
        let strings = try Self.catalogue()
        for shortcut in AppShortcut.all {
            let key = String(localized: shortcut.title)
            XCTAssertNotNil(Self.translation(strings, key, "en"), "no en translation for shortcut “\(key)”")
        }
    }
}
