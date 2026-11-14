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
