import AppIntents

/// Siri and Spotlight phrases for the tunnel switch and the mode switch; both intents also appear in Shortcuts.
struct AnixOpsShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: ToggleTunnelIntent(on: true),
            phrases: [
                "用 \(.applicationName) 连接",
                "Connect \(.applicationName)",
            ],
            shortTitle: "连接",
            systemImageName: "network"
        )
        AppShortcut(
            intent: ToggleTunnelIntent(on: false),
            phrases: [
                "断开 \(.applicationName)",
                "Disconnect \(.applicationName)",
            ],
            shortTitle: "断开",
            systemImageName: "network.slash"
        )
        AppShortcut(
            intent: SetProxyModeIntent(),
            phrases: [
                "把 \(.applicationName) 切换到\(\.$mode)模式",
                "Switch \(.applicationName) to \(\.$mode) mode",
            ],
            shortTitle: "切换代理模式",
            systemImageName: "arrow.triangle.branch"
        )
    }
}
