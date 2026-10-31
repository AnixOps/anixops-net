import NetworkCoreBridge
import SwiftUI

extension DashboardCard {
    var title: String {
        switch self {
        case .speed:
            return String(localized: "速度")
        case .usage:
            return String(localized: "本次用量")
        case .network:
            return String(localized: "网络")
        case .groups:
            return String(localized: "策略组")
        case .topHosts:
            return String(localized: "流量排行")
        case .dns:
            return "DNS"
        case .exitIP:
            return String(localized: "出口 IP")
        case .events:
            return String(localized: "事件")
        }
    }

    var systemImage: String {
        switch self {
        case .speed:
            return "waveform.path.ecg"
        case .usage:
            return "chart.bar"
        case .network:
            return "wifi"
        case .groups:
            return "point.3.connected.trianglepath.dotted"
        case .topHosts:
            return "list.number"
        case .dns:
            return "server.rack"
        case .exitIP:
            return "globe.asia.australia"
        case .events:
            return "clock.arrow.circlepath"
        }
    }
}

/// The dashboard cards in rows: wide cards alone, narrow cards in pairs of equal height.
struct DashboardGrid: View {
    @ObservedObject var tunnel: TunnelController
    @ObservedObject var model: DashboardModel
    let layout: DashboardLayout
    @Binding var tab: AppTab

    var body: some View {
        VStack(spacing: 12) {
            ForEach(layout.rows, id: \.self) { row in
                HStack(alignment: .top, spacing: 12) {
                    ForEach(row) { card in
                        cardView(card)
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    @ViewBuilder
    private func cardView(_ card: DashboardCard) -> some View {
        switch card {
        case .speed:
            SpeedTile(tunnel: tunnel, model: model)
        case .usage:
            UsageTile(tunnel: tunnel, model: model)
        case .network:
            NetworkTile(model: model)
        case .groups:
            GroupsTile(tunnel: tunnel, model: model, tab: $tab)
        case .topHosts:
            TopHostsTile(tunnel: tunnel, model: model)
        case .dns:
            DnsTile(tunnel: tunnel)
        case .exitIP:
            ExitAddressTile(tunnel: tunnel, model: model)
        case .events:
            EventsTile(model: model)
        }
    }
}

/// One dashboard card: a small title row and its content on the surface color.
private struct Tile<Content: View, Trailing: View>: View {
    @Environment(\.brand) private var brand
    let card: DashboardCard
    @ViewBuilder var trailing: Trailing
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Text(card.title)
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(brand.text2)
                    .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 4)
                trailing
            }
            content
        }
        .padding(14)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(brand.surface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }
}

extension Tile where Trailing == EmptyView {
    init(card: DashboardCard, @ViewBuilder content: () -> Content) {
        self.init(card: card, trailing: { EmptyView() }, content: content)
    }
}

/// The chevron that marks a card as a link to another screen.
private struct Disclosure: View {
    @Environment(\.brand) private var brand

    var body: some View {
        Image(systemName: "chevron.right")
            .font(.caption.weight(.semibold))
            .foregroundStyle(brand.text3)
            .accessibilityHidden(true)
    }
}

/// A short gray line for a card with nothing to show yet.
private struct TilePlaceholder: View {
    @Environment(\.brand) private var brand
    let text: LocalizedStringKey

    var body: some View {
        Text(text)
            .font(.subheadline)
            .foregroundStyle(brand.text2)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

// MARK: - Speed

private struct SpeedTile: View {
    @ObservedObject var tunnel: TunnelController
    @ObservedObject var model: DashboardModel
    @Environment(\.brand) private var brand

    var body: some View {
        NavigationLink {
            ConnectionsView(tunnel: tunnel)
        } label: {
            Tile(card: .speed) {
                Text("最近 60 秒")
                    .font(.caption)
                    .foregroundStyle(brand.text3)
                Disclosure()
            } content: {
                if tunnel.connection == .connected {
                    HStack(spacing: 24) {
                        rate("arrow.down", "下载", model.traffic?.downloadRate)
                        rate("arrow.up", "上传", model.traffic?.uploadRate)
                    }
                    Sparkline(history: model.history)
                        .frame(height: 52)
                        .accessibilityHidden(true)
                } else {
                    TilePlaceholder(text: "连接后显示实时速度")
                }
            }
        }
        .buttonStyle(.plain)
    }

    private func rate(_ symbol: String, _ title: LocalizedStringKey, _ value: UInt64?) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Label(title, systemImage: symbol)
                .font(.caption)
                .foregroundStyle(brand.text2)
            Text(value.map(TrafficFormat.rate) ?? "–")
                .font(.title3.weight(.semibold).monospacedDigit())
                .foregroundStyle(brand.text)
                .contentTransition(.numericText())
        }
        .accessibilityElement(children: .combine)
    }
}

/// The last minute of download (filled teal) and upload (amber line), scaled to the window's peak.
private struct Sparkline: View {
    let history: TrafficHistory
    @Environment(\.brand) private var brand

    var body: some View {
        GeometryReader { proxy in
            let size = proxy.size
            let download = points(history.samples.map(\.download), in: size)
            let upload = points(history.samples.map(\.upload), in: size)
            ZStack {
                if download.count > 1 {
                    Path { path in
                        path.move(to: CGPoint(x: download[0].x, y: size.height))
                        download.forEach { path.addLine(to: $0) }
                        path.addLine(to: CGPoint(x: download[download.count - 1].x, y: size.height))
                        path.closeSubpath()
                    }
                    .fill(brand.primary.opacity(0.14))
                    line(download).stroke(brand.primary, style: StrokeStyle(lineWidth: 2, lineJoin: .round))
                    line(upload).stroke(brand.amberText, style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
                }
            }
        }
    }

    private func points(_ values: [UInt64], in size: CGSize) -> [CGPoint] {
        guard values.count > 1 else {
            return []
        }
        let peak = Double(history.peak)
        // Right-aligned: a window that is not full yet grows in from the right edge.
        let step = size.width / Double(history.capacity - 1)
        let start = size.width - step * Double(values.count - 1)
        return values.enumerated().map { index, value in
            CGPoint(x: start + step * Double(index), y: size.height - 2 - (size.height - 4) * Double(value) / peak)
        }
    }

    private func line(_ points: [CGPoint]) -> Path {
        Path { path in
            guard let first = points.first else {
                return
            }
            path.move(to: first)
            points.dropFirst().forEach { path.addLine(to: $0) }
        }
    }
}

// MARK: - Usage

private struct UsageTile: View {
    @ObservedObject var tunnel: TunnelController
    @ObservedObject var model: DashboardModel
    @Environment(\.brand) private var brand

    var body: some View {
        Tile(card: .usage) {
            if let traffic = model.traffic, tunnel.connection == .connected {
                Text(TrafficFormat.bytes(traffic.uploadTotal &+ traffic.downloadTotal))
                    .font(.title3.weight(.semibold).monospacedDigit())
                    .foregroundStyle(brand.text)
                Text("↑\(TrafficFormat.bytes(traffic.uploadTotal)) ↓\(TrafficFormat.bytes(traffic.downloadTotal))")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(brand.text2)
                if let since = tunnel.connectedSince {
                    TimelineView(.periodic(from: since, by: 1)) { context in
                        Text("在线 \(ConnectElapsed.text(from: since, to: context.date))")
                    }
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(brand.text2)
                }
                Text("\(traffic.active) 个连接")
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            } else {
                TilePlaceholder(text: "连接后统计")
            }
        }
    }
}

// MARK: - Network

private struct NetworkTile: View {
    @ObservedObject var model: DashboardModel
    @Environment(\.brand) private var brand

    private var symbol: String {
        switch model.network.interface {
        case .wifi:
            return "wifi"
        case .cellular:
            return "antenna.radiowaves.left.and.right"
        case .wired:
            return "cable.connector"
        case .other:
            return "network"
        case .none:
            return "wifi.slash"
        }
    }

    var body: some View {
        Tile(card: .network) {
            Label {
                Text(DashboardModel.name(of: model.network.interface))
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(brand.text)
            } icon: {
                Image(systemName: symbol)
                    .foregroundStyle(model.network.interface == .none ? brand.warning : brand.primary)
            }
            if model.network.isConstrained {
                Text("低数据模式")
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            } else if model.network.isExpensive {
                Text("按流量计费的网络")
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            }
            Text("本次切换 \(model.network.switches) 次")
                .font(.caption)
                .foregroundStyle(brand.text2)
        }
    }
}

// MARK: - Groups

private struct GroupsTile: View {
    @ObservedObject var tunnel: TunnelController
    @ObservedObject var model: DashboardModel
    @Binding var tab: AppTab
    @Environment(\.brand) private var brand

    var body: some View {
        Tile(card: .groups) {
            Button {
                tab = .proxies
            } label: {
                Text("全部")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(brand.primary)
                    .frame(minHeight: 28)
            }
            .buttonStyle(.plain)
        } content: {
            if tunnel.connection != .connected {
                TilePlaceholder(text: "连接后显示各策略组当前选择")
            } else if model.groups.isEmpty {
                TilePlaceholder(text: "当前节点没有策略组")
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(model.groups.prefix(4).enumerated()), id: \.element.id) { index, group in
                        row(group)
                            .overlay(alignment: .top) {
                                if index > 0 {
                                    Rectangle().fill(brand.border).frame(height: 1)
                                }
                            }
                    }
                }
            }
        }
    }

    private func row(_ group: RuntimeControl.Groups.Group) -> some View {
        let latency = group.members.first { $0.name == group.now }?.latencyMs
        return HStack(spacing: 10) {
            Text(group.id)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(brand.text)
                .lineLimit(1)
            Spacer(minLength: 8)
            Text(group.now ?? "–")
                .font(.subheadline)
                .foregroundStyle(brand.text2)
                .lineLimit(1)
            Text(latency.map { "\($0) ms" } ?? "–")
                .font(.footnote.weight(.semibold).monospacedDigit())
                .foregroundStyle(LatencyColor.color(latency, brand))
                .frame(minWidth: 52, alignment: .trailing)
        }
        .frame(minHeight: 40)
        .accessibilityElement(children: .combine)
    }
}

/// Green-teal under 120 ms, amber under 250 ms, orange above; gray when unknown.
enum LatencyColor {
    static func color(_ milliseconds: UInt32?, _ brand: BrandPalette) -> Color {
        guard let milliseconds else {
            return brand.text3
        }
        if milliseconds < 120 {
            return brand.primary
        }
        return milliseconds < 250 ? brand.amberText : brand.warning
    }
}

// MARK: - Top hosts

private struct TopHostsTile: View {
    @ObservedObject var tunnel: TunnelController
    @ObservedObject var model: DashboardModel
    @Environment(\.brand) private var brand

    var body: some View {
        NavigationLink {
            ConnectionsView(tunnel: tunnel)
        } label: {
            Tile(card: .topHosts) {
                Text("全部请求")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(brand.primary)
                Disclosure()
            } content: {
                if tunnel.connection != .connected {
                    TilePlaceholder(text: "连接后按主机统计流量")
                } else if model.topHosts.isEmpty {
                    TilePlaceholder(text: "还没有流量")
                } else {
                    let peak = Double(max(1, model.topHosts.first?.bytes ?? 1))
                    VStack(spacing: 10) {
                        ForEach(model.topHosts) { host in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack(spacing: 8) {
                                    Text(host.host)
                                        .font(.subheadline)
                                        .foregroundStyle(brand.text)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                    Spacer(minLength: 8)
                                    Text(TrafficFormat.bytes(host.bytes))
                                        .font(.footnote.monospacedDigit())
                                        .foregroundStyle(brand.text2)
                                }
                                GeometryReader { proxy in
                                    ZStack(alignment: .leading) {
                                        Capsule().fill(brand.raised)
                                        Capsule()
                                            .fill(brand.primary)
                                            .frame(width: max(4, proxy.size.width * Double(host.bytes) / peak))
                                    }
                                }
                                .frame(height: 6)
                                .accessibilityHidden(true)
                            }
                            .accessibilityElement(children: .combine)
                        }
                    }
                }
            }
        }
        .buttonStyle(.plain)
    }
}

// MARK: - DNS

private struct DnsTile: View {
    @ObservedObject var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        let dns = tunnel.dns
        let server = dns.servers.first { $0.name == dns.defaultServer } ?? dns.servers.first
        Tile(card: .dns) {
            if let server {
                Text(server.name)
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                Text(server.kind)
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            } else {
                Text("系统 DNS")
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(brand.text)
            }
            if !dns.rules.isEmpty {
                Text("\(dns.rules.count) 条分流规则")
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            }
        }
    }
}

// MARK: - Exit address

private struct ExitAddressTile: View {
    @ObservedObject var tunnel: TunnelController
    @ObservedObject var model: DashboardModel
    @Environment(\.brand) private var brand

    var body: some View {
        Button {
            Task {
                await model.lookUpExitAddress()
            }
        } label: {
            Tile(card: .exitIP) {
                switch model.exitLookup {
                case .idle:
                    Text("轻点查询")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(brand.primary)
                case .loading:
                    Text("查询中…")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(brand.text2)
                case let .done(address):
                    Text(address.ip)
                        .font(.subheadline.weight(.semibold).monospacedDigit())
                        .foregroundStyle(brand.text)
                        .lineLimit(2)
                        .minimumScaleFactor(0.7)
                        .textSelection(.enabled)
                    if let region = address.region {
                        Text(Locale.current.localizedString(forRegionCode: region) ?? region)
                            .font(.caption)
                            .foregroundStyle(brand.text2)
                    }
                case .failed:
                    Text("查询失败，轻点重试")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(brand.warning)
                }
                Text("只在你点按时查询，按当前模式和规则发出")
                    .font(.caption2)
                    .foregroundStyle(brand.text3)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .buttonStyle(.plain)
        .disabled(model.exitLookup == .loading)
    }
}

// MARK: - Events

private struct EventsTile: View {
    @ObservedObject var model: DashboardModel
    @Environment(\.brand) private var brand

    var body: some View {
        Tile(card: .events) {
            if !model.events.events.isEmpty {
                Button("清空") {
                    model.clearEvents()
                }
                .font(.footnote.weight(.semibold))
                .foregroundStyle(brand.primary)
                .buttonStyle(.plain)
                .frame(minHeight: 28)
            }
        } content: {
            if model.events.events.isEmpty {
                TilePlaceholder(text: "暂无事件")
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(model.events.events.prefix(5).enumerated()), id: \.element.id) { index, event in
                        HStack(alignment: .firstTextBaseline, spacing: 10) {
                            Circle()
                                .fill(color(event.kind))
                                .frame(width: 8, height: 8)
                                .accessibilityHidden(true)
                            Text(event.text)
                                .font(.subheadline)
                                .foregroundStyle(brand.text)
                                .frame(maxWidth: .infinity, alignment: .leading)
                            Text(event.date, format: .dateTime.hour().minute())
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(brand.text3)
                        }
                        .padding(.vertical, 8)
                        .overlay(alignment: .top) {
                            if index > 0 {
                                Rectangle().fill(brand.border).frame(height: 1)
                            }
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            }
        }
    }

    private func color(_ kind: DashboardEventLog.Kind) -> Color {
        switch kind {
        case .connected:
            return brand.primary
        case .disconnected:
            return brand.text3
        case .network:
            return brand.amberText
        case .error:
            return brand.warning
        }
    }
}

// MARK: - Edit sheet

/// Shows, hides and reorders the dashboard cards.
struct DashboardEditSheet: View {
    @Binding var layoutStorage: String
    @Environment(\.dismiss) private var dismiss
    @Environment(\.brand) private var brand

    private var layout: DashboardLayout {
        DashboardLayout(storage: layoutStorage)
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(layout.order) { card in
                        Toggle(isOn: shown(card)) {
                            Label(card.title, systemImage: card.systemImage)
                        }
                        .tint(brand.primary)
                        .listRowBackground(brand.surface)
                    }
                    .onMove { offsets, destination in
                        var next = layout
                        next.move(fromOffsets: offsets, toOffset: destination)
                        layoutStorage = next.storage
                    }
                } footer: {
                    Text("拖动右侧把手调整顺序，关掉的卡片不在首页显示。")
                }
                Section {
                    Button("恢复默认") {
                        layoutStorage = ""
                    }
                    .listRowBackground(brand.surface)
                }
            }
            .environment(\.editMode, .constant(.active))
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("编辑首页")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("完成") {
                        dismiss()
                    }
                }
            }
        }
    }

    private func shown(_ card: DashboardCard) -> Binding<Bool> {
        Binding {
            layout.isShown(card)
        } set: { shown in
            var next = layout
            next.setShown(card, shown)
            layoutStorage = next.storage
        }
    }
}
