import XCTest

/// Xcode's accessibility audit on every tab of the iPhone app, in both themes and both languages (docs/ui-ux.md,
/// 自动检查, 第二层): contrast on the real, layered colours, elements a screen reader cannot find or name, hit
/// regions, and the rest of `XCUIAccessibilityAuditType.all`. Every issue of every page is collected first and the
/// test fails once with the whole list, so one CI run shows everything to fix. No issue is waived.
///
/// Launches like EntryInvariantTests: the debug build with demo data, installed in the simulator (`TEST_RUNNER_ANIXOPS_BUNDLE`). On iOS `.all` also covers Dynamic Type and text that is cut off.
final class AccessibilityAuditTests: XCTestCase {
    private static let pages: [(mode: String, page: String)] =
        ["home", "proxies", "rules", "extensions", "settings"].map { ("proxy", $0) }
        + ["requests", "extensions", "exit", "settings"].map { ("debug", $0) }

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
                issues += try AuditIssue.audit(app, on: place)
                app.terminate()
            }
        }
        XCTAssertTrue(issues.isEmpty, "\(issues.count) accessibility issue(s):\n" + issues.joined(separator: "\n"))
    }

    private func launch(mode: String, page: String, appearance: String, language: String) throws -> XCUIApplication {
        let terms = try XCTUnwrap(ProcessInfo.processInfo.environment["ANIXOPS_TERMS"], "set TEST_RUNNER_ANIXOPS_TERMS")
        let bundle = try XCTUnwrap(ProcessInfo.processInfo.environment["ANIXOPS_BUNDLE"], "set TEST_RUNNER_ANIXOPS_BUNDLE")
        let app = XCUIApplication(bundleIdentifier: bundle)
        app.launchArguments = [
            "-AnixOpsDemo", "YES",
            "-AnixOpsMode", mode,
            "-AnixOpsSection", page,
            "-appearance", appearance,
            "-AppleLanguages", "(\(language))",
            "-terms-accepted-version", terms,
        ]
        app.launch()
        XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 30), "\(mode) \(page): the tab bar never appeared")
        // The demo data fills lists and charts over the first seconds; audit what a user would look at.
        Thread.sleep(forTimeInterval: 2)
        return app
    }
}

enum AuditIssue {
    /// Runs the audit and returns every issue it found. XCTest gives up on an audit that takes longer than its
    /// own limit ("Audit failed to complete in time", code -56), which happens on a CI simulator that is still busy;
    /// that is not a finding about the page, so the audit runs again, up to three times in all. Any other error,
    /// and a third time-out, fails the test.
    static func audit(_ app: XCUIApplication, on place: String) throws -> [String] {
        var attempt = 1
        while true {
            var found: [String] = []
            do {
                try app.performAccessibilityAudit(for: .all) { issue in
                    found.append(describe(issue, on: place))
                    return true
                }
                return found
            } catch let error as NSError where error.domain == "com.apple.xcode.xctest.accessibilityAudit" && error.code == -56 && attempt < 3 {
                attempt += 1
            }
        }
    }

    static func describe(_ issue: XCUIAccessibilityAuditIssue, on place: String) -> String {
        let element = issue.element.map { element in
            let label = element.label.isEmpty ? "" : " \"\(element.label.prefix(40))\""
            let identifier = element.identifier.isEmpty ? "" : " #\(element.identifier)"
            return "\(element.elementType)\(identifier)\(label)"
        } ?? "(no element)"
        return "[\(place)] \(issue.compactDescription): \(element) — \(issue.detailedDescription)"
    }
}
