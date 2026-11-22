import AppKit
import NetworkCoreBridge
import NetworkExtension
import SwiftUI

// SwiftUI entry point for the macOS app. Like the iOS app target it compiles the NetworkCoreApp sources and the
// shared host files (TunnelController, ImportView, brand views) directly, so it uses them without imports.
@main
struct NetworkCoreMacApp: App {
    @State private var tunnel = TunnelController()
    @StateObject private var state = MacAppState()
    @State private var updater = AppUpdater()
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system

    init() {
        ClientHealthMonitor.shared.start()
        AppNotifications.install()
    }

    var body: some Scene {
        // One main window: a VPN client has a single connection, so a second one would only repeat it.
        Window("AnixOps Net", id: MacAppState.mainWindowID) {
            MacRootView(tunnel: tunnel, state: state)
                .environment(updater)
                .preferredColorScheme(appearance.colorScheme)
                // The System Extension cannot post notifications; the app posts what scripts posted.
                .task {
                    await tunnel.relayScriptNotifications()
                }
                .onAppear {
                    state.dashboard.startNetworkMonitor()
                }
                // Logged once here, not by each window that shows the dashboard.
                .onChange(of: tunnel.connection) { previous, status in
                    state.dashboard.tunnelChanged(from: previous, to: status, node: tunnel.nodeName)
                }
                .onChange(of: tunnel.message) { _, message in
                    state.dashboard.messageShown(message)
                }
                // `anixops://` links from Shortcuts, other apps and web pages (`AppURLCommand`).
                .onOpenURL { url in
                    Task {
                        if await tunnel.handle(url: url) == .openImport {
                            state.importing = true
                        }
                    }
                }
                .task {
                    #if DEBUG
                    // Screenshots: made-up data instead of the saved configuration (`-AnixOpsDemo YES`).
                    if TunnelController.isDemo {
                        tunnel.loadDemo()
                        return
                    }
                    #endif
                    await tunnel.load()
                    // The `anixops` command-line tool talks to this app (MacControlService).
                    state.startControl(tunnel)
                    // The loopback Prometheus endpoint, when turned on in Settings (MacMetricsService).
                    state.startMetrics(tunnel)
                    // Subscription groups update on their own while the app runs; a Mac app stays open for long.
                    while !Task.isCancelled {
                        await tunnel.refreshDueSubscriptions()
                        // Once a day at most, and only in a build with an update feed (AppUpdater).
                        await updater.checkIfDue()
                        try? await Task.sleep(for: .seconds(15 * 60))
                    }
                }
        }
        // Wide enough for the request table beside its inspector without squeezing the table's columns.
        .defaultSize(width: 1240, height: 800)
        .commands {
            MacCommands(tunnel: tunnel, state: state)
        }

        // The live cards alone, for a corner of the screen; shares the overview's sampler (MacAppState.dashboard).
        Window("仪表盘", id: MacAppState.dashboardWindowID) {
            MacDashboardWindow(tunnel: tunnel, state: state)
                .onAppear {
                    state.dashboard.startNetworkMonitor()
                }
        }
        .defaultSize(width: 760, height: 560)
        .keyboardShortcut(AppShortcut.dashboard.keyboardShortcut)

        MenuBarExtra {
            MenuBarContent(tunnel: tunnel, state: state)
        } label: {
            Image(systemName: tunnel.connection == .connected ? "network.badge.shield.half.filled" : "network")
                .accessibilityLabel("AnixOps Net")
        }
    }
}

/// Window-level state the menu commands and the menu bar item also change.
@MainActor
final class MacAppState: ObservableObject {
    static let mainWindowID = "main"
    static let dashboardWindowID = "dashboard"

    @Published var section: MacSection? = .home
    @Published var importing = false
    /// The command palette (⌘K) lies over the main window; the shortcut list (⌘/) is a sheet of it.
    @Published var showsPalette = false
    @Published var showsShortcuts = false
    /// The one dashboard sampler every window shows (the overview page and the dashboard window).
    let dashboard = DashboardModel()

    /// Serves the command-line tool for the life of the app, also while the window is closed.
    private var control: MacControlService?

    func startControl(_ tunnel: TunnelController) {
        if control == nil {
            control = MacControlService(tunnel: tunnel)
        }
        control?.start()
    }

    /// The Prometheus endpoint; nil until the configuration is loaded.
    @Published private(set) var metrics: MacMetricsService?

    func startMetrics(_ tunnel: TunnelController) {
        if metrics == nil {
            metrics = MacMetricsService(tunnel: tunnel)
        }
        metrics?.apply()
    }

    init() {
        #if DEBUG
        // CI screenshots open a given page: `-AnixOpsSection proxies`.
        if let name = UserDefaults.standard.string(forKey: "AnixOpsSection"), let section = MacSection(rawValue: name) {
            self.section = section
        }
        // … and the command palette over it: `-AnixOpsPalette YES`.
        showsPalette = UserDefaults.standard.bool(forKey: "AnixOpsPalette")
        // … or the import sheet: `-AnixOpsImport YES`.
        importing = UserDefaults.standard.bool(forKey: "AnixOpsImport")
        #endif
    }
}

/// The six desktop sections, matching the Windows client: the five shared tabs plus Activity.
enum MacSection: String, CaseIterable, Identifiable, Hashable {
    case home
    case proxies
    case rules
    case extensions
    case activity
    /// Debug mode's exit (plan G2): direct or through a node, with the subscriptions, nodes and rules.
    case exit
    case settings

    var id: String { rawValue }

    /// The sidebar for a mode. Debug mode is the same on every platform (docs/debug-mode.md): Requests · Rewrite ·
    /// Exit · Settings, opening on the requests, with proxies and rules under Exit.
    static func sections(for mode: AppMode?) -> [MacSection] {
        mode == .debug ? [.activity, .extensions, .exit, .settings] : [.home, .proxies, .rules, .extensions, .activity, .settings]
    }

    /// The section's name in a mode: in debug mode Activity is the requests and Extensions are "Rewrite".
    func title(in mode: AppMode?) -> LocalizedStringKey {
        guard mode == .debug else {
            return title
        }
        switch self {
        case .activity:
            return "请求"
        case .extensions:
            return "改写"
        default:
            return title
        }
    }

    var title: LocalizedStringKey {
        switch self {
        case .home:
            return "概览"
        case .proxies:
            return "代理"
        case .rules:
            return "规则"
        case .extensions:
            return "扩展"
        case .activity:
            return "活动"
        case .exit:
            return "出口"
        case .settings:
            return "设置"
        }
    }

    var systemImage: String {
        switch self {
        case .home:
            return "house.fill"
        case .proxies:
            return "globe"
        case .rules:
            return "arrow.triangle.branch"
        case .extensions:
            return "puzzlepiece.extension.fill"
        case .activity:
            return "waveform.path.ecg"
        case .exit:
            return "globe"
        case .settings:
            return "gearshape.fill"
        }
    }

    /// The sidebar tile color, the same per section as on Windows and Linux.
    var tileColor: Color {
        switch self {
        case .home:
            return MacTileColor.indigo
        case .proxies:
            return MacTileColor.sky
        case .rules:
            return MacTileColor.orange
        case .extensions:
            return MacTileColor.purple
        case .activity:
            return MacTileColor.pink
        case .exit:
            return MacTileColor.sky
        case .settings:
            return MacTileColor.gray
        }
    }
}

/// Glyph tile colors shared with the desktop client (apps/windows-gui/ui/src/styles.css `--tile-*`).
enum MacTileColor {
    static let indigo = Color(red: 0x5B / 255, green: 0x63 / 255, blue: 0xE6 / 255)
    static let sky = Color(red: 0x0E / 255, green: 0xA5 / 255, blue: 0xE9 / 255)
    static let green = Color(red: 0x22 / 255, green: 0xA3 / 255, blue: 0x5A / 255)
    static let orange = Color(red: 0xF0 / 255, green: 0x8A / 255, blue: 0x24 / 255)
    static let purple = Color(red: 0x8B / 255, green: 0x5C / 255, blue: 0xF6 / 255)
    static let pink = Color(red: 0xE0 / 255, green: 0x45 / 255, blue: 0x7B / 255)
    static let teal = Color(red: 0x0D / 255, green: 0x94 / 255, blue: 0x88 / 255)
    static let gray = Color(red: 0x8E / 255, green: 0x8E / 255, blue: 0x93 / 255)
    static let red = Color(red: 0xE5 / 255, green: 0x48 / 255, blue: 0x4D / 255)
}

extension TunnelController {
    var phase: ConnectPhase {
        ConnectPhase(status: connection, hasNode: canConnect)
    }

    var toggleTitle: LocalizedStringKey {
        // "连接" alone is the noun "Connection" in the catalog, so the verb has its own key.
        isConnected ? "断开" : "action.connect"
    }
}

/// The app menu additions: import, connect or disconnect, Settings opening the Settings section, the pages, the
/// command palette and the request list's own commands. Every shortcut comes from `AppShortcut`, which the
/// shortcut list also shows (AnixOps Design `guidelines/keyboard.md` K1, K9, K17).
struct MacCommands: Commands {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState
    /// Set by the request list while it is on screen (`CaptureRequestsView`).
    @FocusedValue(\.requestListCommands) private var requests

    private var pauseTitle: LocalizedStringKey {
        requests?.paused == true ? "继续刷新" : "暂停刷新"
    }

    private var detailsTitle: LocalizedStringKey {
        requests?.showsDetails == true ? "隐藏详情" : "显示详情"
    }

    var body: some Commands {
        CommandGroup(replacing: .appSettings) {
            OpenSectionButton(state: state, section: .settings, title: "设置…")
                .keyboardShortcut(AppShortcut.settings.keyboardShortcut)
        }
        CommandGroup(replacing: .newItem) {
            Button("导入节点…") {
                state.importing = true
            }
            .keyboardShortcut(AppShortcut.importNodes.keyboardShortcut)
        }
        CommandGroup(before: .sidebar) {
            OpenPaletteButton(state: state)
                .keyboardShortcut(AppShortcut.palette.keyboardShortcut)
            Divider()
            Button(pauseTitle) {
                requests?.togglePause()
            }
            .keyboardShortcut(AppShortcut.pauseRequests.keyboardShortcut)
            .disabled(requests == nil)
            Button(detailsTitle) {
                requests?.toggleDetails?()
            }
            .keyboardShortcut(AppShortcut.requestDetails.keyboardShortcut)
            .disabled(requests?.toggleDetails == nil)
            Divider()
        }
        CommandMenu("前往") {
            let sections = MacSection.sections(for: tunnel.appMode).filter { $0 != .settings }
            ForEach(Array(sections.enumerated()), id: \.element) { index, section in
                if let shortcut = AppShortcut.page(index) {
                    OpenSectionButton(state: state, section: section, title: section.title(in: tunnel.appMode))
                        .keyboardShortcut(shortcut.keyboardShortcut)
                }
            }
        }
        CommandMenu("连接") {
            Button(tunnel.toggleTitle) {
                tunnel.toggle()
            }
            .keyboardShortcut(AppShortcut.toggle.keyboardShortcut)
            .disabled(tunnel.phase == .unconfigured || tunnel.phase == .disconnecting)
        }
        CommandGroup(replacing: .help) {
            Button("键盘快捷键") {
                state.showsShortcuts = true
            }
            .keyboardShortcut(AppShortcut.shortcuts.keyboardShortcut)
        }
    }
}

/// Opens the command palette over the main window, bringing the window back if it was closed.
private struct OpenPaletteButton: View {
    @Environment(\.openWindow) private var openWindow
    @ObservedObject var state: MacAppState

    var body: some View {
        Button("命令面板…") {
            openWindow(id: MacAppState.mainWindowID)
            NSApp.activate(ignoringOtherApps: true)
            state.showsPalette.toggle()
        }
    }
}

/// Brings the main window forward on a section; reopens it if the user closed it.
private struct OpenSectionButton: View {
    @Environment(\.openWindow) private var openWindow
    @ObservedObject var state: MacAppState
    let section: MacSection
    let title: LocalizedStringKey

    var body: some View {
        Button(title) {
            state.section = section
            openWindow(id: MacAppState.mainWindowID)
            NSApp.activate(ignoringOtherApps: true)
        }
    }
}

/// The menu bar item: the status, one connect switch, the proxy mode and a way back to the window.
private struct MenuBarContent: View {
    @Environment(\.openWindow) private var openWindow
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState

    var body: some View {
        Text(tunnel.phase.title)
        if tunnel.appMode == .debug {
            Text(tunnel.debugExitSummary)
        } else if let name = tunnel.nodeName {
            Text(name)
        }
        // A connect from here may fail while the window is closed: say so here too.
        if let message = tunnel.message {
            Text(message)
            Button("打开 AnixOps Net 查看") {
                showWindow()
            }
        }
        Divider()
        if tunnel.phase == .unconfigured {
            Button("导入节点…") {
                state.importing = true
                showWindow()
            }
        } else {
            Button(tunnel.toggleTitle) {
                tunnel.toggle()
            }
            .disabled(tunnel.phase == .disconnecting)
            if !tunnel.runsDirectProfile {
                modePicker
            }
        }
        Divider()
        Button("打开 AnixOps Net") {
            showWindow()
        }
        Button("打开仪表盘") {
            openWindow(id: MacAppState.dashboardWindowID)
            NSApp.activate(ignoringOtherApps: true)
        }
        Button("退出 AnixOps Net") {
            NSApp.terminate(nil)
        }
    }

    private var modePicker: some View {
        Picker("代理模式", selection: Binding(
            get: { tunnel.mode },
            set: { mode in
                Task {
                    await tunnel.setMode(mode)
                }
            }
        )) {
            if tunnel.library != nil {
                Text("规则").tag(ProxyMode.rule)
            }
            Text("全局").tag(ProxyMode.global)
            Text("直连").tag(ProxyMode.direct)
        }
        .pickerStyle(.inline)
    }

    private func showWindow() {
        openWindow(id: MacAppState.mainWindowID)
        NSApp.activate(ignoringOtherApps: true)
    }
}
