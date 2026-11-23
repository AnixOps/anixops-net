import NetworkCoreBridge
import SwiftUI

/// The rules page's link to the DNS settings, with what is set now.
struct DnsSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            NavigationLink {
                DnsSettingsView(tunnel: tunnel)
            } label: {
                LabeledContent("DNS", value: summary)
            }
            .listRowBackground(brand.surface)
        }
    }

    private var summary: String {
        let dns = tunnel.dns
        if dns.isEmpty {
            return String(localized: "默认")
        }
        var text: String
        if let name = dns.effectiveDefault {
            text = dns.rules.isEmpty ? name : String(localized: "\(name) 等 \(dns.servers.count) 个")
        } else if !dns.hosts.isEmpty {
            text = String(localized: "\(dns.hosts.count) 条 Hosts")
        } else {
            return "Fake-IP"
        }
        if dns.fakeIP {
            text += " · Fake-IP"
        }
        return text
    }
}

/// DNS servers (UDP, TCP, DNS over TLS, HTTPS or QUIC, through the proxy or directly, or the system's), which ones
/// race to answer by default, the fallback, per-domain servers and local hosts. Saving reconnects a running tunnel.
struct DnsSettingsView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var editing: ServerEdit?
    @State private var addingRule = false
    @State private var addingHost = false
    @State private var confirmingReset = false
    @State private var failure: String?

    private var dns: DnsSettings {
        tunnel.dns
    }

    var body: some View {
        List {
            fakeIPSection
            serversSection
            if !dns.servers.isEmpty {
                rulesSection
            }
            hostsSection
            Section {
                if !dns.isEmpty {
                    Button("恢复默认", role: .destructive) {
                        confirmingReset = true
                    }
                    .listRowBackground(brand.surface)
                }
            } footer: {
                Text("默认设置下所有查询经代理发往 1.1.1.1。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("DNS")
        .inlineNavigationTitle()
        .sheet(item: $editing) { edit in
            DnsServerSheet(edit: edit) { server in
                try save(server, replacing: edit.original?.name)
            }
            .formSheetFrame()
            .brandSheet()
        }
        .sheet(isPresented: $addingRule) {
            DnsRuleSheet(servers: dns.servers.map(\.name)) { text, server in
                commit { try dns.addingRule(text, server: server) }
            }
            .formSheetFrame()
            .brandSheet()
        }
        .sheet(isPresented: $addingHost) {
            DnsHostSheet { name, addresses in
                commit { try dns.settingHost(name, addresses: addresses) }
            }
            .formSheetFrame()
            .brandSheet()
        }
        .confirmationDialog("清除所有 DNS 设置？", isPresented: $confirmingReset, titleVisibility: .visible) {
            Button("恢复默认", role: .destructive) {
                commit(DnsSettings())
            }
        }
        .alert("无法保存", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            Text(failure ?? "")
        }
    }

    private var serversSection: some View {
        Section {
            ForEach(dns.servers) { server in
                Button {
                    editing = ServerEdit(original: server)
                } label: {
                    serverRow(server)
                }
                .listRowBackground(brand.surface)
                .swipeActions(edge: .trailing) {
                    Button(role: .destructive) {
                        commit(dns.removingServer(server.name))
                    } label: {
                        Label("删除", systemImage: "trash")
                    }
                }
                .contextMenu {
                    let defaults = dns.effectiveDefaults
                    if defaults != [server.name] {
                        Button {
                            commit { try dns.settingDefault(server.name) }
                        } label: {
                            Label("设为默认", systemImage: "checkmark.circle")
                        }
                    }
                    if !defaults.contains(server.name), defaults.count < DnsSettings.maxRace {
                        Button {
                            commit { try dns.settingRacing(server.name, true) }
                        } label: {
                            Label("加入默认竞速", systemImage: "flag.2.crossed")
                        }
                    }
                    if defaults.contains(server.name), defaults.count > 1 {
                        Button {
                            commit { try dns.settingRacing(server.name, false) }
                        } label: {
                            Label("退出默认竞速", systemImage: "flag.slash")
                        }
                    }
                    if dns.fallbackServer == server.name {
                        Button {
                            commit { try dns.settingFallback(nil) }
                        } label: {
                            Label("取消备用", systemImage: "arrow.uturn.backward.circle")
                        }
                    } else {
                        Button {
                            commit { try dns.settingFallback(server.name) }
                        } label: {
                            Label("设为备用", systemImage: "arrow.uturn.forward.circle")
                        }
                    }
                    Button(role: .destructive) {
                        commit(dns.removingServer(server.name))
                    } label: {
                        Label("删除", systemImage: "trash")
                    }
                }
            }
            if dns.servers.count < DnsSettings.maxServers {
                Menu {
                    ForEach(DnsSettings.presets.filter { preset in !dns.servers.contains { $0.name == preset.server.name } }) { preset in
                        Button(preset.title) {
                            commit { try dns.saving(preset.server) }
                        }
                    }
                    Button("自定义…") {
                        editing = ServerEdit(original: nil)
                    }
                } label: {
                    Label("添加 DNS 服务器", systemImage: "plus")
                }
                .listRowBackground(brand.surface)
            }
        } header: {
            Text("服务器")
        } footer: {
            Text("没有规则命中的域名由默认服务器解析。长按服务器可设为默认、加入默认竞速（最多 4 个同时查询，取最先的应答）或设为备用（默认或规则的服务器失败时再查，适合公司 VPN 的内部域名）。经代理的服务器在直连模式下也直连，系统 DNS 总是直连。")
        }
    }

    private func serverRow(_ server: DnsSettings.Server) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(verbatim: server.name)
                    .foregroundStyle(brand.text)
                Text(verbatim: server.kind)
                    .font(.caption2.weight(.bold))
                    .padding(.horizontal, 5)
                    .padding(.vertical, 1)
                    .background(brand.raised, in: RoundedRectangle(cornerRadius: 4, style: .continuous))
                    .foregroundStyle(brand.text2)
                Spacer(minLength: 4)
                if dns.effectiveDefaults.contains(server.name) {
                    Text("默认")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(brand.primary)
                }
                if dns.fallbackServer == server.name {
                    Text("备用")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(brand.text2)
                }
            }
            Text(server.isSystem ? String(localized: "直连 · 本机网络的 DNS") : server.via == .direct ? String(localized: "直连 · \(server.url)") : String(localized: "经代理 · \(server.url)"))
                .font(.caption)
                .foregroundStyle(brand.text2)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .padding(.vertical, 2)
    }

    private var rulesSection: some View {
        Section {
            ForEach(Array(dns.rules.enumerated()), id: \.offset) { index, rule in
                HStack(alignment: .firstTextBaseline) {
                    Text(verbatim: rule.summary)
                        .font(.footnote.monospaced())
                        .foregroundStyle(brand.text)
                        .lineLimit(2)
                    Spacer(minLength: 8)
                    Text(verbatim: rule.server)
                        .font(.footnote)
                        .foregroundStyle(brand.primary)
                }
                .listRowBackground(brand.surface)
                .swipeActions(edge: .trailing) {
                    Button(role: .destructive) {
                        var next = dns
                        next.rules.remove(at: index)
                        commit(next)
                    } label: {
                        Label("删除", systemImage: "trash")
                    }
                }
            }
            .onMove { from, to in
                var next = dns
                next.rules.move(fromOffsets: from, toOffset: to)
                commit(next)
            }
            if dns.rules.count < DnsSettings.maxRules {
                Button {
                    addingRule = true
                } label: {
                    Label("添加规则", systemImage: "plus")
                }
                .listRowBackground(brand.surface)
            }
        } header: {
            Text("按域名选择服务器")
        } footer: {
            VStack(alignment: .leading, spacing: 4) {
                Text("从上到下第一条命中的规则生效。RULE-SET 使用规则页的规则集，例如 geosite-cn；只有单个节点、没有订阅规则时它不会命中。")
                if !dns.rules.isEmpty {
                    Text("已用 \(dns.ruleDomainCount) / \(DnsSettings.maxRuleDomains) 个域名、后缀和关键词")
                }
                // Saved over the limit by an earlier version: the tunnel leaves the last rules out.
                if dns.ruleDomainCount > DnsSettings.maxRuleDomains {
                    Text("超出上限，排在后面的规则不会生效。请删除一些规则，较长的列表改用 RULE-SET 或 GEOSITE 规则集。")
                        .foregroundStyle(brand.warning)
                }
            }
        }
    }

    private var fakeIPSection: some View {
        Section {
            Toggle(
                "Fake-IP",
                isOn: Binding(
                    get: { dns.fakeIP },
                    set: { on in commit { try dns.settingFakeIP(on) } }
                )
            )
            .listRowBackground(brand.surface)
        } footer: {
            Text("开启后，应用查询域名时立即拿到 198.18.0.0/15（IPv6 为 fc00::/18）内的临时地址，连接时再还原成域名按规则分流、由代理解析，首次连接更快，非 HTTP/TLS 协议也能按域名分流。局域网、NTP、STUN、Apple 推送和联网检测等域名仍返回真实地址。关闭或更改后，个别应用可能需要重新打开。")
        }
    }

    private var hostsSection: some View {
        Section {
            ForEach(dns.hosts.keys.sorted(), id: \.self) { name in
                LabeledContent {
                    Text(verbatim: (dns.hosts[name] ?? []).joined(separator: ", "))
                        .font(.footnote.monospaced())
                        .lineLimit(2)
                } label: {
                    Text(verbatim: name)
                }
                .listRowBackground(brand.surface)
                .swipeActions(edge: .trailing) {
                    Button(role: .destructive) {
                        var next = dns
                        next.hosts.removeValue(forKey: name)
                        commit(next)
                    } label: {
                        Label("删除", systemImage: "trash")
                    }
                }
            }
            Button {
                addingHost = true
            } label: {
                Label("添加 Hosts", systemImage: "plus")
            }
            .listRowBackground(brand.surface)
        } header: {
            Text("Hosts")
        } footer: {
            Text("这些域名直接返回填写的地址，不查询服务器。*.example.com 匹配它的所有子域名。")
        }
    }

    private func save(_ server: DnsSettings.Server, replacing old: String?) throws {
        let next = try dns.saving(server, replacing: old)
        commit(next)
    }

    private func commit(_ next: DnsSettings) {
        Task {
            await tunnel.applyDns(next)
        }
    }

    /// Applies what `make` returns, or shows why it was refused.
    private func commit(_ make: () throws -> DnsSettings) {
        do {
            commit(try make())
        } catch let error as DnsSettingsError {
            failure = error.message
        } catch {
            failure = String(localized: "无法保存 DNS 设置")
        }
    }
}

/// The server a sheet edits; nil `original` adds one.
struct ServerEdit: Identifiable {
    let id = UUID()
    let original: DnsSettings.Server?
}

private struct DnsServerSheet: View {
    let edit: ServerEdit
    let save: (DnsSettings.Server) throws -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var name = ""
    @State private var url = ""
    @State private var serverName = ""
    @State private var via = DnsSettings.Via.proxy
    @State private var failure: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("名称", text: $name)
                        .plainTextEntry()
                    TextField("地址", text: $url, prompt: Text(verbatim: "https://1.1.1.1/dns-query"))
                        .plainTextEntry(url: true)
                    TextField("TLS 名称（可选）", text: $serverName, prompt: Text(verbatim: "cloudflare-dns.com"))
                        .plainTextEntry()
                } footer: {
                    Text("地址可写 1.1.1.1、udp://、tcp://、tls://（DoT）、https://（DoH）、quic://（DoQ），或 system（本机网络的 DNS）。用 IP 地址写 DoT/DoH/DoQ 时填写证书上的 TLS 名称。")
                }
                .listRowBackground(brand.surface)
                Section {
                    Picker("线路", selection: $via) {
                        Text("经代理").tag(DnsSettings.Via.proxy)
                        Text("直连").tag(DnsSettings.Via.direct)
                    }
                    .pickerStyle(.segmented)
                } footer: {
                    Text("国内的 DNS 服务器建议直连。")
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
            .navigationTitle(edit.original == nil ? "添加 DNS 服务器" : "编辑 DNS 服务器")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        let trimmed = serverName.trimmingCharacters(in: .whitespaces)
                        let server = DnsSettings.Server(
                            name: name.trimmingCharacters(in: .whitespaces),
                            url: url.trimmingCharacters(in: .whitespaces),
                            serverName: trimmed.isEmpty ? nil : trimmed,
                            via: via
                        )
                        do {
                            try save(server)
                            dismiss()
                        } catch let error as DnsSettingsError {
                            failure = error.message
                        } catch {
                            failure = String(localized: "无法保存 DNS 设置")
                        }
                    }
                    .disabled(name.isEmpty || url.isEmpty)
                }
            }
            .onAppear {
                if let original = edit.original {
                    name = original.name
                    url = original.url
                    serverName = original.serverName ?? ""
                    via = original.via
                }
            }
        }
    }
}

private struct DnsRuleSheet: View {
    let servers: [String]
    let add: (String, String) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var text = ""
    @State private var server = ""

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextEditor(text: $text)
                        .font(.footnote.monospaced())
                        .plainTextEntry()
                        .frame(minHeight: 120)
                } header: {
                    Text("域名")
                } footer: {
                    Text(verbatim: "example.com\nDOMAIN,api.example.com\nDOMAIN-KEYWORD,example\nRULE-SET,geosite-cn")
                        .font(.caption.monospaced())
                }
                .listRowBackground(brand.surface)
                Section {
                    Picker("服务器", selection: $server) {
                        ForEach(servers, id: \.self) { name in
                            Text(verbatim: name).tag(name)
                        }
                    }
                }
                .listRowBackground(brand.surface)
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("添加规则")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("添加") {
                        add(text, server)
                        dismiss()
                    }
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || server.isEmpty)
                }
            }
            .onAppear {
                server = servers.first ?? ""
            }
        }
    }
}

private struct DnsHostSheet: View {
    let add: (String, String) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var name = ""
    @State private var addresses = ""

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("域名", text: $name, prompt: Text(verbatim: "nas.lan"))
                        .plainTextEntry(url: true)
                    TextField("IP 地址", text: $addresses, prompt: Text(verbatim: "192.168.1.10"))
                        .plainTextEntry()
                } footer: {
                    Text("多个地址用逗号或空格分隔，最多 8 个。")
                }
                .listRowBackground(brand.surface)
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("添加 Hosts")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("添加") {
                        add(name, addresses)
                        dismiss()
                    }
                    .disabled(name.isEmpty || addresses.isEmpty)
                }
            }
        }
    }
}
