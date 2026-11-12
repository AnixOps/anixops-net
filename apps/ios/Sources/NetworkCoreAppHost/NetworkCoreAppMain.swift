import NetworkCoreBridge
import NetworkExtension
import SwiftUI

// SwiftUI entry point for the containing app. The app target compiles NetworkCoreApp sources directly,
// so this file refers to AppCoordinator without importing the NetworkCoreApp module.
@main
struct NetworkCoreAppMain: App {
    @State private var tunnel = TunnelController()
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @Environment(\.scenePhase) private var scenePhase

    init() {
        AppNotifications.install()
    }

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
    var tunnel: TunnelController
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var tab = AppTab.home
    @State private var importing = false
    @AppStorage(OnboardingView.storageKey) private var onboardingDone = false
    @AppStorage(LiveActivityController.storageKey) private var liveActivity = true
    @Environment(\.scenePhase) private var scenePhase

    /// First run only: after loading, with nothing imported and the welcome screen never closed.
    private var showsOnboarding: Binding<Bool> {
        Binding(
            get: { tunnel.loaded && tunnel.hasNoNodes && !onboardingDone },
            set: { shown in
                if !shown {
                    onboardingDone = true
                }
            }
        )
    }

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
        // `anixops://` links from Shortcuts, other apps and web pages (`AppURLCommand`).
        .onOpenURL { url in
            Task {
                if await tunnel.handle(url: url) == .openImport {
                    importing = true
                }
            }
        }
        .onChange(of: tunnel.loaded) { _, loaded in
            // Someone who already has nodes never needs the welcome screen, even after deleting them all later.
            if loaded, !tunnel.hasNoNodes {
                onboardingDone = true
            }
        }
        .onChange(of: activityInputs, initial: true) { _, _ in
            LiveActivityController.sync(
                enabled: liveActivity,
                connection: tunnel.connection,
                nodeName: tunnel.nodeName,
                outboundKind: tunnel.outboundKind,
                connectedSince: tunnel.connectedSince
            )
        }
        .fullScreenCover(isPresented: showsOnboarding) {
            OnboardingView {
                onboardingDone = true
                // The import sheet opens once the cover has gone.
                Task { @MainActor in
                    try? await Task.sleep(for: .milliseconds(450))
                    importing = true
                }
            } later: {
                onboardingDone = true
            }
            .environment(\.brand, brand)
            .tint(brand.primary)
        }
    }

    /// What the Live Activity shows, plus the foreground state, since iOS only starts one from the open app.
    private var activityInputs: ActivityInputs {
        ActivityInputs(
            enabled: liveActivity,
            active: scenePhase == .active,
            connection: tunnel.connection,
            nodeName: tunnel.nodeName,
            connectedSince: tunnel.connectedSince
        )
    }

    private struct ActivityInputs: Equatable {
        let enabled: Bool
        let active: Bool
        let connection: NEVPNStatus
        let nodeName: String?
        let connectedSince: Date?
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
    /// Navigation and tab bars keep the system material: AnixOps Design's frosted bars over `bg`, in both themes.
    func brandBars(_ brand: BrandPalette) -> some View {
        self
    }
}
