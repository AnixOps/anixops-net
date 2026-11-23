import Foundation

/// What a subscription update does to a running tunnel, and how the background refresh is scheduled. Pure decisions
/// the app's `TunnelController` takes, kept here so they are tested.
public enum SubscriptionRefresh {
    /// Whether a tunnel started on `previous` runs something else once `next` is saved: another active group, another
    /// node, or any change to the nodes, rules or picks the extension builds the engine from (it carries every node of
    /// the active library, so a mode switch in place can reach them). Only the update time is left out, so an update
    /// that brought nothing new keeps the tunnel as it is. Nil means a single pasted node, which no update touches.
    public static func changesRunningTunnel(
        previous: NodeLibrary?,
        next: NodeLibrary?,
        previousActiveID: String?,
        nextActiveID: String?
    ) -> Bool {
        guard previousActiveID == nextActiveID else {
            return true
        }
        switch (previous, next) {
        case (nil, nil):
            return false
        case let (previous?, next?):
            return comparable(previous) != comparable(next)
        default:
            return true
        }
    }

    /// The library without the update time, which every download changes.
    private static func comparable(_ library: NodeLibrary) -> NodeLibrary {
        var library = library
        library.updatedAt = .distantPast
        return library
    }

    /// What `scheduleBackgroundRefresh` does with the system's background refresh request.
    public enum BackgroundPlan: Equatable {
        /// The groups are not known (not read yet, or the Keychain could not be read): ask again at this time rather
        /// than withdraw the request, so the chain of background runs does not end.
        case retry(Date)
        /// No group updates by itself: withdraw the request.
        case cancel
        /// Replace the request with one for this time.
        case submit(Date)
    }

    /// The system runs an app refresh no sooner than this after it was asked.
    public static let minimumBackgroundDelay: TimeInterval = 15 * 60
    /// The retry while the groups are not known.
    public static let unknownRetryDelay: TimeInterval = 60 * 60

    /// `groupsRead` is whether the subscription groups were read from the Keychain in this run of the app; without
    /// that, an empty list only means nothing was read (a background launch before `load()`, or before the first
    /// unlock), and cancelling would stop the background updates until the app is opened again.
    public static func backgroundPlan(groupsRead: Bool, nextUpdate: Date?, now: Date) -> BackgroundPlan {
        guard groupsRead else {
            return .retry(now.addingTimeInterval(unknownRetryDelay))
        }
        guard let nextUpdate else {
            return .cancel
        }
        return .submit(max(nextUpdate, now.addingTimeInterval(minimumBackgroundDelay)))
    }

    /// Defaults key of the connection a background update left on the nodes from before it
    /// (`pendingReconnectApplies`): the time that connection came up.
    public static let pendingReconnectKey = "networkcore.reconnect-pending-session"

    /// Whether the connection up now is the one a background update marked (`marked`, when that connection came up):
    /// it still runs the nodes from before the update and reconnects once the app is in front. A connection that
    /// came up since (On Demand, the widget, the user) already runs the saved profile, and so does none.
    public static func pendingReconnectApplies(marked: Date?, connectedSince: Date?) -> Bool {
        guard let marked, let connectedSince else {
            return false
        }
        return abs(marked.timeIntervalSince(connectedSince)) < 1
    }
}
