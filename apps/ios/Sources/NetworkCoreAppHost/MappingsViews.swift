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
    /// Whether the enabled plugins and these mappings are over the engine config this version takes
    /// (`TunnelController.decryptionExceedsConfig(with:)`), measured when the mappings change.
    @State private var exceedsConfig = false

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
                VStack(alignment: .leading, spacing: 4) {
                    // Within the 8 MB decryption budget but over the engine config this version takes.
                    if exceedsConfig {
                        Text(TunnelController.decryptionExceedsConfigWarning)
                            .foregroundStyle(brand.warning)
                    }
                    Text("从上到下第一条匹配的生效。需要先在 扩展 › HTTPS 解密证书 中安装并信任根证书，HTTPS 请求才能被映射。Map Remote 用 307 重定向发往新地址，方法和请求体保持不变。")
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .task(id: mappings) {
            let library = (try? MitmPluginLibrary.load(from: MitmPluginLibrary.defaultURL)) ?? MitmPluginLibrary()
            exceedsConfig = tunnel.decryptionExceedsConfig(with: library)
        }
        .navigationTitle("映射")
        .inlineNavigationTitle()
        .sheet(item: $editing) { draft in
            MappingEditor(draft: draft) { entry in
                let next = try mappings.saving(entry)
                try tunnel.checkDebugMappingsBudget(next)
                // Asked while the sheet is still open, so the confirmation shows over it.
                guard await tunnel.allowsDecryption(of: next) else {
                    return false
                }
                commit(next)
                return true
            }
            .formSheetFrame()
            .brandSheet()
            .environment(\.brand, brand)
            .tint(brand.primary)
            .decryptionConsentAlert(tunnel, inSheet: true)
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
                        .foregroundStyle(brand.text2)
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
    /// Saves the entry; false keeps the sheet open (the HTTPS decryption confirmation was cancelled).
    let save: @MainActor (DebugMappings.Entry) async throws -> Bool
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var entry: DebugMappings.Entry
    @State private var headerText: String
    @State private var bodyText: String
    /// The body is not UTF-8 text: it is kept as captured and not shown for editing.
    @State private var binaryBody: Bool
    @State private var failure: String?
    @State private var saving = false

    init(draft: MappingDraft, save: @escaping @MainActor (DebugMappings.Entry) async throws -> Bool) {
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
                    .disabled(saving)
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
        saving = true
        Task { @MainActor in
            defer {
                saving = false
            }
            do {
                if try await save(next) {
                    dismiss()
                }
            } catch let error as DebugMappingsError {
                failure = error.message
            } catch {
                failure = String(localized: "无法保存映射")
            }
        }
    }
}

/// The extensions page's link to the decryption list, with how many lines it has. HTTPS decryption is per domain
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
        let mappings = tunnel.debugMappings
        let decrypted = mappings.hosts.count
        let excluded = mappings.excludedHosts.count
        if decrypted == 0, excluded == 0 {
            return String(localized: "未添加")
        }
        return excluded == 0
            ? String(localized: "\(decrypted) 个域名")
            : String(localized: "\(decrypted) 个域名，\(excluded) 个排除")
    }
}

/// The decryption list for developers and power users: domains and wildcard patterns to decrypt, exclusions that win
/// over every include (plugins' too), each line on or paused, edited in place. While the tunnel runs, the engine's
/// per-domain results (`mitm_hosts`) mark the domains whose decryption failed and why, certificate pinning first,
/// with a one-tap exclusion. The list starts empty: nothing is bundled (public builds carry no presets). Saved with
/// the mappings (`debug-mappings.json`); a running tunnel reconnects.
struct DecryptHostsView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var typed = ""
    @State private var excludeTyped = false
    @State private var failure: String?
    @State private var outcomes: RuntimeControl.MitmHosts?
    @State private var plugins = MitmPluginLibrary()
    @State private var editing: DebugMappings.Domain?

    private var mappings: DebugMappings {
        tunnel.debugMappings
    }

    private var effective: DecryptionDomains {
        DecryptionDomains(mappings: mappings, plugins: plugins)
    }

    /// Patterns that plugins and mappings add, not in the list.
    private var otherPatterns: [String] {
        let listed = Set(mappings.domains.map(\.pattern))
        return effective.patterns.filter { !listed.contains($0) }
    }

    private var otherExclusions: [String] {
        let listed = Set(mappings.domains.map(\.pattern))
        return effective.excluded.filter { !listed.contains($0) }
    }

    var body: some View {
        List {
            addSection
            if let outcomes, !outcomes.failing.isEmpty {
                failureSection(outcomes.failing)
            }
            domainSection(mappings.domains.filter { !$0.exclude }, exclude: false)
            domainSection(mappings.domains.filter(\.exclude), exclude: true)
            if !otherPatterns.isEmpty || !otherExclusions.isEmpty {
                otherSection
            }
            helpSection
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("解密域名")
        .inlineNavigationTitle()
        .sheet(item: $editing) { domain in
            DecryptHostEditor(domain: domain) { text, exclude in
                let next = try mappings.editingHost(domain.pattern, to: text, exclude: exclude)
                // Asked while the sheet is still open, so the confirmation shows over it.
                guard await tunnel.allowsDecryption(of: next) else {
                    return false
                }
                commit(next)
                return true
            }
            .formSheetFrame()
            .brandSheet()
            .environment(\.brand, brand)
            .tint(brand.primary)
            .decryptionConsentAlert(tunnel, inSheet: true)
        }
        .task(id: tunnel.connection) {
            plugins = (try? MitmPluginLibrary.load(from: MitmPluginLibrary.defaultURL)) ?? MitmPluginLibrary()
            outcomes = nil
            while tunnel.connection == .connected, !Task.isCancelled {
                await loadOutcomes()
                try? await Task.sleep(for: .seconds(5))
            }
        }
    }

    // MARK: - Sections

    private var addSection: some View {
        Section {
            HStack(spacing: 10) {
                TextField("api.example.com 或 *.example.com", text: $typed)
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
                    .onSubmit(addTyped)
                Button("添加", action: addTyped)
                    .disabled(typed.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            Picker("类型", selection: $excludeTyped) {
                Text("解密").tag(false)
                Text("排除").tag(true)
            }
            .pickerStyle(.segmented)
            if let failure {
                Text(failure)
                    .font(.footnote)
                    .foregroundStyle(brand.danger)
            }
        } footer: {
            Text("* 匹配任意字符（包括点），? 匹配一个字符；*.example.com 匹配所有子域名，不含 example.com 本身。排除的域名永不解密，优先于这里、插件和映射里的所有解密规则；也可以直接写 -example.com。")
        }
        .listRowBackground(brand.surface)
    }

    private func failureSection(_ failing: [RuntimeControl.MitmHosts.Host]) -> some View {
        Section {
            ForEach(failing) { host in
                HStack(spacing: 10) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(verbatim: host.host)
                            .font(.subheadline.weight(.semibold).monospaced())
                            .foregroundStyle(brand.text)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Text(Self.reason(host.failure))
                            .font(.caption)
                            .foregroundStyle(host.looksPinned ? brand.warning : brand.text2)
                        Text(Self.counts(host))
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(brand.text2)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    if effective.excludes(host.host) {
                        Text("已排除")
                            .font(.caption)
                            .foregroundStyle(brand.text2)
                    } else {
                        Button("排除") {
                            add(host.host, exclude: true)
                        }
                        .buttonStyle(.borderless)
                        .font(.subheadline.weight(.semibold))
                        .accessibilityLabel("排除 \(host.host)")
                    }
                }
            }
            Button("清除这些记录", role: .destructive) {
                Task {
                    _ = await tunnel.control(.clearMitmHosts)
                    await loadOutcomes()
                }
            }
        } header: {
            Text("解密失败")
        } footer: {
            Text("根据本机这次连接以来的解密结果自动标出，只记录域名、次数和原因，断开后清空。App 拒绝证书多半是证书固定：它只信任自己的证书，无法解密。排除后它的连接原样转发，App 会照常联网。")
        }
        .listRowBackground(brand.surface)
    }

    @ViewBuilder
    private func domainSection(_ domains: [DebugMappings.Domain], exclude: Bool) -> some View {
        if !exclude || !domains.isEmpty {
            Section {
                if domains.isEmpty {
                    Text("还没有域名。也可以在加密连接的详情里点“解密此域名”。")
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                }
                ForEach(domains) { domain in
                    row(domain)
                }
                .onDelete { offsets in
                    var next = mappings
                    for domain in offsets.map({ domains[$0] }) {
                        next = next.removingHost(domain.pattern)
                    }
                    commit(next)
                }
            } header: {
                Text(exclude ? "排除" : "解密")
            }
            .listRowBackground(brand.surface)
        }
    }

    private func row(_ domain: DebugMappings.Domain) -> some View {
        HStack(spacing: 10) {
            Button {
                editing = domain
            } label: {
                VStack(alignment: .leading, spacing: 3) {
                    Text(verbatim: domain.pattern)
                        .font(.body.monospaced())
                        .foregroundStyle(domain.enabled ? brand.text : brand.text3)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if let status = status(domain) {
                        Text(status)
                            .font(.caption)
                            .foregroundStyle(brand.text2)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint("编辑")
            Toggle(isOn: Binding(
                get: { domain.enabled },
                set: { commit(mappings.settingHost(domain.pattern, enabled: $0)) }
            )) {
                Text(verbatim: domain.pattern)
            }
            .labelsHidden()
        }
        .contextMenu {
            Button {
                editing = domain
            } label: {
                Label("编辑", systemImage: "pencil")
            }
            Button {
                commit(mappings.settingHost(domain.pattern, enabled: !domain.enabled))
            } label: {
                domain.enabled
                    ? Label("停用", systemImage: "pause.circle")
                    : Label("启用", systemImage: "play.circle")
            }
            Button {
                add(domain.pattern, exclude: !domain.exclude)
            } label: {
                domain.exclude
                    ? Label("改为解密", systemImage: "lock.open")
                    : Label("改为排除", systemImage: "lock")
            }
            Button(role: .destructive) {
                commit(mappings.removingHost(domain.pattern))
            } label: {
                Label("删除", systemImage: "trash")
            }
        }
    }

    private var otherSection: some View {
        Section {
            ForEach(otherPatterns, id: \.self) { pattern in
                Text(verbatim: pattern)
                    .font(.body.monospaced())
                    .foregroundStyle(effective.excludes(pattern) ? brand.text3 : brand.text)
            }
            ForEach(otherExclusions, id: \.self) { pattern in
                Text(verbatim: "-" + pattern)
                    .font(.body.monospaced())
                    .foregroundStyle(brand.text2)
            }
        } header: {
            Text("来自插件和映射")
        } footer: {
            Text("在插件和映射里修改。这里的排除同样作用于它们。")
        }
        .listRowBackground(brand.surface)
    }

    private var helpSection: some View {
        Section {
            VStack(alignment: .leading, spacing: 6) {
                Text("哪些 HTTPS 能解密")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(brand.text)
                Text("域名在列表里（或插件、映射里）、没有被排除，并且已经信任 HTTPS 解密证书。加入后新建的连接才会解密，之前的连接要等 App 重新连接。")
                Text("只解密 443 端口的 HTTPS。列表里域名的 HTTP/3（QUIC）连接会被拒绝，让 App 改用可以解密的 HTTPS。")
                Text("不能解密：固定了证书的 App（常见于银行、支付和系统服务），以及只有 IP 地址、没有域名的连接。")
            }
            .font(.footnote)
            .foregroundStyle(brand.text2)
        }
        .listRowBackground(brand.surface)
    }

    // MARK: - Actions

    /// The field keeps what was typed until the line is saved: a cancelled HTTPS decryption confirmation keeps it.
    private func addTyped() {
        let text = typed
        guard let next = adding(text, exclude: excludeTyped) else {
            return
        }
        guard next != mappings else {
            typed = ""
            return
        }
        Task {
            if await tunnel.applyDebugMappings(next), typed == text {
                typed = ""
            }
        }
    }

    private func add(_ text: String, exclude: Bool) {
        if let next = adding(text, exclude: exclude) {
            commit(next)
        }
    }

    /// The list with `text` added, or nil with `failure` saying why it cannot be.
    private func adding(_ text: String, exclude: Bool) -> DebugMappings? {
        do {
            let next = try mappings.addingHost(text, exclude: exclude)
            failure = nil
            return next
        } catch let error as DebugMappingsError {
            failure = error.message
        } catch {
            failure = String(localized: "无法保存映射")
        }
        return nil
    }

    /// Saves the list; a change that decrypts a new domain asks for the HTTPS decryption confirmation first
    /// (`TunnelController.applyDebugMappings`), and cancelling leaves the list as it was.
    private func commit(_ next: DebugMappings) {
        guard next != mappings else {
            return
        }
        Task {
            await tunnel.applyDebugMappings(next)
        }
    }

    private func loadOutcomes() async {
        let answer = await tunnel.control(.mitmHosts)
        outcomes = RuntimeControl.decode(RuntimeControl.MitmHosts.self, from: answer)
    }

    /// What the engine saw for the domains a line names, while the tunnel runs.
    private func status(_ domain: DebugMappings.Domain) -> String? {
        if !domain.enabled {
            return String(localized: "已停用")
        }
        guard let outcomes, !domain.exclude else {
            return nil
        }
        let hosts = outcomes.hosts.filter { DecryptionDomains.matches(domain.pattern, $0.host) }
        guard !hosts.isEmpty else {
            return nil
        }
        let decrypted = Int(clamping: hosts.reduce(UInt64(0)) { $0 &+ $1.decrypted })
        let failing = hosts.filter(\.failing).count
        if failing > 0 {
            return String(localized: "已解密 \(decrypted) 个连接，\(failing) 个域名失败")
        }
        return String(localized: "已解密 \(decrypted) 个连接")
    }

    static func reason(_ failure: RuntimeControl.MitmHosts.Failure?) -> LocalizedStringKey {
        switch failure {
        case .clientRejectedCertificate:
            "App 拒绝了证书：可能固定了证书，或没有信任根证书"
        case .clientClosed:
            "App 在握手时断开：常见于证书固定"
        case .clientHandshakeFailed:
            "与 App 的 TLS 握手失败"
        case .clientTimeout:
            "App 没有在限定时间内完成握手"
        case .originTls:
            "源站 TLS 握手失败，例如它的证书与域名不符"
        case .originUnreachable:
            "连不上源站，或源站没有及时响应"
        case .other, nil:
            "解密失败"
        }
    }

    static func counts(_ host: RuntimeControl.MitmHosts.Host) -> String {
        String(localized: "失败 \(Int(clamping: host.failed)) 次 · 解密 \(Int(clamping: host.decrypted)) 次")
    }
}

/// Edits one line of the decryption list: the pattern and whether it decrypts or excludes.
struct DecryptHostEditor: View {
    let domain: DebugMappings.Domain
    /// Saves the line; false keeps the sheet open (the HTTPS decryption confirmation was cancelled).
    let save: @MainActor (String, Bool) async throws -> Bool
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand
    @State private var text: String
    @State private var exclude: Bool
    @State private var failure: String?
    @State private var saving = false

    init(domain: DebugMappings.Domain, save: @escaping @MainActor (String, Bool) async throws -> Bool) {
        self.domain = domain
        self.save = save
        _text = State(initialValue: domain.pattern)
        _exclude = State(initialValue: domain.exclude)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("api.example.com 或 *.example.com", text: $text)
                        .font(.body.monospaced())
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)
                        #endif
                        .onSubmit(commit)
                    Picker("类型", selection: $exclude) {
                        Text("解密").tag(false)
                        Text("排除").tag(true)
                    }
                    .pickerStyle(.segmented)
                } footer: {
                    Text("* 匹配任意字符（包括点），? 匹配一个字符。排除的域名永不解密。")
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
            .navigationTitle("编辑域名")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存", action: commit)
                        .disabled(saving)
                }
            }
        }
    }

    private func commit() {
        guard !saving else {
            return
        }
        saving = true
        Task { @MainActor in
            defer {
                saving = false
            }
            do {
                if try await save(text, exclude) {
                    dismiss()
                }
            } catch let error as DebugMappingsError {
                failure = error.message
            } catch {
                failure = String(localized: "无法保存映射")
            }
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
