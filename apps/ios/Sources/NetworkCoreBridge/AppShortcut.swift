import Foundation

/// The client's keyboard shortcuts in one table: the menus bind from it and the shortcut list shows it, so the two
/// cannot disagree (AnixOps Design `guidelines/keyboard.md` K3, K9). Keys follow that page's reserved list.
public struct AppShortcut: Equatable, Identifiable, Sendable {
    public struct Modifiers: OptionSet, Hashable, Sendable {
        public let rawValue: Int

        public init(rawValue: Int) {
            self.rawValue = rawValue
        }

        public static let command = Modifiers(rawValue: 1)
        public static let shift = Modifiers(rawValue: 2)
        public static let option = Modifiers(rawValue: 4)
        public static let control = Modifiers(rawValue: 8)
    }

    public enum Group: String, CaseIterable, Sendable {
        case app
        case pages
        case requests
    }

    public let id: String
    /// The command's name, looked up in the app's string catalogue (`String(localized: shortcut.title)`). A
    /// localization value rather than a String, so a literal here is a catalogue key with its interpolations
    /// (`前往第 %lld 页`) and the strings check can see it.
    public let title: String.LocalizationValue
    public let key: Character
    public let modifiers: Modifiers
    public let group: Group

    public init(id: String, title: String.LocalizationValue, key: Character, modifiers: Modifiers, group: Group) {
        self.id = id
        self.title = title
        self.key = key
        self.modifiers = modifiers
        self.group = group
    }

    /// As macOS menus write it: modifiers in the order ⌃⌥⇧⌘, then the key.
    public var display: String {
        var text = ""
        if modifiers.contains(.control) {
            text += "⌃"
        }
        if modifiers.contains(.option) {
            text += "⌥"
        }
        if modifiers.contains(.shift) {
            text += "⇧"
        }
        if modifiers.contains(.command) {
            text += "⌘"
        }
        return text + String(key).uppercased()
    }

    public static let palette = AppShortcut(id: "palette", title: "命令面板", key: "k", modifiers: .command, group: .app)
    public static let shortcuts = AppShortcut(id: "shortcuts", title: "键盘快捷键", key: "/", modifiers: .command, group: .app)
    public static let settings = AppShortcut(id: "settings", title: "设置", key: ",", modifiers: .command, group: .app)
    public static let importNodes = AppShortcut(id: "import", title: "导入节点", key: "i", modifiers: .command, group: .app)
    public static let toggle = AppShortcut(
        id: "toggle", title: "连接或断开", key: "c", modifiers: [.command, .shift], group: .app
    )
    public static let dashboard = AppShortcut(
        id: "dashboard", title: "打开仪表盘", key: "d", modifiers: [.command, .option], group: .app
    )
    public static let pauseRequests = AppShortcut(
        id: "requests.pause", title: "暂停或继续刷新", key: "p", modifiers: [.command, .shift], group: .requests
    )
    public static let requestDetails = AppShortcut(
        id: "requests.details", title: "显示或隐藏详情", key: "i", modifiers: [.command, .option], group: .requests
    )

    /// How many sidebar pages have a shortcut: ⌘1 to ⌘9.
    public static let pageCount = 9

    /// ⌘1 … ⌘9 for the sidebar pages in their order; nil past the ninth.
    public static func page(_ index: Int) -> AppShortcut? {
        guard (0..<pageCount).contains(index) else {
            return nil
        }
        return AppShortcut(
            id: "page.\(index + 1)",
            title: "前往第 \(index + 1) 页",
            key: Character(String(index + 1)),
            modifiers: .command,
            group: .pages
        )
    }

    /// Every fixed shortcut, in the order the shortcut list shows them. The page shortcuts are listed by the
    /// caller, which knows the pages' names.
    public static let all: [AppShortcut] = [
        palette, shortcuts, settings, importNodes, toggle, dashboard, pauseRequests, requestDetails,
    ]
}
