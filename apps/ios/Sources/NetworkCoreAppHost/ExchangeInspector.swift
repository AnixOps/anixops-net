import ImageIO
import NetworkCoreBridge
import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

private struct InspectorEmbeddedKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// The page sits in the Mac window's inspector. A navigation title set there replaces the window's own, so
    /// pages shown in the inspector name themselves in their content instead.
    var inspectorEmbedded: Bool {
        get { self[InspectorEmbeddedKey.self] }
        set { self[InspectorEmbeddedKey.self] = newValue }
    }
}

extension View {
    /// The page's navigation title, left out inside the Mac inspector (`inspectorEmbedded`).
    @ViewBuilder
    func pageTitle(_ title: String, embedded: Bool) -> some View {
        if embedded {
            self
        } else {
            navigationTitle(title).inlineNavigationTitle()
        }
    }
}

/// One decrypted request and its response, as the request inspector shows it: a top line (method, status, URL)
/// over four tabs. The request and the response each switch between headers, query and body; a body is shown by
/// what it is (a JSON tree, a picture, form fields, text), with hex always available.
struct HttpExchangeView: View {
    let tunnel: TunnelController
    let exchange: PacketCapture.Exchange
    /// Where the exchange sits in the capture, for exporting it alone.
    let connectionID: UInt64
    let index: Int
    /// The connection is HTTP/2 and its frame headers were kept.
    var hasFrames = false
    /// Every request of the connection in order, for the timeline. This one sits at `index`; while it is under
    /// way the copy here may be older than `exchange`.
    var siblings: [PacketCapture.Exchange] = []
    @Environment(\.brand) private var brand
    @Environment(\.inspectorEmbedded) private var embedded
    /// The tab stays as chosen from one request to the next.
    @AppStorage("capture.inspector.tab") private var tabName = Tab.response.rawValue
    @State private var mapping: MappingDraft?
    /// Cookies, tokens and passwords are hidden in exports unless turned off (plan F5).
    @State private var redact = true

    enum Tab: String, CaseIterable, Identifiable {
        case overview
        case request
        case response
        case timeline

        var id: String {
            rawValue
        }

        var title: LocalizedStringKey {
            switch self {
            case .overview:
                return "概览"
            case .request:
                return "请求"
            case .response:
                return "响应"
            case .timeline:
                return "时间线"
            }
        }
    }

    private var tab: Binding<Tab> {
        Binding(get: { Tab(rawValue: tabName) ?? .response }, set: { tabName = $0.rawValue })
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            Picker(selection: tab) {
                ForEach(Tab.allCases) { item in
                    Text(item.title).tag(item)
                }
            } label: {
                Text("请求的哪一部分")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 16)
            .padding(.bottom, 10)
            Divider()
            switch tab.wrappedValue {
            case .overview:
                overview
            case .request:
                MessagePane(message: exchange.request, query: CaptureBody.queryPairs(exchange.url), isRequest: true)
            case .response:
                if exchange.status != nil {
                    MessagePane(message: exchange.response, query: [], isRequest: false)
                } else {
                    ContentUnavailableView {
                        Label("没有响应", systemImage: "hourglass")
                    } description: {
                        Text(exchange.endedAt == nil ? "响应到达后显示在这里。" : "这个请求结束时没有收到响应。")
                    }
                    .frame(maxHeight: .infinity)
                }
            case .timeline:
                TimelinePane(exchange: exchange, siblings: timeline.all, current: timeline.current)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .background(brand.bg.ignoresSafeArea())
        .pageTitle("\(exchange.method) \(exchange.status.map { String($0) } ?? "")", embedded: embedded)
        .sheet(item: $mapping) { draft in
            MappingEditor(draft: draft) { entry in
                let next = try tunnel.debugMappings.saving(entry)
                try tunnel.checkDebugMappingsBudget(next)
                // Asked while the sheet is still open, so the confirmation shows over it.
                guard await tunnel.allowsDecryption(of: next) else {
                    return false
                }
                Task {
                    await tunnel.applyDebugMappings(next)
                }
                return true
            }
            .formSheetFrame()
            .brandSheet()
            .environment(\.brand, brand)
            .tint(brand.primary)
            .decryptionConsentAlert(tunnel, inSheet: true)
        }
    }

    /// The connection's requests with this one as it is now, and where it sits among them.
    private var timeline: (all: [PacketCapture.Exchange], current: Int) {
        guard siblings.indices.contains(index) else {
            return ([exchange], 0)
        }
        var all = siblings
        all[index] = exchange
        return (all, index)
    }

    /// Method, status and URL: what the request is, whichever tab is open.
    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(exchange.method)
                .font(.footnote.monospaced().weight(.semibold))
                .foregroundStyle(brand.primary)
            if let status = exchange.status {
                Text(String(status))
                    .font(.footnote.monospaced().weight(.semibold))
                    .foregroundStyle(statusColor(status))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 1)
                    .background(statusColor(status).opacity(0.14), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
            }
            Text(exchange.url)
                .font(.footnote.monospaced())
                .foregroundStyle(brand.text)
                .lineLimit(2)
                .truncationMode(.middle)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    /// Green 2xx, amber 3xx, red 4xx and 5xx, as in the request table.
    private func statusColor(_ status: Int) -> Color {
        if status >= 400 {
            return brand.danger
        }
        if status >= 300 {
            return brand.warning
        }
        return brand.success
    }

    private var overview: some View {
        List {
            Section {
                LabeledContent("方法", value: exchange.method)
                LabeledContent("状态", value: exchange.status.map { String($0) } ?? String(localized: "没有响应"))
                LabeledContent("开始", value: CaptureClock.time(exchange.startedAt))
                if let ended = exchange.endedAt {
                    LabeledContent("耗时", value: "\(Int((ended.timeIntervalSince(exchange.startedAt) * 1000).rounded())) ms")
                }
                if exchange.inProgress {
                    LabeledContent("状态", value: String(localized: "进行中"))
                }
                if let type = exchange.response.header("Content-Type") {
                    LabeledContent("类型", value: type)
                }
                if exchange.request.body.size > 0 {
                    LabeledContent("请求体", value: Self.bodySize(exchange.request.body))
                }
                if exchange.response.body.size > 0 {
                    LabeledContent("响应体", value: Self.bodySize(exchange.response.body))
                }
                if let note = Self.noteText(exchange.note) {
                    LabeledContent("说明", value: note)
                }
                if exchange.rulesSkipped {
                    LabeledContent("改写", value: Self.skippedText(exchange.note))
                        .foregroundStyle(brand.warning)
                }
            }
            .listRowBackground(brand.surface)
            if exchange.hasMessages || hasFrames {
                Section {
                    if exchange.hasMessages {
                        NavigationLink {
                            StreamMessagesView(tunnel: tunnel, exchange: exchange, connectionID: connectionID, index: index)
                        } label: {
                            LabeledContent(
                                exchange.status == 101 ? "WebSocket 消息" : "事件",
                                value: String(exchange.messages?.seen ?? 0)
                            )
                        }
                    }
                    if hasFrames {
                        NavigationLink {
                            H2FramesView(tunnel: tunnel, connectionID: connectionID)
                        } label: {
                            Label("HTTP/2 帧", systemImage: "list.bullet.rectangle")
                        }
                    }
                }
                .listRowBackground(brand.surface)
            }
            Section {
                let captured = replay
                NavigationLink {
                    ReplayView(original: captured.replay, complete: captured.complete, sendOnAppear: true)
                } label: {
                    Label("重放", systemImage: "arrow.clockwise")
                }
                .disabled(!captured.complete)
                NavigationLink {
                    ReplayView(original: captured.replay, complete: captured.complete, sendOnAppear: false)
                } label: {
                    Label("编辑后重放", systemImage: "square.and.pencil")
                }
            } footer: {
                Text(replay.complete ? "从本机重新发出这个请求，不带 Cookie 存储，不跟随重定向。" : "请求体没有完整抓到，只能编辑后重放。")
            }
            .listRowBackground(brand.surface)
            Section {
                Toggle("遮盖 Cookie、令牌和密码", isOn: $redact)
                ForEach(PacketCapture.RequestExport.all.filter { $0.language == nil }) { export in
                    exportLink(export)
                }
                Menu {
                    ForEach(PacketCapture.CodeLanguage.allCases) { language in
                        exportLink(.code(language))
                    }
                } label: {
                    Label("导出为代码", systemImage: "chevron.left.forwardslash.chevron.right")
                }
            } header: {
                Text("导出这条请求")
            } footer: {
                Text(redact ? "Cookie、Authorization 和名字含 token、password、session 等的请求头、参数与字段会换成 <redacted>。" : "导出内容包含完整的 Cookie 和令牌，请只发给信任的人。")
            }
            .listRowBackground(brand.surface)
            Section {
                Button {
                    mapping = MappingDraft(entry: mapLocal, isNew: true)
                } label: {
                    Label("映射到本地", systemImage: "doc.text")
                }
                Button {
                    mapping = MappingDraft(entry: DebugMappings.remote(url: exchange.url), isNew: true)
                } label: {
                    Label("映射到远程", systemImage: "arrow.uturn.right")
                }
            } footer: {
                Text("用这次的响应生成 Map Local，或把这个 URL 转到另一个地址。保存后在 扩展 › Map Local / Map Remote 里管理。")
            }
            .listRowBackground(brand.surface)
        }
        .scrollContentBackground(.hidden)
    }

    private func exportLink(_ export: PacketCapture.RequestExport) -> some View {
        let file = CaptureExportFile(tunnel: tunnel, export: export, connection: connectionID, exchange: index, redact: redact)
        return ShareLink(item: file, preview: SharePreview(file.title)) {
            Label(file.title, systemImage: "square.and.arrow.up")
        }
    }

    /// The request as captured, ready to send again.
    private var replay: (replay: RequestReplay, complete: Bool) {
        RequestReplay.captured(
            method: exchange.method,
            url: exchange.url,
            headers: exchange.request.headers.map { (name: $0.name, value: $0.value) },
            body: exchange.request.body.data,
            bodyTruncated: exchange.request.body.truncated
                || UInt64(exchange.request.body.data.count) != exchange.request.body.size
        )
    }

    /// A Map Local entry answering with this response. A cut-off or still-compressed body cannot be replayed as
    /// captured, so it starts empty for the user to fill in.
    private var mapLocal: DebugMappings.Entry {
        let response = exchange.response
        let encoding = response.headers.first { $0.name.lowercased() == "content-encoding" }?.value.lowercased()
        let usable = !response.body.truncated && UInt64(response.body.data.count) == response.body.size
            && (encoding == nil || encoding == "identity")
        return DebugMappings.local(
            url: exchange.url,
            status: exchange.status,
            headers: response.headers.map { .init(name: $0.name, value: $0.value) },
            body: usable ? response.body.data : Data()
        )
    }

    static func bodySize(_ body: PacketCapture.Body) -> String {
        let size = TrafficFormat.bytes(body.size)
        return body.truncated ? String(localized: "\(size)（保留前 \(TrafficFormat.bytes(UInt64(body.data.count)))）") : size
    }

    /// Why a rewrite rule that would have read or changed the body did not run.
    static func skippedText(_ note: String?) -> String {
        note == "too_large"
            ? String(localized: "改写未生效：内容超过 1 MiB")
            : String(localized: "改写未生效：流式响应")
    }

    static func noteText(_ note: String?) -> String? {
        switch note {
        case "answered_locally":
            return String(localized: "由改写规则直接应答")
        case "streaming":
            return String(localized: "流式响应，未保留正文")
        case "too_large":
            return String(localized: "内容过大，之后不再解密")
        default:
            return nil
        }
    }
}

/// The request or the response: its headers, its query parameters (a request with any) or its body.
private struct MessagePane: View {
    let message: PacketCapture.Exchange.Message
    let query: [CaptureBody.Pair]
    let isRequest: Bool
    @Environment(\.brand) private var brand
    /// Kept across requests, and shared by the request and the response: someone reading bodies keeps reading bodies.
    @AppStorage("capture.inspector.part") private var partName = Part.body.rawValue

    enum Part: String {
        case headers
        case query
        case body
    }

    private var part: Part {
        let chosen = Part(rawValue: partName) ?? .body
        return chosen == .query && query.isEmpty ? .headers : chosen
    }

    var body: some View {
        VStack(spacing: 0) {
            Picker(selection: Binding(get: { part }, set: { partName = $0.rawValue })) {
                Text("头 \(message.headers.count)").tag(Part.headers)
                if !query.isEmpty {
                    Text("查询 \(query.count)").tag(Part.query)
                }
                Text("正文").tag(Part.body)
            } label: {
                Text(isRequest ? "请求的部分" : "响应的部分")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            Divider()
            switch part {
            case .headers:
                PairList(pairs: message.headers.map { CaptureBody.Pair(name: $0.name, value: $0.value) })
            case .query:
                PairList(pairs: query)
            case .body:
                if message.body.size > 0 {
                    BodyPane(message: message, fileName: isRequest ? "request-body" : "response-body")
                } else {
                    Text(isRequest ? "这个请求没有正文。" : "这个响应没有正文。")
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        }
    }
}

/// Names over their values: headers, query parameters, form fields.
private struct PairList: View {
    let pairs: [CaptureBody.Pair]
    @Environment(\.brand) private var brand

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(Array(pairs.enumerated()), id: \.offset) { _, pair in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(pair.name)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(brand.text2)
                        Text(pair.value)
                            .font(.footnote.monospaced())
                            .foregroundStyle(brand.text)
                            .textSelection(.enabled)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 6)
                    .contentShape(Rectangle())
                    .contextMenu {
                        Button("拷贝值") {
                            Pasteboard.copy(pair.value)
                        }
                        Button("拷贝名称和值") {
                            Pasteboard.copy("\(pair.name): \(pair.value)")
                        }
                    }
                    Divider()
                }
            }
            .padding(.horizontal, 16)
        }
    }
}

/// A body, shown by what it is. The view chosen (tree, formatted, raw, hex) is remembered and used again wherever
/// the next body offers it. Protobuf and gRPC are decoded without a schema; pictures are drawn from a thumbnail, so a
/// huge one cannot use up memory. The "more" menu exports the body as shown, and on the Mac opens the types that run
/// no code with their app.
private struct BodyPane: View {
    let message: PacketCapture.Exchange.Message
    /// The exported file's name without its extension.
    let fileName: String
    @Environment(\.brand) private var brand
    @AppStorage("capture.inspector.body") private var modeName = ""
    /// The body decompressed, classified and cut into what the views show; worked out once per body, away from the
    /// main thread. Nil while that runs.
    @State private var loaded: Loaded?
    /// The formatted JSON of the body `loaded.id` names; worked out away from the main thread the first time the
    /// formatted view or the copy button needs it.
    @State private var pretty: (id: UUID, text: String, chunks: CodeTextChunks)?
    @State private var collapsed: Set<String> = []
    @State private var selected: String?
    /// JSONPath of the selected tree row, shown under the tree.
    @State private var path: String?
    @State private var copied = false

    struct Loaded: @unchecked Sendable {
        /// Tells one load from the next, so work finished for an earlier body is not shown with this one.
        let id = UUID()
        let data: Data
        let text: String?
        let kind: CaptureBody.Kind
        /// Only the start could be decompressed.
        let cut: Bool
        /// The decoded Protobuf as text, when the body reads as Protobuf.
        let proto: String?
        /// The picture, when the body is one.
        let picture: Picture?
        /// The type the exported file is named for: Content-Type, or the picture's type found from its bytes.
        let fileType: String?
        /// The JSON containers folded when the tree opens.
        let collapsed: Set<String>
        /// `text`, `proto` and the hex dump, cut into pieces for the code views.
        let textChunks: CodeTextChunks?
        let protoChunks: CodeTextChunks?
        let hex: String
        let hexChunks: CodeTextChunks
    }

    enum Picture {
        case drawn(CGImage)
        /// Its header claims more pixels than `CaptureBinary.maxImagePixels`.
        case tooLarge(width: Int, height: Int)
        /// Cut short, or a format the system does not draw.
        case unreadable
    }

    var body: some View {
        VStack(spacing: 0) {
            if let loaded {
                let mode = CaptureBody.chosenMode(loaded.kind, preferred: CaptureBody.Mode(rawValue: modeName))
                tools(loaded, mode)
                Divider()
                content(loaded, mode)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                Divider()
                statusLine(loaded, mode)
            } else {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task(id: message.body.data) {
            // The previous body goes at once, so its text is never shown under this message's headers.
            loaded = nil
            pretty = nil
            selected = nil
            path = nil
            let body = message.body
            let encoding = message.header("Content-Encoding")
            let contentType = message.header("Content-Type")
            // Decompressing (up to 16 MiB), decoding text, parsing JSON and cutting the text for display all run
            // here, off the main thread. The detached task is not cancelled with this one, so a result that arrives
            // after the view moved on to another body is dropped.
            let next = await Task.detached(priority: .userInitiated) {
                Self.load(body, contentEncoding: encoding, contentType: contentType)
            }.value
            guard !Task.isCancelled else {
                return
            }
            collapsed = next.collapsed
            loaded = next
        }
    }

    /// The body decompressed and classified, with the costly parts of showing it: the Protobuf text, the picture,
    /// the folded tree and the text cut for the code views.
    private nonisolated static func load(_ body: PacketCapture.Body, contentEncoding encoding: String?, contentType: String?) -> Loaded {
        // The kernel undoes gzip, deflate, br and zstd; what it cannot is handed to the system frameworks.
        let decoded = CaptureBodyDecoder.decode(body, contentEncoding: encoding)
        let data: Data
        let text: String?
        if let decoded {
            data = decoded.data
            text = PacketCapture.text(decoded.data)
        } else {
            data = PacketCapture.decode(body.data, contentEncoding: encoding) ?? body.data
            text = PacketCapture.text(data)
        }
        let partial = decoded?.truncated == true
        let cut = partial || body.truncated || UInt64(body.data.count) < body.size
        return classify(data: data, text: text, contentType: contentType, cut: cut, partial: partial)
    }

    private nonisolated static func classify(data: Data, text: String?, contentType: String?, cut: Bool, partial: Bool) -> Loaded {
        let kind = CaptureBody.kind(contentType: contentType, text: text, data: data, cut: cut)
        var proto: String?
        var picture: Picture?
        var fileType = contentType
        switch kind {
        case let .protobuf(.message(message), _):
            proto = CaptureBinary.protoText(message, more: labels.more)
        case let .protobuf(.grpc(body), _):
            proto = CaptureBinary.grpcText(body, labels: labels)
        case .image:
            picture = Self.picture(data)
            if !CaptureBody.isImage(contentType) {
                fileType = CaptureBinary.sniffImage(data)
            }
        default:
            break
        }
        var collapsed: Set<String> = []
        if case let .json(root) = kind {
            collapsed = JsonDocument.initiallyCollapsed(root)
        }
        let hex = PacketCapture.hexDump(data)
        return Loaded(
            data: data,
            text: text,
            kind: kind,
            cut: partial,
            proto: proto,
            picture: picture,
            fileType: fileType,
            collapsed: collapsed,
            textChunks: text.map { CodeTextChunks($0) },
            protoChunks: proto.map { CodeTextChunks($0) },
            hex: hex,
            hexChunks: CodeTextChunks(hex)
        )
    }

    /// The formatted JSON of `loaded`, worked out away from the main thread once and kept while the body is shown.
    @MainActor
    private func formatted(_ loaded: Loaded, _ root: JsonNode) async -> String? {
        if let pretty, pretty.id == loaded.id {
            return pretty.text
        }
        let made = await Task.detached(priority: .userInitiated) {
            let text = JsonDocument.pretty(root)
            return (text, CodeTextChunks(text))
        }.value
        guard !Task.isCancelled, self.loaded?.id == loaded.id else {
            return nil
        }
        pretty = (id: loaded.id, text: made.0, chunks: made.1)
        return made.0
    }

    private nonisolated static var labels: CaptureBinary.Labels {
        CaptureBinary.Labels(
            message: { index, size in String(localized: "消息 \(index) · \(size) 字节") },
            compressed: String(localized: "这条消息是压缩的，导出正文后用其他工具解开。"),
            trailers: String(localized: "尾部"),
            undecoded: String(localized: "这条消息不是 Protobuf，见十六进制。"),
            more: String(localized: "…（只解析了开头部分）")
        )
    }

    private static func title(_ mode: CaptureBody.Mode) -> LocalizedStringKey {
        switch mode {
        case .tree:
            return "树"
        case .pretty:
            return "格式化"
        case .preview:
            return "预览"
        case .table:
            return "表格"
        case .protobuf:
            return "Protobuf"
        case .raw:
            return "原文"
        case .hex:
            return "十六进制"
        }
    }

    /// How to look at the body, on the left; what to do with it, on the right.
    private func tools(_ loaded: Loaded, _ mode: CaptureBody.Mode) -> some View {
        HStack(spacing: 10) {
            let modes = CaptureBody.modes(loaded.kind)
            if modes.count > 1 {
                Picker(selection: Binding(get: { mode }, set: { modeName = $0.rawValue })) {
                    ForEach(modes) { item in
                        Text(Self.title(item)).tag(item)
                    }
                } label: {
                    Text("正文的显示方式")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
            }
            Spacer(minLength: 0)
            Button {
                Task { @MainActor in
                    guard let text = await copyText(loaded, mode) else {
                        return
                    }
                    Pasteboard.copy(text)
                    copied = true
                    try? await Task.sleep(for: .seconds(1.2))
                    copied = false
                }
            } label: {
                Image(systemName: copied ? "checkmark" : "doc.on.doc")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(copied ? "已拷贝" : "拷贝正文")
            .help(copied ? "已拷贝" : "拷贝正文")
            Menu {
                let file = CaptureBodyFile(data: loaded.data, contentType: loaded.fileType, name: fileName)
                ShareLink(item: file, preview: SharePreview(file.fileName)) {
                    Label("导出正文", systemImage: "square.and.arrow.up")
                }
                #if os(macOS)
                if CaptureBinary.fileType(loaded.fileType).opens {
                    Button {
                        file.open()
                    } label: {
                        Label("用其他应用打开", systemImage: "arrow.up.forward.app")
                    }
                }
                #endif
                if mode == .tree, case let .json(root) = loaded.kind {
                    Divider()
                    Button("全部展开") {
                        collapsed = []
                    }
                    Button("全部折叠") {
                        collapsed = Set(JsonDocument.containerIDs(root))
                    }
                }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.button)
            .buttonStyle(.borderless)
            .menuIndicator(.hidden)
            .fixedSize()
            .accessibilityLabel("更多")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }

    /// What the copy button copies: the body as the open view shows it. Nil when the body changed meanwhile.
    @MainActor
    private func copyText(_ loaded: Loaded, _ mode: CaptureBody.Mode) async -> String? {
        switch mode {
        case .tree, .pretty:
            if case let .json(root) = loaded.kind {
                return await formatted(loaded, root)
            }
            return loaded.text ?? ""
        case .raw, .table:
            return loaded.text ?? ""
        case .protobuf:
            return loaded.proto ?? ""
        case .preview, .hex:
            return loaded.hex
        }
    }

    @ViewBuilder
    private func content(_ loaded: Loaded, _ mode: CaptureBody.Mode) -> some View {
        switch mode {
        case .tree:
            if case let .json(root) = loaded.kind {
                JsonTree(root: root, rootID: loaded.id, collapsed: $collapsed, selected: $selected) { row in
                    path = row.path
                }
            }
        case .pretty:
            if case let .json(root) = loaded.kind {
                if let pretty, pretty.id == loaded.id {
                    CodeText(chunks: pretty.chunks)
                } else {
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .task(id: loaded.id) {
                            _ = await formatted(loaded, root)
                        }
                }
            }
        case .preview:
            switch loaded.picture {
            case let .drawn(image):
                ScrollView {
                    Image(image, scale: 1, label: Text("图片预览"))
                        .resizable()
                        .scaledToFit()
                        .frame(maxWidth: CGFloat(image.width), maxHeight: CGFloat(image.height))
                        .padding(16)
                        .frame(maxWidth: .infinity)
                }
            case let .tooLarge(width, height):
                CodeText(chunks: loaded.hexChunks, note: "图片太大（\(width) × \(height)），不在这里画出；可以导出后用其他应用查看。")
            case .unreadable, nil:
                // A picture cut at 64 KiB, or a format the system does not draw.
                CodeText(chunks: loaded.hexChunks, note: "这张图片画不出来，下面是它的十六进制。")
            }
        case .table:
            if case let .form(pairs) = loaded.kind {
                PairList(pairs: pairs)
            }
        case .protobuf:
            CodeText(chunks: loaded.protoChunks ?? CodeTextChunks(""))
        case .raw:
            CodeText(chunks: loaded.textChunks ?? CodeTextChunks(""))
        case .hex:
            CodeText(chunks: loaded.hexChunks)
        }
    }

    /// The picture from its header and a thumbnail no larger than `CaptureBinary.previewPixels` on its longer side:
    /// ImageIO reads the size without decoding, and a picture that claims too many pixels is not decoded at all.
    private nonisolated static func picture(_ data: Data) -> Picture {
        let options = [kCGImageSourceShouldCache: false] as CFDictionary
        guard let source = CGImageSourceCreateWithData(data as CFData, options), CGImageSourceGetCount(source) > 0 else {
            return .unreadable
        }
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, options) as? [CFString: Any]
        let width = properties?[kCGImagePropertyPixelWidth] as? Int ?? 0
        let height = properties?[kCGImagePropertyPixelHeight] as? Int ?? 0
        if width.multipliedReportingOverflow(by: height).overflow || width * height > CaptureBinary.maxImagePixels {
            return .tooLarge(width: width, height: height)
        }
        let thumbnail: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: CaptureBinary.previewPixels,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, thumbnail as CFDictionary), image.width > 0, image.height > 0 else {
            return .unreadable
        }
        return .drawn(image)
    }

    /// The selected value's path (click to copy) and what the body is.
    private func statusLine(_ loaded: Loaded, _ mode: CaptureBody.Mode) -> some View {
        HStack(spacing: 8) {
            if mode == .tree, let path {
                Button {
                    Pasteboard.copy(path)
                } label: {
                    Text(path)
                        .font(.caption.monospaced())
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                .buttonStyle(.plain)
                .help("拷贝路径")
                .accessibilityHint("拷贝路径")
            }
            Spacer(minLength: 0)
            if loaded.cut {
                Text("只解压出开头部分")
                    .lineLimit(1)
                    .help("只解压出开头部分（正文没有完整抓到，或解压后超过 16 MiB）")
            }
            Text([message.header("Content-Type"), message.header("Content-Encoding"), HttpExchangeView.bodySize(message.body)].compactMap { $0 }.joined(separator: " · "))
                .lineLimit(1)
        }
        .font(.caption)
        .foregroundStyle(brand.text2)
        .padding(.horizontal, 16)
        .padding(.vertical, 6)
    }
}

/// Monospaced, selectable text that scrolls. The text comes cut into pieces (`CodeTextChunks`, made off the main
/// thread) that a lazy stack lays out as they scroll into view; very long text is cut so the view stays responsive.
private struct CodeText: View {
    let chunks: CodeTextChunks
    var note: LocalizedStringKey?
    @Environment(\.brand) private var brand

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if let note {
                    Text(note)
                        .font(.footnote)
                        .foregroundStyle(brand.text2)
                        .padding(.bottom, 8)
                }
                ForEach(chunks.chunks.indices, id: \.self) { index in
                    Text(verbatim: chunks.chunks[index])
                        .font(.caption.monospaced())
                        .foregroundStyle(brand.text)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                if chunks.cut {
                    Text(verbatim: "…")
                        .font(.caption.monospaced())
                        .foregroundStyle(brand.text2)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
        }
    }
}

/// A JSON document as a tree that folds. Only the visible lines are built. With a keyboard: up and down move, left
/// folds or goes to the parent, right unfolds, Return or Space toggles, and Command-C copies the selected value.
private struct JsonTree: View {
    let root: JsonNode
    /// Names the body `root` came from; the visible lines are worked out again when it or `collapsed` changes.
    let rootID: UUID
    @Binding var collapsed: Set<String>
    @Binding var selected: String?
    let onSelect: (JsonRow) -> Void
    @Environment(\.brand) private var brand
    @FocusState private var focused: Bool
    /// The visible lines, kept between redraws: selecting a line redraws the tree but folds nothing, so walking the
    /// whole document again for each arrow key is not needed.
    @State private var rows: [JsonRow] = []
    @State private var rowsKey: RowsKey?

    private struct RowsKey: Equatable {
        let rootID: UUID
        let collapsed: Set<String>
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(rows) { row in
                        line(row)
                            .id(row.id)
                    }
                }
                .padding(.vertical, 6)
            }
            .focusable()
            .focused($focused)
            .focusEffectDisabled()
            .onKeyPress(.upArrow) {
                move(rows, by: -1, proxy)
            }
            .onKeyPress(.downArrow) {
                move(rows, by: 1, proxy)
            }
            .onKeyPress(.leftArrow) {
                fold(rows, proxy)
            }
            .onKeyPress(.rightArrow) {
                unfold(rows)
            }
            .onKeyPress(.return) {
                toggleSelected(rows)
            }
            .onKeyPress(.space) {
                toggleSelected(rows)
            }
            #if os(macOS)
            .onCopyCommand {
                guard let row = rows.first(where: { $0.id == selected }) else {
                    return []
                }
                return [NSItemProvider(object: JsonDocument.copyText(row.node) as NSString)]
            }
            #endif
        }
        .accessibilityLabel("JSON 树")
        .onAppear(perform: refreshRows)
        .onChange(of: RowsKey(rootID: rootID, collapsed: collapsed)) {
            refreshRows()
        }
    }

    private func refreshRows() {
        let key = RowsKey(rootID: rootID, collapsed: collapsed)
        guard key != rowsKey else {
            return
        }
        rowsKey = key
        rows = JsonDocument.rows(root, collapsed: collapsed)
    }

    private func line(_ row: JsonRow) -> some View {
        HStack(spacing: 4) {
            Group {
                if let open = row.open {
                    Image(systemName: open ? "chevron.down" : "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(brand.text2)
                } else {
                    Color.clear
                }
            }
            .frame(width: 14, height: 14)
            if let name = row.name {
                Text(name)
                    .fontWeight(.medium)
                    .foregroundStyle(brand.text)
                Text(verbatim: ":")
                    .foregroundStyle(brand.text2)
            } else if let index = row.index {
                Text(verbatim: "\(index)")
                    .foregroundStyle(brand.text2)
                Text(verbatim: ":")
                    .foregroundStyle(brand.text2)
            }
            value(row.node)
            Spacer(minLength: 0)
        }
        .font(.caption.monospaced())
        .lineLimit(1)
        .padding(.leading, 12 + CGFloat(row.depth) * 14)
        .padding(.trailing, 12)
        .frame(maxWidth: .infinity, minHeight: 22, alignment: .leading)
        .background(selected == row.id ? brand.soft : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture {
            focused = true
            select(row)
            if row.open != nil {
                toggle(row.id)
            }
        }
        .contextMenu {
            Button("拷贝这个值") {
                Pasteboard.copy(JsonDocument.copyText(row.node))
            }
            Button("拷贝路径") {
                Pasteboard.copy(row.path)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected == row.id ? .isSelected : [])
    }

    /// A scalar in its color, or how many children a container has.
    @ViewBuilder
    private func value(_ node: JsonNode) -> some View {
        switch node {
        case let .object(members):
            Text(verbatim: "{\(members.count)}")
                .foregroundStyle(brand.text2)
        case let .array(items):
            Text(verbatim: "[\(items.count)]")
                .foregroundStyle(brand.text2)
        case .string:
            // One line of a tree is no place for a 60 KiB string; copying gives all of it.
            Text(verbatim: String((JsonDocument.scalarText(node) ?? "").prefix(400)))
                .foregroundStyle(brand.success)
        case .number:
            Text(verbatim: JsonDocument.scalarText(node) ?? "")
                .foregroundStyle(brand.info)
        case .bool, .null:
            Text(verbatim: JsonDocument.scalarText(node) ?? "")
                .foregroundStyle(brand.warning)
        }
    }

    private func select(_ row: JsonRow) {
        selected = row.id
        onSelect(row)
    }

    private func toggle(_ id: String) {
        if collapsed.contains(id) {
            collapsed.remove(id)
        } else {
            collapsed.insert(id)
        }
    }

    private func move(_ rows: [JsonRow], by step: Int, _ proxy: ScrollViewProxy) -> KeyPress.Result {
        guard !rows.isEmpty else {
            return .ignored
        }
        let at = rows.firstIndex { $0.id == selected } ?? (step > 0 ? -1 : rows.count)
        let next = rows[min(max(at + step, 0), rows.count - 1)]
        select(next)
        proxy.scrollTo(next.id)
        return .handled
    }

    private func fold(_ rows: [JsonRow], _ proxy: ScrollViewProxy) -> KeyPress.Result {
        guard let row = rows.first(where: { $0.id == selected }) else {
            return .ignored
        }
        if row.open == true {
            collapsed.insert(row.id)
        } else if let parent = rows.first(where: { $0.id == row.parent }) {
            select(parent)
            proxy.scrollTo(parent.id)
        }
        return .handled
    }

    private func unfold(_ rows: [JsonRow]) -> KeyPress.Result {
        guard let row = rows.first(where: { $0.id == selected }) else {
            return .ignored
        }
        if row.open == false {
            collapsed.remove(row.id)
        }
        return .handled
    }

    private func toggleSelected(_ rows: [JsonRow]) -> KeyPress.Result {
        guard let row = rows.first(where: { $0.id == selected }), row.open != nil else {
            return .ignored
        }
        toggle(row.id)
        return .handled
    }
}

/// When the request ran, and where it sits among the requests of its connection. The engine records each
/// request's start and end only, so there are no DNS, connect or TLS segments, and none are drawn.
private struct TimelinePane: View {
    let exchange: PacketCapture.Exchange
    let siblings: [PacketCapture.Exchange]
    /// Where `exchange` sits in `siblings`.
    let current: Int
    @Environment(\.brand) private var brand

    var body: some View {
        let bars = CaptureBody.waterfall(siblings.map { (start: $0.startedAt, end: $0.endedAt) }, now: Date())
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                fact("开始", CaptureClock.time(exchange.startedAt))
                fact("结束", exchange.endedAt.map { CaptureClock.time($0) } ?? String(localized: "进行中"))
                if let ended = exchange.endedAt {
                    fact("耗时", "\(Int((ended.timeIntervalSince(exchange.startedAt) * 1000).rounded())) ms")
                }
                Text("同一连接上的 \(siblings.count) 个请求")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(brand.text2)
                    .padding(.top, 16)
                    .padding(.bottom, 6)
                ForEach(Array(siblings.enumerated()), id: \.offset) { position, item in
                    row(item, bars[position], current: position == current)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
        }
    }

    private func fact(_ title: LocalizedStringKey, _ value: String) -> some View {
        VStack(spacing: 0) {
            LabeledContent(title, value: value)
                .font(.footnote)
                .padding(.vertical, 7)
            Divider()
        }
    }

    private func row(_ item: PacketCapture.Exchange, _ bar: CaptureBody.Bar, current: Bool) -> some View {
        HStack(spacing: 8) {
            Text(item.method)
                .foregroundStyle(brand.primary)
            Text(URLComponents(string: item.url)?.path ?? item.url)
                .foregroundStyle(brand.text)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)
            GeometryReader { geometry in
                let width = geometry.size.width
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(brand.border)
                        .frame(height: 4)
                    Capsule()
                        .fill(current ? brand.primary : brand.text3)
                        .frame(width: max(2, width * bar.width), height: 6)
                        .offset(x: min(width * bar.left, max(width - 2, 0)))
                }
                .frame(maxHeight: .infinity)
            }
            .frame(width: 96, height: 14)
            Text(verbatim: bar.millis.map { "\($0) ms" } ?? "…")
                .foregroundStyle(brand.text2)
                .frame(width: 64, alignment: .trailing)
        }
        .font(.caption.monospaced())
        .lineLimit(1)
        .padding(.vertical, 4)
        .padding(.horizontal, 6)
        .background(current ? brand.soft : Color.clear, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
    }
}
