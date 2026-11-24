import NetworkCoreBridge
import SwiftUI

/// Rules: the proxy mode (rules, global or direct), the subscription's policy groups, the local rules and the DNS
/// settings. Only subscriptions that carry rules and groups have groups to show here;
/// any other node list routes by the user's own rules and sends the rest through the selected node.
struct RulesView: View {
    var tunnel: TunnelController
    /// Pushed from another page (debug mode's exit settings) instead of being a tab's root.
    var embedded = false
    @Environment(\.brand) private var brand
    /// The running engine's groups by id: what each group uses now and its members' last delay.
    @State private var runtime: [String: RuntimeControl.Groups.Group] = [:]
    /// Whether the running tunnel took the rules (`RoutingState`); nil while disconnected.
    @State private var routingState: RoutingState?

    var body: some View {
        NavigationContainer(embedded: embedded) {
            Group {
                if let library = tunnel.library, let routing = library.routing {
                    rules(library, routing)
                } else if tunnel.nodeName != nil {
                    List {
                        modeSection(hasRules: tunnel.library != nil)
                        routingNote
                        if tunnel.library != nil {
                            LocalRulesSection(tunnel: tunnel, hasSubscriptionRules: false)
                            ruleTools
                        }
                        DnsSection(tunnel: tunnel)
                        RedirectSection(tunnel: tunnel)
                    }
                    .scrollContentBackground(.hidden)
                } else {
                    ScrollView {
                        BrandEmptyState(
                            systemImage: "arrow.triangle.branch",
                            title: "规则与分流",
                            message: "导入订阅或节点后，可在此选择规则、全局或直连模式。规则模式按自定义规则分流；带规则和策略组的订阅按订阅规则分流。"
                        )
                        .padding(.top, 80)
                    }
                }
            }
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("规则")
            .brandBars(brand)
            .task(id: tunnel.connection) {
                while !Task.isCancelled, tunnel.connection == .connected {
                    await loadRuntime()
                    try? await Task.sleep(for: .seconds(5))
                }
                runtime = [:]
                routingState = nil
            }
        }
    }

    private func loadRuntime() async {
        routingState = await tunnel.routingState()
        if let answer = RuntimeControl.decode(RuntimeControl.Groups.self, from: await tunnel.control(.groups)) {
            runtime = Dictionary(answer.groups.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        }
    }

    private func rules(_ library: NodeLibrary, _ routing: NativeImportReport.Routing) -> some View {
        List {
            modeSection(hasRules: true)

            routingNote

            Section {
                ForEach(routing.groups) { group in
                    groupRow(group, library: library)
                        .listRowBackground(brand.surface)
                }
            } header: {
                Text("策略组 · \(routing.rules.count) 条规则")
            } footer: {
                Text("色条为每个成员最近一次测得的延迟：绿色低于 120 ms，琥珀色低于 250 ms，橙色更慢，灰色无数据。")
            }
            .disabled(tunnel.mode != .rule)

            LocalRulesSection(tunnel: tunnel, hasSubscriptionRules: true)

            RuleSetsSection(tunnel: tunnel, rules: routing.rules, defined: routing.ruleSets,
                            warnings: library.routingWarnings)

            if RuleSetCatalog.hasProcessRules(routing.rules) {
                ProcessRulesNote()
            }
            #if os(iOS)
            if RuleSetCatalog.hasRules(of: "DEVICE-NAME", in: routing.rules) {
                RuleNote(text: String(localized: "订阅含 DEVICE-NAME 规则，iPhone 和 iPad 不支持，规则不生效。"))
            }
            #endif

            ruleTools

            DnsSection(tunnel: tunnel)

            RedirectSection(tunnel: tunnel)
        }
        .scrollContentBackground(.hidden)
    }

    /// Why the running tunnel does not route by every rule: the rules were too large or refused and it runs the
    /// selected node alone, or rule sets over the engine's limit were left out.
    @ViewBuilder
    private var routingNote: some View {
        if tunnel.connection == .connected, let text = routingState?.notice {
            RuleNote(text: text)
        }
    }

    /// The rule test and hit counts while connected, then the GEOIP / IP-ASN data lists.
    @ViewBuilder
    private var ruleTools: some View {
        if tunnel.connection == .connected {
            RuleTestSection(tunnel: tunnel)
        }
        RuleHitsSection(tunnel: tunnel)
        DataListsSection(tunnel: tunnel)
    }

    private func modeSection(hasRules: Bool) -> some View {
        Section {
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
            .listRowBackground(brand.surface)
        } header: {
            Text("代理模式")
        } footer: {
            Text(ProxyModeText.footer(tunnel.mode))
        }
    }

    private func groupRow(_ group: NativeImportReport.Routing.Group, library: NodeLibrary) -> some View {
        PolicyGroupCard(
            group: group,
            selected: library.selection(in: group),
            runtime: runtime[group.id],
            name: { name(of: $0, in: library) },
            select: { member in
                Task {
                    await tunnel.selectMember(member, inGroup: group.id)
                    await loadRuntime()
                }
            }
        )
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
