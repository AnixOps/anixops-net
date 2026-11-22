import CoreTransferable
import NetworkCoreBridge
import SwiftUI
import UniformTypeIdentifiers
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// A capture export the share sheet asks the running engine for when the user picks where it goes, so nothing is
/// read or written until then.
struct CaptureExportFile: Transferable {
    let tunnel: TunnelController
    let command: RuntimeControl
    let title: String

    /// Everything captured, or one connection, as pcapng or HAR.
    init(tunnel: TunnelController, format: PacketCapture.ExportFormat, connection: UInt64?) {
        self.tunnel = tunnel
        command = .exportCapture(format, id: connection)
        switch format {
        case .pcapng:
            title = String(localized: "导出 pcapng（Wireshark）")
        case .har:
            title = String(localized: "导出 HAR（HTTP 请求）")
        }
    }

    /// One request of a connection.
    init(tunnel: TunnelController, export: PacketCapture.RequestExport, connection: UInt64, exchange: Int, redact: Bool) {
        self.tunnel = tunnel
        command = .exportRequest(export, id: connection, exchange: exchange, redact: redact)
        title = Self.title(export)
    }

    static func title(_ export: PacketCapture.RequestExport) -> String {
        switch export {
        case .curl:
            return "cURL"
        case let .code(language):
            return language.title
        case .postman:
            return "Postman"
        case .insomnia:
            return "Insomnia"
        case .har:
            return "HAR"
        }
    }

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(exportedContentType: .data) { file in
            SentTransferredFile(try await file.write())
        }
    }

    func write() async throws -> URL {
        guard let data = await tunnel.control(command),
              let answer = RuntimeControl.decode(PacketCapture.ExportAnswer.self, from: data)
        else {
            throw CocoaError(.fileWriteUnknown)
        }
        return try PacketCapture.write(answer)
    }
}

/// A body as the inspector shows it (decoded), as a file for the share sheet ("导出正文") and, on the Mac, for
/// opening with the app the system uses for its type. The name gets the extension of the body's type.
struct CaptureBodyFile: Transferable {
    let data: Data
    let contentType: String?
    /// Without the extension.
    let name: String

    var fileName: String {
        "\(name).\(CaptureBinary.fileType(contentType).fileExtension)"
    }

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(exportedContentType: .data) { file in
            SentTransferredFile(try file.write())
        }
    }

    /// Writes the body to a folder of its own in the temporary directory, so its name is kept as it is.
    func write() throws -> URL {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("capture-bodies", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent(fileName)
        try data.write(to: url, options: .atomic)
        return url
    }

    #if os(macOS)
    /// Opens the body with its app; only types that run no code where they open (`CaptureBinary.fileType`).
    func open() {
        guard CaptureBinary.fileType(contentType).opens, let url = try? write() else {
            return
        }
        NSWorkspace.shared.open(url)
    }
    #endif
}

/// The "抓包" items of the request list's "more" menu: start or stop, export everything, clear.
struct CaptureMenuItems: View {
    var tunnel: TunnelController
    let status: PacketCapture.Status?
    let onChange: () async -> Void

    var body: some View {
        if let status {
            Button {
                Task {
                    _ = await tunnel.control(.setCapture(!status.enabled))
                    await onChange()
                }
            } label: {
                Label(status.enabled ? LocalizedStringKey("停止抓包") : LocalizedStringKey("开始抓包"), systemImage: status.enabled ? "stop.circle" : "record.circle")
            }
            if status.connections > 0 {
                ForEach(PacketCapture.ExportFormat.allCases) { format in
                    let file = CaptureExportFile(tunnel: tunnel, format: format, connection: nil)
                    ShareLink(item: file, preview: SharePreview(file.title)) {
                        Label(file.title, systemImage: "square.and.arrow.up")
                    }
                }
                Button(role: .destructive) {
                    Task {
                        _ = await tunnel.control(.clearCapture)
                        await onChange()
                    }
                } label: {
                    Label("清空抓包", systemImage: "trash")
                }
            }
        }
    }
}

/// The line under the request list's summary while capture is on.
struct CaptureStatusLine: View {
    let status: PacketCapture.Status
    @Environment(\.brand) private var brand

    var body: some View {
        Label {
            Text("抓包中 · \(status.connections) 个连接 · \(TrafficFormat.bytes(status.usedBytes)) / \(TrafficFormat.bytes(status.maxTotalBytes))")
                .monospacedDigit()
        } icon: {
            Image(systemName: "record.circle")
                .foregroundStyle(brand.danger)
        }
    }
}

/// What capture kept of one connection: the decrypted HTTP requests (HTTPS decryption) and the raw bytes each way,
/// with exports of this connection alone. Refreshed every two seconds while the connection is open.
struct CaptureDetailView: View {
    var tunnel: TunnelController
    let connection: RuntimeControl.Connection
    @Environment(\.brand) private var brand
    @State private var capture: PacketCapture.Connection?
    @State private var loaded = false
    @State private var unreachable = false
    @State private var decryptFailure: String?

    /// The connection's domain while the decrypted-domain list does not cover it; nil for an address or a covered one.
    private var undecryptedHost: String? {
        guard let host = connection.host.flatMap(DebugMappings.normalizeHost), !tunnel.debugMappings.decrypts(host),
              !tunnel.debugMappings.excludes(host)
        else {
            return nil
        }
        return host
    }

    private var encryptedNote: LocalizedStringKey {
        guard let host = connection.host.flatMap(DebugMappings.normalizeHost) else {
            return "这是加密的 TLS 数据。目标是 IP 地址，没有域名可以解密。"
        }
        if tunnel.debugMappings.excludes(host) {
            return "这是加密的 TLS 数据。这个域名在解密列表里被排除，不会解密。"
        }
        if tunnel.debugMappings.decrypts(host) {
            return "这是加密的 TLS 数据。这个域名已在解密列表里，新的连接会被解密；请确认已信任 HTTPS 解密证书。"
        }
        return "这是加密的 TLS 数据。点“解密此域名”后，新的连接会被解密（需要先信任 HTTPS 解密证书）。"
    }

    /// Adds the connection's domain to the decrypted-domain list; a running tunnel reconnects.
    private func decrypt(_ host: String) {
        let next: DebugMappings
        do {
            next = try tunnel.debugMappings.addingHost(host)
        } catch {
            decryptFailure = (error as? DebugMappingsError)?.message ?? String(localized: "无法保存映射")
            return
        }
        decryptFailure = nil
        Task {
            await tunnel.applyDebugMappings(next)
        }
    }

    var body: some View {
        List {
            if let capture {
                Section {
                    LabeledContent("上传", value: Self.totals(capture.upload))
                    LabeledContent("下载", value: Self.totals(capture.download))
                    if capture.decrypted {
                        LabeledContent("内容", value: String(localized: "HTTPS 已解密（MITM）"))
                    }
                } footer: {
                    Text("每个方向保留前 64 KiB。")
                }
                .listRowBackground(brand.surface)
                if !capture.http.isEmpty {
                    Section("HTTP 请求") {
                        ForEach(Array(capture.http.enumerated()), id: \.offset) { index, exchange in
                            NavigationLink {
                                HttpExchangeView(
                                    tunnel: tunnel,
                                    exchange: exchange,
                                    connectionID: connection.id,
                                    index: index,
                                    hasFrames: (capture.h2Frames?.seen ?? 0) > 0,
                                    siblings: capture.http
                                )
                            } label: {
                                ExchangeRow(exchange: exchange)
                            }
                        }
                    }
                    .listRowBackground(brand.surface)
                }
                if let frames = capture.h2Frames, frames.seen > 0 {
                    Section {
                        NavigationLink {
                            H2FramesView(tunnel: tunnel, connectionID: connection.id)
                        } label: {
                            LabeledContent("HTTP/2 帧", value: String(frames.seen))
                        }
                    } footer: {
                        Text("App 与 AnixOps、AnixOps 与服务器两段的帧头，用来看流控、RST_STREAM 和 GOAWAY。")
                    }
                    .listRowBackground(brand.surface)
                }
                if !capture.chunks.isEmpty {
                    Section {
                        if capture.looksLikeTLS, let host = undecryptedHost {
                            Button {
                                decrypt(host)
                            } label: {
                                Label("解密此域名", systemImage: "lock.open")
                            }
                        }
                        ForEach(Array(capture.chunks.enumerated()), id: \.offset) { _, chunk in
                            NavigationLink {
                                ChunkView(chunk: chunk, network: capture.network)
                            } label: {
                                ChunkRow(chunk: chunk)
                            }
                        }
                    } header: {
                        Text(capture.network == "udp" ? "数据报" : "数据")
                    } footer: {
                        if let decryptFailure {
                            Text(decryptFailure)
                                .foregroundStyle(brand.danger)
                        } else if capture.looksLikeTLS {
                            Text(encryptedNote)
                        }
                    }
                    .listRowBackground(brand.surface)
                }
                if capture.http.isEmpty, capture.chunks.isEmpty {
                    Text("还没有数据")
                        .foregroundStyle(brand.text2)
                        .listRowBackground(brand.surface)
                }
                Section {
                    ForEach(PacketCapture.ExportFormat.allCases) { format in
                        if format == .pcapng || !capture.http.isEmpty {
                            let file = CaptureExportFile(tunnel: tunnel, format: format, connection: connection.id)
                            ShareLink(item: file, preview: SharePreview(file.title)) {
                                Label(file.title, systemImage: "square.and.arrow.up")
                            }
                        }
                    }
                } footer: {
                    Text("pcapng 可用 Wireshark 打开，HAR 可导入浏览器开发者工具、Charles 或 Proxyman。导出内容可能包含账号、Cookie 等敏感信息，请只发给信任的人。")
                }
                .listRowBackground(brand.surface)
            } else {
                Text(emptyText)
                    .foregroundStyle(brand.text2)
                    .listRowBackground(brand.surface)
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("抓包内容")
        .inlineNavigationTitle()
        .task {
            while !Task.isCancelled {
                await load()
                guard connection.isActive, !unreachable else {
                    return
                }
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    private var emptyText: LocalizedStringKey {
        if unreachable {
            return "隧道已断开，抓包内容随隧道一起清空。"
        }
        if !loaded {
            return "正在读取…"
        }
        return "这个连接的抓包内容已被更新的连接挤出（抓包内存有上限），或抓包已清空。"
    }

    private func load() async {
        guard let data = await tunnel.control(.captureConnection(connection.id)) else {
            unreachable = capture == nil
            loaded = true
            return
        }
        if let answer = RuntimeControl.decode(PacketCapture.ConnectionAnswer.self, from: data) {
            capture = answer.capture
        }
        loaded = true
    }

    static func totals(_ totals: PacketCapture.DirectionTotals) -> String {
        let kept = TrafficFormat.bytes(totals.captured)
        let seen = TrafficFormat.bytes(totals.seen)
        return totals.truncated ? String(localized: "已保留 \(kept) / 共 \(seen)") : kept
    }
}

private struct ExchangeRow: View {
    let exchange: PacketCapture.Exchange
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(exchange.method)
                    .font(.caption.weight(.bold))
                    .foregroundStyle(brand.primary)
                Text(exchange.status.map { String($0) } ?? "—")
                    .font(.caption.weight(.semibold).monospacedDigit())
                    .foregroundStyle((exchange.status ?? 0) >= 400 ? brand.danger : brand.text2)
                Spacer(minLength: 4)
                Text(TrafficFormat.bytes(exchange.response.body.size))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(brand.text2)
            }
            Text(exchange.url)
                .font(.footnote)
                .foregroundStyle(brand.text)
                .lineLimit(2)
                .truncationMode(.middle)
        }
        .padding(.vertical, 2)
    }
}

private struct ChunkRow: View {
    let chunk: PacketCapture.Chunk
    @Environment(\.brand) private var brand

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: chunk.isUpload ? "arrow.up" : "arrow.down")
                .font(.footnote.weight(.bold))
                .foregroundStyle(chunk.isUpload ? brand.primary : brand.text2)
                .accessibilityLabel(chunk.isUpload ? "发出" : "收到")
            VStack(alignment: .leading, spacing: 2) {
                Text(preview)
                    .font(.caption.monospaced())
                    .foregroundStyle(brand.text)
                    .lineLimit(2)
                Text("\(TrafficFormat.bytes(UInt64(chunk.data.count))) · \(chunk.at.formatted(date: .omitted, time: .standard))")
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(brand.text2)
            }
        }
    }

    private var preview: String {
        let head = chunk.data.prefix(160)
        if let text = PacketCapture.text(Data(head)) {
            return text.replacingOccurrences(of: "\r\n", with: " ⏎ ")
        }
        return head.prefix(24).map { String(format: "%02x", $0) }.joined(separator: " ")
    }
}

/// One chunk's bytes as text or a hex dump.
private struct ChunkView: View {
    enum Style: Hashable {
        case text
        case hex
    }

    let chunk: PacketCapture.Chunk
    let network: String
    @Environment(\.brand) private var brand
    @State private var style = Style.text

    var body: some View {
        let text = PacketCapture.text(chunk.data)
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Picker("显示", selection: $style) {
                    Text("文本").tag(Style.text)
                    Text("十六进制").tag(Style.hex)
                }
                .pickerStyle(.segmented)
                Text("\(chunk.isUpload ? String(localized: "发出") : String(localized: "收到")) \(network.uppercased()) · \(TrafficFormat.bytes(UInt64(chunk.data.count))) · \(chunk.at.formatted(date: .omitted, time: .standard))")
                    .font(.footnote)
                    .foregroundStyle(brand.text2)
                BytesText(text: style == .text ? (text ?? String(localized: "不是文本，请切换到十六进制查看。")) : PacketCapture.hexDump(chunk.data))
            }
            .padding()
        }
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle(chunk.isUpload ? "发出的数据" : "收到的数据")
        .inlineNavigationTitle()
        .onAppear {
            if text == nil {
                style = .hex
            }
        }
    }
}

/// Monospaced, selectable text with a copy button. Very long text is cut so the view stays responsive.
private struct BytesText: View {
    let text: String
    @Environment(\.brand) private var brand

    var body: some View {
        let shown = text.count > 200_000 ? String(text.prefix(200_000)) + "\n…" : text
        VStack(alignment: .leading, spacing: 8) {
            Button {
                Pasteboard.copy(text)
            } label: {
                Label("拷贝", systemImage: "doc.on.doc")
                    .font(.footnote)
            }
            Text(shown)
                .font(.caption.monospaced())
                .foregroundStyle(brand.text)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(10)
                .background(brand.surface, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
    }
}

/// The system pasteboard on iOS and macOS.
enum Pasteboard {
    static func copy(_ text: String) {
        #if os(iOS)
        UIPasteboard.general.string = text
        #elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #endif
    }
}

/// One row of the HTTP request list opened: reads the exchange by its place in the capture and shows it. While it
/// is under way (a slow response, an event stream, a WebSocket) it is read again every second.
struct CapturedRequestView: View {
    var tunnel: TunnelController
    let row: PacketCapture.RequestRow
    @Environment(\.brand) private var brand
    @Environment(\.inspectorEmbedded) private var embedded
    @State private var exchange: PacketCapture.Exchange?
    @State private var hasFrames = false
    /// Every request of the row's connection as first read, for the timeline.
    @State private var siblings: [PacketCapture.Exchange] = []
    @State private var loaded = false

    var body: some View {
        Group {
            if let exchange {
                HttpExchangeView(tunnel: tunnel, exchange: exchange, connectionID: row.connectionID, index: row.index, hasFrames: hasFrames, siblings: siblings)
            } else {
                List {
                    Text(loaded ? "这个请求已被更新的抓包挤出（抓包内存有上限），或抓包已清空。" : "正在读取…")
                        .foregroundStyle(brand.text2)
                        .listRowBackground(brand.surface)
                }
                .scrollContentBackground(.hidden)
                .background(brand.bg.ignoresSafeArea())
                .pageTitle("\(row.method) \(row.status.map { String($0) } ?? "")", embedded: embedded)
            }
        }
        .task {
            let data = await tunnel.control(.captureConnection(row.connectionID))
            if let capture = RuntimeControl.decode(PacketCapture.ConnectionAnswer.self, from: data)?.capture {
                hasFrames = (capture.h2Frames?.seen ?? 0) > 0
                exchange = Self.find(row, in: capture.http)
                siblings = capture.http
            }
            loaded = true
            // Messages are read on their own page; here only the head, status and bodies change.
            while !Task.isCancelled, exchange?.inProgress == true {
                try? await Task.sleep(for: .seconds(1))
                let data = await tunnel.control(.captureExchange(id: row.connectionID, index: row.index, sinceMessage: UInt64.max))
                guard let answer = RuntimeControl.decode(PacketCapture.ExchangeAnswer.self, from: data) else {
                    continue
                }
                exchange = answer.exchange.flatMap { Self.matches(row, $0) ? $0 : nil }
            }
        }
    }

    /// The row's exchange: by `seq` once it ended, else at its index if it is the same request.
    static func find(_ row: PacketCapture.RequestRow, in http: [PacketCapture.Exchange]) -> PacketCapture.Exchange? {
        if row.seq != 0, let found = http.first(where: { $0.seq == row.seq }) {
            return found
        }
        guard http.indices.contains(row.index), matches(row, http[row.index]) else {
            return nil
        }
        return http[row.index]
    }

    private static func matches(_ row: PacketCapture.RequestRow, _ exchange: PacketCapture.Exchange) -> Bool {
        exchange.url == row.url && exchange.method == row.method && exchange.startedAt == row.startedAt
    }
}

/// The captured connections without a request, one row per domain or address: how many, how much, and either a
/// "解密" button or why there is none. A row opens the newest connection's captured bytes.
struct CaptureHostGroupRows: View {
    var tunnel: TunnelController
    let groups: [CaptureHostGroup]
    let domains: DecryptionDomains
    /// The engine's decryption results by domain (`mitm_hosts`); empty from an engine before it.
    var results: [String: RuntimeControl.MitmHosts.Host] = [:]
    let decrypt: (String) -> Void
    @Environment(\.brand) private var brand

    var body: some View {
        ForEach(groups) { group in
            HStack(spacing: 10) {
                NavigationLink {
                    if let connection = group.connections.first {
                        CaptureDetailView(tunnel: tunnel, connection: connection)
                    }
                } label: {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(verbatim: group.name)
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(brand.text)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Text(Self.summary(group))
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(brand.text2)
                        Text(status(group))
                            .font(.caption)
                            .foregroundStyle(brand.text2)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                if group.isDomain, !domains.covers(group.name), !domains.excludes(group.name) {
                    Button("解密") {
                        decrypt(group.name)
                    }
                    .buttonStyle(.borderless)
                    .font(.subheadline.weight(.semibold))
                    .accessibilityLabel("解密 \(group.name)")
                }
            }
            .listRowBackground(brand.surface)
        }
    }

    static func summary(_ group: CaptureHostGroup) -> String {
        let networks = group.networks.map { $0.uppercased() }.joined(separator: "/")
        return String(localized: "\(group.connections.count) 个连接 · \(TrafficFormat.bytes(group.bytes)) · \(networks)")
    }

    private func status(_ group: CaptureHostGroup) -> LocalizedStringKey {
        if !group.isDomain {
            return "只有 IP 地址，没有域名可以解密"
        }
        if let result = results[group.name], result.failing {
            return DecryptHostsView.reason(result.failure)
        }
        if domains.excludes(group.name) {
            return "已排除，不解密"
        }
        if domains.covers(group.name) {
            // Listed, yet nothing decrypted: the connections began before the domain was added, the app pins its
            // certificate, the traffic is not HTTPS on port 443, or capture memory pushed the requests out.
            return "已在解密列表，但没有解密出请求：可能是加入前建立的连接、App 固定了证书，或不是 443 端口的 HTTPS"
        }
        return "未解密，只抓到加密数据"
    }
}

/// Every captured connection without a request, by domain or address (the request list's "查看域名").
struct CaptureHostGroupsView: View {
    var tunnel: TunnelController
    let groups: [CaptureHostGroup]
    let domains: DecryptionDomains
    var results: [String: RuntimeControl.MitmHosts.Host] = [:]
    let decrypt: (String) -> Void
    @Environment(\.brand) private var brand

    var body: some View {
        List {
            Section {
                if groups.isEmpty {
                    Text("所有抓到的连接都解密出了请求。")
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                        .listRowBackground(brand.surface)
                }
                CaptureHostGroupRows(tunnel: tunnel, groups: groups, domains: domains, results: results, decrypt: decrypt)
            } footer: {
                Text("这些连接没有解密，只抓到了加密的 TLS 数据，点进去可以看原始字节。点“解密”把域名加入解密列表，隧道会重新连接，之后的新连接会被解密并列在请求里。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("没有请求的连接")
        .inlineNavigationTitle()
    }
}
