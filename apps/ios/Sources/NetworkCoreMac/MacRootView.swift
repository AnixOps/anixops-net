import SwiftUI

/// The main window, laid out like System Settings: a sidebar of colored glyph tiles with the connection switch at
/// its foot, and each page as a grouped form. Import sits in the sidebar's toolbar; a page's toolbar holds only that
/// page's own actions.
struct MacRootView: View {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        let brand = BrandPalette.resolve(appearance: appearance, scheme: colorScheme)
        NavigationSplitView {
            sidebar
        } detail: {
            NavigationStack {
                page
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                    .navigationTitle(state.section?.title ?? "AnixOps Net")
            }
        }
        .tint(brand.primary)
        .environment(\.brand, brand)
        .sheet(isPresented: $state.importing) {
            ImportView(tunnel: tunnel)
                .frame(width: 480, height: 580)
                .environment(\.brand, brand)
                .tint(brand.primary)
        }
    }

    private var sidebar: some View {
        List(selection: $state.section) {
            Section {
                ForEach(MacSection.allCases.filter { $0 != .settings }) { section in
                    MacSidebarLabel(section: section)
                        .tag(section)
                }
            }
            Section {
                MacSidebarLabel(section: .settings)
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
        switch state.section ?? .home {
        case .home:
            MacHomePage(tunnel: tunnel, state: state)
        case .proxies:
            MacProxiesPage(tunnel: tunnel, state: state)
        case .rules:
            MacRulesPage(tunnel: tunnel)
        case .extensions:
            MacExtensionsPage(tunnel: tunnel)
        case .activity:
            MacActivityPage(tunnel: tunnel)
        case .settings:
            MacSettingsPage(tunnel: tunnel)
        }
    }
}

/// A sidebar row: the section's glyph on a colored tile, as System Settings draws its panes.
private struct MacSidebarLabel: View {
    let section: MacSection

    var body: some View {
        Label {
            Text(section.title)
        } icon: {
            MacTile(systemImage: section.systemImage, color: section.tileColor)
        }
    }
}

/// A white SF Symbol on a small rounded colored square.
struct MacTile: View {
    let systemImage: String
    let color: Color
    var size: CGFloat = 20

    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: size * 0.55, weight: .semibold))
            .foregroundStyle(.white)
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
                    Text(tunnel.nodeName ?? String(localized: "还没有节点"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture {
                    state.section = .home
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
        .tint(.green)
        .disabled(tunnel.phase == .unconfigured || tunnel.phase == .disconnecting)
        .help("连接或断开（⇧⌘C）")
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
