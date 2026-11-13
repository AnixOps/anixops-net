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
        guard let raw = response.notification.request.content.userInfo["open-url"] as? String,
              let url = URL(string: raw), let scheme = url.scheme?.lowercased(),
              ["http", "https", AppURLCommand.scheme].contains(scheme)
        else {
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
                        thread: "plugin-scripts", openURL: item.openURL
                    )
                }
            }
            try? await Task.sleep(for: .seconds(15))
        }
    }

    /// Changes the notification settings and hands them to the tunnel: the saved profile for the next start, and the
    /// running extension at once.
    func applyNotificationSettings(_ next: NotificationSettings) async {
        AppNotifications.save(next)
        if next.scripts || next.connection || next.subscriptions {
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

    var body: some View {
        Section {
            Toggle("插件脚本的通知", isOn: binding(\.scripts))
            Toggle("连接意外断开", isOn: binding(\.connection))
            Toggle("订阅自动更新失败", isOn: binding(\.subscriptions))
        } header: {
            Text("通知")
        } footer: {
            Text("关掉后不再发出这类系统通知；也可以在系统设置里关闭 AnixOps Net 的全部通知。")
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
                }
            }
        )
    }
}
