import Foundation

/// The physical network under the tunnel: the best interface that is not a tunnel, and its gateways. Two keys differ
/// when the device moved to another Wi-Fi, between Wi-Fi and cellular, or to another gateway on the same interface.
public struct NetworkPathKey: Equatable, Sendable {
    public let interface: String
    public let gateways: [String]

    public init(interface: String, gateways: [String]) {
        self.interface = interface
        self.gateways = gateways.sorted()
    }
}

/// What the tunnel tells the engine about the physical network (`RuntimeControl.setNetwork`), for `subnet` policy
/// groups: the interface type, the router (first gateway) and the interface's addresses. The Wi-Fi name is not read:
/// that needs the Access Wi-Fi Information capability (see docs/manual-intervention.md).
public struct NetworkFacts: Equatable, Sendable {
    /// Most addresses the engine takes.
    public static let maxAddresses = 16

    /// `wifi`, `wired` or `cellular`; nil when the interface is something else.
    public let kind: String?
    public let router: String?
    public let addresses: [String]
    /// The cellular radio technology (`LTE`, `NR`, ...) for `CELLULAR-RADIO` rules; only on cellular.
    public let radio: String?

    public init(kind: String?, router: String?, addresses: [String], radio: String? = nil) {
        self.kind = kind
        self.router = router
        self.addresses = Array(addresses.prefix(Self.maxAddresses))
        self.radio = kind == "cellular" ? radio : nil
    }

    /// The engine's name for a Core Telephony radio technology (`CTRadioAccessTechnologyNRNSA` is `NRNSA`), as Surge's
    /// `CELLULAR-RADIO` writes it, upper case; nil for anything else.
    public static func radio(telephony: String?) -> String? {
        let prefix = "CTRadioAccessTechnology"
        guard let telephony, telephony.hasPrefix(prefix) else {
            return nil
        }
        let name = telephony.dropFirst(prefix.count).uppercased()
        guard !name.isEmpty, name.count <= 24, name.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber) }) else {
            return nil
        }
        return name
    }

    /// The `set_network` fields; absent ones stay unknown.
    var fields: [String: Any] {
        var object: [String: Any] = [:]
        if let kind {
            object["type"] = kind
        }
        if let router {
            object["router"] = router
        }
        if !addresses.isEmpty {
            object["addresses"] = addresses
        }
        if let radio {
            object["radio"] = radio
        }
        return object
    }
}

/// Decides when the tunnel tells the engine to rebuild its connections (`RuntimeControl.resetNetwork`).
///
/// The engine's connections to proxy servers outlive a network change: a shared QUIC session keeps sending to the old
/// path and open flows hang until they time out. So the tunnel resets them when the physical network changes and when
/// the device wakes from a sleep long enough for NAT mappings to have expired.
public struct NetworkRecovery: Sendable {
    /// Sleeps at least this long close the open connections on wake; shorter ones only drop pooled sessions. Mobile
    /// carriers' NAT keeps idle TCP mappings for about a minute or more, so connections from before a longer sleep are
    /// usually dead.
    public static let closeAfterSleep: TimeInterval = 60

    private var started = false
    private var current: NetworkPathKey?

    public init() {}

    /// A new path from the monitor (nil while there is no usable network). Returns the request to send, if any: none
    /// for the first path (the one the tunnel started on) or the same network again, none while offline, and a full
    /// reset once a different network, or any network after none, is up.
    public mutating func pathChanged(to key: NetworkPathKey?) -> RuntimeControl? {
        defer {
            current = key
            started = true
        }
        guard started, let key, key != current else {
            return nil
        }
        return .resetNetwork(closeConnections: true)
    }

    /// The request to send when the device wakes after `sleptFor` seconds.
    public static func afterWake(sleptFor: TimeInterval) -> RuntimeControl {
        .resetNetwork(closeConnections: sleptFor >= closeAfterSleep)
    }
}
