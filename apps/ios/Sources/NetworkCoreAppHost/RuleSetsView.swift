import NetworkCoreBridge
import SwiftUI

/// The rules page's list of the subscription's rules and the rule sets they use. While connected each set shows
/// what the engine loaded: entries, last download, the last error and the list lines it could not hold, with a button
/// to download them all now. Rules the import left out are listed below, so none fails silently.
struct RuleSetsSection: View {
    var tunnel: TunnelController
    let rules: [String]
    let defined: [String: EmbeddedRuntimeConfig.RuleSet]?
    /// What importing the subscription's rules left out (`NodeLibrary.routingWarnings`).
    var warnings: [String] = []
    @Environment(\.brand) private var brand
    @State private var status: [String: RuntimeControl.RuleSets.Status] = [:]
    @State private var updating = false

    private var names: [String] {
        RuleSetCatalog.referencedNames(in: rules)
    }

    var body: some View {
        Section {
            NavigationLink {
                RuleListView(rules: rules)
            } label: {
                LabeledContent("全部规则", value: "\(rules.count)")
            }
            .listRowBackground(brand.surface)
            ForEach(names, id: \.self) { name in
                row(name)
                    .listRowBackground(brand.surface)
            }
            if !names.isEmpty, tunnel.connection == .connected {
                Button {
                    updating = true
                    Task {
                        _ = await tunnel.control(.updateRuleSets)
                        // Downloads run in the engine; read the result back after a moment.
                        try? await Task.sleep(for: .seconds(3))
                        await load()
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
                .listRowBackground(brand.surface)
            }
        } header: {
            Text("规则与规则集")
        } footer: {
            Text("GEOIP,CN 和 GEOSITE,cn 没有订阅自带的规则集时，使用公开列表（gaoyifan/china-operator-ip、Loyalsoldier/v2ray-rules-dat），由内核每天直连下载一次。")
        }
        .task(id: tunnel.connection) {
            await load()
        }
        if !warnings.isEmpty {
            Section {
                ForEach(Array(warnings.enumerated()), id: \.offset) { _, warning in
                    Text(verbatim: warning)
                        .font(.caption)
                        .foregroundStyle(brand.warning)
                        .textSelection(.enabled)
                        .listRowBackground(brand.surface)
                }
            } header: {
                Text("导入时跳过的规则")
            } footer: {
                Text("这些规则在导入订阅时没有导入，不会生效。")
            }
        }
    }

    private func row(_ name: String) -> some View {
        let loaded = status[name]
        // The user's data lists replace the subscription's and the built-in set of their name.
        let added = tunnel.localRules.dataSources.first { $0.name == name }.map { EmbeddedRuntimeConfig.RuleSet(url: $0.url) }
        let source = added ?? defined?[name] ?? RuleSetCatalog.known[name]
        let builtIn = source == nil && RuleSetCatalog.builtIn.contains(name)
        return VStack(alignment: .leading, spacing: 3) {
            HStack {
                Text(verbatim: name)
                    .foregroundStyle(brand.text)
                Spacer()
                if let loaded {
                    Text("\(loaded.entries) 条")
                        .font(.footnote.monospacedDigit())
                        .foregroundStyle(brand.text2)
                }
            }
            Group {
                if builtIn {
                    Text("内置：本机和局域网地址")
                        .foregroundStyle(brand.text2)
                } else if source == nil {
                    // The engine refuses RULE-SET and GEOSITE rules without their set, so the app leaves them out.
                    Text(name.hasPrefix("geoip-") || name.hasPrefix("asn-") ? "没有这份数据，相关规则不会命中。可以在“GeoIP 与 ASN 数据”里按 URL 添加列表。" : "订阅没有提供这个规则集，已跳过相关规则")
                        .foregroundStyle(brand.warning)
                } else if let error = loaded?.lastError {
                    Text("上次下载失败：\(error)")
                        .foregroundStyle(brand.danger)
                } else if let updated = loaded?.updatedAt {
                    Text("更新于 \(AgeText.coarse(since: updated))前")
                        .foregroundStyle(brand.text2)
                } else if let url = source?.url {
                    Text(verbatim: SubscriptionLink.displayHost(url))
                        .foregroundStyle(brand.text2)
                }
                if let loaded, loaded.skippedCount > 0 {
                    Text("列表里有 \(loaded.skippedCount) 条规则不支持，已跳过：\(loaded.skippedSummary)")
                        .foregroundStyle(brand.warning)
                }
            }
            .font(.caption)
        }
        .padding(.vertical, 2)
    }

    private func load() async {
        guard let answer = RuntimeControl.decode(RuntimeControl.RuleSets.self, from: await tunnel.control(.ruleSets)) else {
            status = [:]
            return
        }
        status = Dictionary(answer.ruleSets.map { ($0.name, $0) }) { first, _ in first }
    }
}

/// Every rule of the subscription in order, searchable.
struct RuleListView: View {
    let rules: [String]
    @Environment(\.brand) private var brand
    @State private var query = ""

    private var shown: [(offset: Int, element: String)] {
        let all = Array(rules.enumerated())
        let words = query.split(whereSeparator: \.isWhitespace)
        guard !words.isEmpty else {
            return all
        }
        return all.filter { rule in
            words.allSatisfy { rule.element.localizedCaseInsensitiveContains($0) }
        }
    }

    var body: some View {
        List {
            ForEach(shown, id: \.offset) { item in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text("\(item.offset + 1)")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(brand.text2)
                        .frame(minWidth: 28, alignment: .trailing)
                    Text(verbatim: item.element)
                        .font(.footnote.monospaced())
                        .foregroundStyle(brand.text)
                        .textSelection(.enabled)
                }
                .listRowBackground(brand.surface)
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("全部规则")
        .inlineNavigationTitle()
        .searchable(text: $query, prompt: Text("筛选规则"))
    }
}
