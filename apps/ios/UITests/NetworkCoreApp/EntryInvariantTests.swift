import XCTest

/// The entries every page must keep, whatever the navigation becomes (docs/ui-ux.md, 入口不变量). Debug mode once
/// lost its only connection switch on iOS when its overview went away, and a failure caused on another tab showed
/// only on the overview; these checks fail before a change like that ships:
///
/// 1. The connection switch: on the overview in proxy mode, and on every page of debug mode, which has no overview.
///    Exactly one wherever it is.
/// 2. A failure shows on the page the user is on.
///
/// Each case launches the debug build with the demo data (`-AnixOpsDemo YES`, as the screenshots do) on one page.
/// CI installs the app and passes its bundle identifier as `TEST_RUNNER_ANIXOPS_BUNDLE`, and the user agreement's
/// version as `TEST_RUNNER_ANIXOPS_TERMS`, so no case stops at the consent screen.
final class EntryInvariantTests: XCTestCase {
    private static let proxyPages = ["home", "proxies", "rules", "extensions", "settings"]
    private static let debugPages = ["requests", "extensions", "exit", "settings"]
    private static let notice = "UI test notice"

    override func setUp() {
        continueAfterFailure = false
    }

    func testDebugModeHasOneConnectionSwitchOnEveryPage() throws {
        for page in Self.debugPages {
            let app = try launch(mode: "debug", page: page)
            assertOne("connect-switch", in: app, on: "debug \(page)")
            app.terminate()
        }
    }

    func testProxyModeHasTheConnectionSwitchOnTheOverviewOnly() throws {
        for page in Self.proxyPages {
            let app = try launch(mode: "proxy", page: page)
            if page == "home" {
                assertOne("connect-switch", in: app, on: "proxy \(page)")
            } else {
                assertNone("connect-switch", in: app, on: "proxy \(page)")
            }
            app.terminate()
        }
    }

    func testAFailureShowsOnEveryPage() throws {
        for (mode, pages) in [("proxy", Self.proxyPages), ("debug", Self.debugPages)] {
            for page in pages {
                let app = try launch(mode: mode, page: page, notice: Self.notice)
                assertOne("app-notice", in: app, on: "\(mode) \(page)")
                app.terminate()
            }
        }
    }

    /// A language the app is not translated into reads English, as on every client (docs/ui-ux.md, 界面语言), not
    /// the Chinese the catalog is written in (finding ux-i18n-a11y-06).
    func testAnUntranslatedLanguageReadsEnglish() throws {
        let app = try launch(mode: "proxy", page: "home", language: "(ja)")
        XCTAssertTrue(app.tabBars.buttons["Rules"].waitForExistence(timeout: 10), "ja: the Rules tab is not in English")
        XCTAssertFalse(app.tabBars.buttons["规则"].exists, "ja: the tab bar fell back to Chinese")
        app.terminate()
    }

    private func launch(mode: String, page: String, notice: String? = nil, language: String = "(zh-Hans)") throws -> XCUIApplication {
        let terms = try XCTUnwrap(
            ProcessInfo.processInfo.environment["ANIXOPS_TERMS"],
            "set TEST_RUNNER_ANIXOPS_TERMS to the user agreement's version"
        )
        let bundle = try XCTUnwrap(
            ProcessInfo.processInfo.environment["ANIXOPS_BUNDLE"],
            "set TEST_RUNNER_ANIXOPS_BUNDLE to the installed app's bundle identifier"
        )
        let app = XCUIApplication(bundleIdentifier: bundle)
        var arguments = [
            "-AnixOpsDemo", "YES",
            "-AnixOpsMode", mode,
            "-AnixOpsSection", page,
            "-AppleLanguages", language,
            "-terms-accepted-version", terms,
        ]
        if let notice {
            arguments += ["-AnixOpsNotice", notice]
        }
        app.launchArguments = arguments
        app.launch()
        // On a cold CI simulator the first launch now and then never brings up the tab bar (the launch is still
        // settling, or it came up without its arguments, on the consent screen). That says nothing about the page:
        // launch it once more, and fail if the second launch does not show it either.
        if !app.tabBars.firstMatch.waitForExistence(timeout: 30) {
            app.terminate()
            app.launch()
        }
        XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 30), "\(mode) \(page): the tab bar never appeared")
        return app
    }

    private func elements(_ identifier: String, in app: XCUIApplication) -> XCUIElementQuery {
        app.descendants(matching: .any).matching(identifier: identifier)
    }

    private func assertOne(_ identifier: String, in app: XCUIApplication, on page: String) {
        let query = elements(identifier, in: app)
        XCTAssertTrue(query.firstMatch.waitForExistence(timeout: 10), "\(page): no \(identifier)")
        XCTAssertEqual(query.count, 1, "\(page): \(query.count) × \(identifier)")
    }

    private func assertNone(_ identifier: String, in app: XCUIApplication, on page: String) {
        let query = elements(identifier, in: app)
        XCTAssertFalse(query.firstMatch.waitForExistence(timeout: 3), "\(page): unexpected \(identifier)")
    }
}
