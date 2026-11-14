import NetworkCoreBridge
import SwiftUI

/// Debug mode's request list (plan E5): one row per decrypted HTTP request, newest first. Quick filter chips and
/// the search box build one filter expression the engine parses (`CaptureFilter`), so a chip and the same words typed
/// do the same thing. Rows arrive a second at a time while capture is on; the connection list sits behind the
/// toolbar's "连接" button.
struct CaptureRequestsView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var log = CaptureRequestLog()
    @State private var query = ""
    @State private var chips: [CaptureQuickFilter] = []
    @State private var paused = false
    @State private var loaded = false
    /// Nil until read, or when the engine has no capture.
    @State private var capture: PacketCapture.Status?
    /// The engine did not understand the filter; the character it stopped at in the typed text, when known.
    @State private var invalidFilter: Int??
    /// The engine answers no `capture_requests` (a kernel before it).
    @State private var unsupported = false
    @State private var showsSessions = false
    /// A line under the summary after saving a session, or why it failed.
    @State private var notice: String?

    private var filter: String {
        CaptureFilter.compose(chips: chips, text: query)
    }

    private struct LoadKey: Equatable {
        let connected: Bool
        let filter: String
    }

    var body: some View {
        List {
            Section {
                if log.rows.isEmpty {
                    emptyRow
                }
                ForEach(log.rows.reversed()) { row in
                    NavigationLink {
                        CapturedRequestView(tunnel: tunnel, row: row)
                    } label: {
                        CaptureRequestRow(row: row)
                    }
                    .listRowBackground(brand.surface)
                }
            } header: {
                VStack(alignment: .leading, spacing: 4) {
                    Text(summary)
                        .monospacedDigit()
                    if let capture, capture.enabled {
                        CaptureStatusLine(status: capture)
                    }
                    if let notice {
                        Text(notice)
                    }
                    if let invalidFilter {
                        Text(invalidText(invalidFilter))
                            .foregroundStyle(brand.danger)
                    }
                }
            } footer: {
                Text("只列出 HTTPS 解密（MITM）后的请求和明文 HTTP 请求。搜索框可以直接写表达式：host:api.example.com status:>=400 type:json -method:GET，空格表示同时满足，OR 表示或者。")
            }
        }
        .scrollContentBackground(.hidden)
        .topControlBar(brand) {
            chipBar
        }
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("请求")
        .inlineNavigationTitle()
        .searchable(text: $query, prompt: Text("搜索或表达式，如 status:4xx"))
        .autocorrectionDisabled()
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    paused.toggle()
                } label: {
                    Image(systemName: paused ? "play.fill" : "pause.fill")
                }
                .accessibilityLabel(paused ? "继续刷新" : "暂停刷新")
                NavigationLink {
                    ConnectionsView(tunnel: tunnel)
                } label: {
                    Image(systemName: "point.3.connected.trianglepath.dotted")
                }
                .accessibilityLabel("全部连接")
                Menu {
                    if tunnel.connection == .connected {
                        CaptureMenuItems(tunnel: tunnel, status: capture) {
                            log.reset()
                            await load()
                        }
                        Divider()
                        Button {
                            Task {
                                await saveSession()
                            }
                        } label: {
                            Label("保存当前抓包", systemImage: "lock.doc")
                        }
                        .disabled((capture?.connections ?? 0) == 0)
                        Button {
                            showsSessions = true
                        } label: {
                            Label("已保存的会话", systemImage: "clock.arrow.circlepath")
                        }
                    }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .accessibilityLabel("更多")
                .disabled(tunnel.connection != .connected)
            }
        }
        .navigationDestination(isPresented: $showsSessions) {
            CaptureSessionsView(tunnel: tunnel) {
                log.reset()
                notice = String(localized: "正在查看已保存的会话，抓包已停止")
            }
        }
        .task(id: LoadKey(connected: tunnel.connection == .connected, filter: filter)) {
            // A new filter starts over; the short wait lets typing settle before asking the engine.
            try? await Task.sleep(for: .milliseconds(300))
            log.reset()
            invalidFilter = nil
            while !Task.isCancelled {
                if !paused {
                    await load()
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    private var chipBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(CaptureQuickFilter.allCases) { chip in
                    let selected = chips.contains(chip)
                    Button {
                        if selected {
                            chips.removeAll { $0 == chip }
                        } else {
                            chips.append(chip)
                        }
                    } label: {
                        Text(Self.title(chip))
                            .font(.footnote.weight(.semibold))
                            .padding(.horizontal, 12)
                            .frame(minHeight: 32)
                            .foregroundStyle(selected ? brand.onPrimary : brand.text)
                            .background(selected ? brand.primary : brand.surface, in: Capsule())
                            .overlay(Capsule().strokeBorder(selected ? brand.primary : brand.border, lineWidth: 1))
                            .contentShape(Capsule())
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(selected ? .isSelected : [])
                    .accessibilityHint(Text(verbatim: chip.expression))
                }
            }
            .padding(.vertical, 1)
            .padding(.horizontal, 16)
        }
        .padding(.top, 4)
        .padding(.bottom, 8)
    }

    static func title(_ chip: CaptureQuickFilter) -> String {
        switch chip {
        case .errors:
            return String(localized: "出错")
        case .json:
            return "JSON"
        case .images:
            return String(localized: "图片")
        case .writes:
            return String(localized: "非 GET")
        case .http2:
            return "HTTP/2"
        case .large:
            return String(localized: "大于 100 KB")
        case .mapped:
            return String(localized: "本地应答")
        }
    }

    @ViewBuilder
    private var emptyRow: some View {
        VStack(spacing: 12) {
            Text(emptyText)
                .font(.subheadline)
                .foregroundStyle(brand.text2)
                .multilineTextAlignment(.center)
            if tunnel.connection == .connected, let capture, !capture.enabled {
                Button {
                    Task {
                        _ = await tunnel.control(.setCapture(true))
                        await load()
                    }
                } label: {
                    Text("开始抓包")
                        .font(.headline)
                        .frame(minWidth: 160, minHeight: 44)
                }
                .buttonStyle(.brandPrimary)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 24)
        .listRowBackground(Color.clear)
    }

    private var emptyText: LocalizedStringKey {
        if tunnel.connection != .connected {
            return "连接后，经过隧道的 HTTP 请求会逐条列在这里。"
        }
        if unsupported {
            return "当前内核不支持请求列表，请在右上角查看全部连接。"
        }
        if !loaded {
            return "正在读取…"
        }
        if let capture, !capture.enabled {
            return "抓包未开启。开始抓包后，新的请求会出现在这里。"
        }
        if invalidFilter != nil {
            return "表达式有误，改正后再筛选。"
        }
        if !filter.isEmpty {
            return "没有匹配的请求"
        }
        return "还没有请求。要看到 HTTPS 请求，请在 扩展 › HTTPS 解密证书 中安装并信任根证书，再为域名启用解密。"
    }

    private var summary: String {
        let count = String(localized: "\(log.rows.count) 个请求")
        return paused ? count + String(localized: " · 已暂停刷新") : count
    }

    private func invalidText(_ at: Int?) -> String {
        if let at {
            return String(localized: "表达式第 \(at + 1) 个字符附近有误")
        }
        return String(localized: "表达式有误")
    }

    private func saveSession() async {
        let answer = RuntimeControl.decode(PacketCapture.SavedAnswer.self, from: await tunnel.control(.saveCapture(name: nil)))
        if let session = answer?.session {
            notice = String(localized: "已加密保存：\(CaptureSessionsView.title(session))，7 天后自动删除")
        } else {
            notice = String(localized: "无法保存会话")
        }
    }

    private func load() async {
        guard tunnel.connection == .connected else {
            loaded = false
            return
        }
        capture = RuntimeControl.decode(PacketCapture.Status.self, from: await tunnel.control(.captureStatus))
        // Read up to a few pages at once, so a burst of requests shows up without waiting a second per page.
        for _ in 0..<4 {
            let request = RuntimeControl.captureRequests(sinceSeq: log.nextSeq, limit: CaptureRequestLog.pageSize, filter: filter)
            guard let data = await tunnel.control(request) else {
                return
            }
            guard let answer = RuntimeControl.decode(PacketCapture.RequestsAnswer.self, from: data) else {
                unsupported = true
                loaded = true
                return
            }
            unsupported = false
            loaded = true
            if let at = answer.invalidFilterAt {
                invalidFilter = .some(CaptureFilter.textPosition(of: at, chips: chips, text: query))
                return
            }
            invalidFilter = nil
            guard log.apply(answer) else {
                return
            }
        }
    }
}

/// One HTTP request: method and status, host and path, the response's kind, size and time.
struct CaptureRequestRow: View {
    let row: PacketCapture.RequestRow
    @Environment(\.brand) private var brand

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(row.method)
                        .font(.caption.weight(.bold))
                        .foregroundStyle(brand.primary)
                    Text(row.status.map { String($0) } ?? "—")
                        .font(.caption.weight(.semibold).monospacedDigit())
                        .foregroundStyle(statusColor)
                    Text(row.host)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(brand.text)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Text(verbatim: row.path)
                    .font(.footnote.monospaced())
                    .foregroundStyle(brand.text2)
                    .lineLimit(1)
                    .truncationMode(.middle)
                HStack(spacing: 4) {
                    if let kind = row.kind {
                        tag(kind)
                    }
                    if row.isHTTP2 {
                        tag("H2")
                    }
                    if row.note == "answered_locally" {
                        tag(String(localized: "本地应答"))
                    }
                    Text(row.startedAt.formatted(date: .omitted, time: .standard))
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(brand.text3)
                }
            }
            Spacer(minLength: 4)
            VStack(alignment: .trailing, spacing: 3) {
                Text(TrafficFormat.bytes(row.responseSize))
                    .font(.footnote.weight(.semibold).monospacedDigit())
                    .foregroundStyle(brand.text)
                if let duration = row.duration {
                    Text("\(Int((duration * 1000).rounded())) ms")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(brand.text3)
                }
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }

    private var statusColor: Color {
        guard let status = row.status else {
            return brand.danger
        }
        if status >= 400 {
            return brand.danger
        }
        if status >= 300 {
            return brand.warning
        }
        return brand.success
    }

    private func tag(_ text: String) -> some View {
        Text(text)
            .font(.caption2.weight(.bold))
            .lineLimit(1)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .foregroundStyle(brand.text2)
            .background(brand.raised, in: RoundedRectangle(cornerRadius: BrandRadius.badge, style: .continuous))
    }
}
