import NetworkCoreBridge
import SwiftUI

/// Settings: the mode (proxy or debugging); appearance, reconnecting and Wi-Fi rules,
/// privacy and the About section that unlocks developer mode.
struct SettingsView: View {
    var tunnel: TunnelController
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @AppStorage(LiveActivityController.storageKey) private var liveActivity = true
    @State private var crashReports = ClientHealthMonitor.shared.crashReports.isEnabled
    @State private var panelReport = ClientReportStore().isEnabled
    @Environment(\.brand) private var brand

    var body: some View {
        NavigationStack {
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
                TakeoverSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)
                BlockQuicSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)
                NotificationSettingsSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)

                Section {
                    Label("不收集你的流量和浏览记录", systemImage: "hand.raised")
                        .listRowBackground(brand.surface)
                    Label("节点密码和密钥只保存在本机钥匙串", systemImage: "key")
                        .listRowBackground(brand.surface)
                    Toggle("崩溃报告", isOn: $crashReports)
                        .listRowBackground(brand.surface)
                        .onChange(of: crashReports) { _, enabled in
                            ClientHealthMonitor.shared.crashReports.isEnabled = enabled
                        }
                    Toggle("向订阅服务商报告连接质量", isOn: $panelReport)
                        .listRowBackground(brand.surface)
                        .onChange(of: panelReport) { _, enabled in
                            tunnel.setPanelReport(enabled: enabled)
                        }
                } header: {
                    Text("隐私")
                } footer: {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("崩溃时记录脱敏后的信息（不含网址、账号和节点地址），只保存在本机，复制诊断信息时附上。关闭会删除已保存的报告。")
                        Text("开启后，App 打开且已连接时，约每小时把 ANX 节点的连接计数发给提供这些节点的订阅服务商：发往订阅链接去掉参数和扩展名后加 /report 的地址，链接里的令牌随之发出，服务商能据此对应到你的账号。内容为 App 版本、平台、网络类型（Wi-Fi、蜂窝、有线）、统计时长，以及每个节点的名称、服务器地址和端口、协议、成功和各类失败次数、是否因被拒而暂停、时间偏差；不含你访问的网址。默认关闭，关闭时删除保存的计数。")
                    }
                }

                Section {
                    // Proxy mode reaches the usage history from the overview's usage card; debug mode has no
                    // overview, so it is here.
                    if tunnel.appMode == .debug {
                        NavigationLink {
                            TrafficStatsView(tunnel: tunnel)
                        } label: {
                            Text("流量统计")
                        }
                        .listRowBackground(brand.surface)
                    }
                    LogExportButton(tunnel: tunnel)
                        .listRowBackground(brand.surface)
                } header: {
                    Text("诊断")
                } footer: {
                    Text("把诊断信息存成 .log 文件，用系统分享发出去：含版本、扩展内存与事件、运行状态、发热和电量。地址、网址、账号和密钥已去掉。")
                }

                AboutSection(tunnel: tunnel)
                    .listRowBackground(brand.surface)
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("设置")
            .brandBars(brand)
        }
    }
}
