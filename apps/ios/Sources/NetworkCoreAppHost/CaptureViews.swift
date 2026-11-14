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
    let format: PacketCapture.ExportFormat
    /// One connection, or everything captured.
    let connection: UInt64?

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(exportedContentType: .data) { file in
            SentTransferredFile(try await file.write())
        }
    }

    func write() async throws -> URL {
        guard let data = await tunnel.control(.exportCapture(format, id: connection)),
              let answer = RuntimeControl.decode(PacketCapture.ExportAnswer.self, from: data)
        else {
            throw CocoaError(.fileWriteUnknown)
        }
        return try PacketCapture.write(answer)
    }

    var title: String {
        switch format {
        case .pcapng:
            return String(localized: "导出 pcapng（Wireshark）")
        case .har:
            return String(localized: "导出 HAR（HTTP 请求）")
        }
    }
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
                        ForEach(Array(capture.http.enumerated()), id: \.offset) { _, exchange in
                            NavigationLink {
                                HttpExchangeView(exchange: exchange)
                            } label: {
                                ExchangeRow(exchange: exchange)
                            }
                        }
                    }
                    .listRowBackground(brand.surface)
                }
                if !capture.chunks.isEmpty {
                    Section {
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
                        if capture.looksLikeTLS {
                            Text("这是加密的 TLS 数据。要看到 HTTPS 明文，请在扩展里为这个域名启用 HTTPS 解密（MITM），并信任证书。")
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
                    .foregroundStyle(brand.text3)
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
                    .foregroundStyle(brand.text3)
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

/// One decrypted request and its response: URL, status, headers and bodies.
private struct HttpExchangeView: View {
    let exchange: PacketCapture.Exchange
    @Environment(\.brand) private var brand

    var body: some View {
        List {
            Section {
                Text(exchange.url)
                    .font(.footnote.monospaced())
                    .textSelection(.enabled)
                LabeledContent("方法", value: exchange.method)
                LabeledContent("状态", value: exchange.status.map { String($0) } ?? String(localized: "没有响应"))
                LabeledContent("开始", value: exchange.startedAt.formatted(date: .omitted, time: .standard))
                if let ended = exchange.endedAt {
                    LabeledContent("耗时", value: "\(Int((ended.timeIntervalSince(exchange.startedAt) * 1000).rounded())) ms")
                }
                if let note = Self.noteText(exchange.note) {
                    LabeledContent("说明", value: note)
                }
            }
            .listRowBackground(brand.surface)
            message("请求", exchange.request)
            if exchange.status != nil {
                message("响应", exchange.response)
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("\(exchange.method) \(exchange.status.map { String($0) } ?? "")")
        .inlineNavigationTitle()
    }

    @ViewBuilder
    private func message(_ title: String, _ message: PacketCapture.Exchange.Message) -> some View {
        Section("\(title)头") {
            ForEach(Array(message.headers.enumerated()), id: \.offset) { _, header in
                VStack(alignment: .leading, spacing: 2) {
                    Text(header.name)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(brand.text2)
                    Text(header.value)
                        .font(.footnote.monospaced())
                        .textSelection(.enabled)
                }
            }
        }
        .listRowBackground(brand.surface)
        if message.body.size > 0 {
            Section {
                NavigationLink {
                    BodyView(title: "\(title)体", message: message)
                } label: {
                    LabeledContent("\(title)体", value: Self.bodySize(message.body))
                }
            }
            .listRowBackground(brand.surface)
        }
    }

    static func bodySize(_ body: PacketCapture.Body) -> String {
        let size = TrafficFormat.bytes(body.size)
        return body.truncated ? String(localized: "\(size)（保留前 \(TrafficFormat.bytes(UInt64(body.data.count)))）") : size
    }

    static func noteText(_ note: String?) -> String? {
        switch note {
        case "answered_locally":
            return String(localized: "由改写规则直接应答")
        case "streaming":
            return String(localized: "流式响应，未保留内容")
        case "too_large":
            return String(localized: "内容过大，之后不再解密")
        default:
            return nil
        }
    }
}

/// A request or response body: text (decompressed when the headers say so) or a hex dump.
private struct BodyView: View {
    let title: String
    let message: PacketCapture.Exchange.Message
    @Environment(\.brand) private var brand

    var body: some View {
        let encoding = message.header("Content-Encoding")
        let text = PacketCapture.text(message.body.data, contentEncoding: encoding)
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text([message.header("Content-Type"), encoding, HttpExchangeView.bodySize(message.body)].compactMap { $0 }.joined(separator: " · "))
                    .font(.footnote)
                    .foregroundStyle(brand.text2)
                BytesText(text: text ?? PacketCapture.hexDump(message.body.data))
            }
            .padding()
        }
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle(title)
        .inlineNavigationTitle()
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
private enum Pasteboard {
    static func copy(_ text: String) {
        #if os(iOS)
        UIPasteboard.general.string = text
        #elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #endif
    }
}
