import NetworkCoreBridge
import NetworkExtension
import SwiftUI

// SwiftUI entry point for the containing app. The app target compiles NetworkCoreApp sources directly,
// so this file refers to AppCoordinator without importing the NetworkCoreApp module.
@main
struct NetworkCoreAppMain: App {
    @StateObject private var tunnel = TunnelController()
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootView(tunnel: tunnel)
                .preferredColorScheme(appearance.colorScheme)
                .task {
                    await tunnel.load()
                    await tunnel.refreshDueSubscriptions()
                }
        }
        .onChange(of: scenePhase) { _, phase in
            switch phase {
            case .active:
                Task {
                    await tunnel.refreshDueSubscriptions()
                }
            case .background:
                tunnel.scheduleBackgroundRefresh()
            default:
                break
            }
        }
        // Subscription groups that are due update in the background too (`TunnelController.scheduleBackgroundRefresh`).
        .backgroundTask(.appRefresh(TunnelController.backgroundRefreshID)) {
            await tunnel.refreshDueSubscriptions()
        }
    }
}

/// The five top-level sections shared by every AnixOps Net client.
enum AppTab: Hashable, CaseIterable, Identifiable {
    case home
    case proxies
    case rules
    case extensions
    case settings

    var id: Self {
        self
    }

    var title: LocalizedStringKey {
        switch self {
        case .home:
            return "首页"
        case .proxies:
            return "代理"
        case .rules:
            return "规则"
        case .extensions:
            return "扩展"
        case .settings:
            return "设置"
        }
    }

    var systemImage: String {
        switch self {
        case .home:
            return "house"
        case .proxies:
            return "globe"
        case .rules:
            return "arrow.triangle.branch"
        case .extensions:
            return "puzzlepiece.extension"
        case .settings:
            return "slider.horizontal.3"
        }
    }
}

/// Tabs on iPhone and in a narrow iPad window; a sidebar with the same five sections on a regular-width iPad window.
struct RootView: View {
    @ObservedObject var tunnel: TunnelController
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var tab = AppTab.home
    @State private var importing = false

    var body: some View {
        let brand = BrandPalette.resolve(appearance: appearance, scheme: colorScheme)
        Group {
            if sizeClass == .regular {
                splitView(brand)
            } else {
                tabView
            }
        }
        .tint(brand.primary)
        .environment(\.brand, brand)
        .sheet(isPresented: $importing) {
            ImportView(tunnel: tunnel)
                .environment(\.brand, brand)
                .tint(brand.primary)
        }
    }

    private var tabView: some View {
        TabView(selection: $tab) {
            ForEach(AppTab.allCases) { section in
                content(section)
                    .tabItem { Label(section.title, systemImage: section.systemImage) }
                    .tag(section)
            }
        }
    }

    private func splitView(_ brand: BrandPalette) -> some View {
        NavigationSplitView {
            List(selection: Binding<AppTab?>(get: { tab }, set: { tab = $0 ?? tab })) {
                ForEach(AppTab.allCases) { section in
                    Label(section.title, systemImage: section.systemImage)
                        .tag(section)
                }
            }
            .navigationTitle(Text(verbatim: "AnixOps Net"))
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
        } detail: {
            content(tab)
                .id(tab)
        }
        .navigationSplitViewStyle(.balanced)
    }

    @ViewBuilder
    private func content(_ section: AppTab) -> some View {
        switch section {
        case .home:
            HomeView(tunnel: tunnel, tab: $tab, importing: $importing)
        case .proxies:
            ProxiesView(tunnel: tunnel, importing: $importing)
        case .rules:
            RulesView(tunnel: tunnel)
        case .extensions:
            ExtensionsView(tunnel: tunnel)
        case .settings:
            SettingsView(tunnel: tunnel)
        }
    }
}

extension View {
    /// OLED keeps the navigation and tab bars pure black; other themes keep the system material.
    @ViewBuilder
    func brandBars(_ brand: BrandPalette) -> some View {
        if brand.solidBars {
            self
                .toolbarBackground(brand.bg, for: .navigationBar, .tabBar)
                .toolbarBackground(.visible, for: .navigationBar, .tabBar)
        } else {
            self
        }
    }
}
