import NetworkCoreBridge
import NetworkExtension
import SwiftUI

/// Home: the connect card answers "am I connected, through which node, in which mode"; below it a dashboard of
/// cards the user can reorder and hide. Once the connect card scrolls away a compact switch takes its place in
/// the top bar.
struct HomeView: View {
    var tunnel: TunnelController
    @Binding var tab: AppTab
    @Binding var importing: Bool
    @Environment(\.brand) private var brand
    @State private var dashboard = DashboardModel()
    @AppStorage(DashboardLayout.storageKey) private var layoutStorage = ""
    @State private var heroVisible = true
    @State private var editing = false
    @State private var picking = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// The top bar's round buttons grow with Dynamic Type.
    @ScaledMetric(relativeTo: .body) private var headerButton: CGFloat = 40

    private static let scrollSpace = "home-scroll"
    private static let headerHeight: CGFloat = 52

    private var phase: ConnectPhase {
        ConnectPhase(status: tunnel.connection, hasNode: !tunnel.hasNoNodes)
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 12) {
                    ConnectCard(tunnel: tunnel, phase: phase, importing: $importing, picking: $picking)
                        .background(
                            GeometryReader { proxy in
                                Color.clear.preference(
                                    key: HeroBottomKey.self,
                                    value: proxy.frame(in: .named(Self.scrollSpace)).maxY
                                )
                            }
                        )
                    if let notice = tunnel.notice {
                        NoticeCard(notice: notice, perform: perform) {
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
                    withAnimation(reduceMotion ? nil : .easeInOut(duration: BrandMotion.standard)) {
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
            .sheet(isPresented: $picking) {
                NodePickerSheet(tunnel: tunnel) {
                    tab = .proxies
                }
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

    private func perform(_ action: AppNotice.Action) {
        switch action {
        case .retry:
            tunnel.clearMessage()
            tunnel.toggle()
        case .chooseNode:
            picking = true
        case let .refreshSubscription(groupID):
            tunnel.clearMessage()
            Task {
                await tunnel.refreshSubscription(groupID)
            }
        case .importNode:
            tunnel.clearMessage()
            importing = true
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
                    .font(.body.weight(.semibold))
                    .foregroundStyle(brand.primary)
                    .frame(width: headerButton, height: headerButton)
                    .brandGlass(Circle(), fallback: brand.raised)
            }
            .accessibilityLabel("编辑首页")
            Button {
                importing = true
            } label: {
                Image(systemName: "plus")
                    .font(.body.weight(.semibold))
                    .foregroundStyle(brand.primary)
                    .frame(width: headerButton, height: headerButton)
                    .brandGlass(Circle(), fallback: brand.raised)
            }
            .accessibilityLabel("导入节点")
        }
        .frame(minHeight: Self.headerHeight)
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
    var tunnel: TunnelController
    let phase: ConnectPhase
    @Binding var importing: Bool
    /// Opens the node picker.
    @Binding var picking: Bool
    @Environment(\.brand) private var brand
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dynamicTypeSize) private var typeSize

    /// Side by side normally; at the accessibility text sizes the status goes under the button so the node name
    /// and elapsed time keep a full line.
    private var heroLayout: AnyLayout {
        typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 12))
            : AnyLayout(HStackLayout(spacing: 16))
    }

    /// The rule mode needs a node list (subscription rules or the local rules); a single pasted node offers global
    /// and direct only.
    private var modes: [ProxyMode] {
        tunnel.library == nil ? [.global, .direct] : ProxyMode.allCases
    }

    var body: some View {
        VStack(spacing: 14) {
            heroLayout {
                Button(action: tap) {
                    RouteLineMark(phase: phase, size: 60)
                        .frame(width: 92, height: 92)
                        .background(Brand.line.opacity(0.06), in: Circle())
                        .overlay(
                            Circle().strokeBorder(phase == .on ? Brand.sky.opacity(0.7) : Brand.line.opacity(0.25), lineWidth: 1.5)
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
                            picking = true
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
                        .accessibilityHint(Text("选择其他节点"))
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .animation(reduceMotion ? nil : .easeInOut(duration: BrandMotion.standard), value: phase)
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
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                        .frame(maxWidth: .infinity, minHeight: 34)
                        .foregroundStyle(selected ? Brand.slate : Brand.line.opacity(0.85))
                        .background(selected ? Brand.line : Color.clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(4)
        .background(Brand.slate.opacity(0.45), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
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
    var tunnel: TunnelController
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
/// With Reduce Motion on it does not scale; the press still shows as a slight fade.
struct PressableStyle: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed && !reduceMotion ? 0.96 : 1)
            .opacity(configuration.isPressed && reduceMotion ? 0.8 : 1)
            .animation(reduceMotion ? nil : .spring(response: 0.25, dampingFraction: 0.8), value: configuration.isPressed)
    }
}

/// An error or notice: what happened in one plain sentence, the buttons that fix it, and the raw error folded
/// under 技术详情. Developer mode has the full diagnostics.
private struct NoticeCard: View {
    @Environment(\.brand) private var brand
    let notice: AppNotice
    let perform: (AppNotice.Action) -> Void
    let dismiss: () -> Void

    var body: some View {
        BrandCard {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(brand.warning)
                        .accessibilityHidden(true)
                    Text(notice.text)
                        .font(.subheadline)
                        .foregroundStyle(brand.text)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                    Button(action: dismiss) {
                        Image(systemName: "xmark")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(brand.text3)
                            .frame(width: 28, height: 28)
                            // A 44 pt target without moving the icon.
                            .padding(8)
                            .contentShape(Rectangle())
                    }
                    .padding(-8)
                    .accessibilityLabel("关闭提示")
                }
                if !notice.actions.isEmpty {
                    // Stacked when large text no longer fits the buttons side by side.
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) {
                            actionButtons
                        }
                        VStack(spacing: 8) {
                            actionButtons
                        }
                    }
                }
                if let detail = notice.detail {
                    DisclosureGroup {
                        Text(detail)
                            .font(.caption.monospaced())
                            .foregroundStyle(brand.text2)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(10)
                            .background(brand.raised, in: RoundedRectangle(cornerRadius: BrandRadius.chip, style: .continuous))
                            .textSelection(.enabled)
                    } label: {
                        Text("技术详情")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(brand.text2)
                    }
                    .tint(brand.text2)
                }
            }
        }
    }

    private var actionButtons: some View {
        ForEach(Array(notice.actions.enumerated()), id: \.offset) { index, action in
            actionButton(action, primary: index == 0)
        }
    }

    @ViewBuilder
    private func actionButton(_ action: AppNotice.Action, primary: Bool) -> some View {
        let button = Button {
            perform(action)
        } label: {
            Text(Self.title(action))
                .font(.subheadline.weight(.semibold))
                .fixedSize(horizontal: true, vertical: false)
                .frame(maxWidth: .infinity, minHeight: 36)
        }
        if primary {
            button.buttonStyle(.borderedProminent).tint(brand.primary)
        } else {
            button.buttonStyle(.bordered).tint(brand.primary)
        }
    }

    private static func title(_ action: AppNotice.Action) -> LocalizedStringKey {
        switch action {
        case .retry:
            return "重试"
        case .chooseNode:
            return "换个节点"
        case .refreshSubscription:
            return "重新下载订阅"
        case .importNode:
            return "导入节点"
        }
    }
}
