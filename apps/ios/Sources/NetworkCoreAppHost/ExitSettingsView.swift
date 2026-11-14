import NetworkCoreBridge
import SwiftUI

/// Settings › Exit in debug mode (plan G2): whether captured traffic goes straight out or through a node, and the
/// subscriptions, nodes and rules that the proxy mode shows as tabs of their own.
struct ExitSettingsView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var importing = false

    var body: some View {
        List {
            Section {
                DebugExitPicker(tunnel: tunnel)
            } footer: {
                Text(DebugExitPicker.footer(tunnel))
            }
            .listRowBackground(brand.surface)
            Section {
                NavigationLink {
                    ProxiesView(tunnel: tunnel, importing: $importing, embedded: true)
                } label: {
                    LabeledContent("订阅与节点", value: nodeSummary)
                }
                NavigationLink {
                    RulesView(tunnel: tunnel, embedded: true)
                } label: {
                    Text("规则与 DNS")
                }
            } footer: {
                Text("出口为“经过节点”时才用到订阅、节点和规则；直连时它们原样保留。")
            }
            .listRowBackground(brand.surface)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("出口")
        .inlineNavigationTitle()
        .sheet(isPresented: $importing) {
            ImportView(tunnel: tunnel)
                .environment(\.brand, brand)
                .tint(brand.primary)
        }
    }

    private var nodeSummary: String {
        let count = tunnel.subscriptions.groups.reduce(0) { $0 + $1.library.nodes.count }
        if count == 0 {
            return tunnel.hasNoNodes ? String(localized: "未导入") : (tunnel.nodeName ?? "")
        }
        return String(localized: "\(count) 个节点")
    }
}

/// The settings row that opens the exit page, with the exit in use.
struct ExitSettingsRow: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            NavigationLink {
                ExitSettingsView(tunnel: tunnel)
            } label: {
                LabeledContent("出口", value: value)
            }
            .listRowBackground(brand.surface)
        }
    }

    private var value: String {
        if tunnel.usesDirectExit {
            return String(localized: "直连")
        }
        return tunnel.nodeName ?? String(localized: "经过节点")
    }
}
