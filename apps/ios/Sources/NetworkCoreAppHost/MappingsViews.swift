import NetworkCoreBridge
import SwiftUI

/// The extensions page's link to Map Local / Map Remote, with how many are on.
struct MappingsSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            NavigationLink {
                MappingsView(tunnel: tunnel)
            } label: {
                LabeledContent {
                    Text(summary)
                } label: {
                    Label("Map Local / Map Remote", systemImage: "arrow.triangle.swap")
                }
            }
            .listRowBackground(brand.surface)
        } footer: {
            Text("用本机内容应答请求，或把请求转到另一个地址。也可以在请求详情里从抓到的请求一键生成。")
        }
    }

    private var summary: String {
        let count = tunnel.debugMappings.active.count
        return count == 0 ? String(localized: "未开启") : String(localized: "\(count) 条")
    }
}

/// Every Map Local and Map Remote entry: a switch each, tap to edit, swipe or the row menu to delete. Saving
/// reconnects a running tunnel; the matching hosts are decrypted with the device root (Extensions › HTTPS 解密证书).
struct MappingsView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var editing: MappingDraft?
    @State private var deleting: DebugMappings.Entry?

    private var mappings: DebugMappings {
        tunnel.debugMappings
    }

    var body: some View {
        List {
            Section {
                if mappings.entries.isEmpty {
                    Text("还没有映射。在请求详情里选“映射到本地”或“映射到远程”，或点下方添加。")
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                        .listRowBackground(brand.surface)
                }
                ForEach(mappings.entries) { entry in
                    row(entry)
                }
                .onMove { from, to in
                    var next = mappings
                    next.entries.move(fromOffsets: from, toOffset: to)
                    commit(next)
                }
                if mappings.entries.count < DebugMappings.maxEntries {
                    Menu {
                        Button {
                            editing = MappingDraft(entry: .init(kind: .local, pattern: "https://"), isNew: true)
                        } label: {
                            Label("Map Local（本地应答）", systemImage: "doc.text")
                        }
                        Button {
                            editing = MappingDraft(entry: .init(kind: .remote, pattern: "https://", target: "https://"), isNew: true)
                        } label: {
                            Label("Map Remote（转到其他地址）", systemImage: "arrow.uturn.right")
                        }
                    } label: {
                        Label("添加映射", systemImage: "plus")
                    }
                    .listRowBackground(brand.surface)
                }
            } footer: {
                Text("从上到下第一条匹配的生效。需要先在 扩展 › HTTPS 解密证书 中安装并信任根证书，HTTPS 请求才能被映射。Map Remote 用 307 重定向发往新地址，方法和请求体保持不变。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("映射")
        .inlineNavigationTitle()
        .sheet(item: $editing) { draft in
            MappingEditor(draft: draft) { entry in
                let next = try mappings.saving(entry)
                commit(next)
            }
            .formSheetFrame()
            .environment(\.brand, brand)
            .tint(brand.primary)
        }
        .confirmationDialog(
            "删除映射“\(deleting.map(Self.title) ?? "")”？",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible
        ) {
            Button("删除映射", role: .destructive) {
                if let entry = deleting {
                    var next = mappings
                    next.entries.removeAll { $0.id == entry.id }
                    commit(next)
                }
                deleting = nil
            }
        }
    }

    private func row(_ entry: DebugMappings.Entry) -> some View {
        HStack(spacing: 10) {
            Button {
                editing = MappingDraft(entry: entry, isNew: false)
            } label: {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(entry.kind == .local ? "LOCAL" : "REMOTE")
                            .font(.caption2.weight(.bold))
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .foregroundStyle(brand.primary)
                            .background(brand.soft, in: RoundedRectangle(cornerRadius: BrandRadius.badge, style: .continuous))
                        Text(Self.title(entry))
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(brand.text)
                            .lineLimit(1)
                    }
                    Text(verbatim: entry.pattern)
                        .font(.footnote.monospaced())
                        .foregroundStyle(brand.text2)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Text(verbatim: Self.result(entry))
                        .font(.caption.monospaced())
                        .foregroundStyle(brand.text3)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            Toggle("启用", isOn: Binding(
                get: { entry.enabled },
                set: { enabled in
                    var next = mappings
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
                deleting = entry
            } label: {
                Label("删除", systemImage: "trash")
            }
        }
        .contextMenu {
            Button(role: .destructive) {
                deleting = entry
            } label: {
                Label("删除映射", systemImage: "trash")
            }
        }
    }

    static func title(_ entry: DebugMappings.Entry) -> String {
        if !entry.name.isEmpty {
            return entry.name
        }
        return URL(string: entry.pattern)?.host ?? entry.pattern
    }

    static func result(_ entry: DebugMappings.Entry) -> String {
        switch entry.kind {
        case .local:
            let type = entry.header("Content-Type").map { " · \($0)" } ?? ""
            return "→ \(entry.status)\(type) · \(TrafficFormat.bytes(UInt64(entry.body.count)))"
        case .remote:
            return "→ \(entry.target)"
        }
    }

    private func commit(_ next: DebugMappings) {
        Task {
            await tunnel.applyDebugMappings(next)
        }
    }
}

/// The entry a sheet edits.
struct MappingDraft: Identifiable {
    let id = UUID()
    let entry: DebugMappings.Entry
    /// Not saved yet: the sheet says "add".
    let isNew: Bool
}

/// Edits one mapping: how the URL matches, and either the local response (status, headers, body) or the new
/// address.
struct MappingEditor: View {
    let draft: MappingDraft
    let save: (DebugMappings.Entry) throws -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var entry: DebugMappings.Entry
    @State private var headerText: String
    @State private var bodyText: String
    /// The body is not UTF-8 text: it is kept as captured and not shown for editing.
    @State private var binaryBody: Bool
    @State private var failure: String?

    init(draft: MappingDraft, save: @escaping (DebugMappings.Entry) throws -> Void) {
        self.draft = draft
        self.save = save
        _entry = State(initialValue: draft.entry)
        _headerText = State(initialValue: draft.entry.headers.map { "\($0.name): \($0.value)" }.joined(separator: "\n"))
        let text = draft.entry.bodyText
        _bodyText = State(initialValue: text ?? "")
        _binaryBody = State(initialValue: text == nil && !draft.entry.body.isEmpty)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("类型", selection: $entry.kind) {
                        Text("Map Local").tag(DebugMappings.Kind.local)
                        Text("Map Remote").tag(DebugMappings.Kind.remote)
                    }
                    .pickerStyle(.segmented)
                    TextField("名称（可选）", text: $entry.name)
                } footer: {
                    Text(entry.kind == .local ? "Map Local：用这里写的响应直接应答，不连接服务器。" : "Map Remote：把请求转到另一个地址，例如测试环境或本机开发服务器。")
                }
                .listRowBackground(brand.surface)

                Section {
                    Picker("匹配", selection: $entry.match) {
                        Text("此 URL").tag(DebugMappings.Match.url)
                        Text("以此开头").tag(DebugMappings.Match.prefix)
                        Text("正则").tag(DebugMappings.Match.regex)
                    }
                    TextField(entry.match == .regex ? "正则表达式" : "URL", text: $entry.pattern, axis: .vertical)
                        .font(.footnote.monospaced())
                        .plainTextEntry(url: entry.match != .regex)
                } header: {
                    Text("匹配请求")
                } footer: {
                    Text(matchFooter)
                }
                .listRowBackground(brand.surface)

                if entry.kind == .local {
                    localSections
                } else {
                    Section {
                        TextField("目标 URL", text: $entry.target, axis: .vertical)
                            .font(.footnote.monospaced())
                            .plainTextEntry(url: true)
                    } header: {
                        Text("转到")
                    } footer: {
                        Text(entry.match == .regex ? "正则匹配时目标原样使用，可以用 $1 引用第一个括号。" : entry.match == .prefix ? "URL 剩下的部分接在目标后面。" : "原请求的查询参数接在目标后面。")
                    }
                    .listRowBackground(brand.surface)
                }

                if let failure {
                    Text(failure)
                        .foregroundStyle(brand.danger)
                        .listRowBackground(brand.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle(draft.isNew ? "添加映射" : "编辑映射")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        commit()
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var localSections: some View {
        Section {
            TextField("状态码", value: $entry.status, format: .number.grouping(.never))
                .monospacedDigit()
            TextField("响应头", text: $headerText, prompt: Text(verbatim: "Content-Type: application/json"), axis: .vertical)
                .font(.footnote.monospaced())
                .lineLimit(2...8)
                .plainTextEntry()
        } header: {
            Text("响应")
        } footer: {
            Text("响应头每行一个，写“名称: 值”。")
        }
        .listRowBackground(brand.surface)
        Section {
            if binaryBody {
                LabeledContent("响应体", value: String(localized: "二进制内容 · \(TrafficFormat.bytes(UInt64(entry.body.count)))"))
                Button("改为文本") {
                    binaryBody = false
                    bodyText = ""
                }
            } else {
                TextEditor(text: $bodyText)
                    .font(.footnote.monospaced())
                    .frame(minHeight: 160)
                    .plainTextEntry()
            }
        } header: {
            Text("响应体")
        } footer: {
            Text("最大 256 KB。")
        }
        .listRowBackground(brand.surface)
    }

    private var matchFooter: LocalizedStringKey {
        switch entry.match {
        case .url:
            return "这个 URL，查询参数任意。"
        case .prefix:
            return "以这个 URL 开头的所有请求。"
        case .regex:
            return "匹配整个 URL 的正则，例如 ^https://api\\.example\\.com/v1/.*。主机名用 \\. 写出时会自动对它开启解密。"
        }
    }

    private func commit() {
        var next = entry
        next.pattern = next.pattern.trimmingCharacters(in: .whitespacesAndNewlines)
        next.target = next.target.trimmingCharacters(in: .whitespacesAndNewlines)
        if next.kind == .local {
            var headers: [DebugMappings.Header] = []
            for line in headerText.split(whereSeparator: \.isNewline) {
                let text = line.trimmingCharacters(in: .whitespaces)
                guard !text.isEmpty else {
                    continue
                }
                guard let colon = text.firstIndex(of: ":") else {
                    failure = DebugMappingsError.invalidHeader.message
                    return
                }
                headers.append(.init(
                    name: text[..<colon].trimmingCharacters(in: .whitespaces),
                    value: text[text.index(after: colon)...].trimmingCharacters(in: .whitespaces)
                ))
            }
            next.headers = headers
            if !binaryBody {
                next.body = Data(bodyText.utf8)
            }
        }
        do {
            try save(next)
            dismiss()
        } catch let error as DebugMappingsError {
            failure = error.message
        } catch {
            failure = String(localized: "无法保存映射")
        }
    }
}

/// The extensions page's link to the decrypted domains, with how many the list has. HTTPS decryption is per domain
/// (the user's choice, 2026-11-14): this list, the plugins' `[MITM]` and the mappings' hosts.
struct DecryptHostsSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            NavigationLink {
                DecryptHostsView(tunnel: tunnel)
            } label: {
                LabeledContent {
                    Text(summary)
                } label: {
                    Label("解密域名", systemImage: "globe")
                }
            }
            .listRowBackground(brand.surface)
        } footer: {
            Text("只解密这里、插件和映射里列出的域名，其余 HTTPS 原样转发。也可以在加密连接的详情里点“解密此域名”。")
        }
    }

    private var summary: String {
        let count = tunnel.debugMappings.hosts.count
        return count == 0 ? String(localized: "未添加") : String(localized: "\(count) 个域名")
    }
}

/// The domains decrypted besides the plugins' and the mappings': type one to add it, swipe or the row menu to stop
/// decrypting it. Saved with the mappings (`debug-mappings.json`); a running tunnel reconnects.
struct DecryptHostsView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var typed = ""
    @State private var failure: String?

    private var hosts: [String] {
        tunnel.debugMappings.hosts
    }

    var body: some View {
        List {
            Section {
                HStack(spacing: 10) {
                    TextField("api.example.com 或 *.example.com", text: $typed)
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)
                        #endif
                        .onSubmit(add)
                    Button("添加", action: add)
                        .disabled(typed.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                if let failure {
                    Text(failure)
                        .font(.footnote)
                        .foregroundStyle(brand.danger)
                }
            } footer: {
                Text("写 *.example.com 包含它的所有子域名。需要先信任 HTTPS 解密证书，保存后连接中的隧道会自动重连。")
            }
            .listRowBackground(brand.surface)

            Section {
                if hosts.isEmpty {
                    Text("还没有域名。也可以在加密连接的详情里点“解密此域名”。")
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                }
                ForEach(hosts, id: \.self) { host in
                    Text(verbatim: host)
                        .font(.body.monospaced())
                        .contextMenu {
                            Button(role: .destructive) {
                                commit(tunnel.debugMappings.removingHost(host))
                            } label: {
                                Label("不再解密", systemImage: "lock")
                            }
                        }
                }
                .onDelete { offsets in
                    var next = tunnel.debugMappings
                    for host in offsets.map({ hosts[$0] }) {
                        next = next.removingHost(host)
                    }
                    commit(next)
                }
            }
            .listRowBackground(brand.surface)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("解密域名")
        .inlineNavigationTitle()
    }

    private func add() {
        do {
            let next = try tunnel.debugMappings.addingHost(typed)
            failure = nil
            typed = ""
            commit(next)
        } catch let error as DebugMappingsError {
            failure = error.message
        } catch {
            failure = String(localized: "无法保存映射")
        }
    }

    private func commit(_ next: DebugMappings) {
        guard next != tunnel.debugMappings else {
            return
        }
        Task {
            await tunnel.applyDebugMappings(next)
        }
    }
}

/// Debug mode's first-use checklist (the user's choice, 2026-11-14): what is still missing before HTTPS requests show
/// up, in order. Shown in the request list's empty state until nothing is missing.
struct DebugSetupChecklist: View {
    var tunnel: TunnelController
    let capturing: Bool
    @Environment(\.brand) private var brand
    @State private var trusted: Bool?
    @State private var pluginDomains = 0

    enum Step: CaseIterable {
        case connect, certificate, domains, capture
    }

    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        let pending = pendingSteps
        Group {
            if !pending.isEmpty {
                card(pending)
            }
        }
        .task(id: tunnel.debugMappings) {
            refresh()
        }
        .onChange(of: scenePhase) { _, phase in
            // Trust usually changes in Settings or Keychain Access, away from the app.
            if phase == .active {
                refresh()
            }
        }
    }

    private func card(_ pending: [Step]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("抓 HTTPS 请求前还差 \(pending.count) 步")
                .font(.headline)
                .foregroundStyle(brand.text)
            ForEach(Step.allCases, id: \.self) { step in
                row(step, done: !pending.contains(step))
            }
            Text("明文 HTTP 请求不需要这些步骤，抓包开始后就会列出。")
                .font(.footnote)
                .foregroundStyle(brand.text2)
        }
        .padding(14)
        .frame(maxWidth: 520, alignment: .leading)
        .background(brand.surface, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
    }

    private var pendingSteps: [Step] {
        var steps: [Step] = []
        if tunnel.connection != .connected {
            steps.append(.connect)
        }
        if trusted == false {
            steps.append(.certificate)
        }
        if domainCount == 0 {
            steps.append(.domains)
        }
        if !capturing {
            steps.append(.capture)
        }
        return steps
    }

    /// The list's domains, the active mappings' hosts and the enabled plugins' `[MITM]` hosts.
    private var domainCount: Int {
        let mappings = tunnel.debugMappings
        return mappings.hosts.count + mappings.active.compactMap(\.host).count + pluginDomains
    }

    private func refresh() {
        trusted = (try? MitmCertificateStore.load())?.trustState() == .trusted
        let library = (try? MitmPluginLibrary.load(from: MitmPluginLibrary.defaultURL)) ?? MitmPluginLibrary()
        pluginDomains = library.isEnabled
            ? library.plugins.filter(\.isEnabled).reduce(0) { $0 + $1.summary.hostnames.count }
            : 0
    }

    private func row(_ step: Step, done: Bool) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: done ? "checkmark.circle.fill" : Self.symbol(step))
                .foregroundStyle(done ? brand.success : brand.primary)
                .frame(width: 20)
            VStack(alignment: .leading, spacing: 2) {
                Text(Self.title(step))
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(done ? brand.text2 : brand.text)
                if !done {
                    Text(Self.hint(step))
                        .font(.footnote)
                        .foregroundStyle(brand.text2)
                }
            }
        }
    }

    private static func symbol(_ step: Step) -> String {
        switch step {
        case .connect: "power"
        case .certificate: "lock.shield"
        case .domains: "globe"
        case .capture: "record.circle"
        }
    }

    private static func title(_ step: Step) -> LocalizedStringKey {
        switch step {
        case .connect: "连接"
        case .certificate: "信任根证书"
        case .domains: "添加要解密的域名"
        case .capture: "开始抓包"
        }
    }

    private static func hint(_ step: Step) -> LocalizedStringKey {
        switch step {
        case .connect: "用连接开关连接。调试模式默认直连，不需要节点。"
        case .certificate: "在 改写 › HTTPS 解密证书 生成并信任，只需一次。"
        case .domains: "在 改写 › 解密域名 添加，或在加密连接的详情里点“解密此域名”。"
        case .capture: "开始后，新的请求会逐条出现在这里。"
        }
    }
}
