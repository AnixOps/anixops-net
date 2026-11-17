import NetworkCoreBridge
import SwiftUI

/// Settings: how the tunnel takes over traffic, the tunnel interface, the system proxy or both, and what the system
/// proxy leaves alone. A change restarts a running tunnel.
struct TakeoverSection: View {
    var tunnel: TunnelController
    @State private var editingExceptions = false

    var body: some View {
        Section {
            Picker(
                "接管方式",
                selection: Binding(
                    get: { tunnel.takeover },
                    set: { next in
                        Task {
                            await tunnel.setTakeover(next)
                        }
                    }
                )
            ) {
                ForEach(TrafficTakeover.choices(macOS: TunnelController.isMac)) { choice in
                    Text(Self.title(choice)).tag(choice)
                }
            }
            .pickerStyle(.segmented)
            if tunnel.takeover.usesSystemProxy {
                Button {
                    editingExceptions = true
                } label: {
                    LabeledContent("跳过代理的地址") {
                        Text(tunnel.proxyExceptions.isDefault
                            ? String(localized: "默认")
                            : String(localized: "\(tunnel.proxyExceptions.entries.count) 项"))
                    }
                }
                .buttonStyle(.borderless)
            }
        } header: {
            Text("接管方式")
        } footer: {
            Text(Self.explanation(tunnel.takeover))
        }
        .sheet(isPresented: $editingExceptions) {
            ProxyExceptionsEditor(exceptions: tunnel.proxyExceptions) { exceptions in
                Task {
                    await tunnel.setProxyExceptions(exceptions)
                }
            }
        }
    }

    static func title(_ takeover: TrafficTakeover) -> String {
        switch takeover {
        case .tunnelAndProxy:
            String(localized: "VPN + 系统代理")
        case .tunnel:
            String(localized: "仅 VPN")
        case .systemProxy:
            String(localized: "仅系统代理")
        }
    }

    /// What the mode does and what it costs, including the restart.
    static func explanation(_ takeover: TrafficTakeover) -> String {
        let restart = String(localized: "切换时如果已连接，会自动重连一次。")
        switch takeover {
        case .tunnelAndProxy:
            return String(localized: "VPN 接管全部流量，同时把系统 HTTP / HTTPS 代理指向本机。遵守代理设置的 App（大部分网页和系统网络请求）直接交给代理，不经过 VPN 里的 TCP/IP 协议栈，更省电，规则也能直接看到域名和明文 HTTP 的完整网址；其余流量照常走 VPN。") + restart
        case .tunnel:
            return String(localized: "只用 VPN 接管全部流量，不设置系统代理。某个 App 用代理时出问题，可以先换成这一项排查。") + restart
        case .systemProxy:
            return String(localized: "只设置系统 HTTP / HTTPS 代理，不创建默认路由。遵守代理设置的 App 按规则走；不遵守的 App 和 UDP 流量直接出去，不经过规则、MITM 和抓包。最省资源。") + restart
        }
    }
}

/// Edits the system proxy's exceptions as text, one entry per line; lines that do not parse are named and kept
/// out of the saved list.
struct ProxyExceptionsEditor: View {
    let exceptions: ProxyExceptions
    let save: (ProxyExceptions) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var text = ""
    @State private var rejected: [Int] = []

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextEditor(text: $text)
                        .font(.body.monospaced())
                        .plainTextEntry()
                        .frame(minHeight: 220)
                        .onChange(of: text) { _, value in
                            rejected = ProxyExceptions.parse(value).rejected
                        }
                } footer: {
                    Text("每行一项：域名（router.lan）、通配符（*.local）、IP 或网段（192.168.0.0/16）。这些地址不走系统代理；VPN 接管时仍按规则走 VPN。# 开头的行是注释。")
                }
                .listRowBackground(brand.surface)
                if !rejected.isEmpty {
                    Text(String(localized: "第 \(rejected.map(String.init).joined(separator: "、")) 行不是域名、通配符或网段，保存时会去掉。"))
                        .foregroundStyle(brand.danger)
                        .listRowBackground(brand.surface)
                }
                Section {
                    Button("填入默认列表") {
                        text = ProxyExceptions().text
                    }
                    .buttonStyle(.borderless)
                    .disabled(text == ProxyExceptions().text)
                }
                .listRowBackground(brand.surface)
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("跳过代理的地址")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        save(ProxyExceptions.parse(text).exceptions)
                        dismiss()
                    }
                }
            }
            .onAppear {
                text = exceptions.text
                rejected = []
            }
        }
    }
}
