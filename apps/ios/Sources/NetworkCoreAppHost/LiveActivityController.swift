import ActivityKit
import Foundation
import NetworkExtension

/// Keeps one Live Activity in step with the tunnel: started when the tunnel comes up while the app is open,
/// updated when the node or phase changes, ended when the app sees it go down or the user turns the setting off.
/// The activity's own disconnect button ends it at once (`StopTunnelActivityIntent`, in the app's process).
///
/// iOS only starts an activity from the foreground app, so a connection made from a widget or Control Center gets
/// its activity the next time the app opens. A stop the app does not see while suspended (Settings, Control Center,
/// the tunnel failing) ends the activity the next time the app runs: in front, or in a background refresh
/// (`sync(tunnel:)`). iOS ends an activity after 8 hours on its own; the next foreground launch starts a fresh one.
@MainActor
enum LiveActivityController {
    static let storageKey = "live-activity"

    static func sync(enabled: Bool, connection: NEVPNStatus, nodeName: String?, outboundKind: String?, connectedSince: Date?) {
        let state = enabled ? contentState(connection: connection, nodeName: nodeName, outboundKind: outboundKind, connectedSince: connectedSince) : nil
        let running = Activity<TunnelActivityAttributes>.activities
        guard let state else {
            if !running.isEmpty {
                Task { await TunnelActivities.endAll() }
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

    /// The background refresh's check (`NetworkCoreAppMain`): the app may run there without a window, so the setting
    /// is read from the defaults, and an activity the tunnel has left behind is ended before the app is suspended.
    static func sync(tunnel: TunnelController) async {
        let enabled = UserDefaults.standard.object(forKey: storageKey) as? Bool ?? true
        guard enabled, tunnel.isConnected else {
            await TunnelActivities.endAll()
            return
        }
        sync(
            enabled: enabled,
            connection: tunnel.connection,
            nodeName: tunnel.nodeName,
            outboundKind: tunnel.outboundKind,
            connectedSince: tunnel.connectedSince
        )
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
