import XCTest

/// Xcode's accessibility audit on every page of the Mac app, in both themes and both languages (docs/ui-ux.md,
/// 自动检查, 第二层): contrast on the real, layered colours, elements a screen reader cannot find or name, hit
/// regions, and the rest of `XCUIAccessibilityAuditType.all`. Every issue of every page is collected first and the
/// test fails once with the whole list, so one CI run shows everything to fix. No issue is waived.
///
/// Launches like MacEntryInvariantTests: the debug build with demo data, by path (`TEST_RUNNER_ANIXOPS_APP`).
final class MacAccessibilityAuditTests: XCTestCase {
    private static let pages: [(mode: String, page: String)] =
        ["home", "proxies", "rules", "extensions", "activity", "settings"].map { ("proxy", $0) }
        + ["activity", "extensions", "exit", "settings"].map { ("debug", $0) }

    // One test per theme, so CI can run the two side by side (-only-testing).
    func testLightTheme() throws {
        try auditEveryPage(appearance: "light")
    }

    func testDarkTheme() throws {
        try auditEveryPage(appearance: "dark")
    }

    private func auditEveryPage(appearance: String) throws {
        var issues: [String] = []
        for language in ["zh-Hans", "en"] {
            for (mode, page) in Self.pages {
                let app = try launch(mode: mode, page: page, appearance: appearance, language: language)
                let place = "\(mode) \(page) \(appearance) \(language)"
                try app.performAccessibilityAudit(for: .all) { issue in
                    issues.append(AuditIssue.describe(issue, on: place))
                    return true
                }
                app.terminate()
            }
        }
        XCTAssertTrue(issues.isEmpty, "\(issues.count) accessibility issue(s):\n" + issues.joined(separator: "\n"))
    }

    private func launch(mode: String, page: String, appearance: String, language: String) throws -> XCUIApplication {
        let terms = try XCTUnwrap(ProcessInfo.processInfo.environment["ANIXOPS_TERMS"], "set TEST_RUNNER_ANIXOPS_TERMS")
        let path = try XCTUnwrap(ProcessInfo.processInfo.environment["ANIXOPS_APP"], "set TEST_RUNNER_ANIXOPS_APP")
        let app = XCUIApplication(url: URL(fileURLWithPath: path))
        app.launchArguments = [
            "-AnixOpsDemo", "YES",
            "-AnixOpsMode", mode,
            "-AnixOpsSection", page,
            "-appearance", appearance,
            "-AppleLanguages", "(\(language))",
            "-terms-accepted-version", terms,
        ]
        app.launch()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 30), "\(mode) \(page): no window appeared")
        // The demo data fills lists and charts over the first seconds; audit what a user would look at.
        Thread.sleep(forTimeInterval: 2)
        return app
    }
}

enum AuditIssue {
    static func describe(_ issue: XCUIAccessibilityAuditIssue, on place: String) -> String {
        let element = issue.element.map { element in
            let label = element.label.isEmpty ? "" : " \"\(element.label.prefix(40))\""
            let identifier = element.identifier.isEmpty ? "" : " #\(element.identifier)"
            return "\(element.elementType)\(identifier)\(label)"
        } ?? "(no element)"
        return "[\(place)] \(issue.compactDescription): \(element) — \(issue.detailedDescription)"
    }
}
