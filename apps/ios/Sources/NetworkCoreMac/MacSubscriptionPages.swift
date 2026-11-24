import AppKit
import NetworkCoreBridge
import SwiftUI

/// Proxies: every subscription group with its nodes, traffic, expiry and update settings, or the single saved
/// node; delay tests, switching across groups, manual and automatic updates.
struct MacProxiesPage: View {
    var tunnel: TunnelController
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
    @State private var checking: MacEditingNode?
    @State private var notice: String?
    /// A node deletion that takes the nodes connecting through it along, until the user confirms.
    @State private var deletingNode: PendingNodeDeletion?

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
                        Text("导入订阅链接后，每个订阅成为一个订阅组，支持测延迟、切换节点和更新。")
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
                    title: "暂无节点",
                    message: "粘贴订阅链接，或 ss://、vless://、trojan://、vmess://、hysteria2://、tuic:// 等链接和订阅内容，或导入 WireGuard 配置。"
                ) {
                    Button("导入节点") {
                        state.importing = true
                    }
                    .buttonStyle(.brandPrimary)
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
        .confirmationDialog(
            "删除节点“\(deletingNode?.node.displayName ?? "")”？",
            isPresented: Binding(
                get: { deletingNode != nil },
                set: { if !$0 { deletingNode = nil } }
            )
        ) {
            Button("一并删除 \((deletingNode?.dependents.count ?? 0) + 1) 个节点", role: .destructive) {
                if let pending = deletingNode {
                    Task {
                        await tunnel.deleteNode(pending.node.id, inGroup: pending.groupID)
                    }
                }
                deletingNode = nil
            }
        } message: {
            Text("“\(deletingNode?.dependentNames ?? "")”经由这个节点连接，删除后无法使用，会一并删除。")
        }
        .sheet(item: $editing) { item in
            NodeEditSheet(node: item.node) { edited in
                await tunnel.updateNode(edited, inGroup: item.groupID)
            }
            .formSheetFrame()
            .brandSheet()
            .environment(\.brand, brand)
            .tint(brand.primary)
        }
        .sheet(item: $checking) { item in
            NodeCheckupSheet(node: item.node) { throughput in
                await tunnel.checkup(item.node.id, inGroup: item.groupID, throughput: throughput)
            }
            .formSheetFrame()
            .brandSheet()
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
                        Text("按订阅规则分流，节点由策略组决定。可在“规则”页切换策略组或关闭规则。")
                    }
                    Text("延迟为经该节点请求 `http://www.gstatic.com/generate_204` 的耗时，5 秒无响应显示超时。")
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
                                .background(brand.soft, in: Capsule())
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
                    nodeHeader
                    ForEach(shownNodes(group)) { node in
                        Button {
                            guard !routed else {
                                return
                            }
                            Task {
                                await tunnel.selectNode(node.id, inGroup: group.id)
                            }
                        } label: {
                            MacNodeRow(
                                node: node,
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
        .background(brand.surface, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous)
                .strokeBorder(isActive ? brand.primary : brand.border)
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
        Button("节点体检…") {
            checking = MacEditingNode(groupID: group.id, node: node)
        }
        Button("复制分享链接") {
            do {
                let link = try NativeNodeImporterFFI.shareLink(for: node)
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(link, forType: .string)
                notice = String(localized: "已复制分享链接。链接含节点密码，请仅分享给可信任的人。")
            } catch let failure as NodeShareFailure {
                notice = failure.message
            } catch {
                notice = NodeShareFailure.unavailable.message
            }
        }
        if let dependents = group.library.removal(of: node.id) {
            Divider()
            Button(dependents.isEmpty ? LocalizedStringKey("删除节点") : LocalizedStringKey("删除节点…"), role: .destructive) {
                if dependents.isEmpty {
                    Task {
                        await tunnel.deleteNode(node.id, inGroup: group.id)
                    }
                } else {
                    deletingNode = PendingNodeDeletion(groupID: group.id, node: node, dependents: dependents)
                }
            }
        }
    }

    /// The node table's header, which is also its sort control: a click sorts by the column, a second click goes
    /// back to the subscription's own order (AnixOps Design `guidelines/density.md` N16).
    private var nodeHeader: some View {
        HStack(spacing: MacNodeRow.spacing) {
            Color.clear
                .frame(width: MacNodeRow.dotWidth, height: 1)
            sortHeader("名称", .name)
                .frame(maxWidth: .infinity, alignment: .leading)
            Text("类型")
                .frame(width: MacNodeRow.typeWidth, alignment: .leading)
            sortHeader("延迟 ms", .latency)
                .frame(width: MacNodeRow.latencyWidth, alignment: .trailing)
        }
        .font(.callout)
        .foregroundStyle(brand.text2)
        .padding(.horizontal, MacNodeRow.inset)
        .frame(height: 24)
    }

    private func sortHeader(_ title: LocalizedStringKey, _ order: NodeSort) -> some View {
        Button {
            sort = sort == order ? .original : order
        } label: {
            HStack(spacing: 3) {
                Text(title)
                if sort == order {
                    Image(systemName: "chevron.up")
                        .font(.caption2.weight(.semibold))
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(sort == order ? Text("恢复订阅顺序") : Text("按这一列排序"))
        .accessibilityAddTraits(sort == order ? .isSelected : [])
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
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    /// Whether the running tunnel took the rules (`RoutingState`); nil while disconnected.
    @State private var routingState: RoutingState?

    var body: some View {
        content
            .task(id: tunnel.connection) {
                while !Task.isCancelled, tunnel.connection == .connected {
                    routingState = await tunnel.routingState()
                    try? await Task.sleep(for: .seconds(5))
                }
                routingState = nil
            }
    }

    @ViewBuilder
    private var content: some View {
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
                            MacRuleSetsPage(tunnel: tunnel, rules: routing.rules, defined: routing.ruleSets,
                                            warnings: library.routingWarnings)
                        } label: {
                            LabeledContent("规则集", value: "\(RuleSetCatalog.referencedNames(in: routing.rules).count)")
                        }
                        Divider()
                        ruleToolsLink
                        Divider()
                        dnsLink
                        if RuleSetCatalog.hasProcessRules(routing.rules) {
                            Text("订阅含 PROCESS-NAME 规则，iOS 和 macOS 不支持，规则不生效。")
                                .font(.caption)
                                .foregroundStyle(brand.warning)
                        }
                        if RuleSetCatalog.hasRules(of: "CELLULAR-RADIO", in: routing.rules) {
                            Text("订阅含 CELLULAR-RADIO 规则，Mac 不支持，规则不生效。")
                                .font(.caption)
                                .foregroundStyle(brand.warning)
                        }
                    }
                    myRulesCard(hasSubscriptionRules: true)
                }
                .padding(24)
                .frame(maxWidth: 860)
                .frame(maxWidth: .infinity)
            }
        } else if tunnel.nodeName != nil {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    modeCard(hasRules: false)
                    MacCard(title: "规则与 DNS") {
                        ruleToolsLink
                        Divider()
                        dnsLink
                    }
                    myRulesCard(hasSubscriptionRules: false)
                    Text("导入带规则和策略组的订阅后，可以按订阅规则分流和切换策略组。")
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
                message: "导入带规则和策略组的订阅后，可在此按订阅规则分流并切换策略组。当前全部流量走所选节点。"
            )
        }
    }

    /// The user's own rules as text, matched before the subscription's (or, for a plain node list, before everything else goes through the selected node).
    private func myRulesCard(hasSubscriptionRules: Bool) -> some View {
        MacCard(title: "我的规则") {
            NavigationLink {
                RuleTextEditorView(tunnel: tunnel)
                    .frame(minWidth: 520, minHeight: 420)
            } label: {
                LabeledContent("编辑规则文本", value: "\(tunnel.localRules.rules.count)")
            }
            Text(hasSubscriptionRules
                ? "排在订阅规则前面，从上到下匹配。连接中保存会直接换上新规则，已有连接不断开。"
                : "规则模式下排在预设前面，从上到下匹配。连接中保存会直接换上新规则，已有连接不断开。")
                .font(.caption)
                .foregroundStyle(brand.text2)
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
            // Why the running tunnel does not route by every rule (`RoutingState.notice`).
            if tunnel.connection == .connected, let text = routingState?.notice {
                Text(text)
                    .font(.caption)
                    .foregroundStyle(brand.warning)
            }
        }
    }

    /// The rule test, hit counts and the GEOIP / IP-ASN data lists.
    private var ruleToolsLink: some View {
        NavigationLink {
            MacRuleToolsPage(tunnel: tunnel)
        } label: {
            LabeledContent("规则测试、命中与 GeoIP 数据", value: "\(tunnel.localRules.dataSources.count)")
        }
    }

    private var dnsLink: some View {
        NavigationLink {
            DnsSettingsView(tunnel: tunnel)
        } label: {
            LabeledContent("DNS", value: tunnel.dns.isEmpty ? String(localized: "默认") : (tunnel.dns.effectiveDefault ?? (tunnel.dns.hosts.isEmpty ? "Fake-IP" : String(localized: "Hosts"))))
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
                Text("\(group.displayMembers.count) 个成员")
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
    var tunnel: TunnelController
    let rules: [String]
    let defined: [String: EmbeddedRuntimeConfig.RuleSet]?
    let warnings: [String]
    @Environment(\.brand) private var brand

    var body: some View {
        List {
            RuleSetsSection(tunnel: tunnel, rules: rules, defined: defined, warnings: warnings)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg)
        .navigationTitle("规则集")
    }
}

/// The rule test and hit counts while connected, then the GEOIP / IP-ASN data lists and the device names.
private struct MacRuleToolsPage: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        List {
            if tunnel.connection == .connected {
                RuleTestSection(tunnel: tunnel)
            }
            RuleHitsSection(tunnel: tunnel)
            DataListsSection(tunnel: tunnel)
            DeviceNamesSection(tunnel: tunnel)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg)
        .navigationTitle("规则工具")
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

/// One node as a row of the compact node table (AnixOps Design `guidelines/density.md`): the name takes the spare
/// width and truncates (N9, N12), the type and the delay keep fixed columns so numbers never move sideways, the
/// delay is right-aligned in tabular digits with its unit in the header (N10), a failed test is a word and an
/// untested node a dash (N13, N14).
private struct MacNodeRow: View {
    static let spacing: CGFloat = 8
    static let inset: CGFloat = 8
    static let dotWidth: CGFloat = 6
    static let typeWidth: CGFloat = 120
    static let latencyWidth: CGFloat = 72

    let node: NativeImportReport.Node
    let latency: NodeLatency?
    let selected: Bool
    @Environment(\.brand) private var brand
    @State private var hovering = false

    private var name: String {
        node.name.isEmpty ? ImportPreview.mask(node.server) : node.name
    }

    var body: some View {
        HStack(spacing: Self.spacing) {
            Circle()
                .fill(selected ? Brand.amber : Color.clear)
                .frame(width: Self.dotWidth, height: Self.dotWidth)
                .accessibilityLabel("当前节点")
                .accessibilityHidden(!selected)
            Text(verbatim: name)
                .font(.callout.weight(selected ? .semibold : .regular))
                .foregroundStyle(brand.text)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
            Text(ProtocolBadge.spokenName(for: node.protocol))
                .font(.callout)
                .foregroundStyle(brand.text2)
                .lineLimit(1)
                .frame(width: Self.typeWidth, alignment: .leading)
            delay
                .frame(width: Self.latencyWidth, alignment: .trailing)
        }
        .padding(.horizontal, Self.inset)
        .frame(height: 28)
        .background(
            selected ? brand.soft : (hovering ? brand.raised : Color.clear),
            in: RoundedRectangle(cornerRadius: 5)
        )
        .contentShape(Rectangle())
        .onHover { inside in
            hovering = inside
        }
        .help(Text(verbatim: name))
    }

    /// Green, amber or orange by the delay's grade (`LatencyGrade`: under 120 ms, under 250 ms, slower).
    private func gradeColor(_ milliseconds: Int) -> Color {
        switch LatencyGrade(milliseconds: UInt32(clamping: milliseconds)) {
        case .good:
            return brand.success
        case .fair:
            return brand.amberText
        case .poor:
            return brand.warning
        case .unknown:
            return brand.border
        }
    }

    @ViewBuilder
    private var delay: some View {
        switch latency {
        case .none:
            Text(verbatim: "—")
                .font(.callout)
                .foregroundStyle(brand.text2)
                .accessibilityLabel("未测试")
        case .measuring:
            ProgressView()
                .controlSize(.mini)
                .accessibilityLabel("正在测速")
        case let .millis(value):
            HStack(spacing: 5) {
                Circle()
                    .fill(gradeColor(value))
                    .frame(width: 6, height: 6)
                    .accessibilityHidden(true)
                Text(verbatim: "\(value)")
                    .font(.callout.monospacedDigit())
                    .foregroundStyle(brand.text)
            }
        case .timeout:
            Text("超时")
                .font(.callout)
                .foregroundStyle(brand.text2)
        case .failed:
            Text("失败")
                .font(.callout)
                .foregroundStyle(brand.text2)
        case .unsupported:
            Text("不支持")
                .font(.callout)
                .foregroundStyle(brand.text2)
        }
    }
}
