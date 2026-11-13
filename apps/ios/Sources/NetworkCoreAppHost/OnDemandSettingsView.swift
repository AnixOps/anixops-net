import NetworkCoreBridge
import SwiftUI

/// Settings sections for reconnecting by itself (VPN On Demand) and the per Wi-Fi rules. Used in the iOS settings and
/// the macOS settings page; every change applies at once.
struct OnDemandSections: View {
    var tunnel: TunnelController
    @State private var settings = OnDemandSettings()
    @State private var newSSID = ""
    @State private var loaded = false

    var body: some View {
        Group {
            Section {
                Toggle("自动重连", isOn: $settings.enabled)
            } header: {
                Text("连接")
            } footer: {
                Text("隧道意外退出（崩溃、内存超限、系统更新）后由系统几秒内重新连接。手动断开时会一并关闭，下次连接再打开。")
            }

            if settings.enabled {
                Section {
                    ForEach($settings.wifi) { $rule in
                        Picker(rule.ssid, selection: $rule.action) {
                            actionChoices
                        }
                        .pickerStyle(.menu)
                        #if os(macOS)
                        .contextMenu {
                            Button("删除 \(rule.ssid)", role: .destructive) {
                                settings.removeRule(ssid: rule.ssid)
                            }
                        }
                        #endif
                    }
                    #if os(iOS)
                    .onDelete { offsets in
                        settings.wifi.remove(atOffsets: offsets)
                    }
                    #endif
                    HStack {
                        TextField("Wi-Fi 名称", text: $newSSID)
                            #if os(iOS)
                            .textInputAutocapitalization(.never)
                            #endif
                            .autocorrectionDisabled()
                            .onSubmit(addRule)
                        Button("添加", action: addRule)
                            .buttonStyle(.bordered)
                            .disabled(!canAdd)
                    }
                    Picker("其他 Wi-Fi", selection: $settings.otherWiFi) {
                        actionChoices
                    }
                    #if os(iOS)
                    Picker("蜂窝网络", selection: $settings.cellular) {
                        actionChoices
                    }
                    #endif
                } header: {
                    Text("Wi-Fi 规则")
                } footer: {
                    Text("加入网络时由系统立即执行，不需要打开 App。设为“断开”的网络下，手动连接也会被断开。")
                }
            }
        }
        .onAppear {
            guard !loaded else {
                return
            }
            settings = tunnel.onDemand
            loaded = true
        }
        .onChange(of: settings) { _, next in
            guard loaded else {
                return
            }
            Task { await tunnel.setOnDemand(next) }
        }
    }

    @ViewBuilder
    private var actionChoices: some View {
        ForEach(OnDemandSettings.Action.allCases) { action in
            Text(Self.label(action)).tag(action)
        }
    }

    private var canAdd: Bool {
        let name = newSSID.trimmingCharacters(in: .whitespacesAndNewlines)
        return !name.isEmpty && name.utf8.count <= OnDemandSettings.maxSSIDBytes
            && settings.wifi.count < OnDemandSettings.maxWiFiRules
    }

    private func addRule() {
        guard canAdd else {
            return
        }
        // A newly named network is usually a trusted one (home, office): it starts as "disconnect".
        settings.setRule(ssid: newSSID, action: .disconnect)
        newSSID = ""
    }

    static func label(_ action: OnDemandSettings.Action) -> String {
        switch action {
        case .connect:
            return String(localized: "连接")
        case .disconnect:
            return String(localized: "断开")
        case .ignore:
            return String(localized: "保持不变")
        }
    }
}
