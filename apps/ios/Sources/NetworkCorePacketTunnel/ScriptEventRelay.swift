import Foundation
import NetworkCoreBridge
import UserNotifications

/// Connects the engine's plugin scripts that run outside HTTP flows to the device: it tells the engine when the
/// network changes (`network-changed` scripts) and shows what scripts post (`$notification.post`, `$notify`) as
/// local notifications. Without such scripts in the running config it does nothing.
///
/// On macOS the tunnel is a System Extension running as root, outside the user's session, so it cannot post
/// notifications: it leaves them in the engine and the app reads and posts them (`TunnelController.relayScriptNotifications`).
final class ScriptEventRelay: @unchecked Sendable {
    private let queue = DispatchQueue(label: "networkcore.packet-tunnel.script-events")
    private var timer: DispatchSourceTimer?
    private var control: (@Sendable (Data) -> Data?)?
    /// `NotificationSettings.scripts`: when off, what scripts post is read and dropped.
    private var postsNotifications = true

    func setPostsNotifications(_ on: Bool) {
        queue.async { [weak self] in
            self?.postsNotifications = on
        }
    }

    private static let pollSeconds = 15
    /// While the device sleeps notifications can wait a minute; `poll()` on wake shows them at once.
    private static let sleepingPollSeconds = 60
    /// At most this many notifications are shown per poll; scripts that post more are cut short.
    private static let maxPerPoll = 8

    /// Starts relaying for the engine behind `control`. Returns how many scheduled scripts it found.
    @discardableResult
    func start(control: @escaping @Sendable (Data) -> Data?) -> Int {
        queue.sync {
            stopLocked()
            let scripts = RuntimeControl.decode(
                RuntimeControl.ScheduledScripts.self, from: control(RuntimeControl.scripts.request)
            )?.scripts ?? []
            // HTTP scripts post notifications too, so the poll runs whenever the engine answers `scripts`.
            guard RuntimeControl.decode(
                RuntimeControl.ScriptNotifications.self, from: control(RuntimeControl.scriptNotifications.request)
            ) != nil else {
                return 0
            }
            self.control = control
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(
                deadline: .now() + .seconds(Self.pollSeconds), repeating: .seconds(Self.pollSeconds), leeway: .seconds(3)
            )
            timer.setEventHandler { [weak self] in
                self?.pollLocked()
            }
            timer.resume()
            self.timer = timer
            return scripts.count
        }
    }

    /// Polls less often while the device sleeps, as usual again after wake.
    func setSleeping(_ sleeping: Bool) {
        queue.async { [self] in
            let seconds = sleeping ? Self.sleepingPollSeconds : Self.pollSeconds
            timer?.schedule(deadline: .now() + .seconds(seconds), repeating: .seconds(seconds), leeway: .seconds(sleeping ? 10 : 3))
        }
    }

    func stop() {
        queue.sync {
            stopLocked()
        }
    }

    /// The device's network changed: runs the `network-changed` scripts, then shows what they post soon after.
    func networkChanged() {
        queue.async { [self] in
            guard let control else {
                return
            }
            _ = control(RuntimeControl.networkChanged.request)
            queue.asyncAfter(deadline: .now() + .seconds(3)) { [weak self] in
                self?.pollLocked()
            }
        }
    }

    /// Shows what scripts posted, e.g. after the device wakes.
    func poll() {
        queue.async { [weak self] in
            self?.pollLocked()
        }
    }

    private func stopLocked() {
        timer?.cancel()
        timer = nil
        control = nil
    }

    private func pollLocked() {
        #if os(macOS)
        return
        #else
        guard let control,
              let answer = RuntimeControl.decode(
                  RuntimeControl.ScriptNotifications.self, from: control(RuntimeControl.scriptNotifications.request)
              ),
              !answer.notifications.isEmpty
        else {
            return
        }
        guard postsNotifications else {
            return
        }
        let center = UNUserNotificationCenter.current()
        for notification in answer.notifications.prefix(Self.maxPerPoll) {
            let content = UNMutableNotificationContent()
            content.title = notification.title
            content.subtitle = notification.subtitle
            content.body = notification.content
            content.threadIdentifier = "plugin-scripts"
            if let openURL = notification.openURL {
                content.userInfo = ["open-url": openURL]
            }
            center.add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
        }
        #endif
    }
}
