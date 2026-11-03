import ActivityKit
import Foundation
import NetworkExtension

/// Keeps one Live Activity in step with the tunnel: started when the tunnel comes up while the app is open,
/// updated when the node or phase changes, ended when it goes down or the user turns the setting off.
///
/// iOS only starts an activity from the foreground app, so a connection made from a widget or Control Center gets
/// its activity the next time the app opens. iOS ends an activity after 8 hours on its own; the next foreground
/// launch starts a fresh one.
@MainActor
enum LiveActivityController {
    static let storageKey = "live-activity"

    static func sync(enabled: Bool, connection: NEVPNStatus, nodeName: String?, outboundKind: String?, connectedSince: Date?) {
        let state = enabled ? contentState(connection: connection, nodeName: nodeName, outboundKind: outboundKind, connectedSince: connectedSince) : nil
        let running = Activity<TunnelActivityAttributes>.activities
        guard let state else {
            for activity in running {
                Task { await activity.end(nil, dismissalPolicy: .immediate) }
            }
            return
        }
        let content = ActivityContent(state: state, staleDate: nil)
        if let current = running.first {
            // Leftovers from an earlier launch, if any, would show twice.
            for extra in running.dropFirst() {
                Task { await extra.end(nil, dismissalPolicy: .immediate) }
            }
            if current.content.state != state {
                Task { await current.update(content) }
            }
            return
        }
        guard ActivityAuthorizationInfo().areActivitiesEnabled else {
            return
        }
        _ = try? Activity.request(attributes: TunnelActivityAttributes(), content: content, pushType: nil)
    }

    /// Nil while the tunnel is down or stopping, which ends the activity.
    private static func contentState(
        connection: NEVPNStatus,
        nodeName: String?,
        outboundKind: String?,
        connectedSince: Date?
    ) -> TunnelActivityAttributes.ContentState? {
        let phase: TunnelActivityAttributes.ContentState.Phase
        switch connection {
        case .connected:
            phase = .connected
        case .connecting:
            phase = .connecting
        case .reasserting:
            phase = .reasserting
        default:
            return nil
        }
        return TunnelActivityAttributes.ContentState(
            phase: phase,
            nodeName: nodeName,
            outboundKind: outboundKind,
            connectedSince: phase == .connected ? connectedSince : nil
        )
    }
}
