import NetworkCoreBridge
import SwiftUI

/// Proxies: every subscription group (Loon / Shadowrocket style) with its nodes, traffic, expiry and update
/// settings, or the single saved node; delay tests and switching across groups.
struct ProxiesView: View {
    @ObservedObject var tunnel: TunnelController
    @Binding var importing: Bool
    @Environment(\.brand) private var brand
    @State private var testing = false
    @State private var collapsed: Set<String> = []
    @State private var renaming: SubscriptionGroup?
    @State private var newName = ""
    @State private var deleting: SubscriptionGroup?
    @State private var query = ""
    @AppStorage("proxies.sort") private var sort = NodeSort.original
    @State private var editing: EditingNode?

    var body: some View {
        NavigationStack {
            Group {
                if !tunnel.subscriptions.groups.isEmpty {
                    groupList
                } else if let name = tunnel.nodeName {
                    singleNode(name)
                } else {
                    empty
                }
            }
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("代理")
            .toolbar {
                if !tunnel.subscriptions.groups.isEmpty {
                    ToolbarItem(placement: .topBarLeading) {
                        Button {
                            testing = true
                            Task {
                                await tunnel.testLatency()
                                testing = false
                            }
                        } label: {
                            Image(systemName: "speedometer")
                        }
                        .disabled(testing)
                        .accessibilityLabel("测试延迟")
                    }
                }
                if !tunnel.subscriptions.groups.isEmpty {
                    ToolbarItem(placement: .topBarTrailing) {
                        Menu {
                            Picker("排序", selection: $sort) {
                                Text("订阅顺序").tag(NodeSort.original)
                                Text("按名称").tag(NodeSort.name)
                                Text("按延迟").tag(NodeSort.latency)
                            }
                        } label: {
                            Image(systemName: "arrow.up.arrow.down")
                        }
                        .accessibilityLabel("排序")
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        importing = true
                    } label: {
                        Image(systemName: "plus")
                    }
                    .accessibilityLabel("导入节点")
                }
            }
            .brandBars(brand)
            .alert("重命名订阅组", isPresented: Binding(
                get: { renaming != nil },
                set: { if !$0 { renaming = nil } }
            )) {
                TextField("名称", text: $newName)
                Button("取消", role: .cancel) {
                    renaming = nil
                }
                Button("保存") {
                    if let group = renaming {
                        tunnel.renameSubscription(group.id, to: newName)
                    }
                    renaming = nil
                }
            }
            .sheet(item: $editing) { item in
                NodeEditSheet(node: item.node) { edited in
                    await tunnel.updateNode(edited, inGroup: item.groupID)
                }
                .environment(\.brand, brand)
                .tint(brand.primary)
            }
            .confirmationDialog(
                "删除订阅组“\(deleting?.name ?? "")”？",
                isPresented: Binding(
                    get: { deleting != nil },
                    set: { if !$0 { deleting = nil } }
                ),
                titleVisibility: .visible
            ) {
                Button("删除", role: .destructive) {
                    if let group = deleting {
                        Task {
                            await tunnel.deleteSubscription(group.id)
                        }
                    }
                    deleting = nil
                }
            } message: {
                Text("会删除这个订阅组的全部节点。正在使用它时，会切换到下一个订阅组。")
            }
        }
    }

    private var groupList: some View {
        List {
            if tunnel.subscriptions.activeID == nil, let name = tunnel.nodeName {
                Section {
                    singleNodeRow(name)
                        .listRowBackground(brand.surface)
                } header: {
                    Text("当前节点")
                }
            }
            ForEach(tunnel.subscriptions.groups) { group in
                let nodes = shownNodes(group)
                if filter.isEmpty || !nodes.isEmpty {
                    groupSection(group, nodes: nodes)
                }
            }
            Section {
            } footer: {
                VStack(alignment: .leading, spacing: 6) {
                    if tunnel.library?.routesByRules == true {
                        Text("正在按订阅规则分流，节点由策略组决定；可以在“规则”页切换策略组或关闭规则。其他订阅组会作为“订阅·名称”策略组出现在规则的选择组里。")
                    }
                    Text("下拉更新全部订阅。延迟为经该节点请求 http://www.gstatic.com/generate_204 的 HTTP 真连接时间，5 秒无响应显示超时。连接中切换节点会自动重连。")
                }
            }
        }
        .scrollContentBackground(.hidden)
        .searchable(text: $query, prompt: Text("筛选节点，/正则/ 匹配名称"))
        .refreshable {
            await tunnel.refreshAllSubscriptions()
        }
    }

    @ViewBuilder
    private func groupSection(_ group: SubscriptionGroup, nodes: [NativeImportReport.Node]) -> some View {
        let isActive = tunnel.subscriptions.activeID == group.id
        let routed = isActive && group.library.routesByRules
        Section {
            SubscriptionGroupSummary(group: group, updating: tunnel.updatingGroups.contains(group.id))
                .listRowBackground(brand.surface)
            if !collapsed.contains(group.id) || !filter.isEmpty {
                ForEach(nodes) { node in
                    Button {
                        guard !routed else {
                            return
                        }
                        Task {
                            await tunnel.selectNode(node.id, inGroup: group.id)
                        }
                    } label: {
                        row(
                            node,
                            latency: tunnel.latencies[TunnelController.latencyKey(group: group.id, node: node.id)],
                            selected: isActive && !routed && node.id == group.library.selectedID
                        )
                    }
                    .buttonStyle(.plain)
                    // Not `.disabled`: nodes the rules pick stay editable from the menu and swipe actions.
                    .opacity(routed ? 0.6 : 1)
                    .listRowBackground(brand.surface)
                    .contextMenu {
                        nodeMenu(node, in: group)
                    }
                    .swipeActions(edge: .trailing) {
                        Button {
                            editing = EditingNode(groupID: group.id, node: node)
                        } label: {
                            Label("编辑", systemImage: "pencil")
                        }
                        .tint(brand.primary)
                    }
                }
            }
        } header: {
            HStack(spacing: 8) {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        if collapsed.contains(group.id) {
                            collapsed.remove(group.id)
                        } else {
                            collapsed.insert(group.id)
                        }
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: collapsed.contains(group.id) ? "chevron.right" : "chevron.down")
                            .font(.caption.weight(.semibold))
                        Text(verbatim: group.name)
                            .lineLimit(1)
                        if isActive {
                            Text("使用中")
                                .font(.caption2.weight(.semibold))
                                .padding(.horizontal, 6)
                                .padding(.vertical, 2)
                                .background(brand.primary.opacity(0.18), in: Capsule())
                        }
                    }
                }
                .buttonStyle(.plain)
                .accessibilityLabel(collapsed.contains(group.id) ? "展开 \(group.name)" : "收起 \(group.name)")
                Spacer()
                groupMenu(group)
            }
        }
    }

    private var filter: NodeFilter {
        NodeFilter(query)
    }

    /// A group's nodes in the chosen order, narrowed by the search box.
    private func shownNodes(_ group: SubscriptionGroup) -> [NativeImportReport.Node] {
        let sorted = sort.sorted(group.library.nodes) { node in
            if case let .millis(value)? = tunnel.latencies[TunnelController.latencyKey(group: group.id, node: node.id)] {
                return value
            }
            return nil
        }
        return filter.apply(sorted)
    }

    @ViewBuilder
    private func nodeMenu(_ node: NativeImportReport.Node, in group: SubscriptionGroup) -> some View {
        Button {
            editing = EditingNode(groupID: group.id, node: node)
        } label: {
            Label("编辑", systemImage: "pencil")
        }
        if group.library.canRemove(node.id) {
            Button(role: .destructive) {
                Task {
                    await tunnel.deleteNode(node.id, inGroup: group.id)
                }
            } label: {
                Label("删除节点", systemImage: "trash")
            }
        }
    }

    private func groupMenu(_ group: SubscriptionGroup) -> some View {
        SubscriptionGroupMenu(tunnel: tunnel, group: group) {
            newName = group.name
            renaming = group
        } delete: {
            deleting = group
        }
    }

    private func row(_ node: NativeImportReport.Node, latency result: NodeLatency?, selected: Bool) -> some View {
        HStack(spacing: 12) {
            ProtocolBadge(outboundKind: node.protocol)
            VStack(alignment: .leading, spacing: 2) {
                Text(node.name.isEmpty ? ImportPreview.mask(node.server) : node.name)
                    .font(.body.weight(selected ? .semibold : .regular))
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                Text(ProtocolBadge.spokenName(for: node.protocol))
                    .font(.footnote)
                    .foregroundStyle(brand.text2)
            }
            Spacer()
            NodeLatencyText(result: result)
            if selected {
                Circle()
                    .fill(Brand.amber)
                    .frame(width: 8, height: 8)
                    .accessibilityLabel("当前节点")
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }

    private func singleNode(_ name: String) -> some View {
        List {
            Section {
                singleNodeRow(name)
                    .listRowBackground(brand.surface)
            } header: {
                Text("节点")
            } footer: {
                Text("导入订阅链接后，每个订阅是一个订阅组，可以测延迟、切换节点、手动或自动更新。")
            }
        }
        .scrollContentBackground(.hidden)
    }

    private func singleNodeRow(_ name: String) -> some View {
        HStack(spacing: 12) {
            ProtocolBadge(outboundKind: tunnel.outboundKind)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(brand.text)
                Text(ProtocolBadge.spokenName(for: tunnel.outboundKind))
                    .font(.footnote)
                    .foregroundStyle(brand.text2)
            }
            Spacer()
            Circle()
                .fill(Brand.amber)
                .frame(width: 8, height: 8)
                .accessibilityLabel("当前节点")
        }
        .padding(.vertical, 4)
    }

    private var empty: some View {
        ScrollView {
            BrandEmptyState(
                systemImage: "globe",
                title: "还没有节点",
                message: "粘贴订阅链接，或 ss://、vless://、trojan://、vmess://、hysteria2://、tuic:// 等链接和订阅内容，或导入 WireGuard 配置。"
            ) {
                Button {
                    importing = true
                } label: {
                    Text("导入节点")
                        .font(.body.weight(.semibold))
                        .padding(.horizontal, 24)
                        .frame(height: 44)
                }
                .buttonStyle(.borderedProminent)
                .padding(.top, 6)
            }
            .padding(.top, 80)
        }
    }
}


/// The node an edit sheet is open for.
private struct EditingNode: Identifiable {
    let groupID: String
    let node: NativeImportReport.Node

    var id: String {
        "\(groupID)\u{1F}\(node.id)"
    }
}

/// Edits a node's name, server and port. Credentials and transport settings stay as imported.
private struct NodeEditSheet: View {
    let node: NativeImportReport.Node
    let save: (NativeImportReport.Node) async -> Bool
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @State private var server: String
    @State private var port: String
    @State private var error: String?
    @State private var saving = false

    init(node: NativeImportReport.Node, save: @escaping (NativeImportReport.Node) async -> Bool) {
        self.node = node
        self.save = save
        _name = State(initialValue: node.name)
        _server = State(initialValue: node.server)
        _port = State(initialValue: String(node.port))
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("名称", text: $name)
                    TextField("服务器", text: $server)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                    TextField("端口", text: $port)
                        .keyboardType(.numberPad)
                } header: {
                    Text(ProtocolBadge.spokenName(for: node.protocol))
                } footer: {
                    VStack(alignment: .leading, spacing: 6) {
                        if let error {
                            Label(error, systemImage: "exclamationmark.circle.fill")
                                .foregroundStyle(brand.danger)
                        }
                        Text("密码、密钥和传输设置保持导入时的值。订阅更新后会恢复为订阅里的节点。")
                    }
                }
                .listRowBackground(brand.surface)
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("编辑节点")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        submit()
                    }
                    .disabled(saving)
                }
            }
        }
    }

    private func submit() {
        switch NodeEdit.apply(to: node, name: name, server: server, port: port) {
        case let .failure(failure):
            error = failure.message
        case let .success(edited):
            saving = true
            Task {
                let saved = await save(edited)
                saving = false
                if saved {
                    dismiss()
                } else {
                    error = String(localized: "保存失败")
                }
            }
        }
    }
}
