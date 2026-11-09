import ActivityKit
import SwiftUI
import WidgetKit

/// Lock Screen banner and Dynamic Island while the tunnel is up: phase, node, time connected, and a disconnect
/// button that runs the same intent as the widget.
struct TunnelLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: TunnelActivityAttributes.self) { context in
            LockScreenActivityView(state: context.state)
                .activityBackgroundTint(Brand.slate)
                .activitySystemActionForegroundColor(Brand.line)
        } dynamicIsland: { context in
            let state = context.state
            return DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    StaticRouteLine(phase: state.connectPhase, size: 44)
                        .padding(.leading, 4)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    ElapsedText(state: state)
                        .font(.title3.weight(.semibold).monospacedDigit())
                        .foregroundStyle(Brand.sky)
                        .padding(.trailing, 4)
                }
                DynamicIslandExpandedRegion(.center) {
                    VStack(spacing: 2) {
                        Text(state.connectPhase.title)
                            .font(.headline)
                        NodeLine(state: state)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                DynamicIslandExpandedRegion(.bottom) {
                    DisconnectButton()
                        .padding(.top, 4)
                }
            } compactLeading: {
                Image(systemName: "network")
                    .foregroundStyle(state.phase == .connected ? Brand.sky : Brand.amber)
                    .accessibilityLabel(state.connectPhase.title)
            } compactTrailing: {
                ElapsedText(state: state)
                    .font(.caption.weight(.semibold).monospacedDigit())
                    .foregroundStyle(Brand.sky)
                    .frame(maxWidth: 52)
            } minimal: {
                Image(systemName: "network")
                    .foregroundStyle(state.phase == .connected ? Brand.sky : Brand.amber)
                    .accessibilityLabel(state.connectPhase.title)
            }
            .keylineTint(Brand.sky)
        }
    }
}

private struct LockScreenActivityView: View {
    let state: TunnelActivityAttributes.ContentState

    var body: some View {
        HStack(spacing: 14) {
            StaticRouteLine(phase: state.connectPhase, size: 52)
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(state.connectPhase.title)
                        .font(.headline)
                    ElapsedText(state: state)
                        .font(.subheadline.weight(.semibold).monospacedDigit())
                        .foregroundStyle(Brand.sky)
                }
                NodeLine(state: state)
                    .font(.subheadline)
                    .foregroundStyle(Brand.line.opacity(0.8))
            }
            Spacer(minLength: 0)
            DisconnectButton()
        }
        .foregroundStyle(Brand.line)
        .padding(16)
    }
}

/// Protocol badge and node name, or the app name when the node has none.
private struct NodeLine: View {
    let state: TunnelActivityAttributes.ContentState

    var body: some View {
        HStack(spacing: 6) {
            if let name = state.nodeName {
                Text(ProtocolBadge.abbreviation(for: state.outboundKind))
                    .font(.caption2.weight(.bold))
                    .padding(.horizontal, 5)
                    .padding(.vertical, 1)
                    .background(Brand.line.opacity(0.15), in: RoundedRectangle(cornerRadius: 4, style: .continuous))
                Text(name)
                    .lineLimit(1)
            } else {
                Text(verbatim: "AnixOps Net")
            }
        }
    }
}

/// Counts up from the moment the connection came up; empty while connecting.
private struct ElapsedText: View {
    let state: TunnelActivityAttributes.ContentState

    var body: some View {
        if let since = state.connectedSince {
            Text(timerInterval: since ... Date.distantFuture, countsDown: false)
                .multilineTextAlignment(.trailing)
        } else {
            Text(verbatim: "")
        }
    }
}

private struct DisconnectButton: View {
    var body: some View {
        Button(intent: ToggleTunnelIntent(on: false)) {
            Label("断开", systemImage: "stop.fill")
                .font(.subheadline.weight(.semibold))
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
        }
        .buttonStyle(.plain)
        .foregroundStyle(Brand.slate)
        .background(Brand.line, in: Capsule())
    }
}

extension TunnelActivityAttributes.ContentState {
    var connectPhase: ConnectPhase {
        switch phase {
        case .connecting:
            return .connecting
        case .connected:
            return .on
        case .reasserting:
            return .reasserting
        }
    }
}
