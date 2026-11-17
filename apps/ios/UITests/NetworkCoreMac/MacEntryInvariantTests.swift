import XCTest

/// The entries every page of the Mac app must keep, whatever the navigation becomes (docs/ui-ux.md, 入口不变量):
///
/// 1. The connection switch at the bottom of the sidebar, on every page of both modes. Exactly one there; the
///    overview's larger switch has its own identifier (`home-connect-switch`).
/// 2. A failure shows on the page the user is on: the overview's notice, or the notice above any other page.
///
/// Each case launches the debug build with the demo data (`-AnixOpsDemo YES`, as the screenshots do) on one page.
/// CI passes the user agreement's version as `TEST_RUNNER_ANIXOPS_TERMS`, so no case stops at the consent screen.
final class MacEntryInvariantTests: XCTestCase {
    private static let proxyPages = ["home", "proxies", "rules", "extensions", "activity", "settings"]
    private static let debugPages = ["activity", "extensions", "exit", "settings"]
    private static let notice = "UI test notice"

    override func setUp() {
        continueAfterFailure = false
    }

    func testEveryPageHasOneSidebarConnectionSwitch() throws {
        for (mode, pages) in [("proxy", Self.proxyPages), ("debug", Self.debugPages)] {
            for page in pages {
                let app = try launch(mode: mode, page: page)
                assertOne("connect-switch", in: app, on: "\(mode) \(page)")
                app.terminate()
            }
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

    private func launch(mode: String, page: String, notice: String? = nil) throws -> XCUIApplication {
        let terms = try XCTUnwrap(
            ProcessInfo.processInfo.environment["ANIXOPS_TERMS"],
            "set TEST_RUNNER_ANIXOPS_TERMS to the user agreement's version"
        )
        let app = XCUIApplication()
        var arguments = [
            "-AnixOpsDemo", "YES",
            "-AnixOpsMode", mode,
            "-AnixOpsSection", page,
            "-appearance", "light",
            "-AppleLanguages", "(zh-Hans)",
            "-terms-accepted-version", terms,
        ]
        if let notice {
            arguments += ["-AnixOpsNotice", notice]
        }
        app.launchArguments = arguments
        app.launch()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 30), "\(mode) \(page): no window appeared")
        return app
    }

    private func assertOne(_ identifier: String, in app: XCUIApplication, on page: String) {
        let query = app.windows.firstMatch.descendants(matching: .any).matching(identifier: identifier)
        XCTAssertTrue(query.firstMatch.waitForExistence(timeout: 10), "\(page): no \(identifier)")
        XCTAssertEqual(query.count, 1, "\(page): \(query.count) × \(identifier)")
    }
}
