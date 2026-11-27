import Foundation

/// Whether this build runs mesh networks: `enabled`, and an engine that answers `capabilities.mesh` (mesh-status.md).
/// The feature set does not tell: from kernel v0.3.0-rc.2 on (pinned: v0.3.0, 6790d867) it is 7 with or without the `mesh` feature. The app shows
/// the 组网 entry from it, whether the tunnel runs or not.
public enum MeshSupport {
    /// The control request that asks an engine what it can do.
    public static let capabilitiesRequest = Data(#"{"cmd":"capabilities"}"#.utf8)

    /// Whether the answer to `capabilitiesRequest` (`{"capabilities":{"mesh":bool}}`) says the engine runs mesh.
    /// No answer, a refusal or anything else is no.
    public static func runsMesh(capabilitiesAnswer data: Data?) -> Bool {
        meshCapability(capabilitiesAnswer: data) ?? false
    }

    /// What the answer says of mesh: `true` or `false` from `{"capabilities":{"mesh":bool}}`, `false` from the engine's
    /// own refusal (`{"ok":false,...}`: an engine that does not know the command), and `nil` for no answer or one that
    /// is not an answer at all, which may be asked again.
    static func meshCapability(capabilitiesAnswer data: Data?) -> Bool? {
        guard let data,
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        else {
            return nil
        }
        if let capabilities = object["capabilities"] as? [String: Any],
           let mesh = capabilities["mesh"] as? NSNumber {
            return mesh.boolValue
        }
        return object["ok"] as? Bool == false ? false : nil
    }

    private static let probeLock = NSLock()
    private static var probe: (() -> Data?)?
    /// The engine's answer, kept once it was one; a failed probe is not kept.
    private static var probed: Bool?
    private static var probing = false
    private static var retryAfter = Date.distantPast
    /// Counts the installs: a probe that was asked before the latest install answers into nothing (its answer is
    /// returned to its caller, never kept), so a late answer cannot overwrite what a newer install has cached.
    private static var generation = 0
    /// How long after a failed probe the next read asks again.
    static let retryDelay: TimeInterval = 5
    /// Posted (on the thread that got the answer) when the engine's answer to the capabilities probe arrives or
    /// changes, so what reads `available` can read it again: the first read answers no while the engine is asked in
    /// the background.
    public static let availabilityDidChange = Notification.Name("MeshSupport.availabilityDidChange")

    /// The install the current probe belongs to (for tests of a stale answer).
    static var probeGeneration: Int {
        probeLock.lock()
        defer { probeLock.unlock() }
        return generation
    }

    /// Installs how the app asks an engine for its capabilities: the answer to `capabilitiesRequest` from an engine
    /// built with the release gate's `allow_mesh` (the Bridge links no engine). Until then no engine runs mesh. With
    /// the gate open it asks at once, off the calling thread.
    public static func installCapabilitiesProbe(_ probe: @escaping () -> Data?) {
        probeLock.lock()
        self.probe = probe
        generation += 1
        probed = nil
        probing = false
        retryAfter = .distantPast
        probeLock.unlock()
        // A build with the gate closed never asks an engine (`available` short-circuits).
        if enabled {
            _ = engineRunsMesh
        }
    }

    /// Whether the engine this build links answers `capabilities.mesh`. Never waits for the engine: the first reads (and
    /// any read after a failed probe, five seconds on) answer no and ask in the background; a successful answer is kept
    /// for good, a failed one is asked again later.
    static var engineRunsMesh: Bool {
        probeLock.lock()
        defer { probeLock.unlock() }
        if let probed {
            return probed
        }
        if let probe, !probing, Date() >= retryAfter {
            probing = true
            let generation = self.generation
            DispatchQueue.global(qos: .utility).async { _ = askEngine(probe: probe, generation: generation) }
        }
        return false
    }

    /// Asks the installed probe now, on the calling thread, and keeps the answer if it is one. `nil` when it is not (or
    /// no probe is installed), and the next read asks again after `retryDelay`. The lock is not held while the engine
    /// answers.
    @discardableResult
    static func askEngine() -> Bool? {
        probeLock.lock()
        let probe = self.probe
        let generation = self.generation
        probeLock.unlock()
        return askEngine(probe: probe, generation: generation)
    }

    /// Asks `probe`, which was installed as `generation`; the answer is kept only while that is still the latest
    /// install.
    @discardableResult
    static func askEngine(probe: (() -> Data?)?, generation: Int) -> Bool? {
        let answer = probe.flatMap { meshCapability(capabilitiesAnswer: $0()) }
        probeLock.lock()
        guard generation == self.generation else {
            probeLock.unlock()
            return answer
        }
        probing = false
        var changed = false
        if let answer {
            changed = probed != answer
            probed = answer
        } else {
            retryAfter = Date().addingTimeInterval(retryDelay)
        }
        probeLock.unlock()
        if changed {
            NotificationCenter.default.post(name: availabilityDidChange, object: nil)
        }
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
        available(enabled: enabled)
    }

    /// [`available`] with the gate as a value, for tests; `available` passes the release gate.
    static func available(enabled: Bool) -> Bool {
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
