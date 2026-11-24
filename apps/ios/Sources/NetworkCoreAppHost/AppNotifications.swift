import Foundation
import NetworkCoreBridge
import NetworkExtension
import SwiftUI
import UserNotifications
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// System notifications posted by the app: subscription update failures on both platforms and, on macOS, what the
/// tunnel would post on iOS (script notifications and an unexpected stop), since the macOS System Extension runs as
/// root and cannot post. Every kind follows `NotificationSettings`.
enum AppNotifications {
    static var settings: NotificationSettings {
        NotificationSettings.fromProfileValue(UserDefaults.standard.data(forKey: NotificationSettings.storageKey))
    }

    static func save(_ settings: NotificationSettings) {
        UserDefaults.standard.set(settings.profileValue, forKey: NotificationSettings.storageKey)
    }

    /// Asks once; the system remembers the answer.
    static func requestAuthorization() async {
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
    }

    /// The system's answer on notifications for this app.
    static func authorizationStatus() async -> NotificationPermission.Status {
        switch await UNUserNotificationCenter.current().notificationSettings().authorizationStatus {
        case .notDetermined:
            return .notDetermined
        case .denied:
            return .denied
        default:
            return .allowed
        }
    }

    /// Set while `requestIfUndetermined` runs, so two status changes in a row ask once.
    @MainActor private static var asking = false

    /// Asks for permission when it was never asked, a kind of notification is on and the app is in front
    /// (`NotificationPermission.shouldAsk`). The switches are on by default, and without an answer the system drops
    /// every notification, the stop and subscription ones included. Called once the tunnel is first connected, after
    /// the system's own prompt to add the VPN configuration, rather than at launch. Only the app can ask: the tunnel
    /// extension cannot show the prompt.
    @MainActor static func requestIfUndetermined() async {
        guard !asking else {
            return
        }
        asking = true
        defer { asking = false }
        let status = await authorizationStatus()
        #if DEBUG
        let demo = TunnelController.isDemo
        #else
        let demo = false
        #endif
        guard NotificationPermission.shouldAsk(status: status, settings: settings, appActive: isAppActive, demo: demo) else {
            return
        }
        await requestAuthorization()
    }

    @MainActor private static var isAppActive: Bool {
        #if os(macOS)
        NSApp.isActive
        #else
        UIApplication.shared.applicationState == .active
        #endif
    }

    /// Opens the system settings where this app's notifications are turned on.
    @MainActor static func openSystemSettings() {
        #if os(macOS)
        if let url = URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension") {
            NSWorkspace.shared.open(url)
        }
        #else
        if let url = URL(string: UIApplication.openNotificationSettingsURLString) {
            UIApplication.shared.open(url)
        }
        #endif
    }

    static func post(title: String, subtitle: String = "", body: String, thread: String, id: String? = nil, openURL: String? = nil) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.subtitle = subtitle
        content.body = body
        content.threadIdentifier = thread
        if let openURL {
            content.userInfo = ["open-url": openURL]
        }
        UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: id ?? UUID().uuidString, content: content, trigger: nil)
        )
    }

    /// Opens a script notification's `open-url` when the user taps it; set once at launch.
    static let delegate = NotificationDelegate()

    static func install() {
        UNUserNotificationCenter.current().delegate = delegate
    }
}

final class NotificationDelegate: NSObject, UNUserNotificationCenterDelegate {
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        defer { completionHandler() }
        // Web links only (`NotificationPermission.openableURL`): an `anixops://` link from a script would run its
        // command (disconnect, direct mode, another node) the moment the notification is tapped. Checked here too,
        // so notifications posted by an older version and still in Notification Center are covered.
        guard let url = NotificationPermission.openableURL(
            response.notification.request.content.userInfo["open-url"] as? String
        ) else {
            return
        }
        DispatchQueue.main.async {
            #if os(macOS)
            NSWorkspace.shared.open(url)
            #else
            UIApplication.shared.open(url)
            #endif
        }
    }

    /// The open app shows its own notice, so only script notifications appear as banners in the foreground.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        let fromScript = notification.request.content.threadIdentifier == "plugin-scripts"
        completionHandler(fromScript ? [.banner, .list, .sound] : [.list])
    }
}

extension TunnelController {
    /// macOS: reads what scripts posted from the running engine and posts it, every 15 seconds while connected.
    /// When script notifications are off they are read and dropped, so the engine's queue does not fill.
    func relayScriptNotifications() async {
        while !Task.isCancelled {
            if connection == .connected,
               let answer = RuntimeControl.decode(RuntimeControl.ScriptNotifications.self, from: await control(.scriptNotifications)),
               AppNotifications.settings.scripts {
                for item in answer.notifications.prefix(8) {
                    AppNotifications.post(
                        title: item.title, subtitle: item.subtitle, body: item.content,
                        thread: "plugin-scripts",
                        openURL: NotificationPermission.openableURL(item.openURL)?.absoluteString
                    )
                }
            }
            try? await Task.sleep(for: .seconds(15))
        }
    }

    /// Changes the notification settings and hands them to the tunnel: the saved profile for the next start, and the
    /// running extension at once.
    func applyNotificationSettings(_ next: NotificationSettings) async {
        let previous = AppNotifications.settings
        AppNotifications.save(next)
        // Asked when a kind was turned on, not when one was turned off.
        let turnedOn = (next.scripts && !previous.scripts) || (next.connection && !previous.connection)
            || (next.subscriptions && !previous.subscriptions)
        if turnedOn {
            await AppNotifications.requestAuthorization()
        }
        await saveNotificationSettingsToProfile(next)
        guard let session = tunnelSession, connection == .connected else {
            return
        }
        try? session.sendProviderMessage(TunnelAppMessage.notificationSettings(next.profileValue).encoded) { _ in }
    }
}

/// The notification switches in Settings, shared by the iOS settings form and the macOS settings page.
struct NotificationSettingsSection: View {
    var tunnel: TunnelController
    @State private var settings = AppNotifications.settings
    /// The system's answer, read when the section appears and when the app comes back from the system settings.
    @State private var permission: NotificationPermission.Status?
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Section {
            switch permission.map({ NotificationPermission.offer(status: $0, settings: settings) }) ?? .nothing {
            case .ask:
                Button("允许 AnixOps Net 发送通知") {
                    Task {
                        await AppNotifications.requestAuthorization()
                        permission = await AppNotifications.authorizationStatus()
                    }
                }
            case .openSystemSettings:
                Button("在系统设置中开启通知") {
                    AppNotifications.openSystemSettings()
                }
            case .nothing:
                EmptyView()
            }
            Toggle("插件脚本的通知", isOn: binding(\.scripts))
            Toggle("连接意外断开", isOn: binding(\.connection))
            Toggle("订阅自动更新失败", isOn: binding(\.subscriptions))
        } header: {
            Text("通知")
        } footer: {
            if permission == .denied, settings.anyOn {
                Text("系统设置里关闭了 AnixOps Net 的通知，上面的开关目前不会发出任何通知。")
            } else {
                Text("关闭后不再发出此类系统通知。全部通知可在系统设置中关闭。")
            }
        }
        .task {
            permission = await AppNotifications.authorizationStatus()
        }
        .onChange(of: scenePhase) { _, phase in
            guard phase == .active else {
                return
            }
            Task {
                permission = await AppNotifications.authorizationStatus()
            }
        }
    }

    private func binding(_ keyPath: WritableKeyPath<NotificationSettings, Bool>) -> Binding<Bool> {
        Binding(
            get: { settings[keyPath: keyPath] },
            set: { on in
                settings[keyPath: keyPath] = on
                let next = settings
                Task {
                    await tunnel.applyNotificationSettings(next)
                    permission = await AppNotifications.authorizationStatus()
                }
            }
        )
    }
}
