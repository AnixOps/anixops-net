import Foundation
import Security

/// A 组网 (mesh) network this device saved: the kernel's `config_version: 1` network configuration without its
/// secret (docs/architecture/mesh-client.md; networkcore_anixops docs/architecture/mesh-module.md, 配置). The secret
/// is in the shared Keychain (`MeshNetworkLibrary.secretAccount(_:)`), and goes into the configuration only when
/// the configuration is handed to the engine (`runtimeConfig(secret:privateKey:)`), as does the network's static
/// private key (`MeshNetworkLibrary.privateKeyAccount(_:)`, `node.private_key`). Phones are access nodes: no listening port,
/// no relay, no advertised subnets.
public struct MeshNetwork: Codable, Equatable, Identifiable {
    /// `network.name`: the network's id in the status snapshot and the mesh outbound's tag.
    public var name: String
    /// The configuration as JSON text, without `network.secret`.
    public var config: String
    /// Joined by this device: the network goes into the tunnel profile.
    public var joined: Bool

    public var id: String { name }

    public init(name: String, config: String, joined: Bool) {
        self.name = name
        self.config = config
        self.joined = joined
    }

    /// The configuration the engine takes: this one with `network.secret` filled in, and `node.private_key` when
    /// `privateKey` is given (the node section is made if the saved text has none). Nil when the saved text is not
    /// a JSON object.
    public func runtimeConfig(secret: String, privateKey: String? = nil) -> [String: Any]? {
        guard var object = (try? JSONSerialization.jsonObject(with: Data(config.utf8))) as? [String: Any] else {
            return nil
        }
        var network = object["network"] as? [String: Any] ?? [:]
        network["secret"] = secret
        object["network"] = network
        if let privateKey {
            var node = object["node"] as? [String: Any] ?? [:]
            node["private_key"] = privateKey
            object["node"] = node
        }
        return object
    }

    /// `mesh_save_network`'s argument as JSON text, secret and private key included. Nil as for `runtimeConfig`.
    public func runtimeConfigJSON(secret: String, privateKey: String? = nil) -> String? {
        guard let object = runtimeConfig(secret: secret, privateKey: privateKey),
              let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    /// The encryption mode as the status snapshot words it: `forward_secrecy`, `standard` or `none`; nil when the
    /// saved text is not a JSON object.
    public var security: String? {
        guard let object = (try? JSONSerialization.jsonObject(with: Data(config.utf8))) as? [String: Any] else {
            return nil
        }
        let network = object["network"] as? [String: Any]
        if network?["cipher"] as? String == "none" {
            return "none"
        }
        return network?["secure_mode"] as? Bool == false ? "standard" : "forward_secrecy"
    }

    /// The network's key in the profile's `routing.outbounds` (`[A-Za-z0-9._-]` only): `mesh-` and the name, or
    /// `mesh-x` and the name's UTF-8 bytes in hex when the name has other characters, so two names never share a key.
    public var outboundKey: String {
        let allowed = name.unicodeScalars.allSatisfy { scalar in
            scalar.isASCII && (CharacterSet.alphanumerics.contains(scalar) || scalar == "." || scalar == "_" || scalar == "-")
        }
        if allowed, !name.isEmpty {
            return "mesh-\(name)"
        }
        return "mesh-x" + name.utf8.map { String(format: "%02x", $0) }.joined()
    }

    /// The `routing.outbounds` entry: `{"type":"mesh"}` with the configuration, its secret and its private key.
    /// Nil as for `runtimeConfig`.
    public func outboundEntry(secret: String, privateKey: String? = nil) -> [String: JSONValue]? {
        guard let text = runtimeConfigJSON(secret: secret, privateKey: privateKey),
              var entry = try? JSONDecoder().decode([String: JSONValue].self, from: Data(text.utf8))
        else {
            return nil
        }
        entry["type"] = .string("mesh")
        return entry
    }
}

/// What the 添加网络 form holds (docs/mesh-ui.md, 添加网络): name, secret, peers, virtual address, encryption. The
/// region is assigned by the network, and the name other members show is the device name.
public struct MeshNetworkDraft: Equatable {
    public enum Security: String, CaseIterable {
        /// The default: managed network, secure mode (forward secrecy).
        case forwardSecrecy = "forward_secrecy"
        /// A network in migration that runs the older mode (no forward secrecy), encrypted.
        case standard
    }

    public enum Problem: Equatable {
        case name
        case secret
        case peers
        case peer(String)
        case address
    }

    /// The default listening port of mesh members.
    public static let defaultPort: UInt16 = 11010

    public var name = ""
    public var secret = ""
    /// One peer per line: `host:port`, `[v6]:port` or `host`, optionally with `tcp://` or `udp://` in front.
    public var peers = ""
    /// The IPv4 virtual address with its prefix, such as `10.144.144.5/24`; empty when the network assigns one.
    public var address = ""
    public var security = Security.forwardSecrecy

    public init() {}

    /// The first thing wrong with the form, or nil when it can be saved.
    public var problem: Problem? {
        let name = name.trimmingCharacters(in: .whitespaces)
        if name.isEmpty || name.count > 64 || name.contains(where: { $0.isNewline || $0 == "/" }) {
            return .name
        }
        if secret.isEmpty {
            return .secret
        }
        let lines = peerLines
        if lines.isEmpty {
            return .peers
        }
        for line in lines where Self.peer(line) == nil {
            return .peer(line)
        }
        let address = address.trimmingCharacters(in: .whitespaces)
        if !address.isEmpty, !Self.validAddress(address) {
            return .address
        }
        return nil
    }

    private var peerLines: [String] {
        peers.split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }

    /// The saved network (configuration without the secret); nil while `problem` is not nil.
    public func network() -> MeshNetwork? {
        guard problem == nil else {
            return nil
        }
        var network: [String: Any] = ["name": name.trimmingCharacters(in: .whitespaces)]
        switch security {
        case .forwardSecrecy:
            network["kind"] = "managed"
            network["secure_mode"] = true
        case .standard:
            network["kind"] = "legacy"
            network["secure_mode"] = false
        }
        var node: [String: Any] = ["role": "access"]
        let address = address.trimmingCharacters(in: .whitespaces)
        if !address.isEmpty {
            node["address"] = address
        }
        let peers: [[String: Any]] = peerLines.compactMap(Self.peer).map { peer in
            ["server": peer.server, "port": Int(peer.port), "transport": peer.transport]
        }
        let config: [String: Any] = ["config_version": 1, "network": network, "node": node, "peers": peers]
        guard let data = try? JSONSerialization.data(withJSONObject: config, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8)
        else {
            return nil
        }
        return MeshNetwork(name: network["name"] as? String ?? "", config: text, joined: true)
    }

    /// A peer line: `[tcp://|udp://]host[:port]`, with IPv6 hosts in brackets.
    public static func peer(_ line: String) -> (server: String, port: UInt16, transport: String)? {
        var rest = line.trimmingCharacters(in: .whitespaces)
        var transport = "tcp"
        for scheme in ["tcp", "udp"] where rest.lowercased().hasPrefix("\(scheme)://") {
            transport = scheme
            rest = String(rest.dropFirst(scheme.count + 3))
        }
        let host: String
        var port = defaultPort
        if rest.hasPrefix("[") {
            guard let close = rest.firstIndex(of: "]") else {
                return nil
            }
            host = String(rest[rest.index(after: rest.startIndex)..<close])
            let after = rest[rest.index(after: close)...]
            if !after.isEmpty {
                guard after.hasPrefix(":"), let value = UInt16(after.dropFirst()), value > 0 else {
                    return nil
                }
                port = value
            }
            guard IPAddressText.isIPv6(host) else {
                return nil
            }
        } else {
            let parts = rest.split(separator: ":", omittingEmptySubsequences: false)
            guard parts.count <= 2, let first = parts.first, !first.isEmpty else {
                return nil
            }
            host = String(first)
            if parts.count == 2 {
                guard let value = UInt16(parts[1]), value > 0 else {
                    return nil
                }
                port = value
            }
            guard host.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "." || $0 == "-") }) else {
                return nil
            }
        }
        return (host, port, transport)
    }

    /// An IPv4 address with a prefix of /8 to /30 (mesh-module.md, `node.address`).
    static func validAddress(_ text: String) -> Bool {
        let parts = text.split(separator: "/", omittingEmptySubsequences: false)
        guard parts.count == 2, IPAddressText.isIPv4(String(parts[0])),
              let prefix = Int(parts[1]), (8...30).contains(prefix)
        else {
            return false
        }
        return true
    }
}

/// The saved mesh networks: the list in the app's settings, each secret and static private key in the shared Keychain.
public enum MeshNetworkLibrary {
    static let defaultsKey = "networkcore.mesh-networks"

    /// The Keychain account of a network's secret.
    public static func secretAccount(_ name: String) -> String {
        "mesh-network.\(name)"
    }

    /// The Keychain account of a network's static private key (`node.private_key`, secure mode). The prefix differs
    /// from `secretAccount(_:)`'s, so no network name can make the two accounts meet.
    public static func privateKeyAccount(_ name: String) -> String {
        "mesh-network-key.\(name)"
    }

    /// Whether `key` is a static private key the kernel takes: standard base64 with padding of exactly 32 bytes.
    public static func isValidPrivateKey(_ key: String) -> Bool {
        Data(base64Encoded: key)?.count == 32
    }

    /// A new static private key: 32 bytes from the system random source, as standard base64 with padding. Throws
    /// when the random source fails, so no network is saved with a weak or empty key.
    public static func newPrivateKey() throws -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        let status = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        guard status == errSecSuccess else {
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(status))
        }
        return Data(bytes).base64EncodedString()
    }

    /// The network's saved private key; nil when there is none or the saved value is not a valid key. The extension
    /// reads this and never creates a key: without one it leaves `node.private_key` out.
    public static func privateKey(_ name: String) -> String? {
        guard let key = try? NodeSecretStore.value(account: privateKeyAccount(name)), isValidPrivateKey(key) else {
            return nil
        }
        return key
    }

    /// The network's private key, created and saved when absent or not valid. An existing key is never replaced:
    /// the panel lists its public key, and a new one would lock the device out.
    @discardableResult
    public static func ensurePrivateKey(_ name: String) throws -> String {
        // A Keychain that cannot be read now (locked, interrupted) throws here instead of reading as "no key": a
        // new key would replace the one the panel lists. Only a missing or malformed key is replaced.
        if let key = try NodeSecretStore.value(account: privateKeyAccount(name)), isValidPrivateKey(key) {
            return key
        }
        let key = try newPrivateKey()
        _ = try NodeSecretStore.save(key, account: privateKeyAccount(name))
        return key
    }

    /// Gives every joined network without a key one, for networks saved by earlier versions. The app calls it before
    /// the networks go to the tunnel profile; a failure leaves that network without a key (it runs without
    /// `node.private_key`), and the next call tries again.
    public static func ensurePrivateKeys(_ defaults: UserDefaults = .standard) {
        for network in load(defaults) where network.joined {
            _ = try? ensurePrivateKey(network.name)
        }
    }

    public static func load(_ defaults: UserDefaults = .standard) -> [MeshNetwork] {
        guard let data = defaults.data(forKey: defaultsKey) else {
            return []
        }
        return (try? JSONDecoder().decode([MeshNetwork].self, from: data)) ?? []
    }

    public static func store(_ networks: [MeshNetwork], _ defaults: UserDefaults = .standard) {
        defaults.set(try? JSONEncoder().encode(networks), forKey: defaultsKey)
    }

    /// Saves `network` with its secret, in place of a saved network of the same name. Only one network is joined
    /// at a time on a phone (docs/mesh-ui.md, 入口与导航): joining this one marks the others not joined.
    public static func save(_ network: MeshNetwork, secret: String, _ defaults: UserDefaults = .standard) throws {
        // The key first: when it cannot be made, no network is saved without one. A saved key stays as it is.
        _ = try ensurePrivateKey(network.name)
        _ = try NodeSecretStore.save(secret, account: secretAccount(network.name))
        var networks = load(defaults).filter { $0.name != network.name }
        if network.joined {
            networks = networks.map { MeshNetwork(name: $0.name, config: $0.config, joined: false) }
        }
        networks.append(network)
        store(networks, defaults)
    }

    public static func remove(_ name: String, _ defaults: UserDefaults = .standard) {
        try? NodeSecretStore.delete(account: secretAccount(name))
        try? NodeSecretStore.delete(account: privateKeyAccount(name))
        store(load(defaults).filter { $0.name != name }, defaults)
    }

    /// Marks `name` joined (and the others not) or not joined.
    public static func setJoined(_ name: String, _ joined: Bool, _ defaults: UserDefaults = .standard) {
        let networks = load(defaults).map { network in
            MeshNetwork(name: network.name, config: network.config, joined: network.name == name ? joined : (joined ? false : network.joined))
        }
        store(networks, defaults)
    }

    public static func secret(_ name: String) -> String? {
        try? NodeSecretStore.value(account: secretAccount(name))
    }

    /// `TunnelProfileKeys.meshNetworks`: the joined networks, without their secrets. Nil when none is joined.
    public static func profileValue(_ networks: [MeshNetwork]) -> Data? {
        let joined = networks.filter(\.joined)
        guard !joined.isEmpty else {
            return nil
        }
        return try? JSONEncoder().encode(joined)
    }

    /// `profileValue(_:)` behind the release gate (`MeshSupport.enabled`): nil while it is closed, so no joined network
    /// reaches a tunnel profile in a release build.
    public static func gatedProfileValue(_ networks: [MeshNetwork], enabled: Bool = MeshSupport.enabled) -> Data? {
        enabled ? profileValue(networks) : nil
    }

    /// `fromProfileValue(_:)` behind the release gate: none while it is closed, whatever an older profile names, so
    /// the extension of a release build never joins a network.
    public static func gatedNetworks(fromProfileValue value: Data?, enabled: Bool = MeshSupport.enabled) -> [MeshNetwork] {
        enabled ? fromProfileValue(value) : []
    }

    /// The joined networks a profile names; empty when it names none or the value is malformed.
    public static func fromProfileValue(_ value: Data?) -> [MeshNetwork] {
        guard let value, let networks = try? JSONDecoder().decode([MeshNetwork].self, from: value) else {
            return []
        }
        return networks.filter(\.joined)
    }

    /// The `routing.outbounds` entries of `networks`, each with the secret `secret` gives for its name and the
    /// private key `privateKey` gives (none: no `node.private_key`), and the names left out: no secret, or a
    /// configuration that is not a JSON object.
    public static func outbounds(
        _ networks: [MeshNetwork],
        secret: (String) -> String?,
        privateKey: (String) -> String? = { _ in nil }
    ) -> (outbounds: [String: [String: JSONValue]], missing: [String]) {
        var outbounds: [String: [String: JSONValue]] = [:]
        var missing: [String] = []
        for network in networks {
            guard let value = secret(network.name), let entry = network.outboundEntry(secret: value, privateKey: privateKey(network.name)) else {
                missing.append(network.name)
                continue
            }
            outbounds[network.outboundKey] = entry
        }
        return (outbounds, missing)
    }

    /// The secrets of the joined networks by name, for `TunnelStartSecrets.meshSecrets`; nil when there are none.
    public static func joinedSecrets(_ defaults: UserDefaults = .standard) -> [String: String]? {
        guard MeshSupport.enabled else {
            return nil
        }
        var secrets: [String: String] = [:]
        for network in load(defaults) where network.joined {
            secrets[network.name] = secret(network.name)
        }
        return secrets.isEmpty ? nil : secrets
    }

    /// The static private keys of the joined networks by name, for `TunnelStartSecrets.meshPrivateKeys` (a macOS
    /// System Extension cannot read the user's Keychain); nil when there are none.
    public static func joinedPrivateKeys(_ defaults: UserDefaults = .standard) -> [String: String]? {
        guard MeshSupport.enabled else {
            return nil
        }
        var keys: [String: String] = [:]
        for network in load(defaults) where network.joined {
            keys[network.name] = privateKey(network.name)
        }
        return keys.isEmpty ? nil : keys
    }
}
