import NetworkCoreBridge
import SwiftUI

/// Debug mode's connect switch on iPhone and iPad. Debug mode has no overview, which is where the proxy client's
/// connect card lives, so the tunnel is started and stopped here instead: a bar above the tab bar (or at the foot
/// of an iPad window's detail column) with the state, the exit and a switch. It stays in the same place on every
/// tab and on pages pushed from them, like the Mac sidebar's switch (docs/ui-ux.md, scope S1).
struct DebugConnectBar: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    private var phase: ConnectPhase {
        ConnectPhase(status: tunnel.connection, hasNode: tunnel.canConnect)
    }

    var body: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 10) {
                Circle()
                    .fill(dotColor)
                    .frame(width: 8, height: 8)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 1) {
                    Text(phase.title)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(brand.text)
                    detail
                        .font(.footnote.monospacedDigit())
                        .lineLimit(2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityElement(children: .combine)
                if tunnel.notice != nil {
                    Button {
                        tunnel.clearMessage()
                    } label: {
                        Image(systemName: "xmark")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(brand.text2)
                            .frame(width: 32, height: 32)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("关闭提示")
                }
                Toggle(isOn: Binding(
                    get: { tunnel.isConnected },
                    set: { on in
                        if on != tunnel.isConnected {
                            tunnel.toggle()
                        }
                    }
                )) {
                    Text("连接")
                }
                .toggleStyle(.switch)
                .labelsHidden()
                .tint(brand.success)
                .disabled(phase == .unconfigured || phase == .disconnecting)
                .accessibilityLabel(Text("连接"))
                .sensoryFeedback(.success, trigger: phase) { _, new in new == .on }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .frame(minHeight: 52)
        }
        .background(.bar)
    }

    private var dotColor: Color {
        switch phase {
        case .on:
            return brand.success
        case .connecting, .reasserting, .disconnecting:
            return brand.warning
        case .off, .unconfigured:
            return brand.text3
        }
    }

    /// An error first (the overview's notice card is not there in debug mode), then the elapsed time while
    /// connected, otherwise where the traffic goes out.
    @ViewBuilder
    private var detail: some View {
        if let notice = tunnel.notice {
            Text(notice.text)
                .foregroundStyle(brand.danger)
        } else if phase == .on, let since = tunnel.connectedSince {
            TimelineView(.periodic(from: since, by: 1)) { context in
                Text(verbatim: "\(ConnectElapsed.text(from: since, to: context.date)) · \(tunnel.debugExitSummary)")
            }
            .foregroundStyle(brand.text2)
        } else if phase == .off, case let .blocked(reason, _) = tunnel.coordinator.decision {
            Text(reason)
                .foregroundStyle(brand.text2)
        } else {
            Text(verbatim: tunnel.debugExitSummary)
                .foregroundStyle(brand.text2)
        }
    }
}

extension View {
    /// Puts debug mode's connect bar under the content; proxy mode has the overview's connect card instead. The
    /// inset is always there (empty in proxy mode), so switching modes does not rebuild the page under it.
    func debugConnectBar(_ tunnel: TunnelController) -> some View {
        safeAreaInset(edge: .bottom, spacing: 0) {
            if tunnel.appMode == .debug {
                DebugConnectBar(tunnel: tunnel)
            }
        }
    }
}
