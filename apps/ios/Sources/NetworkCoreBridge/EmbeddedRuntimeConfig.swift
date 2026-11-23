import Foundation

/// JSON config envelope for the embedded runtime, schema version 1.
///
/// Mirrors `RuntimeConfig` in crates/engine-embedded/src/config.rs. Optional DNS and limit sections are
/// left to the runtime defaults.
///
/// The tunnel profile stores this config without its secrets (`withoutSecret()`); they live in the Keychain
/// behind `NETunnelProviderProtocol.passwordReference` as a `NodeSecret`, and the extension restores them with
/// `withSecret(_:)` just before handing the config to the runtime. To follow a subscription's rules it also adds the
/// `routing` section from the node library (`withRouting(_:)`), which replaces `outbound` in the encoded envelope.
public struct EmbeddedRuntimeConfig: Codable, Equatable {
    public static let schemaVersion = 1

    public enum Outbound: Equatable {
        case shadowsocks(server: String, port: UInt16, method: String, password: String)
        case wireguard(WireGuardPeer)
        case vless(VlessOutbound)
        /// Any other outbound the runtime registry knows, as the Rust importer produced it.
        case native(NativeOutbound)
    }

    /// An outbound the app passes through without interpreting: Trojan, VMess, VLESS over TLS, Hysteria2, TUIC
    /// and the rest of crates/node-import's output, including every Shadowsocks, VLESS REALITY or WireGuard node
    /// with more than its dedicated case holds (a `via`, a transport, multiplexing, a WireGuard address). `settings`
    /// (everything but the type and address) is the secret: it holds the password or user id, and the profile keeps
    /// only `type`, `server` and `port`.
    public struct NativeOutbound: Equatable {
        static let addressKeys: Set<String> = ["type", "server", "port"]

        public var type: String
        public var server: String
        public var port: UInt16
        public var settings: [String: JSONValue]

        public init(type: String, server: String, port: UInt16, settings: [String: JSONValue]) {
            self.type = type
            self.server = server
            self.port = port
            self.settings = settings
        }

        /// The runtime outbound entry: the settings with the type and address, as `encode(to:)` writes it.
        var entry: [String: JSONValue] {
            var entry = settings
            entry["type"] = .string(type)
            entry["server"] = .string(server)
            entry["port"] = .int(Int64(port))
            return entry
        }
    }

    /// One VLESS + REALITY server. `uuid` is the secret.
    public struct VlessOutbound: Equatable {
        public var server: String
        public var port: UInt16
        public var uuid: String
        public var flow: String
        public var reality: RealitySettings
    }

    public struct RealitySettings: Codable, Equatable {
        public var serverName: String
        public var publicKey: String
        public var shortID: String
        public var fingerprint: String

        enum CodingKeys: String, CodingKey, CaseIterable {
            case serverName = "server_name"
            case publicKey = "public_key"
            case shortID = "short_id"
            case fingerprint
        }
    }

    /// One WireGuard peer, keys in standard base64.
    public struct WireGuardPeer: Equatable {
        public var privateKey: String
        public var peerPublicKey: String
        public var presharedKey: String?
        public var server: String
        public var port: UInt16
        public var persistentKeepaliveSecs: UInt16?
    }

    /// The `mitm` section: enabled plugin texts and the device root. It holds the root's private key, so it is
    /// never stored in the tunnel profile; the extension adds it with `withMitm(plugins:authority:)`.
    public struct Mitm: Codable, Equatable {
        public var plugins: [String]
        /// Root certificate, base64 DER.
        public var caCertificate: String
        /// Root private key, base64 X9.63.
        public var caPrivateKey: String
        /// `script-path` to the local file holding the copy the user allowed.
        public var scriptAssets: [String: String]
        /// Remote `[Map Local]` body URL to the local file holding the downloaded copy.
        public var resourceAssets: [String: String] = [:]
        /// File backing `$persistentStore`.
        public var scriptStorePath: String?

        enum CodingKeys: String, CodingKey {
            case plugins
            case caCertificate = "ca_certificate"
            case caPrivateKey = "ca_private_key"
            case scriptAssets = "script_assets"
            case resourceAssets = "resource_assets"
            case scriptStorePath = "script_store_path"
        }
    }

    /// The `routing` section: every subscription node by id, with the subscription's rules and policy groups. It
    /// holds every node's credentials, so it is never stored in the tunnel profile; the extension builds it from the
    /// node library (`NodeLibrary.runtimeRouting()`).
    public struct Routing: Codable, Equatable {
        public var outbounds: [String: [String: JSONValue]]
        public var rules: [String]
        public var groups: [NativeImportReport.Routing.Group]
        /// Where the global mode sends everything: a group id, a node id or `DIRECT`. The engine defaults to the
        /// first group, otherwise the first node.
        public var global: String?
        /// The lists `RULE-SET,<name>` rules name; `geosite-<category>` and `geoip-<cc>` back `GEOSITE` and `GEOIP`.
        public var ruleSets: [String: RuleSet]?
        /// Device names by address, for `DEVICE-NAME` rules.
        public var devices: [String: String]?

        enum CodingKeys: String, CodingKey {
            case outbounds
            case rules
            case groups
            case global
            case ruleSets = "rule_sets"
            case devices
        }

        public init(
            outbounds: [String: [String: JSONValue]],
            rules: [String],
            groups: [NativeImportReport.Routing.Group],
            global: String? = nil,
            ruleSets: [String: RuleSet]? = nil,
            devices: [String: String]? = nil
        ) {
            self.outbounds = outbounds
            self.rules = rules
            self.groups = groups
            self.global = global
            self.ruleSets = ruleSets
            self.devices = devices
        }
    }

    /// One rule set: inline entries, or a list the engine downloads from `url` every `intervalSecs` (at least 600,
    /// one day by default) through `via` (a group, node or `DIRECT`, the default).
    public struct RuleSet: Codable, Equatable {
        public var domain: [String]?
        public var domainSuffix: [String]?
        public var domainKeyword: [String]?
        public var ipCidr: [String]?
        public var url: String?
        /// How a bare domain line of a downloaded list matches: `suffix` (the default) or `domain`.
        public var format: String?
        public var intervalSecs: UInt32?
        public var via: String?

        enum CodingKeys: String, CodingKey {
            case domain
            case domainSuffix = "domain_suffix"
            case domainKeyword = "domain_keyword"
            case ipCidr = "ip_cidr"
            case url
            case format
            case intervalSecs = "interval_secs"
            case via
        }

        public init(url: String, format: String? = nil, intervalSecs: UInt32? = nil, via: String? = nil) {
            self.url = url
            self.format = format
            self.intervalSecs = intervalSecs
            self.via = via
        }
    }

    public let schemaVersion: Int
    public let profileID: String
    public let mtu: Int
    /// The selected node. With `routing` set it is left out of the encoded envelope, which carries exactly one of
    /// the two; it still names the node for the app and diagnostics.
    public private(set) var outbound: Outbound
    public private(set) var routing: Routing?
    public private(set) var mitm: Mitm?
    /// The mode the engine starts in; absent is `rule`. The app switches it later without a restart
    /// (`RuntimeControl.setMode`).
    public private(set) var mode: ProxyMode?
    /// The `dns` section; absent keeps the engine's default resolver. The profile stores it apart
    /// (`TunnelProfileKeys.dns`) and the extension adds it with `withDns(_:)`.
    public private(set) var dns: DnsSettings?
    /// The engine's `redirects` lines (`TrafficRedirects.lines`); absent rewrites nothing. The profile stores them
    /// apart (`TunnelProfileKeys.redirects`) and the extension adds them with `withRedirects(_:)`.
    public private(set) var redirects: [String]?
    /// The engine's `block_quic`: refuse UDP to port 443 so apps fall back from QUIC to TCP. Written only when on,
    /// so profiles still suit engines without it. The profile stores it apart (`TunnelProfileKeys.blockQuic`) and the
    /// extension adds it with `withBlockQuic(_:)`; the app switches it later without a restart
    /// (`RuntimeControl.setBlockQuic`).
    public private(set) var blockQuic: Bool?

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case profileID = "profile_id"
        case mtu
        case outbound
        case routing
        case mitm
        case mode
        case dns
        case redirects
        case blockQuic = "block_quic"
    }

    /// Only the extension encodes a routed config, and only for the runtime; nothing decodes one.
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(schemaVersion, forKey: .schemaVersion)
        try container.encode(profileID, forKey: .profileID)
        try container.encode(mtu, forKey: .mtu)
        if let routing {
            try container.encode(routing, forKey: .routing)
        } else {
            try container.encode(outbound, forKey: .outbound)
        }
        try container.encodeIfPresent(mitm, forKey: .mitm)
        try container.encodeIfPresent(mode, forKey: .mode)
        try container.encodeIfPresent(dns, forKey: .dns)
        try container.encodeIfPresent(redirects, forKey: .redirects)
        try container.encodeIfPresent(blockQuic, forKey: .blockQuic)
    }

    public init(node: ShadowsocksNode, profileID: String, mtu: Int = 1500) {
        schemaVersion = Self.schemaVersion
        self.profileID = profileID
        self.mtu = mtu
        outbound = .shadowsocks(server: node.host, port: node.port, method: node.method, password: node.password)
    }

    public init(node: WireGuardNode, profileID: String) {
        schemaVersion = Self.schemaVersion
        self.profileID = profileID
        mtu = node.mtu ?? WireGuardNode.defaultMTU
        outbound = .wireguard(
            WireGuardPeer(
                privateKey: node.privateKey,
                peerPublicKey: node.peerPublicKey,
                presharedKey: node.presharedKey,
                server: node.endpointHost,
                port: node.endpointPort,
                persistentKeepaliveSecs: node.persistentKeepalive
            )
        )
    }

    public init(node: VlessNode, profileID: String, mtu: Int = 1500) {
        schemaVersion = Self.schemaVersion
        self.profileID = profileID
        self.mtu = mtu
        outbound = .vless(
            VlessOutbound(
                server: node.host,
                port: node.port,
                uuid: node.uuid,
                flow: node.flow,
                reality: RealitySettings(
                    serverName: node.serverName,
                    publicKey: node.publicKey,
                    shortID: node.shortID,
                    fingerprint: "auto"
                )
            )
        )
    }

    public init(outbound: Outbound, profileID: String, mtu: Int = 1500) {
        schemaVersion = Self.schemaVersion
        self.profileID = profileID
        self.mtu = mtu
        self.outbound = outbound
    }

    /// The secrets this config carries, for the Keychain.
    public var secret: NodeSecret {
        switch outbound {
        case let .shadowsocks(_, _, _, password):
            return NodeSecret(password: password)
        case let .wireguard(peer):
            return NodeSecret(privateKey: peer.privateKey, presharedKey: peer.presharedKey)
        case let .vless(vless):
            return NodeSecret(uuid: vless.uuid)
        case let .native(native):
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            let settings = (try? encoder.encode(native.settings)).map { String(decoding: $0, as: UTF8.self) }
            return NodeSecret(outbound: settings)
        }
    }

    /// The same config with every secret removed, for storing in the tunnel profile.
    public func withoutSecret() -> EmbeddedRuntimeConfig {
        var copy = withSecret(NodeSecret())
        copy.mitm = nil
        copy.routing = nil
        copy.mode = nil
        copy.dns = nil
        copy.redirects = nil
        copy.blockQuic = nil
        return copy
    }

    /// The same config resolving with `dns`. Rule sets its rules name that the routing section lacks come from
    /// the catalog when it knows them; a name nothing defines simply never matches.
    public func withDns(_ dns: DnsSettings?) -> EmbeddedRuntimeConfig {
        var copy = self
        copy.dns = dns.flatMap { $0.isEmpty ? nil : $0 }
        if let dns = copy.dns, var routing = copy.routing {
            var sets = routing.ruleSets ?? [:]
            for name in dns.ruleSetNames where sets[name] == nil {
                sets[name] = RuleSetCatalog.known[name]
            }
            routing.ruleSets = sets.isEmpty ? nil : sets
            copy.routing = routing
        }
        return copy
    }

    /// The same config starting in `mode`.
    /// The same config rewriting destinations by `lines`; empty rewrites nothing.
    public func withRedirects(_ lines: [String]) -> EmbeddedRuntimeConfig {
        var copy = self
        copy.redirects = lines.isEmpty ? nil : lines
        return copy
    }

    /// The same config refusing UDP to port 443 when `on`; off leaves the key out.
    public func withBlockQuic(_ on: Bool) -> EmbeddedRuntimeConfig {
        var copy = self
        copy.blockQuic = on ? true : nil
        return copy
    }

    public func withMode(_ mode: ProxyMode?) -> EmbeddedRuntimeConfig {
        var copy = self
        copy.mode = mode
        return copy
    }

    /// The rule lines to swap into an engine running `self` so it routes like `next`, or nil when more than the
    /// lines differ (nodes, groups, the global target, rule sets, DNS, mode or anything else): those need a restart.
    /// Equal lines are returned too, so a caller can tell "nothing to do" (`== routing.rules`) itself.
    public func reloadableRules(to next: EmbeddedRuntimeConfig) -> [String]? {
        guard let running = routing, let target = next.routing else {
            return nil
        }
        var same = next
        same.routing?.rules = running.rules
        guard same == self else {
            return nil
        }
        return target.rules
    }

    /// The same config routing by a subscription's rules and groups; nil goes back to the selected node alone.
    public func withRouting(_ routing: Routing?) -> EmbeddedRuntimeConfig {
        var copy = self
        copy.routing = routing
        return copy
    }

    /// The same config as the engine runs the selected node alone: without subscription rules to add, or when the
    /// engine refuses them. That is the top-level outbound, but for two cases:
    /// - A profile saved before 1.0 may hold a shorter copy of a subscription's Shadowsocks, VLESS REALITY or
    ///   WireGuard node, in its dedicated case without what that case has no field for (an xhttp transport,
    ///   multiplexing, the WireGuard address). When the refused routing section has the same node as its global
    ///   target, that full node runs instead.
    /// - A WireGuard node from the importer (native, with its own `local_address`) is the only node of a routing
    ///   section, as in the subscription's one. As the top-level outbound the engine would run it as the
    ///   whole-device tunnel, sealing the device's packets straight to the peer from the tunnel interface's
    ///   address, the fixed `TunnelInterface.proxy` for such a node rather than the address the server gave it: the
    ///   peer drops them and DNS to 198.18.0.2 goes unanswered. A pasted `wg-quick` config (`.wireguard`) stays the
    ///   whole-device tunnel, with the interface its file describes.
    public func selectedNodeAlone() -> EmbeddedRuntimeConfig {
        var copy = self
        copy.routing = nil
        if let routing, let id = routing.global, let node = routing.outbounds[id], node["via"] == nil,
           outbound.isDedicatedCopy(of: node),
           let full = try? JSONDecoder().decode(Outbound.self, from: JSONEncoder().encode(node))
        {
            copy.outbound = full
        }
        if case let .native(native) = copy.outbound, native.type == "wireguard" {
            copy.routing = Routing(
                outbounds: [native.type: native.entry],
                rules: ["FINAL,\(native.type)"],
                groups: [],
                global: native.type
            )
        }
        return copy
    }

    /// The same config decrypting HTTPS for the plugins' hosts with `authority`, running the allowed scripts in
    /// `scriptAssets`. No plugins removes the section.
    public func withMitm(
        plugins: [String],
        authority: MitmCertificateAuthority,
        scriptAssets: [String: String] = [:],
        resourceAssets: [String: String] = [:],
        scriptStorePath: String? = nil
    ) throws -> EmbeddedRuntimeConfig {
        var copy = self
        copy.mitm = plugins.isEmpty ? nil : Mitm(
            plugins: plugins,
            caCertificate: authority.certificateDER.base64EncodedString(),
            caPrivateKey: try authority.privateKeyRepresentation().base64EncodedString(),
            scriptAssets: scriptAssets,
            resourceAssets: resourceAssets,
            scriptStorePath: scriptStorePath
        )
        return copy
    }

    /// The same config without HTTPS decryption.
    public func withoutMitm() -> EmbeddedRuntimeConfig {
        var copy = self
        copy.mitm = nil
        return copy
    }

    /// The same config with its secrets set from `secret`.
    public func withSecret(_ secret: NodeSecret) -> EmbeddedRuntimeConfig {
        var copy = self
        switch outbound {
        case let .shadowsocks(server, port, method, _):
            copy.outbound = .shadowsocks(server: server, port: port, method: method, password: secret.password ?? "")
        case var .wireguard(peer):
            peer.privateKey = secret.privateKey ?? ""
            peer.presharedKey = secret.presharedKey
            copy.outbound = .wireguard(peer)
        case var .vless(vless):
            vless.uuid = secret.uuid ?? ""
            copy.outbound = .vless(vless)
        case var .native(native):
            native.settings = secret.outbound
                .flatMap { try? JSONDecoder().decode([String: JSONValue].self, from: Data($0.utf8)) } ?? [:]
            copy.outbound = .native(native)
        }
        return copy
    }

    public static func decoded(from data: Data) throws -> EmbeddedRuntimeConfig {
        try JSONDecoder().decode(EmbeddedRuntimeConfig.self, from: data)
    }

    public func encoded() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(self)
    }
}

extension EmbeddedRuntimeConfig.Mitm {
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            plugins: try container.decode([String].self, forKey: .plugins),
            caCertificate: try container.decode(String.self, forKey: .caCertificate),
            caPrivateKey: try container.decode(String.self, forKey: .caPrivateKey),
            scriptAssets: try container.decodeIfPresent([String: String].self, forKey: .scriptAssets) ?? [:],
            scriptStorePath: try container.decodeIfPresent(String.self, forKey: .scriptStorePath)
        )
    }
}

extension EmbeddedRuntimeConfig.Outbound: Codable {
    enum CodingKeys: String, CodingKey {
        case type
        case server
        case port
        case method
        case password
        case privateKey = "private_key"
        case peerPublicKey = "peer_public_key"
        case presharedKey = "preshared_key"
        case persistentKeepaliveSecs = "persistent_keepalive_secs"
        case uuid
        case flow
        case reality
    }

    /// The keys of a dedicated case: all of `needed` (the type, the address and `needs`) and nothing outside
    /// `allowed` (those and `mayHave`).
    private struct DedicatedKeys {
        let needed: Set<String>
        let allowed: Set<String>

        init(needs: [CodingKeys], mayHave: [CodingKeys] = []) {
            let address: [CodingKeys] = [.type, .server, .port]
            needed = Set((address + needs).map(\.rawValue))
            allowed = needed.union(mayHave.map(\.rawValue))
        }

        func match(_ keys: Set<String>) -> Bool {
            needed.isSubset(of: keys) && keys.isSubset(of: allowed)
        }
    }

    private static let shadowsocksKeys = DedicatedKeys(needs: [.method, .password])
    private static let wireGuardKeys = DedicatedKeys(
        needs: [.privateKey, .peerPublicKey],
        mayHave: [.presharedKey, .persistentKeepaliveSecs]
    )
    private static let vlessKeys = DedicatedKeys(needs: [.uuid, .reality], mayHave: [.flow])
    /// `RealitySettings` needs every one of its fields and keeps no other.
    private static let realityKeys = Set(EmbeddedRuntimeConfig.RealitySettings.CodingKeys.allCases.map(\.rawValue))

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)
        let server = try container.decode(String.self, forKey: .server)
        let port = try container.decode(UInt16.self, forKey: .port)
        let all = try decoder.container(keyedBy: DynamicCodingKey.self)
        let keys = Set(all.allKeys.map(\.stringValue))
        if let dedicated = try Self.dedicated(type, server: server, port: port, keys: keys, in: container) {
            self = dedicated
            return
        }
        var settings: [String: JSONValue] = [:]
        for key in all.allKeys where !EmbeddedRuntimeConfig.NativeOutbound.addressKeys.contains(key.stringValue) {
            settings[key.stringValue] = try all.decode(JSONValue.self, forKey: key)
        }
        self = .native(EmbeddedRuntimeConfig.NativeOutbound(type: type, server: server, port: port, settings: settings))
    }

    /// The dedicated case of an outbound whose keys are exactly what that case keeps, otherwise nil and the outbound
    /// stays native. A node with anything more (a `via`, a transport, multiplexing, a WireGuard address and MTU)
    /// would lose it in the dedicated case. The profile copies stay in their case too (`withoutSecret()`): a
    /// dedicated case keeps its secret fields there, empty, and a native outbound keeps only `type`, `server` and
    /// `port`, which no dedicated case takes, so `withSecret(_:)` gives it back all its settings from the Keychain.
    private static func dedicated(
        _ type: String,
        server: String,
        port: UInt16,
        keys: Set<String>,
        in container: KeyedDecodingContainer<CodingKeys>
    ) throws -> EmbeddedRuntimeConfig.Outbound? {
        switch type {
        case "shadowsocks" where shadowsocksKeys.match(keys):
            return .shadowsocks(
                server: server,
                port: port,
                method: try container.decode(String.self, forKey: .method),
                password: try container.decode(String.self, forKey: .password)
            )
        case "wireguard" where wireGuardKeys.match(keys):
            return .wireguard(
                EmbeddedRuntimeConfig.WireGuardPeer(
                    privateKey: try container.decode(String.self, forKey: .privateKey),
                    peerPublicKey: try container.decode(String.self, forKey: .peerPublicKey),
                    presharedKey: try container.decodeIfPresent(String.self, forKey: .presharedKey),
                    server: server,
                    port: port,
                    persistentKeepaliveSecs: try container.decodeIfPresent(UInt16.self, forKey: .persistentKeepaliveSecs)
                )
            )
        // VLESS over TLS or a transport is a native outbound; only REALITY over TCP has its own case.
        case "vless" where vlessKeys.match(keys):
            let reality = try container.nestedContainer(keyedBy: DynamicCodingKey.self, forKey: .reality)
            guard Set(reality.allKeys.map(\.stringValue)) == realityKeys else {
                return nil
            }
            return .vless(
                EmbeddedRuntimeConfig.VlessOutbound(
                    server: server,
                    port: port,
                    uuid: try container.decode(String.self, forKey: .uuid),
                    flow: try container.decodeIfPresent(String.self, forKey: .flow) ?? "",
                    reality: try container.decode(EmbeddedRuntimeConfig.RealitySettings.self, forKey: .reality)
                )
            )
        default:
            return nil
        }
    }

    public func encode(to encoder: Encoder) throws {
        if case let .native(native) = self {
            var all = encoder.container(keyedBy: DynamicCodingKey.self)
            for (key, value) in native.settings where !EmbeddedRuntimeConfig.NativeOutbound.addressKeys.contains(key) {
                try all.encode(value, forKey: DynamicCodingKey(key))
            }
            try all.encode(native.type, forKey: DynamicCodingKey("type"))
            try all.encode(native.server, forKey: DynamicCodingKey("server"))
            try all.encode(native.port, forKey: DynamicCodingKey("port"))
            return
        }
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case let .shadowsocks(server, port, method, password):
            try container.encode("shadowsocks", forKey: .type)
            try container.encode(server, forKey: .server)
            try container.encode(port, forKey: .port)
            try container.encode(method, forKey: .method)
            try container.encode(password, forKey: .password)
        case let .wireguard(peer):
            try container.encode("wireguard", forKey: .type)
            try container.encode(peer.server, forKey: .server)
            try container.encode(peer.port, forKey: .port)
            try container.encode(peer.privateKey, forKey: .privateKey)
            try container.encode(peer.peerPublicKey, forKey: .peerPublicKey)
            try container.encodeIfPresent(peer.presharedKey, forKey: .presharedKey)
            try container.encodeIfPresent(peer.persistentKeepaliveSecs, forKey: .persistentKeepaliveSecs)
        case let .vless(vless):
            try container.encode("vless", forKey: .type)
            try container.encode(vless.server, forKey: .server)
            try container.encode(vless.port, forKey: .port)
            try container.encode(vless.uuid, forKey: .uuid)
            try container.encode(vless.flow, forKey: .flow)
            try container.encode(vless.reality, forKey: .reality)
        case .native:
            // Encoded above.
            break
        }
    }
}

extension EmbeddedRuntimeConfig.Outbound {
    /// Whether this dedicated case holds the node of the runtime outbound entry `entry`: the same type, address and
    /// secret, whatever else `entry` has. A native outbound is never such a copy.
    func isDedicatedCopy(of entry: [String: JSONValue]) -> Bool {
        switch self {
        case let .shadowsocks(server, port, method, password):
            return Self.matches(entry, type: "shadowsocks", server: server, port: port)
                && entry["method"] == .string(method)
                && entry["password"] == .string(password)
        case let .vless(vless):
            return Self.matches(entry, type: "vless", server: vless.server, port: vless.port)
                && entry["uuid"] == .string(vless.uuid)
        case let .wireguard(peer):
            return Self.matches(entry, type: "wireguard", server: peer.server, port: peer.port)
                && entry["private_key"] == .string(peer.privateKey)
                && entry["peer_public_key"] == .string(peer.peerPublicKey)
        case .native:
            return false
        }
    }

    private static func matches(_ entry: [String: JSONValue], type: String, server: String, port: UInt16) -> Bool {
        entry["type"] == .string(type) && entry["server"] == .string(server) && entry["port"] == .int(Int64(port))
    }
}

/// Secrets of one node as stored in the Keychain: a small JSON object.
public struct NodeSecret: Codable, Equatable {
    public var password: String?
    public var privateKey: String?
    public var presharedKey: String?
    /// VLESS user id.
    public var uuid: String?
    /// JSON settings of a native outbound (`EmbeddedRuntimeConfig.NativeOutbound.settings`).
    public var outbound: String?

    enum CodingKeys: String, CodingKey {
        case password
        case privateKey = "private_key"
        case presharedKey = "preshared_key"
        case uuid
        case outbound
    }

    public init(
        password: String? = nil,
        privateKey: String? = nil,
        presharedKey: String? = nil,
        uuid: String? = nil,
        outbound: String? = nil
    ) {
        self.password = password
        self.privateKey = privateKey
        self.presharedKey = presharedKey
        self.uuid = uuid
        self.outbound = outbound
    }

    /// The value written to the Keychain.
    public func keychainValue() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return String(decoding: try encoder.encode(self), as: UTF8.self)
    }

    /// Reads a Keychain value. Items saved before secrets became JSON hold the bare Shadowsocks password.
    public static func fromKeychainValue(_ value: String) -> NodeSecret {
        if let data = value.data(using: .utf8),
           let secret = try? JSONDecoder().decode(NodeSecret.self, from: data),
           secret != NodeSecret()
        {
            return secret
        }
        return NodeSecret(password: value)
    }
}

/// Keys shared by the app and the extension in `NETunnelProviderProtocol.providerConfiguration`.
public enum TunnelProfileKeys {
    /// `Data` holding the encoded `EmbeddedRuntimeConfig`, without secrets.
    public static let runtimeConfig = "networkcore.runtime-config"
    /// `Data` holding the encoded `TunnelInterface`. Profiles saved before it existed use `TunnelInterface.proxy`.
    public static let tunnelInterface = "networkcore.tunnel-interface"
    /// Display name of the imported node. Not secret.
    public static let nodeName = "networkcore.node-name"
    /// The version of the node key this profile was saved with (`NodeSecretStore.versionedAccount`). The extension
    /// reads that key and no other, so the profile never runs with the key of a profile saved after it. Profiles
    /// saved before it existed read the fixed account.
    public static let nodeKeyVersion = "networkcore.node-key-version"
    /// `true` when the tunnel follows the subscription's rules and groups: the extension then adds the `routing`
    /// section from the node library in the Keychain (`NodeLibrary.keychainAccount`). Absent means the selected node
    /// carries all traffic.
    public static let routing = "networkcore.routing"
    /// `Data` holding a JSON array of the enabled MITM plugin texts (`MitmPluginLibrary.profileValue`). Absent or
    /// empty means no HTTPS decryption.
    public static let mitmPlugins = "networkcore.mitm-plugins"
    /// `Data` holding a JSON object of `script-path` to source for the scripts the user allowed in the enabled
    /// plugins (`MitmScriptLibrary.profileValue`). Scripts left out do nothing.
    public static let mitmScripts = "networkcore.mitm-scripts"
    /// `Data` holding a JSON object of remote `[Map Local]` body URL to its downloaded bytes for the enabled plugins
    /// (`MitmResourceLibrary.profileValue`). Lines whose body is absent answer nothing.
    public static let mitmResources = "networkcore.mitm-resources"
    /// `Data` pointing at the HTTPS decryption payload in the shared Keychain (`MitmProfileBundle`). Profiles
    /// saved by this version carry it instead of `mitmPlugins`, `mitmScripts` and `mitmResources`, which made the
    /// configuration too large for iOS to save.
    public static let mitmBundle = "networkcore.mitm-bundle"
    /// The `ProxyMode` raw value the tunnel starts in. Profiles saved before it existed use `rule` when
    /// `routing` is set and `global` otherwise (`ProxyMode.saved(in:)`).
    public static let mode = "networkcore.mode"
    /// `Data` holding the encoded `DnsSettings`. Absent keeps the engine's default resolver.
    public static let dns = "networkcore.dns"
    /// `Data` holding a JSON array of the engine's `redirects` lines (`TrafficRedirects.profileValue`). Absent
    /// rewrites nothing.
    public static let redirects = "networkcore.redirects"
    /// `true` when the tunnel refuses UDP to port 443 so apps fall back from QUIC (HTTP/3) to TCP, where HTTPS
    /// decryption and domain rules see them. Absent is off.
    public static let blockQuic = "networkcore.block-quic"
    /// `Data` holding the encoded `LocalRules`: the rule mode of a node list without subscription rules, and the
    /// user's rules in front of a subscription's. Absent is no rules of the user's.
    public static let localRules = "networkcore.local-rules"
    /// `Data` holding the encoded `NotificationSettings`. Absent posts every kind of notification.
    public static let notifications = "networkcore.notifications"
    /// The `TrafficTakeover` raw value: the tunnel interface, the system proxy, or both. Absent is
    /// `TrafficTakeover.default`.
    public static let takeover = "networkcore.takeover"
    /// `Data` holding a JSON array of the system proxy's exceptions (`ProxyExceptions.profileValue`). Absent is
    /// `ProxyExceptions.defaults`.
    public static let proxyExceptions = "networkcore.proxy-exceptions"
    /// `true` when the Mac acts as the gateway of other devices on the local network (`GatewayMode`). Absent is off.
    public static let gateway = "networkcore.gateway"
}

extension EmbeddedRuntimeConfig {
    // Here rather than in AnxLicense.swift: `outbound` and `routing` are only settable in this file.
    /// The same config with each ANX outbound given the license fields of its node's origin (anx spec §2.4.1): the
    /// subscription's nodes by outbound id, the selected node by its server and port. An outbound without an origin,
    /// or whose origin has no license, is left as it is.
    public func withAnxLicense(_ licenses: OutboundLicenses) -> EmbeddedRuntimeConfig {
        var copy = self
        if case var .native(native) = outbound, native.type == "anx",
           let fields = licenses.fields(server: native.server, port: native.port)
        {
            native.settings.merge(fields) { _, new in new }
            copy.outbound = .native(native)
        }
        if var routing = copy.routing {
            for (id, settings) in routing.outbounds where settings["type"] == .string("anx") {
                if let fields = licenses.fields(outbound: id) {
                    routing.outbounds[id] = settings.merging(fields) { _, new in new }
                }
            }
            copy.routing = routing
        }
        return copy
    }
}
