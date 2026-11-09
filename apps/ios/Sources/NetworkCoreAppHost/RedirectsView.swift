import NetworkCoreBridge
import SwiftUI

/// The rules page's link to the destination rewrites, with how many are on.
struct RedirectSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            NavigationLink {
                RedirectsView(tunnel: tunnel)
            } label: {
                LabeledContent("重定向", value: summary)
            }
            .listRowBackground(brand.surface)
        }
    }

    private var summary: String {
        let count = tunnel.redirects.lines.count
        return count == 0 ? String(localized: "未开启") : String(localized: "\(count) 条")
    }
}

/// Destination rewrites: send a game's (or any app's) TCP and UDP traffic for one server to another address, such
/// as a development server, without changing the app. Saving reconnects a running tunnel.
struct RedirectsView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var editing: RedirectEdit?
    @State private var failure: String?

    private var redirects: TrafficRedirects {
        tunnel.redirects
    }

    var body: some View {
        List {
            Section {
                ForEach(redirects.entries) { entry in
                    row(entry)
                }
                .onMove { from, to in
                    var next = redirects
                    next.entries.move(fromOffsets: from, toOffset: to)
                    commit(next)
                }
                if redirects.entries.count < TrafficRedirects.maxEntries {
                    Button {
                        editing = RedirectEdit(original: nil)
                    } label: {
                        Label("添加重定向", systemImage: "plus")
                    }
                    .listRowBackground(brand.surface)
                }
            } footer: {
                Text("命中的 TCP 连接和 UDP 流改发到新地址，App 看到的仍是原地址，不需要改 App。从上到下第一条命中的生效。改写后按新地址走规则：开发服务器在内网时，再在订阅规则里让内网网段走能到达它的节点。域名只能匹配 TCP（按 TLS 名称或 HTTP Host）；按域名连接的 App 也可以直接用 DNS 页的 Hosts。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("重定向")
        .inlineNavigationTitle()
        .sheet(item: $editing) { edit in
            RedirectSheet(edit: edit) { source, ports, network, target in
                let next = try redirects.adding(
                    source: source,
                    ports: ports,
                    network: network,
                    target: target,
                    replacing: edit.original?.id
                )
                commit(next)
            }
            .formSheetFrame()
        }
        .alert("无法保存", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            Text(failure ?? "")
        }
    }

    private func row(_ entry: TrafficRedirects.Entry) -> some View {
        HStack {
            Button {
                editing = RedirectEdit(original: entry)
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: entry.match)
                        .font(.footnote.monospaced())
                        .lineLimit(2)
                    Text(verbatim: "→ \(entry.target)")
                        .font(.footnote.monospaced())
                        .foregroundStyle(brand.text2)
                }
            }
            .buttonStyle(.plain)
            Spacer()
            Toggle("启用", isOn: Binding(
                get: { entry.enabled },
                set: { enabled in
                    var next = redirects
                    if let index = next.entries.firstIndex(where: { $0.id == entry.id }) {
                        next.entries[index].enabled = enabled
                        commit(next)
                    }
                }
            ))
            .labelsHidden()
        }
        .listRowBackground(brand.surface)
        .swipeActions(edge: .trailing) {
            Button(role: .destructive) {
                var next = redirects
                next.entries.removeAll { $0.id == entry.id }
                commit(next)
            } label: {
                Label("删除", systemImage: "trash")
            }
        }
    }

    private func commit(_ next: TrafficRedirects) {
        Task {
            await tunnel.applyRedirects(next)
        }
    }
}

/// The entry a sheet edits; nil `original` adds one.
struct RedirectEdit: Identifiable {
    let id = UUID()
    let original: TrafficRedirects.Entry?
}

private struct RedirectSheet: View {
    let edit: RedirectEdit
    let save: (String, String, TrafficRedirects.Network, String) throws -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var source = ""
    @State private var ports = ""
    @State private var network = TrafficRedirects.Network.any
    @State private var target = ""
    @State private var failure: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("原地址", text: $source, prompt: Text(verbatim: "203.0.113.10"))
                        .plainTextEntry(url: true)
                    TextField("端口（可选）", text: $ports, prompt: Text(verbatim: "7000-7100"))
                        .plainTextEntry()
                    Picker("协议", selection: $network) {
                        Text("全部").tag(TrafficRedirects.Network.any)
                        Text(verbatim: "TCP").tag(TrafficRedirects.Network.tcp)
                        Text(verbatim: "UDP").tag(TrafficRedirects.Network.udp)
                    }
                    .pickerStyle(.segmented)
                } header: {
                    Text("匹配")
                } footer: {
                    Text("原地址可写 IP、网段（203.0.113.0/24）或域名（含子域名），也可以直接写一条规则，例如 DOMAIN,login.example.com。")
                }
                .listRowBackground(brand.surface)
                Section {
                    TextField("新地址", text: $target, prompt: Text(verbatim: "10.8.0.5:7000"))
                        .plainTextEntry(url: true)
                } header: {
                    Text("改发到")
                } footer: {
                    Text("IP 或域名，可带 :端口；不写端口沿用原端口，只改端口写 :端口。IPv6 带端口写 [地址]:端口。")
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
            .navigationTitle(edit.original == nil ? "添加重定向" : "编辑重定向")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        do {
                            try save(source, ports, network, target)
                            dismiss()
                        } catch let error as TrafficRedirectsError {
                            failure = error.message
                        } catch {
                            failure = String(localized: "无法保存重定向")
                        }
                    }
                    .disabled(target.isEmpty)
                }
            }
            .onAppear {
                guard let original = edit.original else {
                    return
                }
                target = original.target
                if let fields = TrafficRedirects.fields(of: original) {
                    source = fields.source
                    ports = fields.ports
                    network = fields.network
                } else {
                    source = original.match
                }
            }
        }
    }
}
