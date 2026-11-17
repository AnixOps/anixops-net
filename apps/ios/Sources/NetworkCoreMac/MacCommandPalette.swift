import NetworkCoreBridge
import SwiftUI

extension AppShortcut {
    var keyboardShortcut: KeyboardShortcut {
        var flags: EventModifiers = []
        if modifiers.contains(.command) {
            flags.insert(.command)
        }
        if modifiers.contains(.shift) {
            flags.insert(.shift)
        }
        if modifiers.contains(.option) {
            flags.insert(.option)
        }
        if modifiers.contains(.control) {
            flags.insert(.control)
        }
        return KeyboardShortcut(KeyEquivalent(key), modifiers: flags)
    }
}

extension MacSection {
    /// The section's name as text, for the command palette and the shortcut list; the same words as `title(in:)`.
    func name(in mode: AppMode?) -> String {
        if mode == .debug {
            switch self {
            case .activity:
                return String(localized: "请求")
            case .extensions:
                return String(localized: "改写")
            default:
                break
            }
        }
        switch self {
        case .home:
            return String(localized: "概览")
        case .proxies:
            return String(localized: "代理")
        case .rules:
            return String(localized: "规则")
        case .extensions:
            return String(localized: "扩展")
        case .activity:
            return String(localized: "活动")
        case .exit:
            return String(localized: "出口")
        case .settings:
            return String(localized: "设置")
        }
    }
}

/// The command palette (⌘K): every page, the app's commands and every node, found by typing a part of the name or
/// its pinyin initials (AnixOps Design `guidelines/keyboard.md` section 2). It lies over the window, not in a
/// sheet, so a command that opens a sheet (import) can run straight away.
struct MacCommandPalette: View {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState
    @Environment(\.openWindow) private var openWindow
    @Environment(\.brand) private var brand
    @AppStorage("palette.recent") private var recentStore = ""
    @State private var query = ""
    @State private var entries: [Entry] = []
    @State private var selectedID: String?
    @FocusState private var focused: Bool

    private struct Entry {
        let item: CommandPalette.Item
        let run: () -> Void
    }

    private var recent: [String] {
        recentStore.split(separator: "\n").map(String.init)
    }

    private var results: [CommandPalette.Item] {
        CommandPalette.search(query, in: entries.map(\.item), recent: recent)
    }

    var body: some View {
        let rows = results
        ZStack(alignment: .top) {
            Color.black.opacity(0.18)
                .ignoresSafeArea()
                .contentShape(Rectangle())
                .onTapGesture {
                    close()
                }
                .accessibilityHidden(true)
            VStack(spacing: 0) {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                    TextField("输入命令、页面或节点名称", text: $query)
                        .textFieldStyle(.plain)
                        .font(.title3)
                        .focused($focused)
                        .onSubmit {
                            run(selectedID ?? rows.first?.id)
                        }
                        .onKeyPress(.upArrow) {
                            move(rows, by: -1)
                            return .handled
                        }
                        .onKeyPress(.downArrow) {
                            move(rows, by: 1)
                            return .handled
                        }
                        .onExitCommand {
                            close()
                        }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 13)
                Divider()
                if rows.isEmpty {
                    Text("没有匹配的命令")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 28)
                } else {
                    ScrollViewReader { proxy in
                        ScrollView {
                            LazyVStack(spacing: 0) {
                                ForEach(rows) { item in
                                    row(item, selected: item.id == (selectedID ?? rows.first?.id))
                                        .id(item.id)
                                }
                            }
                            .padding(6)
                        }
                        .frame(maxHeight: 360)
                        .onChange(of: selectedID) { _, id in
                            if let id {
                                proxy.scrollTo(id)
                            }
                        }
                    }
                }
            }
            .frame(width: 560)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(brand.border, lineWidth: 1))
            .shadow(color: .black.opacity(0.25), radius: 24, y: 10)
            .padding(.top, 72)
            .accessibilityAddTraits(.isModal)
        }
        .onAppear {
            entries = makeEntries()
            focused = !Self.shownForScreenshot
        }
        .onChange(of: query) { _, _ in
            // The best match is selected after every key press, so Return runs what is on top.
            selectedID = nil
        }
    }

    /// CI screenshots open the palette with `-AnixOpsPalette YES`. The search field stays unfocused there:
    /// its blinking caret made the picture differ from run to run.
    private static var shownForScreenshot: Bool {
        #if DEBUG
        UserDefaults.standard.bool(forKey: "AnixOpsPalette")
        #else
        false
        #endif
    }

    private func row(_ item: CommandPalette.Item, selected: Bool) -> some View {
        Button {
            run(item.id)
        } label: {
            HStack(spacing: 10) {
                Image(systemName: symbol(item.kind))
                    .frame(width: 18)
                    .foregroundStyle(selected ? brand.onPrimary : Color.secondary)
                Text(verbatim: item.title)
                    .lineLimit(1)
                if let detail = item.detail {
                    Text(verbatim: detail)
                        .font(.callout)
                        .lineLimit(1)
                        .foregroundStyle(selected ? brand.onPrimary.opacity(0.8) : Color.secondary)
                }
                Spacer(minLength: 8)
                if let shortcut = item.shortcut {
                    Text(verbatim: shortcut)
                        .font(.callout.monospaced())
                        .foregroundStyle(selected ? brand.onPrimary.opacity(0.8) : Color.secondary)
                }
            }
            .padding(.horizontal, 10)
            .frame(height: 32)
            .foregroundStyle(selected ? brand.onPrimary : brand.text)
            .background(selected ? brand.primary : Color.clear, in: RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private func symbol(_ kind: CommandPalette.Kind) -> String {
        switch kind {
        case .page:
            return "sidebar.left"
        case .command:
            return "command"
        case .node:
            return "globe"
        }
    }

    private func move(_ rows: [CommandPalette.Item], by step: Int) {
        guard !rows.isEmpty else {
            return
        }
        let current = rows.firstIndex { $0.id == selectedID } ?? 0
        selectedID = rows[min(max(current + step, 0), rows.count - 1)].id
    }

    private func run(_ id: String?) {
        guard let id, let entry = entries.first(where: { $0.item.id == id }) else {
            return
        }
        recentStore = CommandPalette.remember(id, in: recent).joined(separator: "\n")
        close()
        entry.run()
    }

    private func close() {
        state.showsPalette = false
    }

    /// What can be run now: the pages of this mode, the commands that apply in the current state, and the nodes
    /// of every subscription.
    private func makeEntries() -> [Entry] {
        var list: [Entry] = []
        let mode = tunnel.appMode
        let pageWord = String(localized: "页面")
        for (index, section) in MacSection.sections(for: mode).enumerated() {
            let shortcut = section == .settings ? AppShortcut.settings.display : AppShortcut.page(index)?.display
            list.append(Entry(
                item: .init(
                    id: "page.\(section.rawValue)", kind: .page, title: section.name(in: mode), detail: pageWord,
                    shortcut: shortcut
                ),
                run: { state.section = section }
            ))
        }
        if tunnel.phase != .unconfigured, tunnel.phase != .disconnecting {
            let title = tunnel.isConnected ? String(localized: "断开") : String(localized: "action.connect")
            list.append(Entry(
                item: .init(id: "cmd.toggle", kind: .command, title: title, shortcut: AppShortcut.toggle.display),
                run: { tunnel.toggle() }
            ))
        }
        list.append(Entry(
            item: .init(
                id: "cmd.import", kind: .command, title: String(localized: "导入节点…"),
                shortcut: AppShortcut.importNodes.display
            ),
            run: { state.importing = true }
        ))
        if !tunnel.runsDirectProfile, tunnel.phase != .unconfigured {
            var modes: [(ProxyMode, String)] = []
            if tunnel.library != nil {
                modes.append((.rule, String(localized: "规则")))
            }
            modes.append((.global, String(localized: "全局")))
            modes.append((.direct, String(localized: "直连")))
            let prefix = String(localized: "代理模式")
            for (value, name) in modes where value != tunnel.mode {
                list.append(Entry(
                    item: .init(id: "cmd.mode.\(value.rawValue)", kind: .command, title: "\(prefix)：\(name)"),
                    run: {
                        Task {
                            await tunnel.setMode(value)
                        }
                    }
                ))
            }
        }
        let takeoverPrefix = String(localized: "接管方式")
        for value in TrafficTakeover.choices(macOS: true) where value != tunnel.takeover {
            list.append(Entry(
                item: .init(
                    id: "cmd.takeover.\(value.rawValue)", kind: .command,
                    title: "\(takeoverPrefix)：\(TakeoverSection.title(value))"
                ),
                run: {
                    Task {
                        await tunnel.setTakeover(value)
                    }
                }
            ))
        }
        if !tunnel.subscriptions.groups.isEmpty {
            list.append(Entry(
                item: .init(id: "cmd.subscriptions", kind: .command, title: String(localized: "更新全部订阅")),
                run: {
                    Task {
                        await tunnel.refreshAllSubscriptions()
                    }
                }
            ))
        }
        list.append(Entry(
            item: .init(
                id: "cmd.dashboard", kind: .command, title: String(localized: "打开仪表盘"),
                shortcut: AppShortcut.dashboard.display
            ),
            run: { openWindow(id: MacAppState.dashboardWindowID) }
        ))
        list.append(Entry(
            item: .init(
                id: "cmd.shortcuts", kind: .command, title: String(localized: "键盘快捷键"),
                shortcut: AppShortcut.shortcuts.display
            ),
            run: { state.showsShortcuts = true }
        ))
        let switchWord = String(localized: "切换节点")
        for group in tunnel.subscriptions.groups {
            for node in group.library.nodes {
                list.append(Entry(
                    item: .init(
                        id: "node.\(group.id).\(node.id)", kind: .node, title: node.name,
                        detail: "\(switchWord) · \(group.name) · \(node.protocol)"
                    ),
                    run: {
                        Task {
                            await tunnel.selectNode(node.id, inGroup: group.id)
                        }
                    }
                ))
            }
        }
        return list
    }
}

/// The shortcut list (⌘/), drawn from the same table the menus bind from (`AppShortcut`).
struct MacShortcutList: View {
    let mode: AppMode?
    @Environment(\.dismiss) private var dismiss

    private struct Line: Identifiable {
        let id: String
        let title: String
        let keys: String
    }

    private func lines(_ group: AppShortcut.Group) -> [Line] {
        if group == .pages {
            return MacSection.sections(for: mode).enumerated().compactMap { index, section in
                guard section != .settings, let shortcut = AppShortcut.page(index) else {
                    return nil
                }
                return Line(id: shortcut.id, title: section.name(in: mode), keys: shortcut.display)
            }
        }
        return AppShortcut.all.filter { $0.group == group }.map { shortcut in
            Line(
                id: shortcut.id,
                title: String(localized: String.LocalizationValue(shortcut.title)),
                keys: shortcut.display
            )
        }
    }

    private func heading(_ group: AppShortcut.Group) -> LocalizedStringKey {
        switch group {
        case .app:
            return "应用"
        case .pages:
            return "页面"
        case .requests:
            return "请求"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("键盘快捷键")
                .font(.title3.weight(.semibold))
                .padding(.bottom, 12)
            ForEach(AppShortcut.Group.allCases, id: \.self) { group in
                Text(heading(group))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .padding(.top, 10)
                    .padding(.bottom, 4)
                ForEach(lines(group)) { line in
                    HStack {
                        Text(verbatim: line.title)
                        Spacer()
                        Text(verbatim: line.keys)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                    }
                    .frame(height: 26)
                }
            }
            HStack {
                Spacer()
                Button("完成") {
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(.top, 16)
        }
        .padding(20)
        .frame(width: 380)
    }
}
