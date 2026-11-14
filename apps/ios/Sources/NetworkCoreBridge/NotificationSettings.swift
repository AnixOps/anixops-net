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
