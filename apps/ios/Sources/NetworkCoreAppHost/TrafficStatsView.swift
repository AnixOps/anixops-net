import Charts
import NetworkCoreBridge
import SwiftUI

/// Traffic statistics: today or the last seven days, by hour, by host or by policy. The tunnel extension counts
/// them; while the tunnel is off the page shows the last copy it read.
struct TrafficStatsView: View {
    enum Period: Hashable {
        case today
        case week
    }

    enum Page: Hashable {
        case hours
        case hosts
        case policies
    }

    @ObservedObject var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var stats: TrafficStats?
    @State private var fromCache = false
    @State private var period = Period.today
    @State private var page = Page.hours
    @State private var confirmingClear = false

    private static let cacheKey = "traffic-stats-cache"

    private var since: Date {
        let calendar = Calendar.current
        let today = calendar.startOfDay(for: Date())
        switch period {
        case .today:
            return today
        case .week:
            return calendar.date(byAdding: .day, value: -6, to: today) ?? today
        }
    }

    private var hours: [TrafficStats.Hour] {
        stats?.hours(since: since) ?? []
    }

    var body: some View {
        List {
            Section {
                VStack(spacing: 10) {
                    Picker("时间范围", selection: $period) {
                        Text("今天").tag(Period.today)
                        Text("7 天").tag(Period.week)
                    }
                    .pickerStyle(.segmented)
                    Picker("分组", selection: $page) {
                        Text("按小时").tag(Page.hours)
                        Text("按主机").tag(Page.hosts)
                        Text("按策略").tag(Page.policies)
                    }
                    .pickerStyle(.segmented)
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
            }

            Section {
                totals
                    .listRowBackground(brand.surface)
            } footer: {
                if fromCache {
                    Text("隧道未连接，显示上次读取的统计。")
                }
            }

            Section {
                switch page {
                case .hours:
                    chart
                        .listRowBackground(brand.surface)
                case .hosts:
                    shares(TrafficStats.hosts(hours))
                case .policies:
                    shares(TrafficStats.policies(hours))
                }
            } footer: {
                Text("统计由隧道每 5 秒读取一次连接列表累计，只记主机名和字节数，保存在本机，保留 7 天。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("流量统计")
        .inlineNavigationTitle()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(role: .destructive) {
                    confirmingClear = true
                } label: {
                    Image(systemName: "trash")
                }
                .disabled(tunnel.connection != .connected || (stats?.hours.isEmpty ?? true))
                .accessibilityLabel("清空统计")
            }
        }
        .confirmationDialog("清空全部流量统计？", isPresented: $confirmingClear, titleVisibility: .visible) {
            Button("清空", role: .destructive) {
                Task {
                    if let cleared = await tunnel.trafficStats(clear: true) {
                        show(cleared)
                    }
                }
            }
        }
        .task(id: tunnel.connection) {
            loadCache()
            while !Task.isCancelled, tunnel.connection == .connected {
                if let next = await tunnel.trafficStats() {
                    show(next)
                }
                try? await Task.sleep(for: .seconds(10))
            }
        }
    }

    private var totals: some View {
        let upload = hours.reduce(UInt64(0)) { $0 &+ $1.upload }
        let download = hours.reduce(UInt64(0)) { $0 &+ $1.download }
        return VStack(alignment: .leading, spacing: 4) {
            Text(TrafficFormat.bytes(upload &+ download))
                .font(.title2.weight(.semibold).monospacedDigit())
                .foregroundStyle(brand.text)
            Text("↑\(TrafficFormat.bytes(upload)) ↓\(TrafficFormat.bytes(download))")
                .font(.footnote.monospacedDigit())
                .foregroundStyle(brand.text2)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private var chart: some View {
        if hours.isEmpty {
            Text(stats == nil ? LocalizedStringKey("连接后开始统计") : LocalizedStringKey("这段时间没有流量"))
                .font(.subheadline)
                .foregroundStyle(brand.text2)
        } else {
            Chart(hours, id: \.start) { hour in
                BarMark(
                    x: .value("时间", hour.start, unit: period == .today ? .hour : .day),
                    y: .value("MB", Double(hour.total) / 1_048_576)
                )
                .foregroundStyle(brand.primary)
            }
            .chartYAxisLabel("MB")
            .frame(height: 200)
            .padding(.vertical, 8)
        }
    }

    @ViewBuilder
    private func shares(_ rows: [TrafficStats.Share]) -> some View {
        if rows.isEmpty {
            Text(stats == nil ? LocalizedStringKey("连接后开始统计") : LocalizedStringKey("这段时间没有流量"))
                .font(.subheadline)
                .foregroundStyle(brand.text2)
                .listRowBackground(brand.surface)
        } else {
            let peak = Double(max(1, rows.first?.bytes ?? 1))
            ForEach(rows) { row in
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 8) {
                        Text(verbatim: displayName(row.name))
                            .font(.subheadline)
                            .foregroundStyle(brand.text)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Spacer(minLength: 8)
                        Text(TrafficFormat.bytes(row.bytes))
                            .font(.footnote.monospacedDigit())
                            .foregroundStyle(brand.text2)
                    }
                    GeometryReader { proxy in
                        ZStack(alignment: .leading) {
                            Capsule().fill(brand.raised)
                            Capsule()
                                .fill(brand.primary)
                                .frame(width: max(4, proxy.size.width * Double(row.bytes) / peak))
                        }
                    }
                    .frame(height: 6)
                    .accessibilityHidden(true)
                }
                .padding(.vertical, 2)
                .accessibilityElement(children: .combine)
                .listRowBackground(brand.surface)
            }
        }
    }

    private func displayName(_ name: String) -> String {
        switch name {
        case TrafficStats.otherHosts:
            return String(localized: "其他主机")
        case "DIRECT":
            return String(localized: "直连")
        case "REJECT":
            return String(localized: "拒绝")
        default:
            return name
        }
    }

    private func show(_ next: TrafficStats) {
        stats = next
        fromCache = false
        if let data = try? next.encoded() {
            UserDefaults.standard.set(data, forKey: Self.cacheKey)
        }
    }

    private func loadCache() {
        guard stats == nil || tunnel.connection != .connected else {
            return
        }
        if let data = UserDefaults.standard.data(forKey: Self.cacheKey), let cached = try? TrafficStats.decoded(from: data) {
            stats = cached
            fromCache = tunnel.connection != .connected
        }
    }
}
