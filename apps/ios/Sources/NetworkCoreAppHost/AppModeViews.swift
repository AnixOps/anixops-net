import NetworkCoreBridge
import SwiftUI

extension AppMode {
    var title: LocalizedStringKey {
        switch self {
        case .debug:
            return "调试网络"
        case .proxy:
            // The mode's name, singular; "代理" alone is the Proxies page.
            return "mode.proxy"
        }
    }

    /// One line on what the mode is for.
    var summary: LocalizedStringKey {
        switch self {
        case .debug:
            return "抓包、解密 HTTPS、改写和重放请求，给 App 开发和测试用。不需要节点，流量默认直连。"
        case .proxy:
            return "导入订阅或节点，按规则把流量交给代理或直连。"
        }
    }

    var systemImage: String {
        switch self {
        case .debug:
            return "ladybug"
        case .proxy:
            return "point.3.connected.trianglepath.dotted"
        }
    }
}

extension DebugExit {
    var title: LocalizedStringKey {
        switch self {
        case .direct:
            return "直连"
        case .node:
            return "经过节点"
        }
    }
}

/// First launch (plan A4): which of the two modes the app opens in. Debug first, since the app is a debugging tool
/// with the proxy as its base. Someone who arrives through a subscription link never sees it (`RootView`).
struct ModeChoiceView: View {
    let choose: (AppMode) -> Void
    @Environment(\.brand) private var brand
    @State private var selection = AppMode.debug

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Spacer(minLength: 24)
            BrandMark(size: 72)
            Text("你想怎么用\nAnixOps Net？")
                .font(BrandFont.display)
                .foregroundStyle(brand.text)
                .fixedSize(horizontal: false, vertical: true)
            VStack(spacing: 12) {
                ForEach([AppMode.debug, .proxy]) { mode in
                    option(mode)
                }
            }
            .padding(.top, 4)
            Spacer()
            Button {
                choose(selection)
            } label: {
                Text("继续")
                    .font(.headline)
                    .frame(maxWidth: .infinity, minHeight: 52)
            }
            .buttonStyle(.brandPrimary)
            Text("以后可以在设置里切换，切换不会删除订阅、节点和插件。")
                .font(.footnote)
                .foregroundStyle(brand.text2)
                .frame(maxWidth: .infinity)
                .multilineTextAlignment(.center)
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 16)
        .frame(maxWidth: 520)
        .frame(maxWidth: .infinity)
        .background(brand.bg.ignoresSafeArea())
    }

    private func option(_ mode: AppMode) -> some View {
        let selected = selection == mode
        return Button {
            selection = mode
        } label: {
            HStack(alignment: .top, spacing: 12) {
                BrandIconWell(systemImage: mode.systemImage, size: 36)
                VStack(alignment: .leading, spacing: 4) {
                    Text(mode.title)
                        .font(.headline)
                        .foregroundStyle(brand.text)
                    Text(mode.summary)
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 8)
                Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                    .font(.title3)
                    .foregroundStyle(selected ? brand.primary : brand.text3)
                    .accessibilityHidden(true)
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(brand.surface, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous)
                    .strokeBorder(selected ? brand.primary : brand.border, lineWidth: selected ? 2 : 1)
            )
            .contentShape(RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }
}

/// The settings rows that pick the mode, and in debug mode the exit: an instant switch each (a segmented picker,
/// AnixOps Design S2), with what the choice changes underneath.
struct AppModeSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            Picker("使用方式", selection: Binding(
                get: { tunnel.appMode ?? .proxy },
                set: { next in
                    Task {
                        await tunnel.setAppMode(next)
                    }
                }
            )) {
                ForEach(AppMode.allCases) { mode in
                    Text(mode.title).tag(mode)
                }
            }
            .pickerStyle(.segmented)
        } header: {
            Text("使用方式")
        } footer: {
            Text(tunnel.appMode == .debug
                ? "调试模式把请求放在首位，订阅和节点收进“出口”。切换不会删除任何数据，连接中会自动重连。"
                : "代理模式按订阅规则分流。切换不会删除任何数据，连接中会自动重连。")
        }
    }
}

/// Debug mode's exit picker: direct, or through the selected node.
struct DebugExitPicker: View {
    var tunnel: TunnelController

    var body: some View {
        Picker("出口", selection: Binding(
            get: { tunnel.debugExit },
            set: { next in
                Task {
                    await tunnel.setDebugExit(next)
                }
            }
        )) {
            ForEach(DebugExit.allCases) { exit in
                Text(exit.title).tag(exit)
            }
        }
        .pickerStyle(.segmented)
    }

    /// What the exit does now, for the footer under the picker.
    static func footer(_ tunnel: TunnelController) -> LocalizedStringKey {
        switch tunnel.debugExit {
        case .direct:
            return "抓到的请求直接发往目标服务器，不经过任何节点。"
        case .node:
            if tunnel.hasNoNodes {
                return "还没有节点，暂时直连。导入订阅或节点后，请求经过选中的节点发出，可以用来对比不同地区的出口。"
            }
            return "请求经过选中的节点发出，按订阅规则分流；可以切换节点对比不同地区的出口。"
        }
    }
}

/// A tab's root wraps its page in a navigation stack; the same page pushed from another one must not add a second.
struct NavigationContainer<Content: View>: View {
    let embedded: Bool
    @ViewBuilder let content: Content

    var body: some View {
        if embedded {
            content
        } else {
            NavigationStack {
                content
            }
        }
    }
}
