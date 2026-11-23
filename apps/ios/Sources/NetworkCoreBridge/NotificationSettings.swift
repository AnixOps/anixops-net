import Foundation

/// Which system notifications the client posts. Each kind can be turned off in Settings; all are on by default.
///
/// - `scripts`: what plugin scripts post (`$notification.post`, `$notify`), such as sign-in results.
/// - `connection`: the tunnel stopped on its own (node failure, network loss), not when the user disconnects.
/// - `subscriptions`: an automatic subscription update failed.
///
/// The app keeps the choice in its own defaults and carries a copy in the tunnel profile
/// (`TunnelProfileKeys.notifications`), so the extension knows it at start; a change while connected reaches the
/// running extension as `TunnelAppMessage.notificationSettings`.
public struct NotificationSettings: Codable, Equatable, Sendable {
    public static let storageKey = "notifications.settings"

    public var scripts: Bool
    public var connection: Bool
    public var subscriptions: Bool

    public init(scripts: Bool = true, connection: Bool = true, subscriptions: Bool = true) {
        self.scripts = scripts
        self.connection = connection
        self.subscriptions = subscriptions
    }

    /// Unknown keys are ignored and missing ones are on, so settings saved by another version still read.
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        scripts = try container.decodeIfPresent(Bool.self, forKey: .scripts) ?? true
        connection = try container.decodeIfPresent(Bool.self, forKey: .connection) ?? true
        subscriptions = try container.decodeIfPresent(Bool.self, forKey: .subscriptions) ?? true
    }

    public var profileValue: Data {
        (try? JSONEncoder().encode(self)) ?? Data()
    }

    /// The settings in a profile or message; nil or unreadable data is the default, everything on.
    public static func fromProfileValue(_ data: Data?) -> NotificationSettings {
        guard let data, let settings = try? JSONDecoder().decode(NotificationSettings.self, from: data) else {
            return NotificationSettings()
        }
        return settings
    }

    /// Whether a tunnel stop with this `NEProviderStopReason` raw value is worth a notification: the provider or
    /// its connection failed, or the network went away. Stops the user or the system asked for (disconnect, sleep,
    /// another VPN, an app update) stay quiet.
    public static func notifiesStop(reasonCode: Int) -> Bool {
        unexpectedStopReasons.contains(reasonCode)
    }

    /// `providerFailed` 2, `noNetworkAvailable` 3, `unrecoverableNetworkChange` 4, `configurationFailed` 7,
    /// `connectionFailed` 14, `internalError` 17.
    static let unexpectedStopReasons: Set<Int> = [2, 3, 4, 7, 14, 17]
}

/// When the app asks for permission to post, and what Settings › Notifications offers for it. The switches are on
/// by default, but the system drops every notification until the user allowed them; the app asks the first time a
/// notification can matter (the tunnel came up while the app is in front), never at launch and never in the demo.
public enum NotificationPermission {
    /// The system's answer, as far as the app cares.
    public enum Status: Equatable, Sendable {
        /// Never asked.
        case notDetermined
        /// The user said no, or turned the app's notifications off in the system settings.
        case denied
        /// Allowed, provisionally or for a time-limited session included.
        case allowed
    }

    /// What the notification section shows under its switches.
    public enum Offer: Equatable, Sendable {
        /// Nothing to offer: allowed, or every kind is off.
        case nothing
        /// A button that asks now.
        case ask
        /// The switches do nothing until notifications are turned on in the system settings: a button that opens them.
        case openSystemSettings
    }

    /// Whether to show the system's permission prompt now: not asked yet, at least one kind is on, and the app is in
    /// front (the prompt shows only there) outside the demo.
    public static func shouldAsk(status: Status, settings: NotificationSettings, appActive: Bool, demo: Bool) -> Bool {
        status == .notDetermined && appActive && !demo && settings.anyOn
    }

    public static func offer(status: Status, settings: NotificationSettings) -> Offer {
        guard settings.anyOn else {
            return .nothing
        }
        switch status {
        case .notDetermined:
            return .ask
        case .denied:
            return .openSystemSettings
        case .allowed:
            return .nothing
        }
    }

    /// The link a tapped notification may open: http or https only, as on every other client
    /// (`crates/app-notifications`). An `anixops://` link would run its command (disconnect, switch to direct, pick a
    /// node) at once, so a script could change the connection behind a tap; other schemes open other apps.
    public static func openableURL(_ raw: String?) -> URL? {
        guard let raw, let url = URL(string: raw.trimmingCharacters(in: .whitespacesAndNewlines)),
              let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
              let host = url.host, !host.isEmpty
        else {
            return nil
        }
        return url
    }
}

extension NotificationSettings {
    /// Whether any kind of notification is on.
    public var anyOn: Bool {
        scripts || connection || subscriptions
    }
}
