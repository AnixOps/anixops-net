import AppKit
import NetworkCoreBridge
import SwiftUI

/// Proxies: every subscription group with its nodes, traffic, expiry and update settings, or the single saved
/// node; delay tests, switching across groups, manual and automatic updates.
struct MacProxiesPage: View {
    @ObservedObject var tunnel: TunnelController
    @ObservedObject var state: MacAppState
    @Environment(\.brand) private var brand
    @State private var testing = false
    @State private var updatingAll = false
    @State private var collapsed: Set<String> = []
    @State private var renaming: SubscriptionGroup?
    @State private var newName = ""
    @State private var deleting: SubscriptionGroup?
    @State private var query = ""
    @AppStorage("proxies.sort") private var sort = NodeSort.original
    @State private var editing: MacEditingNode?
    @State private var notice: String?

    var body: some View {
        Group {
            if !tunnel.subscriptions.groups.isEmpty {
                groups
            } else if let name = tunnel.nodeName {
                ScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        MacCard(title: "节点") {
                            singleNodeRow(name)
                        }
                        Text("导入订阅链接后，每个订阅是一个订阅组，可以测延迟、切换节点、手动或自动更新。")
                            .font(.caption)
                            .foregroundStyle(brand.text2)
                            .padding(.horizontal, 4)
                    }
                    .padding(24)
                    .frame(maxWidth: 860)
                    .frame(maxWidth: .infinity)
                }
            } else {
                BrandEmptyState(
                    systemImage: "globe",
                    title: "还没有节点",
                    message: "粘贴订阅链接，或 ss://、vless://、trojan://、vmess://、hysteria2://、tuic:// 等链接和订阅内容，或导入 WireGuard 配置。"
                ) {
                    Button("导入节点") {
                        state.importing = true
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .padding(.top, 6)
                }
                .frame(maxWidth: 520)
                .padding(.top, 80)
            }
        }
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
        .confirmationDialog(
            "删除订阅组“\(deleting?.name ?? "")”？",
            isPresented: Binding(
                get: { deleting != nil },
                set: { if !$0 { deleting = nil } }
            )
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
        .sheet(item: $editing) { item in
            NodeEditSheet(node: item.node) { edited in
                await tunnel.updateNode(edited, inGroup: item.groupID)
            }
            .formSheetFrame()
            .environment(\.brand, brand)
            .tint(brand.primary)
        }
        .alert(notice ?? "", isPresented: Binding(get: { notice != nil }, set: { if !$0 { notice = nil } })) {
            Button("好", role: .cancel) {}
        }
    }

    private var groups: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 10) {
                    Button {
                        testing = true
                        Task {
                            await tunnel.testLatency()
                            testing = false
                        }
                    } label: {
                        Label("测试延迟", systemImage: "speedometer")
                    }
                    .disabled(testing)
                    Button {
                        updatingAll = true
                        Task {
                            await tunnel.refreshAllSubscriptions()
                            updatingAll = false
                        }
                    } label: {
                        Label("更新全部订阅", systemImage: "arrow.clockwise")
                    }
                    .disabled(updatingAll)
                    Spacer()
                    Picker("排序", selection: $sort) {
                        Text("订阅顺序").tag(NodeSort.original)
                        Text("按名称").tag(NodeSort.name)
                        Text("按延迟").tag(NodeSort.latency)
                    }
                    .pickerStyle(.menu)
                    .fixedSize()
                }

                if tunnel.subscriptions.activeID == nil, let name = tunnel.nodeName {
                    MacCard(title: "当前节点") {
                        singleNodeRow(name)
                    }
                }

                ForEach(tunnel.subscriptions.groups) { group in
                    groupCard(group)
                }

                VStack(alignment: .leading, spacing: 6) {
                    if tunnel.library?.routesByRules == true {
                        Text("正在按订阅规则分流，节点由策略组决定；可以在“规则”页切换策略组或关闭规则。其他订阅组会作为“订阅·名称”策略组出现在规则的选择组里。")
                    }
                    Text("延迟为经该节点请求 http://www.gstatic.com/generate_204 的 HTTP 真连接时间，5 秒无响应显示超时。连接中切换节点会自动重连。")
                }
                .font(.caption)
                .foregroundStyle(brand.text2)
                .padding(.horizontal, 4)
            }
            .padding(24)
            .frame(maxWidth: 860)
            .frame(maxWidth: .infinity)
        }
        .searchable(text: $query, prompt: Text("筛选节点，/正则/ 匹配名称"))
    }

    private func groupCard(_ group: SubscriptionGroup) -> some View {
        let isActive = tunnel.subscriptions.activeID == group.id
        let routed = isActive && group.library.routesByRules
        let isCollapsed = collapsed.contains(group.id)
        return VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        if isCollapsed {
                            collapsed.remove(group.id)
                        } else {
                            collapsed.insert(group.id)
                        }
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: isCollapsed ? "chevron.right" : "chevron.down")
                            .font(.caption.weight(.semibold))
                            .frame(width: 12)
                        Text(verbatim: group.name)
                            .font(.headline)
                            .foregroundStyle(brand.text)
                            .lineLimit(1)
                        if isActive {
                            Text("使用中")
                                .font(.caption2.weight(.semibold))
                                .padding(.horizontal, 6)
                                .padding(.vertical, 2)
                                .background(brand.primary.opacity(0.18), in: Capsule())
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(isCollapsed ? "展开 \(group.name)" : "收起 \(group.name)")
                Spacer()
                SubscriptionGroupMenu(tunnel: tunnel, group: group) {
                    newName = group.name
                    renaming = group
                } delete: {
                    deleting = group
                }
                .menuStyle(.button)
                .buttonStyle(.borderless)
                .menuIndicator(.hidden)
                .fixedSize()
            }

            SubscriptionGroupSummary(group: group, updating: tunnel.updatingGroups.contains(group.id))

            if !isCollapsed {
                Divider()
                VStack(spacing: 0) {
                    ForEach(shownNodes(group)) { node in
                        Button {
                            guard !routed else {
                                return
                            }
                            Task {
                                await tunnel.selectNode(node.id, inGroup: group.id)
                            }
                        } label: {
                            nodeRow(
                                node,
                                latency: tunnel.latencies[TunnelController.latencyKey(group: group.id, node: node.id)],
                                selected: isActive && !routed && node.id == group.library.selectedID
                            )
                        }
                        .buttonStyle(.plain)
                        // Not `.disabled`: nodes the rules pick stay editable from the menu.
                        .opacity(routed ? 0.6 : 1)
                        .contextMenu {
                            nodeMenu(node, in: group)
                        }
                    }
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(brand.surface, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(isActive ? brand.primary.opacity(0.6) : brand.border.opacity(0.6))
        )
    }

    /// A group's nodes in the chosen order, narrowed by the search field.
    private func shownNodes(_ group: SubscriptionGroup) -> [NativeImportReport.Node] {
        let sorted = sort.sorted(group.library.nodes) { node in
            if case let .millis(value)? = tunnel.latencies[TunnelController.latencyKey(group: group.id, node: node.id)] {
                return value
            }
            return nil
        }
        return NodeFilter(query).apply(sorted)
    }

    @ViewBuilder
    private func nodeMenu(_ node: NativeImportReport.Node, in group: SubscriptionGroup) -> some View {
        Button("编辑…") {
            editing = MacEditingNode(groupID: group.id, node: node)
        }
        Button("复制分享链接") {
            do {
                let link = try NativeNodeImporterFFI.shareLink(for: node)
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(link, forType: .string)
                notice = String(localized: "已复制分享链接。链接里含有节点密码，只分享给信任的人。")
            } catch let failure as NodeShareFailure {
                notice = failure.message
            } catch {
                notice = NodeShareFailure.unavailable.message
            }
        }
        if group.library.canRemove(node.id) {
            Divider()
            Button("删除节点", role: .destructive) {
                Task {
                    await tunnel.deleteNode(node.id, inGroup: group.id)
                }
            }
        }
    }

    private func nodeRow(_ node: NativeImportReport.Node, latency: NodeLatency?, selected: Bool) -> some View {
        HStack(spacing: 12) {
            ProtocolBadge(outboundKind: node.protocol)
            VStack(alignment: .leading, spacing: 2) {
                Text(node.name.isEmpty ? ImportPreview.mask(node.server) : node.name)
                    .font(.body.weight(selected ? .semibold : .regular))
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                Text(ProtocolBadge.spokenName(for: node.protocol))
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            }
            Spacer()
            NodeLatencyText(result: latency)
            if selected {
                Circle()
                    .fill(Brand.amber)
                    .frame(width: 8, height: 8)
                    .accessibilityLabel("当前节点")
            }
        }
        .padding(.vertical, 6)
        .contentShape(Rectangle())
    }

    private func singleNodeRow(_ name: String) -> some View {
        HStack(spacing: 12) {
            ProtocolBadge(outboundKind: tunnel.outboundKind)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(brand.text)
                Text(ProtocolBadge.spokenName(for: tunnel.outboundKind))
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            }
            Spacer()
            Circle()
                .fill(Brand.amber)
                .frame(width: 8, height: 8)
                .accessibilityLabel("当前节点")
        }
    }
}

/// Rules: the proxy mode (rules, global or direct), the active subscription's policy groups (including the other
/// subscription groups as "订阅·名称" groups), its rules and rule sets, and the DNS settings.
struct MacRulesPage: View {
    @ObservedObject var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        if let library = tunnel.library, let routing = library.routing {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    modeCard(hasRules: true)
                    MacCard(title: "策略组") {
                        ForEach(routing.groups) { group in
                            groupRow(group, library: library)
                            if group.id != routing.groups.last?.id {
                                Divider()
                            }
                        }
                        Text("自动选择和故障转移的组由内核按延迟和可用性挑选节点。")
                            .font(.caption)
                            .foregroundStyle(brand.text2)
                    }
                    .disabled(tunnel.mode != .rule)
                    MacCard(title: "规则与 DNS") {
                        NavigationLink {
                            RuleListView(rules: routing.rules)
                        } label: {
                            LabeledContent("全部规则", value: "\(routing.rules.count)")
                        }
                        Divider()
                        NavigationLink {
                            MacRuleSetsPage(tunnel: tunnel, rules: routing.rules, defined: routing.ruleSets)
                        } label: {
                            LabeledContent("规则集", value: "\(RuleSetCatalog.referencedNames(in: routing.rules).count)")
                        }
                        Divider()
                        dnsLink
                    }
                }
                .padding(24)
                .frame(maxWidth: 860)
                .frame(maxWidth: .infinity)
            }
        } else if tunnel.nodeName != nil {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    modeCard(hasRules: false)
                    MacCard(title: "DNS") {
                        dnsLink
                    }
                    Text("导入带规则和策略组的订阅（Clash、Surge、Loon 配置）后，可以按订阅规则分流和切换策略组。")
                        .font(.caption)
                        .foregroundStyle(brand.text2)
                        .padding(.horizontal, 4)
                }
                .padding(24)
                .frame(maxWidth: 860)
                .frame(maxWidth: .infinity)
            }
        } else {
            MacPlaceholderPage(
                systemImage: "arrow.triangle.branch",
                title: "规则与分流",
                message: "导入带规则和策略组的订阅（Clash、Surge、Loon 配置）后，可以在这里按订阅规则分流和切换策略组。现在全部流量走所选节点。"
            )
        }
    }

    private func modeCard(hasRules: Bool) -> some View {
        MacCard(title: "代理模式") {
            Picker("代理模式", selection: Binding(
                get: { tunnel.mode },
                set: { mode in
                    Task {
                        await tunnel.setMode(mode)
                    }
                }
            )) {
                if hasRules {
                    Text("规则").tag(ProxyMode.rule)
                }
                Text("全局").tag(ProxyMode.global)
                Text("直连").tag(ProxyMode.direct)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            Text(ProxyModeText.footer(tunnel.mode))
                .font(.caption)
                .foregroundStyle(brand.text2)
        }
    }

    private var dnsLink: some View {
        NavigationLink {
            DnsSettingsView(tunnel: tunnel)
        } label: {
            LabeledContent("DNS", value: tunnel.dns.isEmpty ? String(localized: "默认") : (tunnel.dns.effectiveDefault ?? String(localized: "Hosts")))
        }
    }

    @ViewBuilder
    private func groupRow(_ group: NativeImportReport.Routing.Group, library: NodeLibrary) -> some View {
        if group.isSelectable {
            Picker(selection: Binding(
                get: { library.selection(in: group) ?? "" },
                set: { member in
                    Task {
                        await tunnel.selectMember(member, inGroup: group.id)
                    }
                }
            )) {
                ForEach(group.members, id: \.self) { member in
                    Text(verbatim: name(of: member, in: library))
                        .tag(member)
                }
            } label: {
                Text(verbatim: group.id)
            }
            .pickerStyle(.menu)
        } else {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: group.id)
                        .foregroundStyle(brand.text)
                    Text(group.typeTitle)
                        .font(.caption)
                        .foregroundStyle(brand.text2)
                }
                Spacer()
                Text("\(group.members.count) 个成员")
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            }
        }
    }

    private func name(of member: String, in library: NodeLibrary) -> String {
        switch member {
        case "DIRECT":
            return String(localized: "直连")
        case "REJECT":
            return String(localized: "拒绝")
        default:
            return library.displayName(ofMember: member)
        }
    }
}

/// The rule sets the active subscription's rules use, with what the engine loaded.
private struct MacRuleSetsPage: View {
    @ObservedObject var tunnel: TunnelController
    let rules: [String]
    let defined: [String: EmbeddedRuntimeConfig.RuleSet]?
    @Environment(\.brand) private var brand

    var body: some View {
        List {
            RuleSetsSection(tunnel: tunnel, rules: rules, defined: defined)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg)
        .navigationTitle("规则集")
    }
}

/// The node an edit sheet is open for.
private struct MacEditingNode: Identifiable {
    let groupID: String
    let node: NativeImportReport.Node

    var id: String {
        "\(groupID)\u{1F}\(node.id)"
    }
}
