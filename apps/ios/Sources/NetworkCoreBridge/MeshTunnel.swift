import Foundation

/// Whether this build runs mesh networks: `enabled`, and an engine of feature set 7 or later (mesh-status.md).
/// The app shows the 组网 entry from it, whether the tunnel runs or not.
public enum MeshSupport {
    public static let minimumFeatureSet: UInt32 = 7

    /// Whether this build offers 组网 at all. Off until kernel 0.3 is complete and audited, whatever the engine
    /// runs: the release gate, not a capability (the same as client-core's `mesh::ENABLED`). A build with the
    /// `MESH_PREVIEW` compilation condition turns it on for testing.
    #if MESH_PREVIEW
    public static let enabled = true
    #else
    public static let enabled = false
    #endif

    public static var available: Bool {
        enabled && NetworkCoreRuntimeFFI.featureSetVersion >= minimumFeatureSet
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
            routes: tunnel["routes"] as? [String] ?? [],
            routesVersion: (tunnel["routesVersion"] as? NSNumber)?.intValue ?? 0,
            dnsServers: dns?["servers"] as? [String] ?? [],
            matchDomains: dns?["matchDomains"] as? [String] ?? []
        )
    }

    /// The interface of a mesh-only tunnel: `base`'s addresses and MTU, these routes, and each DNS server as a host
    /// route so its queries reach the tunnel. The servers only answer their suffixes: without any, the device keeps its
    /// own DNS. Nil (`tunnel` unknown yet, as right after the start) routes nothing.
    public static func interface(_ tunnel: MeshTunnel?, base: TunnelInterface = .proxy) -> TunnelInterface {
        let servers = tunnel.map { $0.matchDomains.isEmpty ? [] : $0.dnsServers } ?? []
        var routes = tunnel?.routes ?? []
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
