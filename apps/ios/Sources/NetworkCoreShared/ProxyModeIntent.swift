import AppIntents
import Foundation
import NetworkCoreBridge
import NetworkExtension
import WidgetKit

/// The proxy mode as Shortcuts and the Control Center offer it.
enum ProxyModeOption: String, AppEnum {
    case rule
    case global
    case direct

    static let typeDisplayRepresentation: TypeDisplayRepresentation = "代理模式"
    static let caseDisplayRepresentations: [ProxyModeOption: DisplayRepresentation] = [
        .rule: DisplayRepresentation(title: "规则", image: .init(systemName: "arrow.triangle.branch")),
        .global: DisplayRepresentation(title: "全局", image: .init(systemName: "globe")),
        .direct: DisplayRepresentation(title: "直连", image: .init(systemName: "arrow.right")),
    ]

    var mode: ProxyMode {
        ProxyMode(rawValue: rawValue) ?? .global
    }

    init(_ mode: ProxyMode) {
        self = ProxyModeOption(rawValue: mode.rawValue) ?? .global
    }

    var symbol: String {
        switch self {
        case .rule:
            return "arrow.triangle.branch"
        case .global:
            return "globe"
        case .direct:
            return "arrow.right"
        }
    }
}

/// Switches the saved tunnel to a mode. Compiled into the app and the widget extension, so Shortcuts, the Action
/// button and the Control Center share it. A connected engine switches in place.
struct SetProxyModeIntent: AppIntent {
    static let title: LocalizedStringResource = "切换 AnixOps Net 代理模式"
    static let description = IntentDescription("切换到规则、全局或直连模式。连接中立即生效。")

    @Parameter(title: "模式", default: .global)
    var mode: ProxyModeOption

    static var parameterSummary: some ParameterSummary {
        Summary("切换到\(\.$mode)模式")
    }

    init() {}

    init(mode: ProxyModeOption) {
        self.mode = mode
    }

    func perform() async throws -> some IntentResult {
        try await TunnelSwitch.setMode(mode.mode)
        WidgetCenter.shared.reloadAllTimelines()
        if #available(iOS 18.0, *) {
            ControlCenter.shared.reloadAllControls()
        }
        return .result()
    }
}

extension TunnelSwitch {
    /// The mode the saved tunnel runs in.
    static func currentMode() async -> ProxyMode {
        let manager = try? await NETunnelProviderManager.loadAllFromPreferences().first
        return ProxyMode.saved(in: (manager?.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration)
    }

    /// Saves `mode` in the tunnel profile and tells a running engine. An engine that does not answer is restarted.
    static func setMode(_ mode: ProxyMode) async throws {
        guard let manager = try await NETunnelProviderManager.loadAllFromPreferences().first,
              let tunnelProtocol = manager.protocolConfiguration as? NETunnelProviderProtocol
        else {
            throw Failure.notConfigured
        }
        var configuration = tunnelProtocol.providerConfiguration ?? [:]
        // The rule mode needs the subscription's rules; the extension falls back to global without them.
        configuration[TunnelProfileKeys.mode] = mode.rawValue
        if mode == .rule {
            configuration[TunnelProfileKeys.routing] = true
        } else if mode == .global {
            configuration.removeValue(forKey: TunnelProfileKeys.routing)
        }
        tunnelProtocol.providerConfiguration = configuration
        manager.protocolConfiguration = tunnelProtocol
        try await manager.saveToPreferences()
        try await manager.loadFromPreferences()

        guard manager.connection.status == .connected, let session = manager.connection as? NETunnelProviderSession else {
            return
        }
        let answer = RuntimeControl.decode(
            RuntimeControl.ModeAnswer.self,
            from: await send(RuntimeControl.setMode(mode), through: session)
        )
        if answer?.mode == mode {
            _ = await send(.closeAll, through: session)
        } else {
            try await restart(manager)
        }
    }

    private static func send(_ command: RuntimeControl, through session: NETunnelProviderSession) async -> Data? {
        await withCheckedContinuation { continuation in
            do {
                try session.sendProviderMessage(TunnelAppMessage.control(command.request).encoded) { data in
                    continuation.resume(returning: data)
                }
            } catch {
                continuation.resume(returning: nil)
            }
        }
    }

    /// Stops the tunnel, waits up to five seconds for it to go down, and starts it on the saved profile.
    private static func restart(_ manager: NETunnelProviderManager) async throws {
        manager.connection.stopVPNTunnel()
        for _ in 0..<50 where manager.connection.status != .disconnected {
            try await Task.sleep(nanoseconds: 100_000_000)
        }
        do {
            try manager.connection.startVPNTunnel()
        } catch {
            throw Failure.startFailed
        }
    }
}
