#if canImport(ActivityKit)
import ActivityKit
import AppIntents
import WidgetKit

/// The Live Activity's disconnect button. A `LiveActivityIntent` runs in the app's process, which the system launches
/// in the background when it is not running, and that process can end the activity. `ToggleTunnelIntent` runs in the
/// widget extension: it stopped the tunnel but left the activity on the Lock Screen saying "connected", with its
/// timer still counting, until the app was next opened. Compiled into the app and the widget extension, which draws
/// the button. Not offered in Shortcuts, which has "Disconnect" already (`AppShortcuts`).
struct StopTunnelActivityIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "断开 AnixOps Net"
    static let isDiscoverable = false

    init() {}

    func perform() async throws -> some IntentResult {
        // The activity ends whatever the stop says: without a saved profile (`notConfigured`) no tunnel runs either.
        _ = try? await TunnelSwitch.set(on: false)
        // Awaited before returning, or the process may be suspended before the system has the end.
        await TunnelActivities.endAll()
        WidgetCenter.shared.reloadAllTimelines()
        if #available(iOS 18.0, *) {
            ControlCenter.shared.reloadAllControls()
        }
        return .result()
    }
}
#endif
