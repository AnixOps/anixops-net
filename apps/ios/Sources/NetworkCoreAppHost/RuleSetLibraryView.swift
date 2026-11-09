import NetworkCoreBridge
import SwiftUI

/// Open-source rule lists from GitHub, grouped by what they cover, each added in one tap with its usual policy, plus
/// any other list by URL. Adding reconnects a running tunnel.
struct RuleSetLibraryView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var addingURL: RuleSetEdit?
    @State private var failure: String?

    private var rules: LocalRules {
        tunnel.localRules
    }

    private var isFull: Bool {
        rules.subscriptions.count >= LocalRules.maxSubscriptions
    }

    var body: some View {
        List {
            Section {
                Button {
                    addingURL = RuleSetEdit(original: nil)
                } label: {
                    Label("通过链接添加", systemImage: "link")
                        .foregroundStyle(brand.primary)
                }
                .disabled(isFull)
            } footer: {
                Text(isFull
                    ? String(localized: "已达到 \(LocalRules.maxSubscriptions) 个规则集的上限，先删掉不用的再添加。")
                    : String(localized: "任意 Surge、Loon、Clash 规则列表的 https 链接，例如 GitHub raw 地址。"))
            }
            .listRowBackground(brand.surface)

            ForEach(OpenRuleSets.Category.allCases) { category in
                Section {
                    ForEach(OpenRuleSets.entries.filter { $0.category == category }) { entry in
                        row(entry)
                    }
                } header: {
                    Text(title(category))
                }
                .listRowBackground(brand.surface)
            }

            Section {
                EmptyView()
            } footer: {
                Text("列表来自 GitHub 上的开源项目 blackmatrix7/ios_rule_script 和 Loyalsoldier/surge-rules，由内核经代理下载并每天更新。内核用紧凑格式保存规则集，20 万条的完整去广告列表也只占约 5 MB 隧道内存。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("开源规则库")
        .inlineNavigationTitle()
        .sheet(item: $addingURL) { edit in
            RuleSetSubscriptionSheet(edit: edit, rules: rules) { next in
                commit(next)
            }
            .formSheetFrame()
        }
        .alert("无法添加", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            Text(failure ?? "")
        }
    }

    private func row(_ entry: OpenRuleSets.Entry) -> some View {
        let added = rules.subscriptions.first { $0.catalogID == entry.id || $0.url == entry.url }
        return HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: entry.name)
                    .foregroundStyle(brand.text)
                Text(verbatim: entry.source)
                    .font(.caption)
                    .foregroundStyle(brand.text3)
            }
            Spacer(minLength: 8)
            PolicyChip(policy: added?.policy ?? entry.policy)
            if added != nil {
                Button {
                    remove(entry)
                } label: {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.title3)
                        .foregroundStyle(brand.primary)
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("移除 \(entry.name)")
            } else {
                Button {
                    add(entry)
                } label: {
                    Image(systemName: "plus.circle")
                        .font(.title3)
                        .foregroundStyle(isFull ? brand.text3 : brand.primary)
                }
                .buttonStyle(.borderless)
                .disabled(isFull)
                .accessibilityLabel("添加 \(entry.name)")
            }
        }
        .frame(minHeight: 44)
        .contextMenu {
            if added == nil {
                ForEach(LocalRules.Policy.allCases) { policy in
                    Button("添加为\(LocalRulesText.policy(policy))") {
                        add(entry, policy: policy)
                    }
                }
            }
        }
    }

    private func title(_ category: OpenRuleSets.Category) -> String {
        switch category {
        case .block:
            return String(localized: "广告与追踪")
        case .ai:
            return String(localized: "AI 服务")
        case .media:
            return String(localized: "流媒体")
        case .services:
            return String(localized: "常用海外服务")
        case .direct:
            return String(localized: "建议直连")
        }
    }

    private func add(_ entry: OpenRuleSets.Entry, policy: LocalRules.Policy? = nil) {
        do {
            commit(try rules.saving(entry.subscription(policy: policy)))
        } catch {
            failure = String(localized: "最多 \(LocalRules.maxSubscriptions) 个规则集")
        }
    }

    private func remove(_ entry: OpenRuleSets.Entry) {
        var next = rules
        next.subscriptions.removeAll { $0.catalogID == entry.id || $0.url == entry.url }
        commit(next)
    }

    private func commit(_ next: LocalRules) {
        tunnel.updateLocalRules(next)
    }
}
