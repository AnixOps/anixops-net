@testable import NetworkCoreBridge
import XCTest

final class CommandPaletteTests: XCTestCase {
    private let items = [
        CommandPalette.Item(id: "page.home", kind: .page, title: "概览", detail: "页面", shortcut: "⌘1"),
        CommandPalette.Item(id: "page.proxies", kind: .page, title: "代理", detail: "页面", shortcut: "⌘2"),
        CommandPalette.Item(id: "cmd.import", kind: .command, title: "导入节点", shortcut: "⌘I"),
        CommandPalette.Item(id: "cmd.mode.global", kind: .command, title: "代理模式：全局"),
        CommandPalette.Item(id: "node.a.1", kind: .node, title: "香港 01", detail: "切换节点 · Example · trojan"),
        CommandPalette.Item(id: "node.a.2", kind: .node, title: "HK Premium", detail: "切换节点 · Example · vless"),
        CommandPalette.Item(id: "node.a.3", kind: .node, title: "东京 02", detail: "切换节点 · Example · trojan"),
    ]

    private func ids(_ query: String, recent: [String] = []) -> [String] {
        CommandPalette.search(query, in: items, recent: recent).map(\.id)
    }

    func testEmptyQueryListsRecentThenPagesAndCommandsWithoutNodes() {
        XCTAssertEqual(ids(""), ["page.home", "page.proxies", "cmd.import", "cmd.mode.global"])
        XCTAssertEqual(
            ids("  ", recent: ["node.a.3", "cmd.import"]),
            ["node.a.3", "cmd.import", "page.home", "page.proxies", "cmd.mode.global"]
        )
    }

    func testRecentItemsThatNoLongerExistAreSkipped() {
        XCTAssertEqual(ids("", recent: ["node.gone"]).first, "page.home")
    }

    func testWholeNameBeatsStartBeatsPart() {
        XCTAssertEqual(ids("代理"), ["page.proxies", "cmd.mode.global"])
        XCTAssertEqual(ids("节点"), ["cmd.import", "node.a.1", "node.a.2", "node.a.3"])
    }

    func testMatchingIgnoresCaseAndSurroundingSpaces() {
        XCTAssertEqual(ids(" hk "), ["node.a.2"])
        XCTAssertEqual(ids("PREMIUM"), ["node.a.2"])
    }

    func testPinyinInitialsFindChineseNames() {
        XCTAssertEqual(CommandPalette.initials("香港 01"), "xg01")
        XCTAssertEqual(CommandPalette.initials("HK Premium"), "hkpremium")
        XCTAssertEqual(ids("xg"), ["node.a.1"])
        XCTAssertEqual(ids("dj02"), ["node.a.3"])
        XCTAssertEqual(ids("drjd"), ["cmd.import"])
    }

    func testDetailLineMatchesLast() {
        XCTAssertEqual(ids("trojan"), ["node.a.1", "node.a.3"])
        XCTAssertEqual(ids("页面"), ["page.home", "page.proxies"])
    }

    func testRecentWinsAmongEqualMatches() {
        XCTAssertEqual(ids("trojan", recent: ["node.a.3"]), ["node.a.3", "node.a.1"])
    }

    func testNoMatchIsEmptyAndLimitHolds() {
        XCTAssertEqual(ids("zzzz"), [])
        XCTAssertEqual(CommandPalette.search("", in: items, limit: 2).count, 2)
    }

    func testRememberKeepsNewestFirstOnceEach() {
        XCTAssertEqual(CommandPalette.remember("b", in: ["a", "b", "c"]), ["b", "a", "c"])
        XCTAssertEqual(
            CommandPalette.remember("g", in: ["a", "b", "c", "d", "e"]),
            ["g", "a", "b", "c", "d"]
        )
    }
}

final class AppShortcutTests: XCTestCase {
    func testDisplayOrdersModifiersAsMenusDo() {
        XCTAssertEqual(AppShortcut.palette.display, "⌘K")
        XCTAssertEqual(AppShortcut.toggle.display, "⇧⌘C")
        XCTAssertEqual(AppShortcut.requestDetails.display, "⌥⌘I")
        XCTAssertEqual(AppShortcut.settings.display, "⌘,")
    }

    func testNoTwoCommandsShareAShortcut() {
        let pages = (0..<AppShortcut.pageCount).compactMap(AppShortcut.page)
        let keys = (AppShortcut.all + pages).map(\.display)
        XCTAssertEqual(Set(keys).count, keys.count)
        XCTAssertEqual(Set(AppShortcut.all.map(\.id)).count, AppShortcut.all.count)
    }

    func testSystemShortcutsAreNotTaken() {
        // keyboard.md K2: copy, paste, cut, undo, select all, find, close, quit, minimise and hide stay the system's.
        let system = Set(["⌘C", "⌘V", "⌘X", "⌘Z", "⌘A", "⌘F", "⌘W", "⌘Q", "⌘M", "⌘H"])
        for shortcut in AppShortcut.all {
            XCTAssertFalse(system.contains(shortcut.display), shortcut.id)
        }
    }

    func testPagesAreCommandOneToNine() {
        XCTAssertEqual(AppShortcut.page(0)?.display, "⌘1")
        XCTAssertEqual(AppShortcut.page(8)?.display, "⌘9")
        XCTAssertNil(AppShortcut.page(9))
        XCTAssertNil(AppShortcut.page(-1))
    }
}
