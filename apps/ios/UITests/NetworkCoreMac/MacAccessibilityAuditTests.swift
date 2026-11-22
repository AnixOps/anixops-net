import XCTest

/// Xcode's accessibility audit on every page of the Mac app, in both themes and both languages (docs/ui-ux.md,
/// 自动检查, 第二层): contrast on the real, layered colours, elements a screen reader cannot find or name, hit
/// regions, and the rest of `XCUIAccessibilityAuditType.all`. Every issue of every page is collected first and the
/// test fails once with the whole list, so one CI run shows everything to fix. No issue in the app's own windows is
/// waived. The Touch Bar a text field puts up (emoji and candidate pickers) belongs to the system, not to the app,
/// so issues on it are counted and named in the log but do not fail the test.
///
/// Every page's window and every element with an issue are attached to the result as pictures
/// (`<place> #<n>`), which CI exports next to the result bundle, so a finding can be seen, not only read.
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
                let window = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
                window.name = place
                window.lifetime = .keepAlways
                add(window)
                let found = try AuditIssue.audit(app, on: place, numberedFrom: issues.count + 1)
                issues += found.issues
                for attachment in found.pictures {
                    add(attachment)
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
    /// Runs the audit and returns every issue it found. XCTest gives up on an audit that takes longer than its
    /// own limit ("Audit failed to complete in time", code -56), which happens on a busy CI machine; that is not a
    /// finding about the page, so the audit runs again, up to three times in all. Any other error, and a third
    /// time-out, fails the test.
    static func audit(_ app: XCUIApplication, on place: String, numberedFrom first: Int) throws
        -> (issues: [String], pictures: [XCTAttachment]) {
        var attempt = 1
        while true {
            var found: [XCUIAccessibilityAuditIssue] = []
            do {
                try app.performAccessibilityAudit(for: .all) { issue in
                    found.append(issue)
                    return true
                }
            } catch let error as NSError where error.domain == "com.apple.xcode.xctest.accessibilityAudit" && error.code == -56 && attempt < 3 {
                attempt += 1
                continue
            }
            let system = touchBarElements(of: app)
            let window = app.windows.firstMatch.frame
            var issues: [String] = []
            var pictures: [XCTAttachment] = []
            for issue in found {
                if let element = issue.element, system.contains(key(of: element)) {
                    print("[\(place)] system Touch Bar, not the app: \(describe(issue, on: place, number: nil))")
                    continue
                }
                let number = first + issues.count
                issues.append(describe(issue, on: place, number: number))
                if let element = issue.element, element.exists, window.contains(element.frame), !element.frame.isEmpty {
                    let picture = XCTAttachment(screenshot: element.screenshot())
                    picture.name = "\(place) #\(number)"
                    picture.lifetime = .keepAlways
                    pictures.append(picture)
                }
            }
            return (issues, pictures)
        }
    }

    /// The elements of the system Touch Bar, by type, label and frame (XCUIElement has no identity to compare).
    private static func touchBarElements(of app: XCUIApplication) -> Set<String> {
        var keys = Set<String>()
        for bar in app.touchBars.allElementsBoundByIndex {
            keys.insert(key(of: bar))
            for element in bar.descendants(matching: .any).allElementsBoundByIndex {
                keys.insert(key(of: element))
            }
        }
        return keys
    }

    private static func key(of element: XCUIElement) -> String {
        "\(element.elementType.rawValue)|\(element.identifier)|\(element.label)|\(element.frame)"
    }

    /// The issue, its element and where it is: the frame in screen points and, for an element without a label, the
    /// first texts inside it, so a group or a cell can be found in the code.
    static func describe(_ issue: XCUIAccessibilityAuditIssue, on place: String, number: Int?) -> String {
        let element = issue.element.map { element in
            let label = element.label.isEmpty ? "" : " \"\(element.label.prefix(40))\""
            let identifier = element.identifier.isEmpty ? "" : " #\(element.identifier)"
            let frame = element.frame
            let at = " @\(Int(frame.minX)),\(Int(frame.minY)) \(Int(frame.width))x\(Int(frame.height))"
            var inside = ""
            if element.label.isEmpty {
                let query = element.descendants(matching: .staticText)
                let texts = (0..<min(query.count, 3)).map { String(query.element(boundBy: $0).label.prefix(24)) }
                if !texts.isEmpty {
                    inside = " containing \(texts)"
                }
            }
            return "\(element.elementType)\(identifier)\(label)\(at)\(inside)"
        } ?? "(no element)"
        let tag = number.map { "#\($0) " } ?? ""
        return "\(tag)[\(place)] \(issue.compactDescription): \(element) — \(issue.detailedDescription)"
    }
}
