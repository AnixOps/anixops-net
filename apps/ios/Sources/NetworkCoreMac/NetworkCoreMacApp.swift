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

    var body: some Scene {
        // One window: a VPN client has a single connection, so a second window would only repeat it.
        Window("AnixOps Net", id: MacAppState.mainWindowID) {
            MacRootView(tunnel: tunnel, state: state)
                .environment(updater)
                .preferredColorScheme(appearance.colorScheme)
                .task {
                    #if DEBUG
                    // Screenshots: made-up data instead of the saved configuration (`-AnixOpsDemo YES`).
                    if TunnelController.isDemo {
                        tunnel.loadDemo()
                        return
                    }
                    #endif
                    await tunnel.load()
                    // Subscription groups update on their own while the app runs; a Mac app stays open for long.
                    while !Task.isCancelled {
                        await tunnel.refreshDueSubscriptions()
                        // Once a day at most, and only in a build with an update feed (AppUpdater).
                        await updater.checkIfDue()
                        try? await Task.sleep(for: .seconds(15 * 60))
                    }
                }
        }
        .defaultSize(width: 980, height: 680)
        .commands {
            MacCommands(tunnel: tunnel, state: state)
        }

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

    @Published var section: MacSection? = .home
    @Published var importing = false

    init() {
        #if DEBUG
        // CI screenshots open a given page: `-AnixOpsSection proxies`.
        if let name = UserDefaults.standard.string(forKey: "AnixOpsSection"), let section = MacSection(rawValue: name) {
            self.section = section
        }
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
    case settings

    var id: String { rawValue }

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
        ConnectPhase(status: connection, hasNode: nodeName != nil)
    }

    var toggleTitle: LocalizedStringKey {
        // "连接" alone is the noun "Connection" in the catalog, so the verb has its own key.
        isConnected ? "断开" : "action.connect"
    }
}

/// The app menu additions: import, connect or disconnect, and Settings opening the Settings section.
struct MacCommands: Commands {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState

    var body: some Commands {
        CommandGroup(replacing: .appSettings) {
            OpenSectionButton(state: state, section: .settings, title: "设置…")
                .keyboardShortcut(",", modifiers: .command)
        }
        CommandGroup(replacing: .newItem) {
            Button("导入节点…") {
                state.importing = true
            }
            .keyboardShortcut("i", modifiers: .command)
        }
        CommandMenu("连接") {
            Button(tunnel.toggleTitle) {
                tunnel.toggle()
            }
            .keyboardShortcut("c", modifiers: [.command, .shift])
            .disabled(tunnel.phase == .unconfigured || tunnel.phase == .disconnecting)
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
        if let name = tunnel.nodeName {
            Text(name)
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
        Divider()
        Button("打开 AnixOps Net") {
            showWindow()
        }
        Button("退出 AnixOps Net") {
            NSApp.terminate(nil)
        }
    }

    private func showWindow() {
        openWindow(id: MacAppState.mainWindowID)
        NSApp.activate(ignoringOtherApps: true)
    }
}
