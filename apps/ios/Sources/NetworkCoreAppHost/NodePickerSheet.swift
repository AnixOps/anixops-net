import NetworkCoreBridge
import SwiftUI

/// The one node picker every "current node" button opens: the active subscription group's nodes, fastest first,
/// with a search box and a delay test. Picking a node switches to it at once; a connected tunnel reconnects on its
/// own. When the subscription's rules choose the node, the sheet says so and points to the policy groups instead.
struct NodePickerSheet: View {
    var tunnel: TunnelController
    /// Opens the Proxies tab, for the policy groups and the other subscriptions.
    let showProxies: () -> Void
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var testing = false

    private var group: SubscriptionGroup? {
        tunnel.subscriptions.active
    }

    var body: some View {
        NavigationStack {
            content
                .scrollContentBackground(.hidden)
                .background(brand.bg.ignoresSafeArea())
                .navigationTitle("选择线路")
                .inlineNavigationTitle()
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("完成") {
                            dismiss()
                        }
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Button {
                            Task {
                                testing = true
                                await tunnel.testLatency()
                                testing = false
                            }
                        } label: {
                            if testing {
                                ProgressView()
                            } else {
                                Text("全部测速")
                            }
                        }
                        .disabled(testing || group == nil)
                    }
                }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    @ViewBuilder
    private var content: some View {
        if let group {
            if group.library.routesByRules {
                routed
            } else {
                list(group)
            }
        } else {
            BrandEmptyState(systemImage: "globe", title: "没有可选的节点", message: "导入订阅后，可以在这里切换节点。")
        }
    }

    private func list(_ group: SubscriptionGroup) -> some View {
        let nodes = shownNodes(group)
        return List {
            Section {
                ForEach(nodes) { node in
                    Button {
                        Task {
                            await tunnel.selectNode(node.id, inGroup: group.id)
                            dismiss()
                        }
                    } label: {
                        row(node, in: group)
                    }
                    .buttonStyle(.plain)
                    .listRowBackground(brand.surface)
                }
            } header: {
                Text(verbatim: group.name)
            } footer: {
                if nodes.isEmpty {
                    Text("没有匹配的节点。")
                } else {
                    Text("按延迟排序，未测速的节点在后。连接中切换节点会自动重连。")
                }
            }
            Section {
                Button("查看全部订阅和策略组") {
                    dismiss()
                    showProxies()
                }
                .listRowBackground(brand.surface)
            }
        }
        .searchable(text: $query, prompt: Text("搜索节点，/正则/ 匹配名称"))
    }

    private var routed: some View {
        List {
            Section {
                Text("当前订阅按规则分流，每条流量走哪个节点由策略组决定。")
                    .foregroundStyle(brand.text2)
                    .listRowBackground(brand.surface)
                Button("去代理页调整策略组") {
                    dismiss()
                    showProxies()
                }
                .listRowBackground(brand.surface)
            }
        }
    }

    private func row(_ node: NativeImportReport.Node, in group: SubscriptionGroup) -> some View {
        let selected = tunnel.subscriptions.activeID == group.id && node.id == group.library.selectedID
        return HStack(spacing: 12) {
            ProtocolBadge(outboundKind: node.protocol)
            Text(node.name.isEmpty ? ImportPreview.mask(node.server) : node.name)
                .font(.body.weight(selected ? .semibold : .regular))
                .foregroundStyle(brand.text)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
            NodeLatencyText(result: tunnel.latencies[TunnelController.latencyKey(group: group.id, node: node.id)])
            Image(systemName: "checkmark")
                .font(.body.weight(.semibold))
                .foregroundStyle(brand.primary)
                .opacity(selected ? 1 : 0)
                .accessibilityHidden(!selected)
                .accessibilityLabel("当前节点")
        }
        .frame(minHeight: 44)
        .contentShape(Rectangle())
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    /// The group's nodes, fastest first, narrowed by the search box.
    private func shownNodes(_ group: SubscriptionGroup) -> [NativeImportReport.Node] {
        let sorted = NodeSort.latency.sorted(group.library.nodes) { node in
            latencyMillis(group: group, node: node)
        }
        return NodeFilter(query).apply(sorted)
    }

    private func latencyMillis(group: SubscriptionGroup, node: NativeImportReport.Node) -> Int? {
        let key = TunnelController.latencyKey(group: group.id, node: node.id)
        if case let .millis(value)? = tunnel.latencies[key] {
            return value
        }
        return nil
    }
}
