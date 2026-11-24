import NetworkCoreBridge
import SwiftUI

/// Settings: the mode (proxy or debugging); appearance, reconnecting, notifications and the About section that
/// unlocks developer mode. Everything rarely used sits one level down, under 高级 and 隐私与诊断, so the first screen
/// stays short for everyday use.
struct SettingsView: View {
    var tunnel: TunnelController
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @AppStorage(LiveActivityController.storageKey) private var liveActivity = true
    @State private var path: [SettingsSubpage] = SettingsSubpage.launch.map { [$0] } ?? []
    @Environment(\.brand) private var brand

    var body: some View {
        NavigationStack(path: $path) {
            Form {
                AppModeSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)

                Section {
                    Picker("外观", selection: $appearance.normalized) {
                        ForEach(Appearance.choices) { option in
                            Text(option.label).tag(option)
                        }
                    }
                    .pickerStyle(.segmented)
                    .listRowBackground(brand.surface)
                    Toggle("锁定屏幕和灵动岛显示连接", isOn: $liveActivity)
                        .listRowBackground(brand.surface)
                } header: {
                    Text("外观")
                } footer: {
                    Text("连接后显示节点和已连接时长，可以直接断开。")
                }

                OnDemandSections(tunnel: tunnel)

                Section {
                    NavigationLink(value: SettingsSubpage.notifications) {
                        Label("通知", systemImage: "bell")
                    }
                    .listRowBackground(brand.surface)
                }

                Section {
                    NavigationLink(value: SettingsSubpage.privacy) {
                        Label("隐私与诊断", systemImage: "hand.raised")
                    }
                    .listRowBackground(brand.surface)
                    NavigationLink(value: SettingsSubpage.advanced) {
                        Label("高级", systemImage: "gearshape.2")
                    }
                    .listRowBackground(brand.surface)
                }

                AboutSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("设置")
            .brandBars(brand)
            .navigationDestination(for: SettingsSubpage.self) { page in
                subpage(page)
                    .scrollContentBackground(.hidden)
                    .background(brand.bg.ignoresSafeArea())
                    .navigationTitle(page.title)
                    .brandBars(brand)
            }
        }
    }

    @ViewBuilder
    private func subpage(_ page: SettingsSubpage) -> some View {
        switch page {
        case .advanced:
            Form {
                TakeoverSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)
                BlockQuicSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)
            }
        case .privacy:
            Form {
                PrivacySettingsSection(tunnel: tunnel)

                Section {
                    // Proxy mode reaches the usage history from the overview's usage card; debug mode has no
                    // overview, so it is here.
                    if tunnel.appMode == .debug {
                        NavigationLink {
                            TrafficStatsView(tunnel: tunnel)
                        } label: {
                            Text("流量统计")
                        }
                    }
                    LogExportButton(tunnel: tunnel)
                } header: {
                    Text("诊断")
                } footer: {
                    Text("将诊断信息保存为 .log 文件，经系统分享发送；地址、网址、账号和密钥已去除。")
                }
                .listRowBackground(brand.surface)
            }
        case .notifications:
            Form {
                NotificationSettingsSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)
            }
        }
    }
}
