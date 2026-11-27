import Foundation

/// Whether this build runs mesh networks: `enabled`, and an engine that answers `capabilities.mesh` (mesh-status.md).
/// The feature set does not tell: since kernel 0.3.0-rc.2 it is 7 with or without the `mesh` feature. The app shows
/// the 组网 entry from it, whether the tunnel runs or not.
public enum MeshSupport {
    /// The control request that asks an engine what it can do.
    public static let capabilitiesRequest = Data(#"{"cmd":"capabilities"}"#.utf8)

    /// Whether the answer to `capabilitiesRequest` (`{"capabilities":{"mesh":bool}}`) says the engine runs mesh.
    /// No answer, a refusal or anything else is no.
    public static func runsMesh(capabilitiesAnswer data: Data?) -> Bool {
        guard let data,
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let capabilities = object["capabilities"] as? [String: Any],
              let mesh = capabilities["mesh"] as? NSNumber
        else {
            return false
        }
        return mesh.boolValue
    }

    private static let probeLock = NSLock()
    private static var probe: (() -> Data?)?
    private static var probed: Bool?

    /// Installs how the app asks an engine for its capabilities: the answer to `capabilitiesRequest` from an engine
    /// built with the release gate's `allow_mesh` (the Bridge links no engine). Until then no engine runs mesh.
    public static func installCapabilitiesProbe(_ probe: @escaping () -> Data?) {
        probeLock.lock()
        defer { probeLock.unlock() }
        self.probe = probe
        probed = nil
    }

    /// Whether the engine this build links answers `capabilities.mesh`; asked once, then kept.
    static var engineRunsMesh: Bool {
        probeLock.lock()
        defer { probeLock.unlock() }
        if let probed {
            return probed
        }
        guard let probe else {
            return false
        }
        let answer = runsMesh(capabilitiesAnswer: probe())
        probed = answer
        return answer
    }

    /// Whether this build offers 组网 at all. Off until kernel 0.3 is complete and audited, whatever the engine
    /// runs: the release gate, not a capability (the same as client-core's `mesh::ENABLED`). A build with the
    /// `MESH_PREVIEW` compilation condition turns it on for testing.
    #if MESH_PREVIEW
    public static let enabled = true
    #else
    public static let enabled = false
    #endif

    /// Whether this build shows the kernel 0.4 parts of 组网 (regions, host groups, external networks, advertising,
    /// relay and host switches). A second gate behind `enabled`, off until kernel 0.4 is complete and audited (the
    /// same as client-core's `mesh::V04_ENABLED`); the `MESH04_PREVIEW` compilation condition turns it on for
    /// testing. While it is off the app reads the snapshot through `MeshView.stripV04`.
    #if MESH04_PREVIEW
    public static let v04Enabled = enabled
    #else
    public static let v04Enabled = false
    #endif

    public static var available: Bool {
        enabled && engineRunsMesh
    }
}

/// The `tunnel` section of the engine's `mesh_status` snapshot (networkcore_anixops docs/architecture/mesh-status.md,
/// 隧道生命周期, R23): the networks the tunnel has to carry and the DNS for the internal domain names. With only the
/// mesh on (`TunnelProfileKeys.meshOnly`), the extension routes exactly these into the tunnel and applies them again
/// each time `routesVersion` changes.
public struct MeshTunnel: Equatable {
    /// Networks to route into the tunnel, in CIDR form, as the engine orders them.
    public var routes: [String]
    /// Goes up by one each time `routes` change; 1 after the engine starts.
    public var routesVersion: Int
    /// The tunnel's DNS address answering the internal domain names; empty without them.
    public var dnsServers: [String]
    /// The suffixes those servers answer.
    public var matchDomains: [String]

    public init(routes: [String], routesVersion: Int, dnsServers: [String] = [], matchDomains: [String] = []) {
        self.routes = routes
        self.routesVersion = routesVersion
        self.dnsServers = dnsServers
        self.matchDomains = matchDomains
    }

    /// The section of a `mesh_status` answer; nil for a refusal or an answer without one.
    public init?(status data: Data?) {
        guard let data,
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let tunnel = object["tunnel"] as? [String: Any]
        else {
            return nil
        }
        let dns = tunnel["dns"] as? [String: Any]
        self.init(
            routes: (tunnel["routes"] as? [String] ?? []).filter { !Self.tooWide($0) },
            routesVersion: (tunnel["routesVersion"] as? NSNumber)?.intValue ?? 0,
            dnsServers: dns?["servers"] as? [String] ?? [],
            matchDomains: dns?["matchDomains"] as? [String] ?? []
        )
    }

    /// The widest IPv4 and IPv6 prefixes a mesh route may have (intranet-mesh §15.4, U2), as client-core's
    /// `mesh::too_wide`.
    static let minimumPrefixV4 = 8
    static let minimumPrefixV6 = 7

    /// Whether `route` (CIDR text, or a bare address for one host) is wider than any mesh network: a default route,
    /// one of its halves, or any other IPv4 range wider than /8 or IPv6 range wider than /7. The tunnel never carries
    /// such a route whatever the engine names. What does not read as a route is not judged here.
    public static func tooWide(_ route: String) -> Bool {
        let parts = route.trimmingCharacters(in: .whitespaces).split(separator: "/", maxSplits: 1)
        guard parts.count == 2, let length = UInt8(parts[1].trimmingCharacters(in: .whitespaces)) else {
            return false
        }
        let address = parts[0].trimmingCharacters(in: .whitespaces)
        var v4 = in_addr()
        var v6 = in6_addr()
        if inet_pton(AF_INET, address, &v4) == 1 {
            return Int(length) < minimumPrefixV4
        }
        if inet_pton(AF_INET6, address, &v6) == 1 {
            return Int(length) < minimumPrefixV6
        }
        return false
    }

    /// The interface of a mesh-only tunnel: `base`'s addresses and MTU, these routes, and each DNS server as a host
    /// route so its queries reach the tunnel. The servers only answer their suffixes: without any, the device keeps its
    /// own DNS. Nil (`tunnel` unknown yet, as right after the start) routes nothing.
    public static func interface(_ tunnel: MeshTunnel?, base: TunnelInterface = .proxy) -> TunnelInterface {
        let servers = tunnel.map { $0.matchDomains.isEmpty ? [] : $0.dnsServers } ?? []
        var routes = (tunnel?.routes ?? []).filter { !tooWide($0) }
        for server in servers where !routes.contains(server) {
            routes.append(server)
        }
        return TunnelInterface(addresses: base.addresses, dnsServers: servers, includedRoutes: routes, mtu: base.mtu)
    }
}

extension EmbeddedRuntimeConfig {
    /// The profile of a tunnel with only the mesh on: the joined networks' outbounds and nothing else, the rule mode,
    /// everything that is not a mesh route direct (mesh-status.md, 只开组网). The engine sends each network's routes to
    /// its outbound; the tunnel carries only those routes, so the final rule only matters for what reaches it anyway.
    public static func meshOnly(_ outbounds: [String: [String: JSONValue]], profileID: String) -> EmbeddedRuntimeConfig {
        // The top-level outbound is never encoded beside a routing section; it only names what the config is.
        EmbeddedRuntimeConfig(
            outbound: .native(NativeOutbound(type: "direct", server: "", port: 0, settings: [:])),
            profileID: profileID
        )
        .withRouting(Routing(outbounds: outbounds, rules: ["FINAL,DIRECT"], groups: []))
        .withMode(.rule)
    }
}
