import NetworkCoreBridge
import SwiftUI

// Subscription group pieces shared by the iOS proxies tab and the macOS proxies page.

/// A group's actions: update now, the automatic update interval, rename and delete. `rename` and `delete` let the
/// page show its own prompt.
struct SubscriptionGroupMenu: View {
    @ObservedObject var tunnel: TunnelController
    let group: SubscriptionGroup
    let rename: () -> Void
    let delete: () -> Void

    var body: some View {
        Menu {
            if group.isUpdatable {
                Button {
                    Task {
                        await tunnel.refreshSubscription(group.id)
                    }
                } label: {
                    Label("立即更新", systemImage: "arrow.clockwise")
                }
                .disabled(tunnel.updatingGroups.contains(group.id))
                Picker(selection: Binding(
                    get: { group.updateIntervalHours },
                    set: { tunnel.setUpdateInterval(group.id, hours: $0) }
                )) {
                    ForEach(UpdateInterval.choices(including: group.updateIntervalHours), id: \.self) { hours in
                        Text(UpdateInterval.label(hours)).tag(hours)
                    }
                } label: {
                    Label("自动更新", systemImage: "clock.arrow.circlepath")
                }
                .pickerStyle(.menu)
            }
            Button(action: rename) {
                Label("重命名", systemImage: "pencil")
            }
            Button(role: .destructive, action: delete) {
                Label("删除", systemImage: "trash")
            }
        } label: {
            Image(systemName: "ellipsis.circle")
                .font(.body)
        }
        .accessibilityLabel("订阅组操作")
    }
}

/// A node's delay test result.
struct NodeLatencyText: View {
    let result: NodeLatency?
    @Environment(\.brand) private var brand

    var body: some View {
        switch result {
        case .none:
            EmptyView()
        case .measuring:
            ProgressView()
                .controlSize(.small)
        case let .millis(value):
            Text("\(value) ms")
                .font(.footnote.monospacedDigit())
                .foregroundStyle(value < 300 ? brand.text2 : brand.danger)
        case .timeout:
            Text("超时")
                .font(.footnote)
                .foregroundStyle(brand.danger)
        case .failed:
            Text("失败")
                .font(.footnote)
                .foregroundStyle(brand.danger)
        case .unsupported:
            Text("不支持")
                .font(.footnote)
                .foregroundStyle(brand.text3)
        }
    }
}

/// The first row of a subscription group: node count, last update, traffic used and expiry.
struct SubscriptionGroupSummary: View {
    let group: SubscriptionGroup
    let updating: Bool
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Text("\(group.library.nodes.count) 个节点")
                Text(verbatim: "·")
                if updating {
                    ProgressView()
                        .controlSize(.mini)
                    Text("正在更新")
                } else {
                    Text("更新于 \(group.library.updatedAt, style: .relative)前")
                }
                Spacer(minLength: 0)
                if group.isUpdatable {
                    Text(UpdateInterval.label(group.updateIntervalHours))
                        .foregroundStyle(brand.text3)
                }
            }
            .font(.footnote)
            .foregroundStyle(brand.text2)

            if let info = group.userInfo {
                if let fraction = info.usedFraction {
                    ProgressView(value: fraction)
                        .tint(fraction > 0.9 ? brand.danger : brand.primary)
                }
                HStack(spacing: 6) {
                    if let used = info.used {
                        if let total = info.total, total > 0 {
                            Text("已用 \(Self.bytes(used)) / \(Self.bytes(total))")
                        } else {
                            Text("已用 \(Self.bytes(used))")
                        }
                    }
                    Spacer(minLength: 0)
                    if let expire = info.expire {
                        Text("到期 \(expire, format: .dateTime.year().month().day())")
                            .foregroundStyle(expire < Date() ? brand.danger : brand.text2)
                    }
                }
                .font(.caption)
                .foregroundStyle(brand.text2)
            }

            if let failed = group.lastFailureAt, !updating {
                Label("上次更新失败（\(failed, format: .dateTime.month().day().hour().minute())）", systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(brand.danger)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }

    static func bytes(_ value: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: value, countStyle: .binary)
    }
}

/// Automatic update intervals offered for a subscription group, in hours; 0 is by hand.
enum UpdateInterval {
    static let presets = [0, 1, 3, 6, 12, 24, 48, 72, 168]

    /// The presets, plus a server-chosen interval that is not one of them.
    static func choices(including current: Int) -> [Int] {
        presets.contains(current) ? presets : (presets + [current]).sorted()
    }

    static func label(_ hours: Int) -> String {
        switch hours {
        case 0:
            return String(localized: "手动更新")
        case 168:
            return String(localized: "每周自动更新")
        case let hours where hours % 24 == 0:
            return String(localized: "每 \(hours / 24) 天自动更新")
        default:
            return String(localized: "每 \(hours) 小时自动更新")
        }
    }
}
