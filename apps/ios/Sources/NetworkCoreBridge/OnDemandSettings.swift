import Foundation
#if canImport(NetworkExtension)
import NetworkExtension
#endif

/// Keeps the tunnel up by itself (VPN On Demand) and decides per Wi-Fi network whether it runs.
///
/// With it on, the system starts the tunnel again when the extension ended without the user asking: a crash, the
/// memory limit, an update. A disconnect from the app, the widget or Control Center turns On Demand off first, so the
/// system does not undo it; connecting turns it back on. The Wi-Fi rules (Surge's and Loon's SSID policies) become On
/// Demand rules, so the system applies them as soon as the device joins the network, without the app running.
public struct OnDemandSettings: Codable, Equatable, Sendable {
    /// What the tunnel does on one kind of network.
    public enum Action: String, Codable, CaseIterable, Identifiable, Sendable {
        /// Connect, and reconnect after the extension ends.
        case connect
        /// Disconnect, and stay off while on this network.
        case disconnect
        /// Leave the tunnel as it is.
        case ignore

        public var id: String { rawValue }
    }

    /// One Wi-Fi network by its name (SSID) and what the tunnel does there.
    public struct WiFiRule: Codable, Equatable, Hashable, Identifiable, Sendable {
        public var ssid: String
        public var action: Action

        public var id: String { ssid }

        public init(ssid: String, action: Action) {
            self.ssid = ssid
            self.action = action
        }
    }

    /// Reconnects by itself. Off, the tunnel runs only while the user keeps it on and stays down after a crash.
    public var enabled: Bool
    /// Named Wi-Fi networks, first match wins.
    public var wifi: [WiFiRule]
    /// Any other Wi-Fi network.
    public var otherWiFi: Action
    /// Cellular data (iOS).
    public var cellular: Action

    public init(enabled: Bool = true, wifi: [WiFiRule] = [], otherWiFi: Action = .connect, cellular: Action = .connect) {
        self.enabled = enabled
        self.wifi = wifi
        self.otherWiFi = otherWiFi
        self.cellular = cellular
    }

    /// Longest SSID (IEEE 802.11: 32 bytes) and most named networks kept.
    public static let maxSSIDBytes = 32
    public static let maxWiFiRules = 64

    /// Adds or replaces the rule for `ssid` (trimmed); returns false for an empty or too long name or a full list.
    @discardableResult
    public mutating func setRule(ssid: String, action: Action) -> Bool {
        let name = ssid.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name.utf8.count <= Self.maxSSIDBytes else {
            return false
        }
        if let index = wifi.firstIndex(where: { $0.ssid == name }) {
            wifi[index].action = action
            return true
        }
        guard wifi.count < Self.maxWiFiRules else {
            return false
        }
        wifi.append(WiFiRule(ssid: name, action: action))
        return true
    }

    public mutating func removeRule(ssid: String) {
        wifi.removeAll { $0.ssid == ssid }
    }

    /// One On Demand rule, independent of NetworkExtension so it can be tested anywhere.
    public struct Step: Equatable, Sendable {
        public enum Interface: String, Equatable, Sendable {
            case any
            case wifi
            case cellular
        }

        public var action: Action
        public var interface: Interface
        /// Only for `wifi`: the networks it applies to; empty means every Wi-Fi.
        public var ssids: [String]

        public init(action: Action, interface: Interface, ssids: [String]) {
            self.action = action
            self.interface = interface
            self.ssids = ssids
        }
    }

    /// The On Demand rules in the order the system checks them, nil when On Demand is off. Named networks come first
    /// (grouped by action, keeping their order), then other Wi-Fi, cellular, and a final catch-all that connects so
    /// wired and other interfaces (macOS Ethernet) keep the tunnel up.
    public var steps: [Step]? {
        guard enabled else {
            return nil
        }
        var steps: [Step] = []
        var seen = Set<String>()
        for action in [Action.disconnect, .ignore, .connect] {
            let names = wifi.filter { $0.action == action && seen.insert($0.ssid).inserted }.map(\.ssid)
            if !names.isEmpty {
                steps.append(Step(action: action, interface: .wifi, ssids: names))
            }
        }
        steps.append(Step(action: otherWiFi, interface: .wifi, ssids: []))
        steps.append(Step(action: cellular, interface: .cellular, ssids: []))
        steps.append(Step(action: .connect, interface: .any, ssids: []))
        return steps
    }

    /// Saved in the app's own defaults; the rules themselves live in the VPN configuration.
    public static let storageKey = "net.anixops.on-demand"

    public static func load(from defaults: UserDefaults = .standard) -> OnDemandSettings {
        guard let data = defaults.data(forKey: storageKey),
              let settings = try? JSONDecoder().decode(OnDemandSettings.self, from: data)
        else {
            return OnDemandSettings()
        }
        return settings
    }

    public func save(to defaults: UserDefaults = .standard) {
        if let data = try? JSONEncoder().encode(self) {
            defaults.set(data, forKey: Self.storageKey)
        }
    }
}

#if canImport(NetworkExtension)
extension OnDemandSettings {
    /// `steps` as NetworkExtension rules; nil when On Demand is off.
    public var rules: [NEOnDemandRule]? {
        #if os(iOS)
        steps?.map(Self.rule(for:))
        #else
        // macOS has no cellular interface type: such a rule would match every interface.
        steps?.filter { $0.interface != .cellular }.map(Self.rule(for:))
        #endif
    }

    private static func rule(for step: Step) -> NEOnDemandRule {
        let rule: NEOnDemandRule
        switch step.action {
        case .connect:
            rule = NEOnDemandRuleConnect()
        case .disconnect:
            rule = NEOnDemandRuleDisconnect()
        case .ignore:
            rule = NEOnDemandRuleIgnore()
        }
        switch step.interface {
        case .any:
            rule.interfaceTypeMatch = .any
        case .wifi:
            rule.interfaceTypeMatch = .wiFi
            if !step.ssids.isEmpty {
                rule.ssidMatch = step.ssids
            }
        case .cellular:
            #if os(iOS)
            rule.interfaceTypeMatch = .cellular
            #endif
        }
        return rule
    }

    /// Applies the rules to `manager` and sets whether they run now: `active` is true while the user wants the tunnel
    /// up. Changes nothing on disk; the caller saves.
    public func apply(to manager: NEVPNManager, active: Bool) {
        manager.onDemandRules = rules
        manager.isOnDemandEnabled = active && rules != nil
    }
}

extension NEVPNManager {
    /// Turns On Demand on again for a connect from outside the app (widget, Control Center, Shortcuts) when the saved
    /// configuration carries rules, so a crash after such a connect still recovers.
    public func resumeOnDemand() -> Bool {
        let wanted = !(onDemandRules ?? []).isEmpty
        guard isOnDemandEnabled != wanted else {
            return false
        }
        isOnDemandEnabled = wanted
        return true
    }
}
#endif
