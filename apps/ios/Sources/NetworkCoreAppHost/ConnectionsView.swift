import NetworkCoreBridge
import SwiftUI

/// Byte counts and rates as the traffic card and the connection list show them.
enum TrafficFormat {
    static func bytes(_ value: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: value), countStyle: .binary)
    }

    static func rate(_ value: UInt64) -> String {
        "\(bytes(value))/s"
    }
}

/// Home screen card while connected: upload and download speed, totals and open connections, refreshed every
/// second from the engine. Tapping it opens the connection list.
struct TrafficCard: View {
    @ObservedObject var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var traffic: RuntimeControl.Traffic?

    var body: some View {
        NavigationLink {
            ConnectionsView(tunnel: tunnel)
        } label: {
            BrandCard {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 16) {
                        speed("arrow.up", "上传", traffic?.uploadRate)
                        speed("arrow.down", "下载", traffic?.downloadRate)
                        Spacer(minLength: 0)
                        Image(systemName: "chevron.right")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(brand.text3)
                    }
                    if let traffic {
                        Text("本次已用 ↑\(TrafficFormat.bytes(traffic.uploadTotal)) ↓\(TrafficFormat.bytes(traffic.downloadTotal)) · \(traffic.active) 个连接")
                            .font(.footnote.monospacedDigit())
                            .foregroundStyle(brand.text2)
                    } else {
                        Text("连接记录")
                            .font(.footnote)
                            .foregroundStyle(brand.text2)
                    }
                }
            }
        }
        .buttonStyle(.plain)
        .task(id: tunnel.connection) {
            await poll()
        }
    }

    private func speed(_ symbol: String, _ title: LocalizedStringKey, _ value: UInt64?) -> some View {
        HStack(spacing: 6) {
            Image(systemName: symbol)
                .font(.footnote.weight(.bold))
                .foregroundStyle(brand.primary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 0) {
                Text(title)
                    .font(.caption)
                    .foregroundStyle(brand.text2)
                Text(value.map(TrafficFormat.rate) ?? "–")
                    .font(.body.weight(.semibold).monospacedDigit())
                    .foregroundStyle(brand.text)
                    .contentTransition(.numericText())
            }
        }
        .accessibilityElement(children: .combine)
    }

    private func poll() async {
        while !Task.isCancelled, tunnel.connection == .connected {
            if let data = await tunnel.control(.traffic), let next = RuntimeControl.decodeTraffic(data) {
                withAnimation(.easeOut(duration: 0.2)) {
                    traffic = next
                }
            }
            try? await Task.sleep(for: .seconds(1))
        }
        traffic = nil
    }
}

/// Every connection the engine is carrying, and the last closed ones: where it went, which rule matched, through
/// which policy groups and node, and how much it moved. Rows show a status color, a local letter badge for the
/// site, protocol and policy tags; chips filter by route, failures or UDP, and a third view sums by host.
/// Refreshed every second while shown.
struct ConnectionsView: View {
    enum Page: Hashable {
        case active
        case closed
        case hosts
    }

    @ObservedObject var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var list = RuntimeControl.Connections()
    @State private var loaded = false
    @State private var page = Page.active
    @State private var filter = RequestFilter.all
    @State private var paused = false
    @State private var query = ""
    @State private var confirmingCloseAll = false

    var body: some View {
        List {
            Section {
                VStack(spacing: 10) {
                    Picker("连接", selection: $page) {
                        Text("活跃 \(list.active.count)").tag(Page.active)
                        Text("已结束 \(list.closed.count)").tag(Page.closed)
                        Text("按主机").tag(Page.hosts)
                    }
                    .pickerStyle(.segmented)
                    filterChips
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
            }
            Section {
                if page == .hosts {
                    if hosts.isEmpty {
                        emptyRow
                    }
                    ForEach(hosts) { host in
                        Button {
                            query = host.host
                            page = .active
                        } label: {
                            HostRow(host: host)
                        }
                        .buttonStyle(.plain)
                        .listRowBackground(brand.surface)
                    }
                } else {
                    if shown.isEmpty {
                        emptyRow
                    }
                    ForEach(shown) { connection in
                        NavigationLink {
                            ConnectionDetail(connection: connection)
                        } label: {
                            ConnectionRow(connection: connection)
                        }
                        .listRowBackground(brand.surface)
                        .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 16))
                        .swipeActions(edge: .trailing) {
                            if connection.isActive {
                                Button(role: .destructive) {
                                    Task {
                                        _ = await tunnel.control(.closeConnection(connection.id))
                                        await load()
                                    }
                                } label: {
                                    Label("断开", systemImage: "xmark.circle")
                                }
                            }
                        }
                    }
                }
            } header: {
                Text(summary)
                    .monospacedDigit()
            } footer: {
                Text("站点标识是本机按主机名生成的字母，不联网取图标。HTTPS、QUIC 等标签按端口推断。上传、下载为该连接经隧道的字节数。目标域名来自请求本身或 TLS SNI / HTTP Host，只保存在内存里，断开隧道即清空。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("请求")
        .inlineNavigationTitle()
        .searchable(text: $query, prompt: Text("筛选域名、规则或节点"))
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    paused.toggle()
                } label: {
                    Image(systemName: paused ? "play.fill" : "pause.fill")
                }
                .accessibilityLabel(paused ? "继续刷新" : "暂停刷新")
                Button(role: .destructive) {
                    confirmingCloseAll = true
                } label: {
                    Image(systemName: "xmark.circle")
                }
                .disabled(list.active.isEmpty)
                .accessibilityLabel("断开全部连接")
            }
        }
        .confirmationDialog("断开全部 \(list.active.count) 个连接？", isPresented: $confirmingCloseAll, titleVisibility: .visible) {
            Button("全部断开", role: .destructive) {
                Task {
                    _ = await tunnel.control(.closeAll)
                    await load()
                }
            }
        } message: {
            Text("App 会自动重新建立需要的连接。")
        }
        .task(id: tunnel.connection) {
            while !Task.isCancelled {
                if !paused {
                    await load()
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    private var filterChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(RequestFilter.allCases) { chip in
                    let selected = filter == chip
                    Button {
                        filter = chip
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
                }
            }
            .padding(.vertical, 1)
        }
    }

    private var emptyRow: some View {
        Text(emptyText)
            .font(.subheadline)
            .foregroundStyle(brand.text2)
            .listRowBackground(brand.surface)
    }

    static func title(_ filter: RequestFilter) -> String {
        switch filter {
        case .all:
            return String(localized: "全部")
        case .proxy:
            return String(localized: "代理")
        case .direct:
            return String(localized: "直连")
        case .reject:
            return String(localized: "拒绝")
        case .failed:
            return String(localized: "出错")
        case .udp:
            return "UDP"
        }
    }

    private func passes(_ connection: RuntimeControl.Connection) -> Bool {
        filter.matches(connection) && connection.matches(query)
    }

    private var shown: [RuntimeControl.Connection] {
        let all = page == .closed ? list.closed : Array(list.active.reversed())
        return all.filter(passes)
    }

    private var hosts: [HostTraffic] {
        HostTraffic.summarize((list.active + list.closed).filter(passes))
    }

    private var summary: String {
        if page == .hosts {
            return String(localized: "\(hosts.count) 个主机")
        }
        let bytes = shown.reduce(UInt64(0)) { $0 &+ $1.upload &+ $1.download }
        let count = String(localized: "\(shown.count) 个请求 · \(TrafficFormat.bytes(bytes))")
        return paused ? count + String(localized: " · 已暂停刷新") : count
    }

    private var emptyText: LocalizedStringKey {
        if tunnel.connection != .connected {
            return "连接后显示经过隧道的每个连接。"
        }
        if !query.isEmpty || filter != .all {
            return "没有匹配的连接"
        }
        if !loaded {
            return "正在读取…"
        }
        switch page {
        case .active:
            return "暂无活跃连接"
        case .closed:
            return "还没有结束的连接"
        case .hosts:
            return "还没有流量"
        }
    }

    private func load() async {
        guard let data = await tunnel.control(.connections), let next = RuntimeControl.decodeConnections(data) else {
            if tunnel.connection != .connected {
                list = RuntimeControl.Connections()
            }
            return
        }
        list = next
        loaded = true
    }
}

/// The colors of a row's status bar and policy tag.
private struct RequestColors {
    let brand: BrandPalette

    func status(_ status: RuntimeControl.Connection.Status) -> Color {
        switch status {
        case .active:
            return brand.primary
        case .done:
            return brand.border
        case .failed:
            return brand.warning
        case .rejected:
            return brand.danger
        }
    }

    func statusText(_ status: RuntimeControl.Connection.Status) -> String {
        switch status {
        case .active:
            return String(localized: "进行中")
        case .done:
            return String(localized: "已结束")
        case .failed:
            return String(localized: "出错")
        case .rejected:
            return String(localized: "已拦截")
        }
    }

    func policy(_ route: RuntimeControl.Connection.Route) -> (foreground: Color, background: Color) {
        switch route {
        case .proxy:
            return (brand.primary, brand.soft)
        case .direct:
            return (brand.text2, brand.raised)
        case .reject:
            return (brand.danger, brand.danger.opacity(0.12))
        }
    }
}

/// The letter badge in place of a site icon, colored from the host name.
private struct MonogramBadge: View {
    let host: String
    @Environment(\.colorScheme) private var scheme

    private static let light: [(Color, Color)] = [
        (Color(hex: 0xCCFBF1), Color(hex: 0x115E59)), (Color(hex: 0xFEF3C7), Color(hex: 0x92400E)),
        (Color(hex: 0xDBEAFE), Color(hex: 0x1E40AF)), (Color(hex: 0xFCE7F3), Color(hex: 0x9D174D)),
        (Color(hex: 0xECFCCB), Color(hex: 0x3F6212)),
    ]
    private static let dark: [(Color, Color)] = [
        (Color(hex: 0x134E4A), Color(hex: 0x99F6E4)), (Color(hex: 0x3B2F0B), Color(hex: 0xFDE68A)),
        (Color(hex: 0x1E3A5F), Color(hex: 0xBFDBFE)), (Color(hex: 0x3F1D38), Color(hex: 0xFBCFE8)),
        (Color(hex: 0x2E3B12), Color(hex: 0xD9F99D)),
    ]

    var body: some View {
        let palette = scheme == .dark ? Self.dark : Self.light
        let colors = palette[HostMonogram.colorIndex(for: host, count: palette.count)]
        Text(HostMonogram.letter(for: host))
            .font(.subheadline.weight(.bold))
            .foregroundStyle(colors.1)
            .frame(width: 32, height: 32)
            .background(colors.0, in: Circle())
            .accessibilityHidden(true)
    }
}

/// A small rounded tag such as `HTTPS` or `Proxy · hk-1`.
private struct RequestTag: View {
    let text: String
    let foreground: Color
    let background: Color

    var body: some View {
        Text(text)
            .font(.caption2.weight(.bold))
            .lineLimit(1)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .foregroundStyle(foreground)
            .background(background, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
    }
}

private struct ConnectionRow: View {
    let connection: RuntimeControl.Connection
    @Environment(\.brand) private var brand

    var body: some View {
        let colors = RequestColors(brand: brand)
        let policy = colors.policy(connection.route)
        HStack(spacing: 10) {
            MonogramBadge(host: connection.hostName)
            VStack(alignment: .leading, spacing: 4) {
                Text(connection.target)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                    .truncationMode(.middle)
                HStack(spacing: 4) {
                    RequestTag(text: connection.protocolLabel, foreground: brand.text2, background: brand.raised)
                    RequestTag(text: Self.policyText(connection), foreground: policy.foreground, background: policy.background)
                    Text(colors.statusText(connection.status))
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(connection.status == .done ? brand.text3 : colors.status(connection.status))
                        .lineLimit(1)
                }
                Text(connection.rule)
                    .font(.caption2)
                    .foregroundStyle(brand.text3)
                    .lineLimit(1)
            }
            Spacer(minLength: 4)
            VStack(alignment: .trailing, spacing: 3) {
                Text(TrafficFormat.bytes(connection.upload &+ connection.download))
                    .font(.footnote.weight(.semibold).monospacedDigit())
                    .foregroundStyle(brand.text)
                Text(Self.duration(connection.duration()))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(brand.text3)
            }
        }
        .padding(.vertical, 8)
        .padding(.leading, 14)
        .background(alignment: .leading) {
            Rectangle()
                .fill(colors.status(connection.status))
                .frame(width: 4)
                .accessibilityHidden(true)
        }
        .accessibilityElement(children: .combine)
    }

    static func policyText(_ connection: RuntimeControl.Connection) -> String {
        switch connection.outbound {
        case "DIRECT":
            return String(localized: "直连")
        case "REJECT":
            return String(localized: "拒绝")
        default:
            return connection.policyLabel
        }
    }

    static func duration(_ seconds: TimeInterval) -> String {
        if seconds < 1 {
            return String(localized: "不到 1 秒")
        }
        return Duration.seconds(seconds.rounded(.down))
            .formatted(.units(allowed: [.hours, .minutes, .seconds], width: .narrow, maximumUnitCount: 2))
    }
}

/// One host in the by-host view: its connection count, main outbound and bytes. Tapping it filters the list.
private struct HostRow: View {
    let host: HostTraffic
    @Environment(\.brand) private var brand

    var body: some View {
        HStack(spacing: 12) {
            MonogramBadge(host: host.host)
            VStack(alignment: .leading, spacing: 2) {
                Text(host.host)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text("\(host.connections) 个连接 · \(host.outbound)")
                    .font(.caption)
                    .foregroundStyle(brand.text2)
                    .lineLimit(1)
            }
            Spacer(minLength: 8)
            Text(TrafficFormat.bytes(host.bytes))
                .font(.footnote.monospacedDigit())
                .foregroundStyle(brand.text2)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityHint("显示这个主机的连接")
    }
}

/// Everything the engine reports about one connection.
private struct ConnectionDetail: View {
    let connection: RuntimeControl.Connection
    @Environment(\.brand) private var brand

    var body: some View {
        List {
            Section {
                item("目标", connection.target)
                if connection.target != connection.destination {
                    item("请求地址", connection.destination)
                }
                item("协议", connection.network.uppercased())
                item("规则", connection.rule)
                item("线路", Self.route(connection))
                if let error = connection.error {
                    item("错误", error == "rejected" ? String(localized: "被规则拒绝") : String(localized: "连接失败"))
                }
            }
            .listRowBackground(brand.surface)
            Section {
                item("上传", TrafficFormat.bytes(connection.upload))
                item("下载", TrafficFormat.bytes(connection.download))
                item("开始", connection.startedAt.formatted(date: .omitted, time: .standard))
                if let ended = connection.endedAt {
                    item("结束", ended.formatted(date: .omitted, time: .standard))
                    item("时长", Duration.seconds(ended.timeIntervalSince(connection.startedAt)).formatted(.units(allowed: [.hours, .minutes, .seconds], width: .narrow)))
                }
            }
            .listRowBackground(brand.surface)
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("连接详情")
        .inlineNavigationTitle()
    }

    private func item(_ title: LocalizedStringKey, _ value: String) -> some View {
        LabeledContent(title) {
            Text(value)
                .textSelection(.enabled)
                .multilineTextAlignment(.trailing)
        }
    }

    /// The policy groups and the outbound, as `Proxy › Auto › hk-1`.
    static func route(_ connection: RuntimeControl.Connection) -> String {
        let chain = connection.chain.last == connection.outbound ? connection.chain : connection.chain + [connection.outbound]
        return chain.joined(separator: " › ")
    }
}
