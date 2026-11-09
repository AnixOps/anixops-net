import SwiftUI

/// Settings: appearance, privacy and the About section that unlocks developer mode.
struct SettingsView: View {
    var tunnel: TunnelController
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @AppStorage(LiveActivityController.storageKey) private var liveActivity = true
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
                } header: {
                    Text("隐私")
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
