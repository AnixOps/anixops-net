import AppKit
import NetworkCoreBridge
import NetworkExtension
import ServiceManagement
import SwiftUI

/// Overview, a dashboard like the Windows and Linux clients: the connection with the node and mode next to the
/// subscription's allowance, then the live cards the iOS home screen shows (speed, usage, network, policy
/// groups, busiest hosts, DNS, exit address, events), four slots a row.
struct MacHomePage: View {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState

    /// Shared with the dashboard window, so both show one sampler's numbers.
    private var dashboard: DashboardModel { state.dashboard }

    var body: some View {
        GeometryReader { window in
            ScrollView {
                VStack(spacing: 12) {
                    HStack(alignment: .top, spacing: 12) {
                        MacHeroCard(tunnel: tunnel, state: state)
                            .frame(maxHeight: .infinity)
                        MacSubscriptionCard(tunnel: tunnel, state: state)
                            .frame(width: min(320, window.size.width * 0.34))
                            .frame(maxHeight: .infinity)
                    }
                    .fixedSize(horizontal: false, vertical: true)
                    if let message = tunnel.message {
                        MacNotice(text: message) {
                            tunnel.clearMessage()
                        }
                    }
                    DashboardGrid(
                        tunnel: tunnel,
                        model: dashboard,
                        layout: DashboardLayout(storage: ""),
                        fixedColumns: window.size.width >= 720 ? 4 : 2,
                        showProxies: { state.section = .proxies }
                    )
                }
                .padding(20)
                .frame(maxWidth: 1240)
                .frame(maxWidth: .infinity)
            }
        }
        .task(id: tunnel.connection) {
            #if DEBUG
            if TunnelController.isDemo, tunnel.connection == .connected {
                dashboard.loadDemo()
            }
            #endif
        }
        .task {
            await dashboard.observe(tunnel)
        }
    }
}

/// The dashboard on its own: every live card in a window that can stay open beside other apps while the main
/// window is closed or on another page. It reads the same `DashboardModel` as the overview, so the two never
/// poll the engine twice.
struct MacDashboardWindow: View {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let brand = BrandPalette.resolve(appearance: appearance, scheme: colorScheme)
        GeometryReader { window in
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    HStack(spacing: 8) {
                        Circle()
                            .fill(tunnel.isConnected ? MacTileColor.green : MacTileColor.gray)
                            .frame(width: 8, height: 8)
                        Text(tunnel.phase.title)
                            .font(.headline)
                        if let name = tunnel.nodeName {
                            Text(verbatim: name)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }
                    DashboardGrid(
                        tunnel: tunnel,
                        model: state.dashboard,
                        layout: DashboardLayout(storage: ""),
                        fixedColumns: window.size.width >= 720 ? 4 : 2,
                        showProxies: {
                            state.section = .proxies
                            openWindow(id: MacAppState.mainWindowID)
                        }
                    )
                }
                .padding(16)
            }
        }
        .background(brand.bg)
        .tint(brand.primary)
        .environment(\.brand, brand)
        .preferredColorScheme(appearance.colorScheme)
        .task {
            await state.dashboard.observe(tunnel)
        }
    }
}

/// The connection on the slate hero: the route-line mark, the state with the elapsed time, the switch, and under
/// them the current node and the proxy mode.
private struct MacHeroCard: View {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState
    @Environment(\.brand) private var brand

    private var phase: ConnectPhase { tunnel.phase }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 16) {
                Button(action: tap) {
                    RouteLineMark(phase: phase, size: 44)
                        .frame(width: 68, height: 68)
                        .background(Brand.line.opacity(0.06), in: Circle())
                        .overlay(
                            Circle().strokeBorder(phase == .on ? Brand.sky.opacity(0.7) : Brand.line.opacity(0.25), lineWidth: 1.5)
                        )
                        .contentShape(Circle())
                }
                .buttonStyle(MacPressableStyle())
                .disabled(phase == .disconnecting)
                .accessibilityLabel(phase.accessibilityLabel)

                VStack(alignment: .leading, spacing: 3) {
                    Text(phase.title)
                        .font(.title.weight(.bold))
                        .contentTransition(.opacity)
                    subtitle
                        .font(.callout.monospacedDigit())
                        .foregroundStyle(Brand.lineMuted)
                        .lineLimit(2)
                }
                .animation(.easeInOut(duration: 0.22), value: phase)
                Spacer(minLength: 8)
                if phase == .unconfigured {
                    Button {
                        state.importing = true
                    } label: {
                        Text("导入节点")
                            .font(.body.weight(.semibold))
                            .padding(.horizontal, 10)
                    }
                    .buttonStyle(MacHeroButtonStyle(filled: true))
                } else {
                    MacConnectSwitch(tunnel: tunnel, controlSize: .large)
                }
            }
            Spacer(minLength: 0)
            HStack(spacing: 10) {
                Button {
                    state.section = .proxies
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "globe")
                        Text(tunnel.nodeName ?? String(localized: "还没有节点"))
                            .lineLimit(1)
                        Image(systemName: "chevron.up.chevron.down")
                            .font(.caption2.weight(.semibold))
                            .opacity(0.6)
                    }
                    .font(.callout.weight(.medium))
                    .padding(.horizontal, 12)
                    .frame(height: 30)
                    .background(Color.white.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
                .buttonStyle(.plain)
                .help("切换节点")
                Spacer(minLength: 8)
                Picker("代理模式", selection: Binding(
                    get: { tunnel.mode },
                    set: { mode in
                        Task {
                            await tunnel.setMode(mode)
                        }
                    }
                )) {
                    if tunnel.library?.routing != nil {
                        Text("规则").tag(ProxyMode.rule)
                    }
                    Text("全局").tag(ProxyMode.global)
                    Text("直连").tag(ProxyMode.direct)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(maxWidth: 240)
                .disabled(phase == .unconfigured)
                // The hero is slate in both themes, so its controls always use the dark appearance.
                .environment(\.colorScheme, .dark)
            }
        }
        .foregroundStyle(Brand.line)
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(brand.heroGradient, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous)
                .strokeBorder(Color.white.opacity(0.08), lineWidth: 1)
        )
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
                Text("安全隧道已建立")
            }
        case .off:
            if case let .blocked(reason, _) = tunnel.coordinator.decision {
                Text(reason)
            } else {
                Text("打开开关连接，或按 ⇧⌘C")
            }
        case .unconfigured:
            Text("导入第一个节点后即可连接")
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
            state.importing = true
        } else {
            tunnel.toggle()
        }
    }
}

/// The subscription in use: the used share of its allowance as a ring, the expiry and the last update.
private struct MacSubscriptionCard: View {
    var tunnel: TunnelController
    @ObservedObject var state: MacAppState
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 6) {
                Text(tunnel.subscriptions.active?.name ?? String(localized: "订阅"))
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(brand.text2)
                    .lineLimit(1)
                Spacer(minLength: 4)
                if let group = tunnel.subscriptions.active, group.isUpdatable {
                    if tunnel.updatingGroups.contains(group.id) {
                        ProgressView()
                            .controlSize(.small)
                    } else {
                        Button("更新") {
                            Task {
                                _ = await tunnel.refreshSubscription(group.id)
                            }
                        }
                        .buttonStyle(.link)
                        .font(.footnote.weight(.semibold))
                    }
                }
            }
            if let group = tunnel.subscriptions.active {
                allowance(group)
                Spacer(minLength: 0)
                VStack(spacing: 0) {
                    fact("到期") {
                        if let expire = group.userInfo?.expire {
                            Text(expire, format: .dateTime.year().month().day())
                                .foregroundStyle(expire < Date() ? brand.danger : brand.text)
                        } else {
                            Text("长期有效")
                        }
                    }
                    Divider()
                    fact("上次更新") {
                        Text(group.library.updatedAt, format: .relative(presentation: .named))
                    }
                }
            } else {
                Text("还没有订阅。导入后节点会出现在“代理”里。")
                    .font(.callout)
                    .foregroundStyle(brand.text2)
                Spacer(minLength: 0)
                Button("导入节点") {
                    state.importing = true
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .background(brand.surface, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
    }

    private func allowance(_ group: SubscriptionGroup) -> some View {
        let info = group.userInfo
        let fraction = info?.usedFraction
        return HStack(spacing: 14) {
            ZStack {
                Circle()
                    .stroke(brand.border, lineWidth: 7)
                if let fraction {
                    Circle()
                        .trim(from: 0, to: max(0.01, min(1, fraction)))
                        .stroke(fraction > 0.9 ? brand.danger : brand.primary, style: StrokeStyle(lineWidth: 7, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                }
                Text(fraction.map { "\(Int(($0 * 100).rounded()))%" } ?? "–")
                    .font(.caption.weight(.semibold).monospacedDigit())
                    .foregroundStyle(brand.text)
            }
            .frame(width: 56, height: 56)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                if let used = info?.used {
                    if let total = info?.total, total > 0 {
                        Text(verbatim: "\(Self.bytes(used)) / \(Self.bytes(total))")
                            .font(.title3.weight(.semibold).monospacedDigit())
                            .foregroundStyle(brand.text)
                    } else {
                        Text("已用 \(Self.bytes(used))")
                            .font(.title3.weight(.semibold).monospacedDigit())
                            .foregroundStyle(brand.text)
                    }
                } else {
                    Text("\(group.library.nodes.count) 个节点")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(brand.text)
                }
                Group {
                    if info?.used != nil {
                        Text("\(group.library.nodes.count) 个节点")
                    } else {
                        Text("订阅没有提供流量信息")
                    }
                }
                .font(.caption)
                .foregroundStyle(brand.text2)
            }
            .lineLimit(1)
            .minimumScaleFactor(0.8)
        }
    }

    private func fact<Value: View>(_ title: LocalizedStringKey, @ViewBuilder value: () -> Value) -> some View {
        HStack {
            Text(title)
                .foregroundStyle(brand.text2)
            Spacer(minLength: 8)
            value()
                .foregroundStyle(brand.text)
                .lineLimit(1)
        }
        .font(.callout)
        .frame(minHeight: 30)
    }

    private static func bytes(_ value: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: value, countStyle: .binary)
    }
}

/// A section whose kernel or platform support is still on the roadmap: says what will live here.
struct MacPlaceholderPage: View {
    let systemImage: String
    let title: LocalizedStringKey
    let message: LocalizedStringKey

    var body: some View {
        BrandEmptyState(systemImage: systemImage, title: title, message: message)
            .frame(maxWidth: 520)
            .padding(.top, 80)
    }
}

/// Activity: the session's facts in one strip, and under it the request list itself (what the tunnel carries now,
/// what just ended, and the totals by host) instead of a link to it.
struct MacActivityPage: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var copied = false
    @State private var showsLog = false

    var body: some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 12) {
                    fact("连接", ConnectPhase.describe(tunnel.connection))
                    fact("节点", tunnel.nodeName ?? String(localized: "未导入"))
                    fact("出站协议", ProtocolBadge.spokenName(for: tunnel.outboundKind))
                    fact("上次停止原因", tunnel.lastDisconnectError ?? String(localized: "无记录"))
                }
                Divider()
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(tunnel.runtimeStatus ?? String(localized: "隧道未运行"))
                        .font(.caption.monospaced())
                        .foregroundStyle(brand.text2)
                        .textSelection(.enabled)
                        .lineLimit(3)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Button("立即刷新") {
                        refresh()
                    }
                    .controlSize(.small)
                    // The extension's event log, runtime status and versions, without node names, addresses or
                    // keys: the Mac counterpart of the iOS developer page's report.
                    Button(copied ? "已复制" : "复制诊断信息") {
                        copyDiagnostics()
                    }
                    .controlSize(.small)
                    Button("扩展日志…") {
                        tunnel.refreshDiagnostics()
                        showsLog = true
                    }
                    .controlSize(.small)
                }
            }
            .padding(16)
            .background(brand.surface, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous).strokeBorder(brand.border.opacity(0.6)))
            .padding(.horizontal, 20)
            .padding(.top, 16)
            ConnectionsView(tunnel: tunnel)
        }
        .background(brand.bg)
        .onAppear(perform: refresh)
        .sheet(isPresented: $showsLog) {
            MacDiagnosticLogSheet(tunnel: tunnel)
        }
    }

    private func fact(_ title: LocalizedStringKey, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(.caption)
                .foregroundStyle(brand.text2)
            Text(value)
                .font(.callout.weight(.semibold))
                .foregroundStyle(brand.text)
                .lineLimit(2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func refresh() {
        copied = false
        tunnel.refreshRuntimeStatus()
        tunnel.refreshLastDisconnectError()
        tunnel.refreshDiagnostics()
    }

    /// Asks the extension for its diagnostics, then copies the report once the answer is in.
    private func copyDiagnostics() {
        tunnel.refreshDiagnostics()
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(500))
            let info = Bundle.main.infoDictionary
            let report = DeveloperReport(
                appVersion: info?["CFBundleShortVersionString"] as? String ?? "unknown",
                appBuild: info?["CFBundleVersion"] as? String ?? "unknown",
                systemVersion: "macOS \(ProcessInfo.processInfo.operatingSystemVersionString)",
                connection: ConnectPhase.describe(tunnel.connection),
                outboundKind: tunnel.outboundKind,
                lastDisconnectError: tunnel.lastDisconnectError,
                diagnostics: tunnel.diagnostics,
                generatedAt: Date(),
                health: ClientHealthMonitor.shared.reportText
            )
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(report.text, forType: .string)
            copied = true
        }
    }
}

/// Settings: appearance, login item, reconnecting and Wi-Fi rules, notifications, privacy and the version.
struct MacSettingsPage: View {
    var tunnel: TunnelController
    @AppStorage(Appearance.storageKey) private var appearance = Appearance.system
    @Environment(\.brand) private var brand
    @State private var opensAtLogin = SMAppService.mainApp.status == .enabled
    @State private var loginError: String?
    @State private var crashReports = ClientHealthMonitor.shared.crashReports.isEnabled

    var body: some View {
        Form {
            Section("外观") {
                Picker("外观", selection: $appearance.normalized) {
                    ForEach(Appearance.choices) { option in
                        Text(option.label).tag(option)
                    }
                }
                .pickerStyle(.segmented)
            }

            Section("启动") {
                Toggle("登录 Mac 后打开 AnixOps Net", isOn: $opensAtLogin)
                    .onChange(of: opensAtLogin) { _, enabled in
                        // Also fires when a failed change is reverted below; only act on a real change.
                        if enabled != (SMAppService.mainApp.status == .enabled) {
                            setOpensAtLogin(enabled)
                        }
                    }
                if let loginError {
                    Text(loginError)
                        .font(.caption)
                        .foregroundStyle(brand.danger)
                }
            }

            OnDemandSections(tunnel: tunnel)
            NotificationSettingsSection(tunnel: tunnel)

            Section {
                Label("不收集你的流量和浏览记录", systemImage: "hand.raised")
                Label("节点密码和密钥只保存在本机钥匙串", systemImage: "key")
                Toggle("崩溃报告", isOn: $crashReports)
                    .onChange(of: crashReports) { _, enabled in
                        ClientHealthMonitor.shared.crashReports.isEnabled = enabled
                    }
            } header: {
                Text("隐私")
            } footer: {
                Text("崩溃时记录脱敏后的信息（不含网址、账号和节点地址），只保存在本机，复制诊断信息时附上。关闭会删除已保存的报告。")
            }

            MacUpdateSection()

            MacCommandLineSection()

            Section("关于") {
                LabeledContent("版本", value: Self.version)
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .frame(maxWidth: 720)
        .frame(maxWidth: .infinity)
    }

    private func setOpensAtLogin(_ enabled: Bool) {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            loginError = nil
        } catch {
            loginError = String(localized: "无法更改登录项，请在 系统设置 > 通用 > 登录项 中调整。")
        }
        let actual = SMAppService.mainApp.status == .enabled
        if actual != opensAtLogin {
            opensAtLogin = actual
        }
    }

    private static var version: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "—"
        let build = info?["CFBundleVersion"] as? String ?? "—"
        return "\(short) (\(build))"
    }
}

/// Settings > Command Line: how to put the bundled `anixops` tool on the PATH (docs/macos-cli.md). The app is
/// sandboxed and cannot write /usr/local/bin itself, so it shows the command to copy.
private struct MacCommandLineSection: View {
    @State private var copied = false

    private var command: String {
        let tool = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/anixops").path
        return "sudo mkdir -p /usr/local/bin && sudo ln -sf '\(tool)' /usr/local/bin/anixops"
    }

    var body: some View {
        Section("命令行") {
            Text("在终端用 anixops 查看状态、连接、切换节点、测速和导入。在终端运行下面的命令后即可使用：")
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack(alignment: .top) {
                Text(command)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                Spacer()
                Button(copied ? "已复制" : "复制") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(command, forType: .string)
                    copied = true
                }
            }
        }
    }
}

/// Settings > Software Update: check, download, verify and install a signed update (AppUpdater). Only the action
/// that applies to the current step is shown.
private struct MacUpdateSection: View {
    @Environment(AppUpdater.self) private var updater
    @Environment(\.brand) private var brand

    var body: some View {
        Section("软件更新") {
            if updater.isEnabled {
                Toggle("自动检查更新", isOn: Binding(
                    get: { updater.automatic },
                    set: { updater.setAutomatic($0) }
                ))
                status
            } else {
                Text("这个版本没有配置更新源，请从发布页下载新版本。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    @ViewBuilder
    private var status: some View {
        switch updater.state {
        case .disabled:
            EmptyView()
        case .idle:
            checkRow(detail: nil)
        case .checking:
            HStack {
                Text("正在检查更新…")
                Spacer()
                ProgressView()
                    .controlSize(.small)
            }
        case let .upToDate(date):
            checkRow(detail: String(localized: "已是最新版本") + " · " + date.formatted(date: .omitted, time: .shortened))
        case let .available(item):
            available(item)
            HStack {
                Spacer()
                Button("下载更新") {
                    Task { await updater.download() }
                }
                .buttonStyle(.brandPrimary)
            }
        case let .downloading(item, fraction):
            VStack(alignment: .leading, spacing: 6) {
                Text("正在下载 \(item.version)…")
                ProgressView(value: fraction)
            }
        case let .ready(item, _):
            available(item)
            Text("安装包已下载并通过签名校验。安装程序打开后 AnixOps Net 会退出，安装完成后重新打开即可。")
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("安装并退出") {
                    updater.install()
                }
                .buttonStyle(.brandPrimary)
            }
        case let .failed(message):
            checkRow(detail: message, failed: true)
        }
    }

    private func checkRow(detail: String?, failed: Bool = false) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text("当前版本 \(updater.currentVersion)")
                if let detail {
                    Text(detail)
                        .font(.caption)
                        .foregroundStyle(failed ? brand.danger : .secondary)
                }
            }
            Spacer()
            Button("检查更新") {
                Task { await updater.check() }
            }
        }
    }

    @ViewBuilder
    private func available(_ item: UpdateItem) -> some View {
        LabeledContent("新版本", value: "\(item.version) (\(item.build))")
        if let notes = item.notes?.trimmingCharacters(in: .whitespacesAndNewlines), !notes.isEmpty {
            Text(notes)
                .font(.callout)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        }
    }
}

/// A titled surface card in the current palette.
struct MacCard<Content: View>: View {
    @Environment(\.brand) private var brand
    let title: LocalizedStringKey
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.headline)
                .foregroundStyle(brand.text)
            content
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(brand.surface, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous).strokeBorder(brand.border.opacity(0.6)))
    }
}

/// An error or notice with a dismiss button.
private struct MacNotice: View {
    @Environment(\.brand) private var brand
    let text: String
    let dismiss: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(brand.warning)
                .accessibilityHidden(true)
            Text(text)
                .font(.callout)
                .foregroundStyle(brand.text)
                .frame(maxWidth: .infinity, alignment: .leading)
                .textSelection(.enabled)
            Button(action: dismiss) {
                Image(systemName: "xmark")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("关闭提示")
        }
        .padding(14)
        .background(brand.surface, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous).strokeBorder(brand.warning.opacity(0.4)))
    }
}

/// Shrinks slightly while pressed.
private struct MacPressableStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 0.96 : 1)
            .animation(.spring(response: 0.25, dampingFraction: 0.8), value: configuration.isPressed)
    }
}

/// The hero's action pill on slate: solid near-white with slate text, or a translucent white pill.
private struct MacHeroButtonStyle: ButtonStyle {
    let filled: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 6)
            .frame(minHeight: 32)
            .foregroundStyle(filled ? Brand.slate : Brand.line)
            .background(filled ? Brand.line : Color.white.opacity(0.14), in: Capsule())
            .contentShape(Capsule())
            .opacity(configuration.isPressed ? 0.82 : 1)
    }
}

/// The extension's event log with the level and module filters, newest first.
private struct MacDiagnosticLogSheet: View {
    var tunnel: TunnelController
    @Environment(\.dismiss) private var dismiss
    @AppStorage(DiagnosticFilterKeys.minimum) private var minimum = DiagnosticLevel.info
    @AppStorage(DiagnosticFilterKeys.module) private var module = ""

    var body: some View {
        let events = (tunnel.diagnostics?.events ?? [])
            .filtered(minimum: minimum, modules: DiagnosticFilterKeys.modules(module))
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                DiagnosticFilterPickers(minimum: $minimum, module: $module)
                    .fixedSize()
                Spacer()
                Button("立即刷新") {
                    tunnel.refreshDiagnostics()
                }
                Button("完成") {
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(12)
            Divider()
            if events.isEmpty {
                Text(tunnel.diagnostics == nil ? "隧道未运行" : "没有符合条件的日志")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(Array(events.reversed().enumerated()), id: \.offset) { _, event in
                    DiagnosticEventRow(event: event)
                }
            }
        }
        .frame(width: 640, height: 480)
    }
}
