import NetworkCoreBridge
import SwiftUI

/// The rules page's list of the subscription's rules and the rule sets they use. While connected each set shows
/// what the engine loaded: entries, last download and the last error, with a button to download them all now.
struct RuleSetsSection: View {
    @ObservedObject var tunnel: TunnelController
    let rules: [String]
    let defined: [String: EmbeddedRuntimeConfig.RuleSet]?
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
            Text("GEOIP,CN、GEOSITE,cn 和 GEOSITE,gfw 没有订阅自带的规则集时，使用公开列表（Loyalsoldier、gaoyifan），由内核每天直连下载一次。")
        }
        .task(id: tunnel.connection) {
            await load()
        }
    }

    private func row(_ name: String) -> some View {
        let loaded = status[name]
        let source = defined?[name] ?? RuleSetCatalog.known[name]
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
                if source == nil {
                    // The engine refuses RULE-SET and GEOSITE rules without their set, so the app leaves them out.
                    Text(name.hasPrefix("geoip-") ? "订阅没有提供这个规则集，相关规则不会命中" : "订阅没有提供这个规则集，已跳过相关规则")
                        .foregroundStyle(brand.warning)
                } else if let error = loaded?.lastError {
                    Text("上次下载失败：\(error)")
                        .foregroundStyle(brand.danger)
                } else if let updated = loaded?.updatedAt {
                    Text("更新于 \(updated, style: .relative)前")
                        .foregroundStyle(brand.text2)
                } else if let url = source?.url {
                    Text(verbatim: SubscriptionLink.displayHost(url))
                        .foregroundStyle(brand.text3)
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
                        .foregroundStyle(brand.text3)
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
