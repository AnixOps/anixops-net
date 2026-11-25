import NetworkCoreBridge
import SwiftUI

/// The main window, laid out like System Settings: a sidebar of colored glyph tiles with the connection switch at
/// its foot, and each page as a grouped form. Import sits in the sidebar's toolbar; a page's toolbar holds only that
/// page's own actions.
struct MacRootView: View {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @AppStorage(TermsConsent.storageKey) private var acceptedTerms = ""
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        let brand = BrandPalette.resolve(appearance: appearance, scheme: colorScheme)
        Group {
            // Nothing else is usable until the user agreement is accepted (`TermsConsent`).
            if TermsConsent.needsConsent(accepted: acceptedTerms) {
                TermsConsentView {
                    acceptedTerms = TermsConsent.version
                }
                .frame(minWidth: 520, minHeight: 600)
            } else if tunnel.loaded, tunnel.appMode == nil {
                ModeChoiceView { mode in
                    Task {
                        await tunnel.setAppMode(mode)
                    }
                }
                .frame(minWidth: 520, minHeight: 600)
            } else {
                NavigationSplitView {
                    sidebar
                } detail: {
                    NavigationStack {
                        page
                            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                            .navigationTitle(state.section?.title(in: tunnel.appMode) ?? "AnixOps Net")
                    }
                    // The overview shows notices in its own place; every other page, and all of debug mode, which
                    // has no overview, shows them above the page, so a failure is told where it happened (the
                    // network extension waiting for approval, a node or a setting that could not be saved).
                    .safeAreaInset(edge: .top, spacing: 0) {
                        if currentSection != .home, let message = tunnel.message {
                            MacNotice(text: message) {
                                tunnel.clearMessage()
                            }
                            .padding(.horizontal, 20)
                            .padding(.vertical, 10)
                        }
                    }
                }
                .overlay {
                    if state.showsPalette {
                        MacCommandPalette(tunnel: tunnel, state: state)
                    }
                }
                .sheet(isPresented: $state.showsShortcuts) {
                    MacShortcutList(mode: tunnel.appMode)
                        .brandSheet()
                }
            }
        }
        .tint(brand.primary)
        .environment(\.brand, brand)
        // Every way of starting HTTPS decryption asks here first (`TunnelController.requireDecryptionConsent()`).
        .decryptionConsentAlert(tunnel)
        .onChange(of: tunnel.appMode, initial: true) { _, mode in
            // Each mode opens on its first section: the overview, or the requests in debug mode. Before the mode
            // has been read there is nothing to check against: resetting then would drop a section that only
            // debug mode has (the CI screenshot of "exit" showed the requests because of it).
            guard mode != nil else {
                return
            }
            let sections = MacSection.sections(for: mode)
            if !sections.contains(state.section ?? .home) {
                state.section = sections.first
            }
        }
        .sheet(isPresented: $state.importing) {
            ImportView(tunnel: tunnel)
                .brandSheet()
                .environment(\.brand, brand)
                .tint(brand.primary)
        }
    }

    /// The page on screen: the chosen section, or the mode's first one before a choice.
    private var currentSection: MacSection {
        state.section ?? MacSection.sections(for: tunnel.appMode)[0]
    }

    private var sidebar: some View {
        List(selection: $state.section) {
            Section {
                ForEach(MacSection.sections(for: tunnel.appMode).filter { $0 != .settings }) { section in
                    MacSidebarLabel(section: section, mode: tunnel.appMode)
                        .tag(section)
                }
            }
            Section {
                MacSidebarLabel(section: .settings, mode: tunnel.appMode)
                    .tag(MacSection.settings)
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            MacSidebarStatus(tunnel: tunnel, state: state)
        }
        // Import changes the whole client, so it sits with the sidebar on every page and never among a page's own
        // toolbar buttons (AnixOps Design, actions S1).
        .toolbar {
            ToolbarItem {
                Button {
                    state.importing = true
                } label: {
                    Label("导入节点", systemImage: "plus")
                }
                .help("导入节点（⌘I）")
            }
        }
        .navigationSplitViewColumnWidth(min: 200, ideal: 220, max: 260)
    }

    @ViewBuilder
    private var page: some View {
        switch currentSection {
        case .home:
            MacHomePage(tunnel: tunnel, state: state)
        case .proxies:
            MacProxiesPage(tunnel: tunnel, state: state)
        case .mesh:
            MacMeshPage(tunnel: tunnel)
        case .rules:
            MacRulesPage(tunnel: tunnel)
        case .extensions:
            MacExtensionsPage(tunnel: tunnel)
        case .activity:
            MacActivityPage(tunnel: tunnel)
        case .exit:
            MacExitPage(tunnel: tunnel, state: state)
        case .settings:
            MacSettingsPage(tunnel: tunnel, state: state)
        }
    }
}

/// A sidebar row: the section's glyph on a colored tile, as System Settings draws its panes.
private struct MacSidebarLabel: View {
    let section: MacSection
    let mode: AppMode?

    var body: some View {
        Label {
            Text(section.title(in: mode))
        } icon: {
            MacTile(systemImage: section.systemImage, color: section.tileColor)
        }
    }
}

/// A white SF Symbol (`Brand.onTile`) on a small rounded colored square (`MacTileColor`).
struct MacTile: View {
    let systemImage: String
    let color: Color
    var size: CGFloat = 20

    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: size * 0.55, weight: .semibold))
            .foregroundStyle(Brand.onTile)
            .frame(width: size, height: size)
            .background(color.gradient, in: RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
            .accessibilityHidden(true)
    }
}

/// A form row label with a colored tile, for the rows that lead somewhere or switch something.
struct MacTileLabel: View {
    let title: LocalizedStringKey
    let systemImage: String
    let color: Color

    var body: some View {
        Label {
            Text(title)
        } icon: {
            MacTile(systemImage: systemImage, color: color)
        }
    }
}

/// The connection at the foot of the sidebar, on every page: the state, the node and a switch.
private struct MacSidebarStatus: View {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState

    /// The node, or in debug mode where the traffic goes out: the direct exit needs no node, so "no nodes yet"
    /// would read as if the switch could not connect.
    private var caption: String {
        if tunnel.appMode == .debug {
            return tunnel.debugExitSummary
        }
        return tunnel.nodeName ?? String(localized: "暂无节点")
    }

    var body: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 10) {
                Circle()
                    .fill(tunnel.phase.dotColor)
                    .frame(width: 8, height: 8)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 1) {
                    Text(tunnel.phase.title)
                        .font(.callout.weight(.semibold))
                    Text(caption)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture {
                    state.section = MacSection.sections(for: tunnel.appMode).first
                }
                MacConnectSwitch(tunnel: tunnel, controlSize: .small)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
        }
    }
}

/// The connect switch: on while connected or connecting; turning it on or off starts the change.
struct MacConnectSwitch: View {
    var tunnel: TunnelController
    var controlSize: ControlSize = .regular
    /// The sidebar's switch is `connect-switch`, on every page in both modes (docs/ui-ux.md, 入口不变量); the
    /// overview's larger copy has its own identifier, so the UI tests can tell them apart.
    var identifier = "connect-switch"
    @Environment(\.brand) private var brand

    var body: some View {
        Toggle(isOn: Binding(
            get: { tunnel.isConnected },
            set: { on in
                if on != tunnel.isConnected {
                    tunnel.toggle()
                }
            }
        )) {
            Text(tunnel.toggleTitle)
        }
        .toggleStyle(.switch)
        .labelsHidden()
        .controlSize(controlSize)
        // Connected is a status, so the switch takes the theme's success colour.
        .tint(brand.success)
        .disabled(tunnel.phase == .unconfigured || tunnel.phase == .disconnecting)
        .help("连接或断开（⇧⌘C）")
        .accessibilityIdentifier(identifier)
    }
}

extension ConnectPhase {
    /// The status dot: green when connected, amber while it changes, gray otherwise.
    var dotColor: Color {
        switch self {
        case .on:
            return .green
        case .connecting, .reasserting, .disconnecting:
            return .orange
        case .off, .unconfigured:
            return Color.secondary
        }
    }
}
