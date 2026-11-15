import NetworkCoreBridge
import SwiftUI

/// Debug mode's request list (plan E5): one row per decrypted HTTP request, newest first. Quick filter chips and
/// the search box build one filter expression the engine parses (`CaptureFilter`), so a chip and the same words typed
/// do the same thing. Rows arrive a second at a time while capture is on; the connection list sits behind the
/// toolbar's "连接" button.
///
/// On the Mac and in a regular-width iPad window the requests are a sortable table beside an inspector that shows
/// the selected one (after Proxyman); on an iPhone they are a list that opens each request on its own page.
struct CaptureRequestsView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @State private var selection: PacketCapture.RequestRow.ID?
    /// Newest first until a column header is clicked.
    @State private var sortOrder = [KeyPathComparator(\PacketCapture.RequestRow.seq, order: .reverse)]
    @State private var showsInspector = true
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

    /// A table with an inspector, or a list that pushes each request.
    private var usesTable: Bool {
        #if os(macOS)
        return true
        #else
        return sizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            if usesTable {
                workbench
            } else {
                list
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
                toolbarItems
            }
        }
        .navigationDestination(isPresented: $showsSessions) {
            CaptureSessionsView(tunnel: tunnel) {
                log.reset()
                notice = String(localized: "正在查看已保存的会话，抓包已停止")
            }
        }
        #if DEBUG
        // Screenshots show the inspector with the newest request in it.
        .onChange(of: log.rows.count) { _, _ in
            if TunnelController.isDemo, selection == nil {
                selection = log.rows.last?.id
            }
        }
        #endif
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

    /// The table, its status lines above it and the inspector beside it.
    private var workbench: some View {
        VStack(alignment: .leading, spacing: 0) {
            statusLines
                .font(.footnote)
                .foregroundStyle(brand.text2)
                .padding(.horizontal, 16)
                .padding(.bottom, 6)
            if log.rows.isEmpty {
                emptyRow
                    .frame(maxHeight: .infinity, alignment: .top)
            } else {
                CaptureRequestTable(rows: log.rows, selection: $selection, sortOrder: $sortOrder)
            }
        }
        .inspector(isPresented: $showsInspector) {
            NavigationStack {
                if let row = log.rows.first(where: { $0.id == selection }) {
                    CapturedRequestView(tunnel: tunnel, row: row)
                        .id(row.id)
                } else {
                    ContentUnavailableView {
                        Label("选择一条请求", systemImage: "cursorarrow.click.2")
                    } description: {
                        Text("在列表里选一条，这里显示它的请求和响应，可以重放、映射和导出。")
                    }
                }
            }
            .inspectorColumnWidth(min: 320, ideal: 420, max: 720)
        }
    }

    private var statusLines: some View {
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
    }

    /// The iPhone list: newest first, each row opening the request on its own page.
    private var list: some View {
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
                statusLines
            } footer: {
                Text("只列出 HTTPS 解密（MITM）后的请求和明文 HTTP 请求。搜索框可以直接写表达式：host:api.example.com status:>=400 type:json -method:GET，空格表示同时满足，OR 表示或者。")
            }
        }
    }

    @ViewBuilder
    private var toolbarItems: some View {
        if usesTable {
            Button {
                showsInspector.toggle()
            } label: {
                Image(systemName: "sidebar.trailing")
            }
            .accessibilityLabel(showsInspector ? "隐藏详情" : "显示详情")
            .help(showsInspector ? "隐藏详情" : "显示详情")
        }
        Button {
            paused.toggle()
        } label: {
            Image(systemName: paused ? "play.fill" : "pause.fill")
        }
        .accessibilityLabel(paused ? "继续刷新" : "暂停刷新")
        .help(paused ? "继续刷新" : "暂停刷新")
        NavigationLink {
            ConnectionsView(tunnel: tunnel)
        } label: {
            Image(systemName: "point.3.connected.trianglepath.dotted")
        }
        .accessibilityLabel("全部连接")
        .help("全部连接")
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
            // What is still missing before HTTPS requests can show up; hidden once nothing is.
            if tunnel.appMode == .debug, filter.isEmpty, invalidFilter == nil {
                DebugSetupChecklist(tunnel: tunnel, capturing: capture?.enabled == true)
                    .padding(.top, 8)
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
        if tunnel.appMode == .debug {
            // The setup checklist under this line says what is missing for HTTPS.
            return "还没有请求。"
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
                    Text(CaptureClock.time(row.startedAt))
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

/// The request table of the Mac and of a wide iPad window: sortable columns, one selected row. Arrow keys move the
/// selection; the context menu copies the URL.
struct CaptureRequestTable: View {
    let rows: [PacketCapture.RequestRow]
    @Binding var selection: PacketCapture.RequestRow.ID?
    @Binding var sortOrder: [KeyPathComparator<PacketCapture.RequestRow>]
    @Environment(\.brand) private var brand

    var body: some View {
        Table(rows.sorted(using: sortOrder), selection: $selection, sortOrder: $sortOrder) {
            TableColumn("时间", value: \.seq) { row in
                Text(CaptureClock.time(row.startedAt))
                    .monospacedDigit()
                    .foregroundStyle(brand.text2)
            }
            .width(min: 64, ideal: 72, max: 96)
            TableColumn("方法", value: \.method) { row in
                Text(row.method)
                    .font(.body.monospaced().weight(.semibold))
                    .foregroundStyle(brand.primary)
            }
            .width(min: 44, ideal: 56, max: 80)
            TableColumn("状态", value: \.sortStatus) { row in
                Text(statusText(row))
                    .fontWeight(.semibold)
                    .monospacedDigit()
                    .foregroundStyle(statusColor(row))
            }
            .width(min: 40, ideal: 48, max: 64)
            TableColumn("主机", value: \.host) { row in
                Text(row.host)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .width(min: 100, ideal: 180)
            TableColumn("路径", value: \.path) { row in
                Text(verbatim: row.path)
                    .font(.body.monospaced())
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .width(min: 120, ideal: 260)
            TableColumn("类型") { row in
                Text(typeText(row))
                    .foregroundStyle(brand.text2)
            }
            .width(min: 48, ideal: 80, max: 140)
            TableColumn("大小", value: \.responseSize) { row in
                Text(row.responseSize > 0 ? TrafficFormat.bytes(row.responseSize) : "")
                    .monospacedDigit()
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .width(min: 56, ideal: 72, max: 96)
            TableColumn("耗时", value: \.sortDuration) { row in
                Text(row.duration.map { "\(Int(($0 * 1000).rounded())) ms" } ?? "")
                    .monospacedDigit()
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .width(min: 56, ideal: 68, max: 96)
        }
        .contextMenu(forSelectionType: PacketCapture.RequestRow.ID.self) { selected in
            if let row = rows.first(where: { selected.contains($0.id) }) {
                Button {
                    Pasteboard.copy(row.url)
                } label: {
                    Label("拷贝网址", systemImage: "doc.on.doc")
                }
            }
        }
    }

    /// HTTP/2, the kind of body and a local answer, in one short line.
    private func typeText(_ row: PacketCapture.RequestRow) -> String {
        var parts: [String] = []
        if row.isHTTP2 {
            parts.append("H2")
        }
        if let kind = row.kind {
            parts.append(kind)
        }
        if row.note == "answered_locally" {
            parts.append(String(localized: "本地应答"))
        }
        return parts.joined(separator: " · ")
    }

    private func statusText(_ row: PacketCapture.RequestRow) -> String {
        if let status = row.status {
            return String(status)
        }
        return row.endedAt == nil && row.note == nil ? "…" : String(localized: "失败")
    }

    /// Green 2xx, amber 3xx, red 4xx/5xx or no response, gray while waiting.
    private func statusColor(_ row: PacketCapture.RequestRow) -> Color {
        guard let status = row.status else {
            if row.note == "answered_locally" || (row.endedAt == nil && row.note == nil) {
                return brand.text3
            }
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
}

/// Request times as `14:03:27` whatever the system's clock setting: a 12-hour time with its "下午" does not fit the
/// table's time column and lines up badly down a list.
enum CaptureClock {
    static func time(_ date: Date) -> String {
        date.formatted(Date.VerbatimFormatStyle(
            format: "\(hour: .twoDigits(clock: .twentyFourHour, hourCycle: .zeroBased)):\(minute: .twoDigits):\(second: .twoDigits)",
            timeZone: .current,
            calendar: .current
        ))
    }
}

extension PacketCapture.RequestRow {
    /// The status column's order: answered requests by code, then the ones without a status.
    var sortStatus: Int {
        status ?? Int.max
    }

    /// The duration column's order: finished requests by time taken, then the ones still waiting.
    var sortDuration: TimeInterval {
        duration ?? .greatestFiniteMagnitude
    }
}
