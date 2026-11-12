import SwiftUI

/// Settings: appearance, privacy and the About section that unlocks developer mode.
struct SettingsView: View {
    var tunnel: TunnelController
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @AppStorage(LiveActivityController.storageKey) private var liveActivity = true
    @State private var crashReports = ClientHealthMonitor.shared.crashReports.isEnabled
    @Environment(\.brand) private var brand

    var body: some View {
        NavigationStack {
            Form {
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
                } header: {
                    Text("隐私")
                } footer: {
                    Text("崩溃时记录脱敏后的信息（不含网址、账号和节点地址），只保存在本机，复制诊断信息时附上。关闭会删除已保存的报告。")
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
