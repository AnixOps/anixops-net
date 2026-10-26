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
/// which policy groups and node, and how much it moved. Refreshed every second while shown.
struct ConnectionsView: View {
    @ObservedObject var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var list = RuntimeControl.Connections()
    @State private var loaded = false
    @State private var showClosed = false
    @State private var paused = false
    @State private var query = ""
    @State private var confirmingCloseAll = false

    var body: some View {
        List {
            Section {
                Picker("连接", selection: $showClosed) {
                    Text("活跃 \(list.active.count)").tag(false)
                    Text("已结束 \(list.closed.count)").tag(true)
                }
                .pickerStyle(.segmented)
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
            }
            Section {
                if shown.isEmpty {
                    Text(emptyText)
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                        .listRowBackground(brand.surface)
                }
                ForEach(shown) { connection in
                    NavigationLink {
                        ConnectionDetail(connection: connection)
                    } label: {
                        ConnectionRow(connection: connection)
                    }
                    .listRowBackground(brand.surface)
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
            } footer: {
                Text("上传、下载为该连接经隧道的字节数。目标域名来自请求本身或 TLS SNI / HTTP Host，只保存在内存里，断开隧道即清空。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("连接")
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

    private var shown: [RuntimeControl.Connection] {
        let all = showClosed ? list.closed : Array(list.active.reversed())
        return all.filter { $0.matches(query) }
    }

    private var emptyText: LocalizedStringKey {
        if tunnel.connection != .connected {
            return "连接后显示经过隧道的每个连接。"
        }
        if !query.isEmpty {
            return "没有匹配的连接"
        }
        return loaded ? (showClosed ? "还没有结束的连接" : "暂无活跃连接") : "正在读取…"
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

private struct ConnectionRow: View {
    let connection: RuntimeControl.Connection
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Text(connection.network.uppercased())
                    .font(.caption2.weight(.bold))
                    .padding(.horizontal, 5)
                    .padding(.vertical, 1)
                    .background(brand.raised, in: RoundedRectangle(cornerRadius: 4, style: .continuous))
                    .foregroundStyle(brand.text2)
                Text(connection.target)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            HStack(spacing: 6) {
                Text(ConnectionDetail.route(connection))
                    .foregroundStyle(outboundColor)
                    .lineLimit(1)
                Spacer(minLength: 4)
                Text("↑\(TrafficFormat.bytes(connection.upload)) ↓\(TrafficFormat.bytes(connection.download))")
                    .monospacedDigit()
                    .foregroundStyle(brand.text2)
            }
            .font(.caption)
            Text(connection.rule)
                .font(.caption2)
                .foregroundStyle(brand.text3)
                .lineLimit(1)
        }
        .padding(.vertical, 2)
    }

    private var outboundColor: Color {
        if connection.error != nil || connection.outbound == "REJECT" {
            return brand.danger
        }
        return connection.outbound == "DIRECT" ? brand.text2 : brand.primary
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
