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
                groupSection(group)
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
        .refreshable {
            await tunnel.refreshAllSubscriptions()
        }
    }

    @ViewBuilder
    private func groupSection(_ group: SubscriptionGroup) -> some View {
        let isActive = tunnel.subscriptions.activeID == group.id
        let routed = isActive && group.library.routesByRules
        Section {
            SubscriptionGroupSummary(group: group, updating: tunnel.updatingGroups.contains(group.id))
                .listRowBackground(brand.surface)
            if !collapsed.contains(group.id) {
                ForEach(group.library.nodes) { node in
                    Button {
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
                    .disabled(routed)
                    .listRowBackground(brand.surface)
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

    private func groupMenu(_ group: SubscriptionGroup) -> some View {
        Menu {
            if group.isUpdatable {
                Button {
                    Task {
                        await tunnel.refreshSubscription(group.id)
                    }
                } label: {
                    Label("立即更新", systemImage: "arrow.clockwise")
                }
                .disabled(tunnel.updatingGroups.contains(group.id))
                Picker(selection: Binding(
                    get: { group.updateIntervalHours },
                    set: { tunnel.setUpdateInterval(group.id, hours: $0) }
                )) {
                    ForEach(UpdateInterval.choices(including: group.updateIntervalHours), id: \.self) { hours in
                        Text(UpdateInterval.label(hours)).tag(hours)
                    }
                } label: {
                    Label("自动更新", systemImage: "clock.arrow.circlepath")
                }
                .pickerStyle(.menu)
            }
            Button {
                newName = group.name
                renaming = group
            } label: {
                Label("重命名", systemImage: "pencil")
            }
            Button(role: .destructive) {
                deleting = group
            } label: {
                Label("删除", systemImage: "trash")
            }
        } label: {
            Image(systemName: "ellipsis.circle")
                .font(.body)
        }
        .accessibilityLabel("订阅组操作")
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
            latency(result)
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

    @ViewBuilder
    private func latency(_ result: NodeLatency?) -> some View {
        switch result {
        case .none:
            EmptyView()
        case .measuring:
            ProgressView()
                .controlSize(.small)
        case let .millis(value):
            Text("\(value) ms")
                .font(.footnote.monospacedDigit())
                .foregroundStyle(value < 300 ? brand.text2 : brand.danger)
        case .timeout:
            Text("超时")
                .font(.footnote)
                .foregroundStyle(brand.danger)
        case .failed:
            Text("失败")
                .font(.footnote)
                .foregroundStyle(brand.danger)
        case .unsupported:
            Text("不支持")
                .font(.footnote)
                .foregroundStyle(brand.text3)
        }
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

/// The first row of a subscription group: node count, last update, traffic used and expiry.
private struct SubscriptionGroupSummary: View {
    let group: SubscriptionGroup
    let updating: Bool
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Text("\(group.library.nodes.count) 个节点")
                Text(verbatim: "·")
                if updating {
                    ProgressView()
                        .controlSize(.mini)
                    Text("正在更新")
                } else {
                    Text("更新于 \(group.library.updatedAt, style: .relative)前")
                }
                Spacer(minLength: 0)
                if group.isUpdatable {
                    Text(UpdateInterval.label(group.updateIntervalHours))
                        .foregroundStyle(brand.text3)
                }
            }
            .font(.footnote)
            .foregroundStyle(brand.text2)

            if let info = group.userInfo {
                if let fraction = info.usedFraction {
                    ProgressView(value: fraction)
                        .tint(fraction > 0.9 ? brand.danger : brand.primary)
                }
                HStack(spacing: 6) {
                    if let used = info.used {
                        if let total = info.total, total > 0 {
                            Text("已用 \(Self.bytes(used)) / \(Self.bytes(total))")
                        } else {
                            Text("已用 \(Self.bytes(used))")
                        }
                    }
                    Spacer(minLength: 0)
                    if let expire = info.expire {
                        Text("到期 \(expire, format: .dateTime.year().month().day())")
                            .foregroundStyle(expire < Date() ? brand.danger : brand.text2)
                    }
                }
                .font(.caption)
                .foregroundStyle(brand.text2)
            }

            if let failed = group.lastFailureAt, !updating {
                Label("上次更新失败（\(failed, format: .dateTime.month().day().hour().minute())）", systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(brand.danger)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }

    static func bytes(_ value: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: value, countStyle: .binary)
    }
}

/// Automatic update intervals offered for a subscription group, in hours; 0 is by hand.
enum UpdateInterval {
    static let presets = [0, 1, 3, 6, 12, 24, 48, 72, 168]

    /// The presets, plus a server-chosen interval that is not one of them.
    static func choices(including current: Int) -> [Int] {
        presets.contains(current) ? presets : (presets + [current]).sorted()
    }

    static func label(_ hours: Int) -> String {
        switch hours {
        case 0:
            return String(localized: "手动更新")
        case 168:
            return String(localized: "每周自动更新")
        case let hours where hours % 24 == 0:
            return String(localized: "每 \(hours / 24) 天自动更新")
        default:
            return String(localized: "每 \(hours) 小时自动更新")
        }
    }
}
