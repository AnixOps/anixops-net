#if canImport(ActivityKit)
import ActivityKit
import Foundation

/// The Live Activity on the Lock Screen and in the Dynamic Island while the tunnel is up. Compiled into the app,
/// which starts, updates and ends it, and the widget extension, which draws it. It carries the node's display name
/// and protocol only, never an address or key.
struct TunnelActivityAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        enum Phase: String, Codable, Hashable {
            case connecting
            case connected
            case reasserting
        }

        var phase: Phase
        var nodeName: String?
        var outboundKind: String?
        /// When the connection came up; the activity counts up from it.
        var connectedSince: Date?
    }
}
#endif
