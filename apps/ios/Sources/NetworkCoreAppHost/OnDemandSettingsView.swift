import NetworkCoreBridge
import SwiftUI

/// Settings section for reconnecting by itself (VPN On Demand), with a row into the per Wi-Fi rules. Used in the iOS
/// settings and the macOS settings page; every change applies at once. This view alone owns the settings and saves
/// them; the rules page edits the same value through a binding, so there is never a second copy that could overwrite
/// the rules.
struct OnDemandSections: View {
    var tunnel: TunnelController
    @State private var settings = OnDemandSettings()
    @State private var loaded = false
    #if os(iOS)
    @Environment(\.brand) private var brand
    #endif

    var body: some View {
        Section {
            Toggle("自动重连", isOn: $settings.enabled)
            if settings.enabled {
                NavigationLink {
                    OnDemandWiFiRulesPage(settings: $settings)
                } label: {
                    LabeledContent("Wi-Fi 规则") {
                        Text("\(settings.wifi.count) 条")
                    }
                }
            }
        } header: {
            Text("连接")
        } footer: {
            Text("意外退出后由系统自动重新连接；手动断开时会一并关闭，下次连接再打开。")
        }
        #if os(iOS)
        .listRowBackground(brand.surface)
        #endif
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

    static func label(_ action: OnDemandSettings.Action) -> String {
        switch action {
        case .connect:
            // "连接" alone is the noun "Connection" in the catalog; the action has its own key.
            return String(localized: "action.connect")
        case .disconnect:
            return String(localized: "断开")
        case .ignore:
            return String(localized: "保持不变")
        }
    }
}

/// The per Wi-Fi rules, one tap below the connection switch. Edits the owner's settings through a binding; the owner
/// saves every change.
struct OnDemandWiFiRulesPage: View {
    @Binding var settings: OnDemandSettings
    @State private var newSSID = ""
    @Environment(\.brand) private var brand

    var body: some View {
        Form {
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
            } footer: {
                Text("加入网络时由系统立即执行，无需打开 App。设为“断开”的网络下，手动连接也会被断开。")
            }
            #if os(iOS)
            .listRowBackground(brand.surface)
            #endif
        }
        .navigationTitle("Wi-Fi 规则")
        #if os(iOS)
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .brandBars(brand)
        #else
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .frame(maxWidth: 720)
        .frame(maxWidth: .infinity)
        #endif
    }

    @ViewBuilder
    private var actionChoices: some View {
        ForEach(OnDemandSettings.Action.allCases) { action in
            Text(OnDemandSections.label(action)).tag(action)
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
}
