import NetworkCoreBridge
#if canImport(UIKit)
import UIKit
#endif
import SwiftUI

/// The rules page's link to the local rules: what unmatched traffic does, and how many rule sets and rules of the
/// user's there are.
struct LocalRulesSection: View {
    var tunnel: TunnelController
    /// Whether the active subscription brings its own rules; the preset then does not apply.
    var hasSubscriptionRules: Bool
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            NavigationLink {
                LocalRulesView(tunnel: tunnel, hasSubscriptionRules: hasSubscriptionRules)
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text(hasSubscriptionRules ? "本地规则" : "分流规则")
                        .foregroundStyle(brand.text)
                    Text(summary)
                        .font(.footnote)
                        .foregroundStyle(brand.text2)
                }
                .padding(.vertical, 2)
            }
            .listRowBackground(brand.surface)
        } footer: {
            if !hasSubscriptionRules {
                Text("订阅没有自带规则时，规则模式按这里的预设、规则集和你的规则分流。")
            }
        }
    }

    private var summary: String {
        let rules = tunnel.localRules
        var parts: [String] = []
        if !hasSubscriptionRules {
            parts.append(LocalRulesText.title(rules.preset))
        }
        let sets = rules.subscriptions.filter(\.enabled).count
        let own = rules.rules.filter(\.enabled).count
        parts.append(String(localized: "\(sets) 个规则集"))
        parts.append(String(localized: "\(own) 条规则"))
        return parts.joined(separator: " · ")
    }
}

enum LocalRulesText {
    static func title(_ preset: LocalRules.Preset) -> String {
        switch preset {
        case .bypassChina:
            return String(localized: "绕过大陆")
        case .gfwList:
            return String(localized: "GFW 列表")
        case .returnChina:
            return String(localized: "回国")
        case .proxyAll:
            return String(localized: "全部代理")
        }
    }

    static func detail(_ preset: LocalRules.Preset) -> String {
        switch preset {
        case .bypassChina:
            return String(localized: "大陆域名和 IP 直连，其余走代理")
        case .gfwList:
            return String(localized: "只有 GFW 列表里的域名走代理，其余直连")
        case .returnChina:
            return String(localized: "大陆域名和 IP 走代理，其余直连，适合在海外访问国内服务")
        case .proxyAll:
            return String(localized: "不用列表，未匹配的流量都走代理")
        }
    }

    static func policy(_ policy: LocalRules.Policy) -> String {
        switch policy {
        case .proxy:
            return String(localized: "代理")
        case .direct:
            return String(localized: "直连")
        case .reject:
            return String(localized: "拒绝")
        }
    }
}

/// Where a rule or rule set sends traffic, as a small label: always a word, coloured only to tell the three apart.
struct PolicyChip: View {
    let policy: LocalRules.Policy
    @Environment(\.brand) private var brand

    var body: some View {
        Text(LocalRulesText.policy(policy))
            .font(.caption.weight(.semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(color.opacity(0.12), in: Capsule())
    }

    private var color: Color {
        switch policy {
        case .proxy:
            return brand.primary
        case .direct:
            return brand.text2
        case .reject:
            return brand.danger
        }
    }
}

/// The local rules, in the order they match: the user's rules, the rule sets (open-source lists from GitHub or any
/// list URL, downloaded and refreshed by the engine), then the preset for whatever is left. Saving reconnects a
/// running tunnel.
struct LocalRulesView: View {
    var tunnel: TunnelController
    var hasSubscriptionRules: Bool
    @Environment(\.brand) private var brand
    @State private var editing: LocalRuleEdit?
    @State private var editingSet: RuleSetEdit?
    @State private var importing = false
    @State private var confirmingClear = false
    @State private var notice: String?
    @State private var status: [String: RuntimeControl.RuleSets.Status] = [:]
    @State private var updating = false
    @State private var showingLibrary = false

    private var rules: LocalRules {
        tunnel.localRules
    }

    var body: some View {
        List {
            rulesSection
            ruleSetsSection
            if !hasSubscriptionRules {
                presetSection
            }
            Section {
                Button("从 Loon / Surge 配置导入规则") {
                    importing = true
                }
                if !rules.rules.isEmpty {
                    Button("清空我的规则", role: .destructive) {
                        confirmingClear = true
                    }
                }
            } footer: {
                Text(hasSubscriptionRules
                    ? "从上到下匹配，先匹配先生效，全部排在订阅规则前面。“代理”指订阅的第一个策略组。"
                    : "从上到下匹配，先匹配先生效：你的规则、规则集，最后是预设。“代理”指当前选中的节点。")
            }
            .listRowBackground(brand.surface)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle(hasSubscriptionRules ? "本地规则" : "分流规则")
        .inlineNavigationTitle()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Button {
                        editing = LocalRuleEdit(original: nil)
                    } label: {
                        Label("添加规则", systemImage: "text.badge.plus")
                    }
                    .disabled(rules.rules.count >= LocalRules.maxRules)
                    Button {
                        showingLibrary = true
                    } label: {
                        Label("添加规则集", systemImage: "square.stack.3d.down.right")
                    }
                    .disabled(rules.subscriptions.count >= LocalRules.maxSubscriptions)
                    Button {
                        importing = true
                    } label: {
                        Label("从配置导入", systemImage: "doc.on.clipboard")
                    }
                } label: {
                    Image(systemName: "plus")
                        .accessibilityLabel("添加")
                }
            }
            if !rules.rules.isEmpty || !rules.subscriptions.isEmpty {
                ToolbarItem(placement: .topBarLeading) {
                    EditButton()
                }
            }
        }
        .navigationDestination(isPresented: $showingLibrary) {
            RuleSetLibraryView(tunnel: tunnel)
        }
        .sheet(item: $editing) { edit in
            LocalRuleSheet(edit: edit) { rule in
                try rules.saving(rule)
            } commit: { next in
                commit(next)
            }
            .formSheetFrame()
        }
        .sheet(item: $editingSet) { edit in
            RuleSetSubscriptionSheet(edit: edit, rules: rules) { next in
                commit(next)
            }
            .formSheetFrame()
        }
        .sheet(isPresented: $importing) {
            LocalRulesImportSheet { text in
                let parsed = LocalRules.parse(text)
                let merged = rules.merging(parsed.rules)
                commit(merged.rules)
                notice = String(localized: "添加了 \(merged.added) 条规则，跳过 \(parsed.skipped + parsed.rules.count - merged.added) 条（不支持、重复或超出上限）。")
            }
            .formSheetFrame()
        }
        .confirmationDialog("清空你添加的全部规则？", isPresented: $confirmingClear, titleVisibility: .visible) {
            Button("清空", role: .destructive) {
                var next = rules
                next.rules = []
                commit(next)
            }
        }
        .alert("本地规则", isPresented: Binding(get: { notice != nil }, set: { if !$0 { notice = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            Text(notice ?? "")
        }
        .task(id: tunnel.connection) {
            await loadStatus()
        }
    }

    // MARK: - My rules

    private var rulesSection: some View {
        Section {
            if rules.rules.isEmpty {
                Button {
                    editing = LocalRuleEdit(original: nil)
                } label: {
                    Label("添加第一条规则", systemImage: "plus.circle")
                        .foregroundStyle(brand.primary)
                }
            }
            ForEach(rules.rules) { rule in
                Button {
                    editing = LocalRuleEdit(original: rule)
                } label: {
                    ruleRow(rule)
                }
                .buttonStyle(.plain)
                .swipeActions(edge: .leading) {
                    Button(rule.enabled ? "停用" : "启用") {
                        toggleRule(rule.id)
                    }
                    .tint(rule.enabled ? .gray : brand.primary)
                }
            }
            .onDelete { offsets in
                var next = rules
                next.rules.remove(atOffsets: offsets)
                commit(next)
            }
            .onMove { offsets, destination in
                var next = rules
                next.rules.move(fromOffsets: offsets, toOffset: destination)
                commit(next)
            }
        } header: {
            Text("我的规则")
        } footer: {
            if rules.rules.isEmpty {
                Text("按域名、IP、端口等写规则，写法和 Surge、Loon 一样。")
            }
        }
        .listRowBackground(brand.surface)
    }

    private func ruleRow(_ rule: LocalRules.Rule) -> some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: rule.value)
                    .font(.callout.monospaced())
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(verbatim: rule.noResolve ? "\(rule.kind) · no-resolve" : rule.kind)
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            }
            Spacer(minLength: 8)
            PolicyChip(policy: rule.policy)
        }
        .opacity(rule.enabled ? 1 : 0.45)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityValue(rule.enabled ? "" : String(localized: "已停用"))
    }

    private func toggleRule(_ id: UUID) {
        var next = rules
        if let index = next.rules.firstIndex(where: { $0.id == id }) {
            next.rules[index].enabled.toggle()
            commit(next)
        }
    }

    // MARK: - Rule sets

    private var ruleSetsSection: some View {
        Section {
            ForEach(rules.subscriptions) { subscription in
                Button {
                    editingSet = RuleSetEdit(original: subscription)
                } label: {
                    ruleSetRow(subscription)
                }
                .buttonStyle(.plain)
                .swipeActions(edge: .leading) {
                    Button(subscription.enabled ? "停用" : "启用") {
                        var next = rules
                        if let index = next.subscriptions.firstIndex(where: { $0.id == subscription.id }) {
                            next.subscriptions[index].enabled.toggle()
                            commit(next)
                        }
                    }
                    .tint(subscription.enabled ? .gray : brand.primary)
                }
            }
            .onDelete { offsets in
                var next = rules
                next.subscriptions.remove(atOffsets: offsets)
                commit(next)
            }
            .onMove { offsets, destination in
                var next = rules
                next.subscriptions.move(fromOffsets: offsets, toOffset: destination)
                commit(next)
            }
            NavigationLink {
                RuleSetLibraryView(tunnel: tunnel)
            } label: {
                Label("从开源规则库添加", systemImage: "square.stack.3d.down.right")
                    .foregroundStyle(brand.primary)
            }
            .disabled(rules.subscriptions.count >= LocalRules.maxSubscriptions)
            if !rules.subscriptions.isEmpty, tunnel.connection == .connected {
                Button {
                    updating = true
                    Task {
                        _ = await tunnel.control(.updateRuleSets)
                        // Downloads run in the engine; read the result back after a moment.
                        try? await Task.sleep(for: .seconds(3))
                        await loadStatus()
                        updating = false
                    }
                } label: {
                    if updating {
                        ProgressView()
                    } else {
                        Label("立即更新规则集", systemImage: "arrow.clockwise")
                    }
                }
                .disabled(updating)
            }
        } header: {
            Text("规则集")
        } footer: {
            Text("规则集由内核经代理下载，每天自动更新。常用的开源规则来自 blackmatrix7/ios_rule_script 和 Loyalsoldier/surge-rules，也可以填任意 Surge、Loon、Clash 规则列表链接。")
        }
        .listRowBackground(brand.surface)
    }

    private func ruleSetRow(_ subscription: LocalRules.Subscription) -> some View {
        let loaded = status[subscription.engineName]
        return HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: subscription.name)
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                Group {
                    if !subscription.enabled {
                        Text("已停用")
                            .foregroundStyle(brand.text3)
                    } else if let error = loaded?.lastError {
                        Label("下载失败：\(error)", systemImage: "exclamationmark.triangle")
                            .foregroundStyle(brand.danger)
                    } else if let loaded, let updated = loaded.updatedAt {
                        Text("\(loaded.entries) 条 · 更新于 \(updated, style: .relative)前")
                            .foregroundStyle(brand.text2)
                    } else {
                        Text(verbatim: SubscriptionLink.displayHost(subscription.url))
                            .foregroundStyle(brand.text3)
                    }
                }
                .font(.caption)
                .lineLimit(1)
            }
            Spacer(minLength: 8)
            PolicyChip(policy: subscription.policy)
        }
        .opacity(subscription.enabled ? 1 : 0.45)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }

    private func loadStatus() async {
        guard tunnel.connection == .connected,
              let answer = RuntimeControl.decode(RuntimeControl.RuleSets.self, from: await tunnel.control(.ruleSets))
        else {
            status = [:]
            return
        }
        status = Dictionary(answer.ruleSets.map { ($0.name, $0) }) { first, _ in first }
    }

    // MARK: - Preset

    private var presetSection: some View {
        Section {
            ForEach(LocalRules.Preset.allCases) { preset in
                Button {
                    guard preset != rules.preset else {
                        return
                    }
                    var next = rules
                    next.preset = preset
                    commit(next)
                } label: {
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(LocalRulesText.title(preset))
                                .foregroundStyle(brand.text)
                            Text(LocalRulesText.detail(preset))
                                .font(.caption)
                                .foregroundStyle(brand.text2)
                        }
                        Spacer(minLength: 8)
                        if preset == rules.preset {
                            Image(systemName: "checkmark")
                                .font(.body.weight(.semibold))
                                .foregroundStyle(brand.primary)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(preset == rules.preset ? .isSelected : [])
            }
            Toggle("局域网和本机地址直连", isOn: Binding(
                get: { rules.bypassLAN },
                set: { on in
                    var next = rules
                    next.bypassLAN = on
                    commit(next)
                }
            ))
            .tint(brand.primary)
        } header: {
            Text("其余流量")
        } footer: {
            Text("没有被上面规则和规则集匹配的流量按预设处理。大陆和 GFW 列表每天自动更新。只在规则模式下生效。")
        }
        .listRowBackground(brand.surface)
    }

    private func commit(_ next: LocalRules) {
        tunnel.updateLocalRules(next)
    }
}

/// The rule set a sheet edits; nil `original` adds one by URL.
struct RuleSetEdit: Identifiable {
    let id = UUID()
    let original: LocalRules.Subscription?
}

/// Adds a rule set by URL, or changes a rule set's name, link and policy.
struct RuleSetSubscriptionSheet: View {
    let edit: RuleSetEdit
    let rules: LocalRules
    let commit: (LocalRules) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var name = ""
    @State private var url = ""
    @State private var policy = LocalRules.Policy.proxy
    @State private var failure: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("名称", text: $name, prompt: Text("例如 广告拦截"))
                        .plainTextEntry()
                    TextField("链接", text: $url, prompt: Text(verbatim: "https://raw.githubusercontent.com/…/rules.list"))
                        .plainTextEntry(url: true)
                } footer: {
                    Text("支持 Surge、Loon 的 .list 和 Clash 的 classical / domain 列表（https）。")
                }
                .listRowBackground(brand.surface)
                Section {
                    Picker("策略", selection: $policy) {
                        ForEach(LocalRules.Policy.allCases) { policy in
                            Text(LocalRulesText.policy(policy)).tag(policy)
                        }
                    }
                    .pickerStyle(.segmented)
                } header: {
                    Text("匹配后")
                }
                .listRowBackground(brand.surface)
                if let failure {
                    Text(failure)
                        .foregroundStyle(brand.danger)
                        .listRowBackground(brand.surface)
                }
                if let original = edit.original {
                    Section {
                        Button("删除规则集", role: .destructive) {
                            var next = rules
                            next.subscriptions.removeAll { $0.id == original.id }
                            commit(next)
                            dismiss()
                        }
                    }
                    .listRowBackground(brand.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle(edit.original == nil ? "添加规则集" : "编辑规则集")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        save()
                    }
                    .disabled(url.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onAppear {
                if let original = edit.original {
                    name = original.name
                    url = original.url
                    policy = original.policy
                }
            }
        }
    }

    private func save() {
        var subscription = edit.original ?? LocalRules.Subscription(name: "", url: "", policy: policy)
        subscription.name = name
        subscription.url = url.trimmingCharacters(in: .whitespaces)
        subscription.policy = policy
        if subscription.url != edit.original?.url {
            subscription.catalogID = nil
        }
        do {
            commit(try rules.saving(subscription))
            dismiss()
        } catch LocalRules.Failure.tooMany {
            failure = String(localized: "最多 \(LocalRules.maxSubscriptions) 个规则集")
        } catch {
            failure = String(localized: "链接必须是 https 地址")
        }
    }
}

struct LocalRuleEdit: Identifiable {
    let id = UUID()
    let original: LocalRules.Rule?
}

private struct LocalRuleSheet: View {
    let edit: LocalRuleEdit
    let save: (LocalRules.Rule) throws -> LocalRules
    let commit: (LocalRules) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var kind = "DOMAIN-SUFFIX"
    @State private var value = ""
    @State private var policy = LocalRules.Policy.proxy
    @State private var noResolve = false
    @State private var failure: String?

    private var isIP: Bool {
        ["IP-CIDR", "IP-CIDR6", "GEOIP"].contains(kind)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("类型", selection: $kind) {
                        ForEach(LocalRules.kinds, id: \.self) { kind in
                            Text(verbatim: kind).tag(kind)
                        }
                    }
                    TextField("值", text: $value, prompt: Text(verbatim: example))
                        .plainTextEntry(url: kind == "RULE-SET")
                    if isIP {
                        Toggle("不解析域名（no-resolve）", isOn: $noResolve)
                    }
                } footer: {
                    Text(hint)
                }
                .listRowBackground(brand.surface)
                Section {
                    Picker("策略", selection: $policy) {
                        ForEach(LocalRules.Policy.allCases) { policy in
                            Text(LocalRulesText.policy(policy)).tag(policy)
                        }
                    }
                    .pickerStyle(.segmented)
                }
                .listRowBackground(brand.surface)
                if let failure {
                    Text(failure)
                        .foregroundStyle(brand.danger)
                        .listRowBackground(brand.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle(edit.original == nil ? "添加规则" : "编辑规则")
            .inlineNavigationTitle()
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
                    .disabled(value.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onAppear {
                if let original = edit.original {
                    kind = original.kind
                    value = original.value
                    policy = original.policy
                    noResolve = original.noResolve
                }
            }
        }
    }

    private var example: String {
        switch kind {
        case "DOMAIN":
            return "api.example.com"
        case "DOMAIN-KEYWORD":
            return "google"
        case "DOMAIN-WILDCARD":
            return "*.example.com"
        case "IP-CIDR":
            return "203.0.113.0/24"
        case "IP-CIDR6":
            return "2001:db8::/32"
        case "GEOIP":
            return "CN"
        case "GEOSITE":
            return "cn"
        case "DST-PORT":
            return "443"
        case "PROTOCOL":
            return "UDP"
        case "USER-AGENT":
            return "Instagram*"
        case "RULE-SET":
            return "https://example.com/rules.list"
        default:
            return "example.com"
        }
    }

    private var hint: String {
        switch kind {
        case "DOMAIN-SUFFIX":
            return String(localized: "匹配这个域名和它的所有子域名。")
        case "GEOSITE":
            return String(localized: "可用 cn 或 gfw 两个列表。")
        case "GEOIP":
            return String(localized: "两位国家或地区代码，例如 CN。")
        case "DST-PORT":
            return String(localized: "一个端口或 起-止 范围，例如 6000-6100。")
        case "PROTOCOL":
            return String(localized: "TCP、UDP、QUIC、HTTP 或 HTTPS。")
        case "RULE-SET":
            return String(localized: "Surge / Loon 格式的规则列表 https 地址，经代理每天下载一次。")
        default:
            return String(localized: "与 Surge、Loon 的同名规则写法相同。")
        }
    }

    private func submit() {
        var line = "\(kind),\(value.trimmingCharacters(in: .whitespaces)),\(policy.rawValue)"
        if isIP, noResolve {
            line += ",no-resolve"
        }
        do {
            var rule = try LocalRules.rule(from: line)
            if let original = edit.original {
                rule.id = original.id
                rule.enabled = original.enabled
            }
            commit(try save(rule))
            dismiss()
        } catch LocalRules.Failure.tooMany {
            failure = String(localized: "最多 \(LocalRules.maxRules) 条规则")
        } catch {
            failure = String(localized: "这条规则的值不对，内核不会接受。")
        }
    }
}

private struct LocalRulesImportSheet: View {
    let add: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var text = ""

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextEditor(text: $text)
                        .font(.footnote.monospaced())
                        .plainTextEntry()
                        .frame(minHeight: 200)
                    #if canImport(UIKit)
                    Button("粘贴剪贴板") {
                        text = UIPasteboard.general.string ?? text
                    }
                    #endif
                } header: {
                    Text("规则")
                } footer: {
                    Text(verbatim: "DOMAIN-SUFFIX,example.com,Proxy\nIP-CIDR,10.0.0.0/8,DIRECT,no-resolve\nRULE-SET,https://example.com/ads.list,REJECT")
                        .font(.caption.monospaced())
                }
                .listRowBackground(brand.surface)
                Section {
                    EmptyView()
                } footer: {
                    Text("可以粘贴整份 Loon / Surge 配置（只读 [Rule] 段）或每行一条规则。DIRECT、REJECT 保留，其他策略名都当作代理；FINAL 和不支持的类型会跳过。")
                }
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("导入规则")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("导入") {
                        add(text)
                        dismiss()
                    }
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
    }
}
