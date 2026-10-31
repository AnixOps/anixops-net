import NetworkCoreBridge
import NetworkExtension
import SwiftUI

/// Home: the connect card answers "am I connected, through which node, in which mode"; below it a dashboard of
/// cards the user can reorder and hide. Once the connect card scrolls away a compact switch takes its place in
/// the top bar.
struct HomeView: View {
    @ObservedObject var tunnel: TunnelController
    @Binding var tab: AppTab
    @Binding var importing: Bool
    @Environment(\.brand) private var brand
    @StateObject private var dashboard = DashboardModel()
    @AppStorage(DashboardLayout.storageKey) private var layoutStorage = ""
    @State private var heroVisible = true
    @State private var editing = false

    private static let scrollSpace = "home-scroll"
    private static let headerHeight: CGFloat = 52

    private var phase: ConnectPhase {
        ConnectPhase(status: tunnel.connection, hasNode: tunnel.nodeName != nil)
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 12) {
                    ConnectCard(tunnel: tunnel, phase: phase, importing: $importing, tab: $tab)
                        .background(
                            GeometryReader { proxy in
                                Color.clear.preference(
                                    key: HeroBottomKey.self,
                                    value: proxy.frame(in: .named(Self.scrollSpace)).maxY
                                )
                            }
                        )
                    if let message = tunnel.message {
                        MessageCard(text: message) {
                            tunnel.clearMessage()
                        }
                    }
                    DashboardGrid(
                        tunnel: tunnel,
                        model: dashboard,
                        layout: DashboardLayout(storage: layoutStorage),
                        tab: $tab
                    )
                }
                .padding(.horizontal, 16)
                .padding(.top, 4)
                .padding(.bottom, 24)
            }
            .coordinateSpace(name: Self.scrollSpace)
            .onPreferenceChange(HeroBottomKey.self) { bottom in
                // The card counts as gone once its bottom edge passes under the top bar.
                let visible = bottom > Self.headerHeight + 12
                if visible != heroVisible {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        heroVisible = visible
                    }
                }
            }
            .safeAreaInset(edge: .top, spacing: 0) {
                header
            }
            .background(brand.bg.ignoresSafeArea())
            .toolbar(.hidden, for: .navigationBar)
            .brandBars(brand)
            .sheet(isPresented: $editing) {
                DashboardEditSheet(layoutStorage: $layoutStorage)
            }
            .task(id: tunnel.connection) {
                await dashboard.poll(tunnel)
            }
            .onAppear {
                dashboard.startNetworkMonitor()
            }
            .onChange(of: tunnel.connection) { previous, status in
                dashboard.tunnelChanged(from: previous, to: status, node: tunnel.nodeName)
            }
            .onChange(of: tunnel.message) { _, message in
                dashboard.messageShown(message)
            }
        }
    }

    private var header: some View {
        HStack(spacing: 8) {
            Wordmark()
            Spacer(minLength: 8)
            if !heroVisible, phase != .unconfigured {
                CompactConnectButton(tunnel: tunnel, phase: phase)
                    .transition(.opacity.combined(with: .scale(scale: 0.9)))
            }
            Button {
                editing = true
            } label: {
                Image(systemName: "square.grid.2x2")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(brand.primary)
                    .frame(width: 40, height: 40)
                    .brandGlass(Circle(), fallback: brand.raised)
            }
            .accessibilityLabel("编辑首页")
            Button {
                importing = true
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(brand.primary)
                    .frame(width: 40, height: 40)
                    .brandGlass(Circle(), fallback: brand.raised)
            }
            .accessibilityLabel("导入节点")
        }
        .frame(height: Self.headerHeight)
        .padding(.horizontal, 16)
        .background(brand.bg)
        .overlay(alignment: .bottom) {
            Rectangle()
                .fill(brand.border)
                .frame(height: 1)
                .opacity(heroVisible ? 0 : 1)
        }
    }
}

/// Where the connect card's bottom edge is in the scroll view, to know when it has scrolled away.
private struct HeroBottomKey: PreferenceKey {
    static let defaultValue: CGFloat = .greatestFiniteMagnitude

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

/// The hero card: the route-line connect button, status, elapsed time, current node and the mode switch.
private struct ConnectCard: View {
    @ObservedObject var tunnel: TunnelController
    let phase: ConnectPhase
    @Binding var importing: Bool
    @Binding var tab: AppTab
    @Environment(\.brand) private var brand

    /// The rule mode needs subscription rules; a single pasted node offers global and direct only.
    private var modes: [ProxyMode] {
        tunnel.library?.routing == nil ? [.global, .direct] : ProxyMode.allCases
    }

    var body: some View {
        VStack(spacing: 14) {
            HStack(spacing: 16) {
                Button(action: tap) {
                    RouteLineMark(phase: phase, size: 60)
                        .frame(width: 92, height: 92)
                        .background(Brand.line.opacity(0.06), in: Circle())
                        .overlay(
                            Circle().strokeBorder(phase == .on ? Brand.mint.opacity(0.7) : Brand.line.opacity(0.25), lineWidth: 1.5)
                        )
                        .contentShape(Circle())
                }
                .buttonStyle(PressableStyle())
                .disabled(phase == .disconnecting)
                .accessibilityLabel(phase.accessibilityLabel)
                .sensoryFeedback(.success, trigger: phase) { _, new in new == .on }
                .sensoryFeedback(.impact(weight: .light), trigger: phase) { _, new in new == .connecting }

                VStack(alignment: .leading, spacing: 3) {
                    Text(phase.title)
                        .font(.title3.weight(.semibold))
                        .contentTransition(.opacity)
                    subtitle
                        .font(.footnote.monospacedDigit())
                        .foregroundStyle(Brand.line.opacity(0.75))
                    if let name = tunnel.nodeName {
                        Button {
                            tab = .proxies
                        } label: {
                            HStack(spacing: 4) {
                                Text(ProtocolBadge.abbreviation(for: tunnel.outboundKind))
                                    .font(.caption2.weight(.bold))
                                    .padding(.horizontal, 5)
                                    .padding(.vertical, 2)
                                    .background(Brand.line.opacity(0.14), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
                                Text(name)
                                    .lineLimit(1)
                                Image(systemName: "chevron.right")
                                    .font(.caption2.weight(.semibold))
                                    .accessibilityHidden(true)
                            }
                            .font(.footnote)
                            .foregroundStyle(Brand.line.opacity(0.85))
                            .frame(minHeight: 28)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(Text("当前节点 \(name)"))
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .animation(.easeInOut(duration: 0.22), value: phase)
            }

            if phase != .unconfigured {
                modePicker
            }
        }
        .foregroundStyle(Brand.line)
        .padding(16)
        .frame(maxWidth: .infinity)
        .background(brand.heroGradient, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
    }

    private var modePicker: some View {
        HStack(spacing: 4) {
            ForEach(modes) { mode in
                let selected = tunnel.mode == mode
                Button {
                    Task {
                        await tunnel.setMode(mode)
                    }
                } label: {
                    Text(ProxyModeText.title(mode))
                        .font(.subheadline.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 34)
                        .foregroundStyle(selected ? Brand.deep : Brand.line.opacity(0.85))
                        .background(selected ? Brand.line : Color.clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(4)
        .background(Brand.deep.opacity(0.45), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("分流模式")
    }

    @ViewBuilder
    private var subtitle: some View {
        switch phase {
        case .on:
            if let since = tunnel.connectedSince {
                TimelineView(.periodic(from: since, by: 1)) { context in
                    Text(ConnectElapsed.text(from: since, to: context.date))
                }
            } else {
                Text(" ")
            }
        case .off:
            if case let .blocked(reason, _) = tunnel.coordinator.decision {
                Text(reason)
            } else {
                Text("轻点连接")
            }
        case .unconfigured:
            Text("轻点导入第一个节点")
        case .connecting:
            Text("正在建立安全隧道")
        case .reasserting:
            Text("网络已切换，马上恢复")
        case .disconnecting:
            Text(" ")
        }
    }

    private func tap() {
        if phase == .unconfigured {
            importing = true
        } else {
            tunnel.toggle()
        }
    }
}

/// The connect switch in the top bar while the hero card is scrolled away: the route-line mark and the elapsed
/// time or status.
private struct CompactConnectButton: View {
    @ObservedObject var tunnel: TunnelController
    let phase: ConnectPhase
    @Environment(\.brand) private var brand

    var body: some View {
        Button {
            tunnel.toggle()
        } label: {
            HStack(spacing: 6) {
                RouteLineMark(phase: phase, size: 22)
                label
                    .font(.footnote.weight(.semibold).monospacedDigit())
                    .lineLimit(1)
            }
            .foregroundStyle(Brand.line)
            .padding(.leading, 8)
            .padding(.trailing, 12)
            .frame(height: 40)
            .background(brand.heroGradient, in: Capsule())
        }
        .buttonStyle(PressableStyle())
        .disabled(phase == .disconnecting)
        .accessibilityLabel(phase.accessibilityLabel)
    }

    @ViewBuilder
    private var label: some View {
        if phase == .on, let since = tunnel.connectedSince {
            TimelineView(.periodic(from: since, by: 1)) { context in
                Text(ConnectElapsed.text(from: since, to: context.date))
            }
        } else {
            Text(phase.title)
        }
    }
}

enum ConnectElapsed {
    static func text(from start: Date, to now: Date) -> String {
        let seconds = max(0, Int(now.timeIntervalSince(start)))
        return String(format: "%02d:%02d:%02d", seconds / 3600, seconds / 60 % 60, seconds % 60)
    }
}

/// Shrinks slightly while pressed, without the default list highlight.
struct PressableStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 0.96 : 1)
            .animation(.spring(response: 0.25, dampingFraction: 0.8), value: configuration.isPressed)
    }
}

/// An error or notice: what happened, with a dismiss button. Developer mode has the full diagnostics.
private struct MessageCard: View {
    @Environment(\.brand) private var brand
    let text: String
    let dismiss: () -> Void

    var body: some View {
        BrandCard {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(brand.warning)
                    .accessibilityHidden(true)
                Text(text)
                    .font(.subheadline)
                    .foregroundStyle(brand.text)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
                Button(action: dismiss) {
                    Image(systemName: "xmark")
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(brand.text3)
                        .frame(width: 28, height: 28)
                }
                .accessibilityLabel("关闭提示")
            }
        }
    }
}
