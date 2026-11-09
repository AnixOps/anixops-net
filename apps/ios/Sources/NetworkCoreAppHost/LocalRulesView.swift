import NetworkCoreBridge
#if canImport(UIKit)
import UIKit
#endif
import SwiftUI

/// The rules page's link to the local rules, with the preset and how many rules of the user's there are.
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
                LabeledContent(hasSubscriptionRules ? "本地规则" : "分流规则", value: summary)
            }
            .listRowBackground(brand.surface)
        } footer: {
            if !hasSubscriptionRules {
                Text("订阅没有自带规则时，规则模式按这里的预设和你自己的规则分流。")
            }
        }
    }

    private var summary: String {
        let count = tunnel.localRules.rules.filter(\.enabled).count
        if hasSubscriptionRules {
            return count == 0 ? String(localized: "未添加") : String(localized: "\(count) 条")
        }
        let preset = LocalRulesText.title(tunnel.localRules.preset)
        return count == 0 ? preset : String(localized: "\(preset) · \(count) 条")
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
            return String(localized: "大陆域名和 IP 直连，其余走代理。")
        case .gfwList:
            return String(localized: "只有 GFW 列表里的域名走代理，其余直连。")
        case .returnChina:
            return String(localized: "大陆域名和 IP 走代理，其余直连，适合在海外访问国内服务。")
        case .proxyAll:
            return String(localized: "不用列表，只按你的规则分流，其余走代理。")
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

/// The local rules: the preset for traffic no rule matched, private networks direct, and the user's own rules in
/// Surge/Loon syntax, added one by one or pasted from a profile's `[Rule]` section. Saving reconnects a running
/// tunnel.
struct LocalRulesView: View {
    var tunnel: TunnelController
    var hasSubscriptionRules: Bool
    @Environment(\.brand) private var brand
    @State private var editing: LocalRuleEdit?
    @State private var importing = false
    @State private var confirmingClear = false
    @State private var notice: String?

    private var rules: LocalRules {
        tunnel.localRules
    }

    var body: some View {
        List {
            if !hasSubscriptionRules {
                presetSection
            }
            rulesSection
            Section {
                Button("添加规则") {
                    editing = LocalRuleEdit(original: nil)
                }
                .disabled(rules.rules.count >= LocalRules.maxRules)
                Button("从 Loon / Surge 配置导入") {
                    importing = true
                }
                if !rules.rules.isEmpty {
                    Button("清空我的规则", role: .destructive) {
                        confirmingClear = true
                    }
                }
            } footer: {
                Text(hasSubscriptionRules
                    ? "你的规则排在订阅规则前面，先匹配先生效；“代理”指订阅的第一个策略组。"
                    : "你的规则排在预设前面，先匹配先生效；“代理”指当前选中的节点。")
            }
            .listRowBackground(brand.surface)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle(hasSubscriptionRules ? "本地规则" : "分流规则")
        .inlineNavigationTitle()
        .toolbar {
            if !rules.rules.isEmpty {
                EditButton()
            }
        }
        .sheet(item: $editing) { edit in
            LocalRuleSheet(edit: edit) { rule in
                try rules.saving(rule)
            } commit: { next in
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
    }

    private var presetSection: some View {
        Section {
            Picker("未匹配的流量", selection: Binding(
                get: { rules.preset },
                set: { preset in
                    var next = rules
                    next.preset = preset
                    commit(next)
                }
            )) {
                ForEach(LocalRules.Preset.allCases) { preset in
                    Text(LocalRulesText.title(preset)).tag(preset)
                }
            }
            Toggle("局域网和本机地址直连", isOn: Binding(
                get: { rules.bypassLAN },
                set: { on in
                    var next = rules
                    next.bypassLAN = on
                    commit(next)
                }
            ))
        } header: {
            Text("预设")
        } footer: {
            Text(LocalRulesText.detail(rules.preset) + " " + String(localized: "列表每天自动更新。只在规则模式下生效。"))
        }
        .listRowBackground(brand.surface)
    }

    private var rulesSection: some View {
        Section {
            if rules.rules.isEmpty {
                Text("还没有规则。可以逐条添加，或粘贴 Loon、Surge 配置里的 [Rule] 段。")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            ForEach(rules.rules) { rule in
                Button {
                    editing = LocalRuleEdit(original: rule)
                } label: {
                    row(rule)
                }
                .buttonStyle(.plain)
                .swipeActions(edge: .leading) {
                    Button(rule.enabled ? "停用" : "启用") {
                        var next = rules
                        if let index = next.rules.firstIndex(where: { $0.id == rule.id }) {
                            next.rules[index].enabled.toggle()
                            commit(next)
                        }
                    }
                    .tint(rule.enabled ? .gray : .green)
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
            Text("我的规则 · \(rules.rules.count)")
        }
        .listRowBackground(brand.surface)
    }

    private func row(_ rule: LocalRules.Rule) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: rule.value)
                    .font(.callout.monospaced())
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(verbatim: rule.kind)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            Text(LocalRulesText.policy(rule.policy))
                .font(.caption.weight(.semibold))
                .foregroundStyle(policyColor(rule.policy))
        }
        .opacity(rule.enabled ? 1 : 0.45)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityValue(rule.enabled ? "" : String(localized: "已停用"))
    }

    private func policyColor(_ policy: LocalRules.Policy) -> Color {
        switch policy {
        case .proxy:
            return Brand.sky
        case .direct:
            return .secondary
        case .reject:
            return brand.danger
        }
    }

    private func commit(_ next: LocalRules) {
        Task {
            _ = await tunnel.applyLocalRules(next)
        }
    }
}

/// The rule a sheet edits; nil `original` adds one.
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
