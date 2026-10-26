import SwiftUI
import WidgetKit

@main
struct NetworkCoreWidgets: WidgetBundle {
    var body: some Widget {
        StatusWidget()
        if #available(iOS 18.0, *) {
            ConnectControl()
            ProxyModeControl()
        }
    }
}

/// Home Screen and Lock Screen widget: is the tunnel up, through which node, with a connect button.
struct StatusWidget: Widget {
    static let kind = "io.anixops.net.status"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: StatusProvider()) { entry in
            StatusWidgetView(entry: entry)
        }
        .configurationDisplayName("连接状态")
        .description("查看是否已连接，轻点即可连接或断开。")
        .supportedFamilies([.systemSmall, .systemMedium, .accessoryCircular, .accessoryRectangular, .accessoryInline])
    }
}

struct StatusEntry: TimelineEntry {
    let date: Date
    let snapshot: TunnelSnapshot
}

/// The app reloads the timeline whenever the tunnel status changes; the 15-minute refresh catches
/// changes made while the app was not running, such as iOS stopping the tunnel.
struct StatusProvider: TimelineProvider {
    func placeholder(in context: Context) -> StatusEntry {
        StatusEntry(date: .now, snapshot: .placeholder)
    }

    func getSnapshot(in context: Context, completion: @escaping (StatusEntry) -> Void) {
        if context.isPreview {
            completion(placeholder(in: context))
            return
        }
        Task {
            completion(StatusEntry(date: .now, snapshot: await TunnelSnapshot.load()))
        }
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<StatusEntry>) -> Void) {
        Task {
            let entry = StatusEntry(date: .now, snapshot: await TunnelSnapshot.load())
            completion(Timeline(entries: [entry], policy: .after(.now.addingTimeInterval(15 * 60))))
        }
    }
}

/// Control Center, Lock Screen and Action button switch (iOS 18).
@available(iOS 18.0, *)
struct ConnectControl: ControlWidget {
    static let kind = "io.anixops.net.connect"

    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: Self.kind, provider: Provider()) { isOn in
            ControlWidgetToggle("AnixOps Net", isOn: isOn, action: ToggleTunnelIntent()) { on in
                Label(on ? "已连接" : "未连接", systemImage: on ? "network" : "network.slash")
            }
            .tint(Brand.iconTeal)
        }
        .displayName("连接 AnixOps Net")
        .description("在控制中心连接或断开。")
    }

    struct Provider: ControlValueProvider {
        var previewValue: Bool { false }

        func currentValue() async throws -> Bool {
            await TunnelSnapshot.load().isOn
        }
    }
}

/// Control Center button that switches to the mode the user picked when adding it (iOS 18). It shows whether that
/// mode is the current one.
@available(iOS 18.0, *)
struct ProxyModeControl: ControlWidget {
    static let kind = "io.anixops.net.mode"

    var body: some ControlWidgetConfiguration {
        AppIntentControlConfiguration(kind: Self.kind, provider: Provider()) { value in
            ControlWidgetButton(action: SetProxyModeIntent(mode: value.mode)) {
                Label(value.isCurrent ? String(localized: "\(value.title) · 当前") : value.title, systemImage: value.mode.symbol)
            }
            .tint(Brand.iconTeal)
        }
        .displayName("代理模式")
        .description("在控制中心切换到规则、全局或直连模式。")
        .promptsForUserConfiguration()
    }

    struct Value: Sendable {
        let mode: ProxyModeOption
        let isCurrent: Bool

        var title: String {
            switch mode {
            case .rule:
                return String(localized: "规则模式")
            case .global:
                return String(localized: "全局模式")
            case .direct:
                return String(localized: "直连模式")
            }
        }
    }

    struct Provider: AppIntentControlValueProvider {
        func previewValue(configuration: ProxyModeControlConfiguration) -> Value {
            Value(mode: configuration.mode, isCurrent: false)
        }

        func currentValue(configuration: ProxyModeControlConfiguration) async throws -> Value {
            Value(mode: configuration.mode, isCurrent: await TunnelSwitch.currentMode() == configuration.mode.mode)
        }
    }
}

/// Which mode a Control Center mode button switches to.
@available(iOS 18.0, *)
struct ProxyModeControlConfiguration: ControlConfigurationIntent {
    static let title: LocalizedStringResource = "代理模式"

    @Parameter(title: "模式", default: .global)
    var mode: ProxyModeOption

    func perform() async throws -> some IntentResult {
        .result()
    }
}
