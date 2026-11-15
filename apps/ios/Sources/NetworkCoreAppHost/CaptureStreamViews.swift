import NetworkCoreBridge
import SwiftUI

/// The server-sent events or WebSocket messages of one exchange, oldest first, read again every second while the
/// exchange is under way. Messages are shown as they went by: decryption only looks, it never changes them.
struct StreamMessagesView: View {
    var tunnel: TunnelController
    let exchange: PacketCapture.Exchange
    let connectionID: UInt64
    let index: Int
    @Environment(\.brand) private var brand
    @State private var log = PacketCapture.StreamMessageLog()
    @State private var inProgress = true
    @State private var gone = false

    private var isWebSocket: Bool {
        exchange.status == 101
    }

    var body: some View {
        List {
            Section {
                if log.messages.isEmpty {
                    Text(gone ? "这个请求已被更新的抓包挤出（抓包内存有上限），或抓包已清空。" : (inProgress ? "等待消息…" : "没有消息"))
                        .foregroundStyle(brand.text2)
                }
                ForEach(log.messages) { message in
                    NavigationLink {
                        StreamMessageDetail(message: message)
                    } label: {
                        StreamMessageRow(message: message)
                    }
                }
            } header: {
                Text(summary)
                    .monospacedDigit()
            } footer: {
                Text(isWebSocket
                    ? "只读查看：消息原样转发，不能改写。每个请求保留最近 512 条、每条前 4 KiB。"
                    : "事件原样逐条转发给 App。每个请求保留最近 512 条、每条前 4 KiB。")
            }
            .listRowBackground(brand.surface)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle(isWebSocket ? "WebSocket 消息" : "事件")
        .inlineNavigationTitle()
        .task {
            inProgress = exchange.inProgress
            if let messages = exchange.messages {
                log.apply(messages)
            }
            while !Task.isCancelled {
                let data = await tunnel.control(.captureExchange(id: connectionID, index: index, sinceMessage: log.nextIndex))
                if let answer = RuntimeControl.decode(PacketCapture.ExchangeAnswer.self, from: data) {
                    guard let current = answer.exchange, current.startedAt == exchange.startedAt, current.url == exchange.url else {
                        gone = true
                        inProgress = false
                        return
                    }
                    if let messages = current.messages {
                        log.apply(messages)
                    }
                    inProgress = current.inProgress
                }
                guard inProgress else {
                    return
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    private var summary: String {
        var parts = [String(localized: "共 \(log.seen) 条")]
        if log.missed > 0 {
            parts.append(String(localized: "较早的 \(log.missed) 条未保留"))
        }
        if inProgress {
            parts.append(String(localized: "进行中"))
        }
        return parts.joined(separator: " · ")
    }
}

/// One message: direction, kind or event name, time and the start of its data.
private struct StreamMessageRow: View {
    let message: PacketCapture.StreamMessage
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Image(systemName: message.isUpload ? "arrow.up" : "arrow.down")
                    .font(.caption.weight(.bold))
                    .foregroundStyle(message.isUpload ? brand.primary : brand.success)
                    .accessibilityLabel(message.isUpload ? "App 发出" : "收到")
                Text(message.title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(brand.text)
                if message.kind != .sse, let label = message.label {
                    Text(label)
                        .font(.caption2)
                        .foregroundStyle(brand.warning)
                }
                Spacer(minLength: 4)
                Text(TrafficFormat.bytes(message.size))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(brand.text3)
                Text(CaptureClock.time(message.at))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(brand.text3)
            }
            if !message.data.isEmpty {
                Text(verbatim: String(message.preview.prefix(200)))
                    .font(.footnote.monospaced())
                    .foregroundStyle(brand.text2)
                    .lineLimit(3)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }
}

/// One message in full (as much as was kept).
private struct StreamMessageDetail: View {
    let message: PacketCapture.StreamMessage
    @Environment(\.brand) private var brand

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text(facts)
                    .font(.footnote)
                    .foregroundStyle(brand.text2)
                Text(verbatim: message.text ?? PacketCapture.hexDump(message.data))
                    .font(.caption.monospaced())
                    .foregroundStyle(brand.text)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
                    .background(brand.surface, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                Button {
                    Pasteboard.copy(message.preview)
                } label: {
                    Label("拷贝", systemImage: "doc.on.doc")
                        .font(.footnote)
                }
            }
            .padding()
        }
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle(message.title)
        .inlineNavigationTitle()
    }

    private var facts: String {
        var parts = [message.isUpload ? String(localized: "App 发出") : String(localized: "收到"), CaptureClock.time(message.at)]
        if let id = message.eventID {
            parts.append("id: \(id)")
        }
        if message.label == "permessage-deflate" {
            parts.append(String(localized: "压缩消息未能解压"))
        }
        let size = TrafficFormat.bytes(message.size)
        parts.append(message.truncated ? String(localized: "\(size)（保留前 \(TrafficFormat.bytes(UInt64(message.data.count)))）") : size)
        return parts.joined(separator: " · ")
    }
}

/// The HTTP/2 frame headers of one decrypted connection on its two links, App ↔ AnixOps and AnixOps ↔ server, oldest
/// first, read again every two seconds. "只看异常" keeps RST_STREAM, GOAWAY and frames with an error code.
struct H2FramesView: View {
    var tunnel: TunnelController
    let connectionID: UInt64
    @Environment(\.brand) private var brand
    @State private var frames: [PacketCapture.H2Frame] = []
    @State private var seen: UInt64 = 0
    @State private var dropped: UInt64 = 0
    @State private var link: PacketCapture.H2Frame.Link?
    @State private var notableOnly = false
    @State private var loaded = false
    @State private var gone = false

    private var shown: [PacketCapture.H2Frame] {
        frames.filter { frame in
            (link == nil || frame.link == link) && (!notableOnly || frame.isNotable)
        }
    }

    var body: some View {
        List {
            Section {
                Picker("链路", selection: $link) {
                    Text("全部").tag(PacketCapture.H2Frame.Link?.none)
                    Text("App ↔ AnixOps").tag(PacketCapture.H2Frame.Link?.some(.app))
                    Text("AnixOps ↔ 服务器").tag(PacketCapture.H2Frame.Link?.some(.origin))
                }
                Toggle("只看 RST_STREAM、GOAWAY 和错误", isOn: $notableOnly)
            }
            .listRowBackground(brand.surface)
            Section {
                if shown.isEmpty {
                    Text(emptyText)
                        .foregroundStyle(brand.text2)
                }
                ForEach(shown) { frame in
                    H2FrameRow(frame: frame)
                }
            } header: {
                Text(summary)
                    .monospacedDigit()
            } footer: {
                Text("只记帧头（类型、标志、流、长度）和错误码、窗口增量等，不保存帧内容。每个连接保留最近 512 帧。")
            }
            .listRowBackground(brand.surface)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("HTTP/2 帧")
        .inlineNavigationTitle()
        .task {
            while !Task.isCancelled {
                await load()
                guard !gone else {
                    return
                }
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    private var emptyText: LocalizedStringKey {
        if gone {
            return "这个连接的抓包内容已被更新的连接挤出（抓包内存有上限），或抓包已清空。"
        }
        return loaded ? "没有符合条件的帧" : "正在读取…"
    }

    private var summary: String {
        var parts = [String(localized: "共 \(seen) 帧")]
        if dropped > 0 {
            parts.append(String(localized: "较早的 \(dropped) 帧未保留"))
        }
        return parts.joined(separator: " · ")
    }

    private func load() async {
        let since = frames.last.map { $0.index + 1 } ?? 0
        let data = await tunnel.control(.captureH2Frames(id: connectionID, since: since))
        loaded = true
        guard let answer = RuntimeControl.decode(PacketCapture.H2FramesAnswer.self, from: data) else {
            return
        }
        guard let kept = answer.frames else {
            gone = frames.isEmpty
            return
        }
        frames.append(contentsOf: kept.frames.filter { $0.index >= since })
        if frames.count > 2000 {
            frames.removeFirst(frames.count - 2000)
        }
        seen = kept.seen
        dropped = kept.seen > UInt64(frames.count) ? kept.seen - UInt64(frames.count) : 0
    }
}

/// One frame: link and direction, type, stream, flags, length and its error or value.
private struct H2FrameRow: View {
    let frame: PacketCapture.H2Frame
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Text(direction)
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(brand.text3)
                Text(frame.type)
                    .font(.caption.weight(.bold).monospaced())
                    .foregroundStyle(frame.isNotable ? brand.danger : brand.text)
                Text(verbatim: "#\(frame.stream)")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(brand.text2)
                if let detail = frame.detail {
                    Text(detail)
                        .font(.caption.monospaced())
                        .foregroundStyle(frame.error != nil ? brand.danger : brand.text2)
                }
                Spacer(minLength: 4)
                Text(CaptureClock.time(frame.at))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(brand.text3)
            }
            Text(verbatim: ([String(localized: "\(frame.length) 字节")] + frame.flags).joined(separator: " · "))
                .font(.caption2.monospaced())
                .foregroundStyle(brand.text3)
        }
        .accessibilityElement(children: .combine)
    }

    /// Which link and which way: the app's link reads App → / ← App, the server's link reads → 服务器 / ← 服务器.
    private var direction: String {
        switch (frame.link, frame.isUpload) {
        case (.app, true):
            return "App →"
        case (.app, false):
            return "→ App"
        case (.origin, true):
            return "→ 服务器"
        case (.origin, false):
            return "服务器 →"
        }
    }
}
