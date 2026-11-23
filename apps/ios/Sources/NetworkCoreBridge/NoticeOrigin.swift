import Foundation

/// Where a home screen notice came from, so a later status change can drop the ones it makes obsolete without
/// comparing their text.
public enum NoticeOrigin: Sendable, Equatable {
    /// Anything else: a failed import, save, subscription update or node; it stays until replaced or dismissed.
    case other
    /// The tunnel stopped on its own ("connection stopped, retry or pick another node").
    case tunnelStopped

    /// Whether a notice of this origin still applies while the tunnel is up (`connected`: connected, connecting or
    /// reasserting). A stop notice is out of date once the tunnel is back, whoever brought it back (On Demand, the
    /// widget, Control Center): its "Retry" would then mean nothing. That also holds for the system's answer about
    /// the stop when it arrives after the tunnel reconnected.
    public func applies(whileConnected connected: Bool) -> Bool {
        !(connected && self == .tunnelStopped)
    }
}
