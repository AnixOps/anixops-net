import Foundation
import NetworkCoreBridge
import NetworkExtension
import os
import Security

/// Packet tunnel backed by the embedded Rust runtime.
///
/// It re-reads platform facts and the runtime handshake on every start and fails closed when either is
/// unavailable. It never starts another process and never logs the tunnel profile.
final class PacketTunnelProvider: NEPacketTunnelProvider {
    /// The node key for this profile. The app saves it under one fixed account and updates it in place, so that
    /// item is read first. The persistent reference is only a fallback: builds that deleted and re-added the item
    /// on every save left references that point at nothing, or at another item once the Keychain reused its row.
    private func nodeKey(for reference: Data) throws -> String {
        if let value = startSecrets?.nodeKey {
            return value
        }
        if let value = try NodeSecretStore.value(account: NodeSecretStore.tunnelProfileAccount) {
            return value
        }
        return try NodeSecretStore.password(for: reference)
    }

    private var session: EmbeddedRuntimeSession?
    /// What the macOS app handed over at start (`TunnelStartSecrets`); nil on iOS, where the Keychain is shared.
    private var startSecrets: TunnelStartSecrets?
    private var startedAt: Date?
    private var memoryPressure: DispatchSourceMemoryPressure?
    /// Recent events for the app's developer mode. Same rule as the system log: fixed reasons and codes only.
    private let events = TunnelDiagnosticLog()
    /// Hourly traffic by host and policy for the app's statistics page.
    private let trafficStats = TrafficStatsRecorder()
    /// Network changes in, script notifications out.
    private let scriptEvents = ScriptEventRelay()
    private var pathObservation: NSKeyValueObservation?
    private var lastPath: NWPath?

    #if os(macOS)
    /// Pins the engine's sockets to the physical interface (see `BoundInterfaceMonitor`).
    private let boundInterface = BoundInterfaceMonitor()
    #endif

    override func startTunnel(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        #if os(macOS)
        // A macOS packet tunnel's own sockets follow the routing table, so with the default route in the tunnel
        // the engine's connections would loop back into it. Pin them to the physical interface first.
        boundInterface.start(events: events) { [weak self] in
            self?.beginTunnel(options: options, completionHandler: completionHandler)
        }
        #else
        beginTunnel(options: options, completionHandler: completionHandler)
        #endif
    }

    private func beginTunnel(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        Self.log.notice("startTunnel")
        events.record("startTunnel")
        watchMemoryPressure()
        startSecrets = Self.resolveStartSecrets(options: options, events: events)
        let completionHandler: (Error?) -> Void = { [events] error in
            if let error = error {
                Self.log.error("startTunnel failed: \(error.localizedDescription, privacy: .public)")
                events.record("startTunnel failed: \(error.localizedDescription)")
            } else {
                Self.log.notice("startTunnel succeeded")
                events.record("startTunnel succeeded")
            }
            completionHandler(error)
        }
        let runtimeFact = NetworkCoreRuntimeFFI.embeddedRuntimeFact(
            reported: EmbeddedRuntimeSession.reportedHandshake(),
            initializationSucceeded: true
        )
        let status = IosPlatformFactsProbe.currentStatus(
            vpnConfiguration: .savedAndAuthorized,
            embeddedRuntime: runtimeFact
        )
        if !status.tunnel.available {
            completionHandler(PacketTunnelFailure(reason: status.tunnel.unavailableReason ?? "iOS tunnel is unavailable"))
            return
        }
        if !status.embeddedRuntime.available {
            completionHandler(
                PacketTunnelFailure(reason: status.embeddedRuntime.unavailableReason ?? "iOS embedded runtime is unavailable")
            )
            return
        }
        guard let tunnelProtocol = protocolConfiguration as? NETunnelProviderProtocol,
              let template = tunnelProtocol.providerConfiguration?[TunnelProfileKeys.runtimeConfig] as? Data
        else {
            completionHandler(PacketTunnelFailure(reason: "tunnel profile is missing"))
            return
        }
        guard let keyReference = tunnelProtocol.passwordReference else {
            completionHandler(PacketTunnelFailure(reason: "tunnel profile has no node key; import the node again"))
            return
        }
        var config: EmbeddedRuntimeConfig
        let interface: TunnelInterface
        do {
            let secret = NodeSecret.fromKeychainValue(try nodeKey(for: keyReference))
            config = try EmbeddedRuntimeConfig.decoded(from: template).withSecret(secret)
            interface = try (tunnelProtocol.providerConfiguration?[TunnelProfileKeys.tunnelInterface] as? Data)
                .map(TunnelInterface.decoded(from:)) ?? .proxy
        } catch let failure as NodeSecretStore.Failure {
            completionHandler(PacketTunnelFailure(reason: "node key is unavailable (keychain status \(failure.status))"))
            return
        } catch {
            completionHandler(PacketTunnelFailure(reason: "tunnel profile is malformed"))
            return
        }
        // The subscription's rules and groups go in whatever the mode, so the app can switch modes without a restart.
        let mode = ProxyMode.saved(in: tunnelProtocol.providerConfiguration)
        let dns = DnsSettings.fromProfileValue(tunnelProtocol.providerConfiguration?[TunnelProfileKeys.dns] as? Data)
        let redirects = TrafficRedirects.lines(
            fromProfileValue: tunnelProtocol.providerConfiguration?[TunnelProfileKeys.redirects] as? Data
        )
        let localRules = LocalRules.fromProfileValue(
            tunnelProtocol.providerConfiguration?[TunnelProfileKeys.localRules] as? Data
        )
        config = withSubscriptionRules(config, localRules: localRules).withMode(mode).withDns(dns).withRedirects(redirects)
        events.record("mode \(mode.rawValue)")
        if !redirects.isEmpty {
            events.record("redirects \(redirects.count)")
        }
        if let dns {
            events.record("dns \(dns.servers.count) servers, \(dns.rules.count) rules, \(dns.hosts.count) hosts")
        }
        events.record("profile loaded, mtu \(interface.mtu), \(interface.addresses.count) addresses, \(interface.includedRoutes.count) routes, \(interface.dnsServers.count) dns servers")

        let session: EmbeddedRuntimeSession
        do {
            let mitm = MitmProfileBundle.fromProfile(
                tunnelProtocol.providerConfiguration, chunks: startSecrets?.mitmBundleChunks
            )
            session = try makeSession(
                config,
                mitmPlugins: mitm.plugins,
                mitmScripts: mitm.scripts,
                mitmResources: mitm.resources
            )
            events.record("runtime initialized")
        } catch let failure as EmbeddedRuntimeSession.Failure {
            completionHandler(PacketTunnelFailure(reason: "embedded runtime rejected the tunnel profile (code \(failure.code))"))
            return
        } catch {
            completionHandler(PacketTunnelFailure(reason: "embedded runtime rejected the tunnel profile"))
            return
        }

        guard let settings = Self.networkSettings(interface, remoteAddress: IPAddressText.tunnelRemoteAddress(for: tunnelProtocol.serverAddress)) else {
            completionHandler(PacketTunnelFailure(reason: "tunnel profile has invalid interface addresses"))
            return
        }
        setTunnelNetworkSettings(settings) { [weak self] error in
            guard let self = self else {
                completionHandler(PacketTunnelFailure(reason: "tunnel provider was released"))
                return
            }
            if let error = error {
                completionHandler(error)
                return
            }
            self.events.record("network settings applied")
            do {
                try session.start()
                self.session = session
                self.startedAt = Date()
                self.trafficStats.start { [weak session] request in
                    session?.control(request)
                }
                let scheduled = self.scriptEvents.start { [weak session] request in
                    session?.control(request)
                }
                if scheduled > 0 {
                    self.events.record("\(scheduled) scheduled scripts")
                }
                self.watchNetworkPath()
                completionHandler(nil)
            } catch let failure as EmbeddedRuntimeSession.Failure {
                completionHandler(PacketTunnelFailure(reason: "embedded runtime could not start (code \(failure.code))"))
            } catch {
                completionHandler(PacketTunnelFailure(reason: "embedded runtime could not start"))
            }
        }
    }

    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        Self.log.notice("stopTunnel reason \(reason.rawValue, privacy: .public)")
        events.record("stopTunnel reason \(reason.rawValue)")
        trafficStats.stop()
        scriptEvents.stop()
        pathObservation?.invalidate()
        pathObservation = nil
        lastPath = nil
        session?.stop()
        session = nil
        startedAt = nil
        #if os(macOS)
        boundInterface.stop()
        #endif
        memoryPressure?.cancel()
        memoryPressure = nil
        completionHandler()
    }

    override func sleep(completionHandler: @escaping () -> Void) {
        events.record("device sleep")
        completionHandler()
    }

    override func wake() {
        events.record("device wake")
        scriptEvents.poll()
    }

    /// Tells the engine when the network under the tunnel changes (Wi-Fi to cellular, another Wi-Fi), for the
    /// plugins' `network-changed` scripts. The first path seen is the one the tunnel started on.
    private func watchNetworkPath() {
        pathObservation?.invalidate()
        pathObservation = observe(\.defaultPath, options: [.initial, .new]) { [weak self] provider, _ in
            guard let path = provider.defaultPath, path.status == .satisfied else {
                return
            }
            DispatchQueue.main.async {
                guard let self else {
                    return
                }
                defer { self.lastPath = path }
                guard let last = self.lastPath, !path.isEqual(to: last) else {
                    return
                }
                self.events.record("network changed")
                self.scriptEvents.networkChanged()
            }
        }
    }

    /// Only fixed reasons and codes are logged, never the profile or keys.
    private static let log = Logger(subsystem: "networkcore.packet-tunnel", category: "tunnel")

    /// The diagnostics request returns `TunnelDiagnostics`; any other message returns the runtime's sanitized
    /// status JSON.
    override func handleAppMessage(_ messageData: Data, completionHandler: ((Data?) -> Void)?) {
        switch TunnelAppMessage.decode(messageData) {
        case .runtimeStatus:
            completionHandler?(session?.statusJSON())
        case let .control(request):
            completionHandler?(session?.control(request))
        case .trafficStats:
            completionHandler?(trafficStats.snapshot())
        case .clearTrafficStats:
            completionHandler?(trafficStats.clear())
        case .diagnostics:
            let diagnostics = TunnelDiagnostics(
                runtimeStatus: session?.statusJSON().flatMap { String(data: $0, encoding: .utf8) },
                memoryFootprintBytes: Self.memoryFootprint(),
                peakMemoryFootprintBytes: Self.peakMemoryFootprint(),
                memoryAvailableBytes: Self.memoryAvailable(),
                uptimeSeconds: startedAt.map { Date().timeIntervalSince($0) },
                handshake: EmbeddedRuntimeSession.reportedHandshake(),
                events: events.snapshot(),
                engineMetrics: RuntimeControl.decode(
                    RuntimeControl.Metrics.self,
                    from: session?.control(RuntimeControl.metrics(anonymous: true).request)
                )?.text
            )
            completionHandler?(try? diagnostics.encoded())
        }
    }

    /// Adds the subscription's rules and groups from the node library in the Keychain, with the user's local rules in
    /// front; a node list without subscription rules gets the local rules and their preset. They are an extra like
    /// HTTPS decryption: without a readable library the selected node carries all traffic, and the reason goes to
    /// the diagnostics.
    private func withSubscriptionRules(_ config: EmbeddedRuntimeConfig, localRules: LocalRules) -> EmbeddedRuntimeConfig {
        do {
            guard let value = try startSecrets?.nodeLibrary ?? NodeSecretStore.value(account: NodeLibrary.keychainAccount),
                  let routing = try NodeLibrary.fromKeychainValue(value).routingForModes(localRules: localRules)
            else {
                events.record("subscription rules off: no node list")
                return config
            }
            events.record("subscription rules on, \(routing.outbounds.count) nodes, \(routing.rules.count) rules, \(routing.groups.count) groups")
            return config.withRouting(routing)
        } catch let failure as NodeSecretStore.Failure {
            events.record("subscription rules off: node list unavailable (keychain status \(failure.status))")
        } catch {
            events.record("subscription rules off: node list is malformed")
        }
        return config
    }

    /// Creates the runtime. When it refuses the subscription's rules, the selected node runs alone.
    private func makeSession(
        _ config: EmbeddedRuntimeConfig,
        mitmPlugins: [String],
        mitmScripts: [String: String],
        mitmResources: [String: Data]
    ) throws -> EmbeddedRuntimeSession {
        let resourceAssets = Self.resourceAssets(mitmResources)
        guard config.routing != nil else {
            return try makeRuntimeSession(
                config, mitmPlugins: mitmPlugins, mitmScripts: mitmScripts, resourceAssets: resourceAssets
            )
        }
        do {
            return try makeRuntimeSession(
                config, mitmPlugins: mitmPlugins, mitmScripts: mitmScripts, resourceAssets: resourceAssets
            )
        } catch let failure as EmbeddedRuntimeSession.Failure {
            events.record("subscription rules off: runtime refused them (code \(failure.code))")
        } catch {
            events.record("subscription rules off: runtime refused them")
        }
        return try makeRuntimeSession(
            config.withRouting(nil), mitmPlugins: mitmPlugins, mitmScripts: mitmScripts, resourceAssets: resourceAssets
        )
    }

    /// Creates the runtime, decrypting HTTPS for the enabled plugins when the device root is in the Keychain and
    /// running the scripts the user allowed. HTTPS decryption is an extra: when the scripts are refused the tunnel
    /// retries without them, when the root is missing or the runtime refuses the plugins it starts without
    /// decryption, and the reason goes to the diagnostics.
    private func makeRuntimeSession(
        _ config: EmbeddedRuntimeConfig,
        mitmPlugins: [String],
        mitmScripts: [String: String],
        resourceAssets: [String: String]
    ) throws -> EmbeddedRuntimeSession {
        if !mitmPlugins.isEmpty {
            do {
                if let authority = try mitmAuthority() {
                    if !mitmScripts.isEmpty {
                        do {
                            let session = try EmbeddedRuntimeSession(
                                config: config.withMitm(
                                    plugins: mitmPlugins,
                                    authority: authority,
                                    scriptAssets: try Self.scriptAssets(mitmScripts),
                                    resourceAssets: resourceAssets,
                                    scriptStorePath: try Self.scriptStoreURL().path
                                ).encoded(),
                                packetFlow: packetFlow
                            )
                            events.record("https decryption on, \(mitmPlugins.count) plugins, \(mitmScripts.count) scripts")
                            return session
                        } catch let failure as EmbeddedRuntimeSession.Failure {
                            events.record("scripts off: runtime refused them (code \(failure.code))")
                        } catch {
                            events.record("scripts off: could not write them")
                        }
                    }
                    let session = try EmbeddedRuntimeSession(
                        config: config.withMitm(
                            plugins: mitmPlugins, authority: authority, resourceAssets: resourceAssets
                        ).encoded(),
                        packetFlow: packetFlow
                    )
                    events.record("https decryption on, \(mitmPlugins.count) plugins")
                    return session
                }
                events.record("https decryption off: no root certificate")
            } catch let failure as MitmCertificateStore.Failure {
                events.record("https decryption off: root certificate unavailable (keychain status \(failure.status))")
            } catch let failure as EmbeddedRuntimeSession.Failure {
                events.record("https decryption off: runtime refused the plugins (code \(failure.code))")
            } catch {
                events.record("https decryption off: setup failed")
            }
        }
        return try EmbeddedRuntimeSession(config: config.withoutMitm().encoded(), packetFlow: packetFlow)
    }

    /// The MITM root: the one the macOS app handed over, else the shared Keychain's.
    private func mitmAuthority() throws -> MitmCertificateAuthority? {
        if let stored = startSecrets?.mitmAuthority {
            return try MitmCertificateStore.authority(fromStoredValue: stored)
        }
        return try MitmCertificateStore.load()
    }

    /// On macOS the tunnel may run as a System Extension, a root process that cannot read the user's Keychain, so
    /// the app passes the Keychain values in the start options. They are kept in the extension's own container for
    /// starts without options (on demand, after sleep). iOS shares the Keychain and never passes them.
    private static func resolveStartSecrets(options: [String: NSObject]?, events: TunnelDiagnosticLog) -> TunnelStartSecrets? {
        #if os(macOS)
        let url = try? FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent(TunnelStartSecrets.fileName)
        if let handed = TunnelStartSecrets(options: options) {
            if let url {
                do {
                    try handed.save(to: url)
                } catch {
                    events.record("start secrets not kept: could not write them")
                }
            }
            events.record("start secrets from the app")
            return handed
        }
        if let url, let kept = TunnelStartSecrets.load(from: url) {
            events.record("start secrets from the last start")
            return kept
        }
        return nil
        #else
        return nil
        #endif
    }

    /// Writes the allowed scripts where the runtime reads them (it pins each file by SHA-256 at start).
    private static func scriptAssets(_ sources: [String: String]) throws -> [String: String] {
        let caches = try FileManager.default.url(for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        return try MitmScriptLibrary.writeAssets(sources, to: caches.appendingPathComponent("mitm-scripts", isDirectory: true))
    }

    /// Writes the downloaded `[Map Local]` bodies where the runtime reads them. Without them those lines answer
    /// nothing, so a failure only costs them.
    private static func resourceAssets(_ data: [String: Data]) -> [String: String] {
        guard !data.isEmpty,
              let caches = try? FileManager.default.url(
                  for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true
              )
        else {
            return [:]
        }
        return (try? MitmResourceLibrary.writeAssets(
            data, to: caches.appendingPathComponent("mitm-resources", isDirectory: true)
        )) ?? [:]
    }

    /// `$persistentStore`, kept in the extension's own container across connections.
    private static func scriptStoreURL() throws -> URL {
        try FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("mitm-script-store.json")
    }

    /// Records system memory pressure, the usual warning before iOS kills an extension over its memory limit.
    private func watchMemoryPressure() {
        guard memoryPressure == nil else {
            return
        }
        let source = DispatchSource.makeMemoryPressureSource(eventMask: [.warning, .critical], queue: .global())
        source.setEventHandler { [weak self, weak source] in
            guard let self = self, let source = source else {
                return
            }
            let level = source.data.contains(.critical) ? "critical" : "warning"
            let footprint = Self.memoryFootprint().map(DeveloperReport.megabytes) ?? "unknown"
            let available = Self.memoryAvailable().map(DeveloperReport.megabytes) ?? "unknown"
            Self.log.notice("memory pressure \(level, privacy: .public)")
            self.events.record("memory pressure \(level), footprint \(footprint), headroom \(available)")
        }
        source.resume()
        memoryPressure = source
    }

    /// The extension's physical memory footprint, the figure iOS compares against the extension memory limit.
    static func memoryFootprint() -> UInt64? {
        vmInfo()?.phys_footprint
    }

    /// The highest footprint since the extension started: a spike past the limit ends the session even when the
    /// current footprint looks small.
    static func peakMemoryFootprint() -> UInt64? {
        // The ledger field is a signed int64_t, unlike phys_footprint.
        guard let peak = vmInfo()?.ledger_phys_footprint_peak, peak >= 0 else {
            return nil
        }
        return UInt64(peak)
    }

    /// How far the extension is from its memory limit (iOS only; nil elsewhere).
    static func memoryAvailable() -> UInt64? {
        #if os(iOS) && !targetEnvironment(macCatalyst)
            return UInt64(os_proc_available_memory())
        #else
            return nil
        #endif
    }

    private static func vmInfo() -> task_vm_info_data_t? {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? info : nil
    }

    /// Interface settings from the saved profile. The provider's own sockets (to the proxy server or the
    /// WireGuard endpoint) are never routed into the tunnel by the system. Returns nil for an address or route
    /// that does not parse.
    static func networkSettings(_ interface: TunnelInterface, remoteAddress: String) -> NEPacketTunnelNetworkSettings? {
        let addresses = interface.addresses.map(CIDR.parse)
        let routes = interface.includedRoutes.map(CIDR.parse)
        guard addresses.allSatisfy({ $0 != nil }), routes.allSatisfy({ $0 != nil }) else {
            return nil
        }
        let v4Addresses = addresses.compactMap { $0 }.filter { !$0.isIPv6 }
        let v6Addresses = addresses.compactMap { $0 }.filter { $0.isIPv6 }
        let v4Routes = routes.compactMap { $0 }.filter { !$0.isIPv6 }
        let v6Routes = routes.compactMap { $0 }.filter { $0.isIPv6 }

        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: remoteAddress)
        if !v4Addresses.isEmpty {
            let ipv4 = NEIPv4Settings(
                addresses: v4Addresses.map(\.address),
                subnetMasks: v4Addresses.compactMap(\.ipv4SubnetMask)
            )
            ipv4.includedRoutes = v4Routes.map { route in
                route.isDefaultRoute
                    ? NEIPv4Route.default()
                    : NEIPv4Route(destinationAddress: route.address, subnetMask: route.ipv4SubnetMask ?? "255.255.255.255")
            }
            settings.ipv4Settings = ipv4
        }
        if !v6Addresses.isEmpty {
            let ipv6 = NEIPv6Settings(
                addresses: v6Addresses.map(\.address),
                networkPrefixLengths: v6Addresses.map { NSNumber(value: $0.prefixLength) }
            )
            ipv6.includedRoutes = v6Routes.map { route in
                route.isDefaultRoute
                    ? NEIPv6Route.default()
                    : NEIPv6Route(destinationAddress: route.address, networkPrefixLength: NSNumber(value: route.prefixLength))
            }
            settings.ipv6Settings = ipv6
        }
        if !interface.dnsServers.isEmpty {
            let dns = NEDNSSettings(servers: interface.dnsServers)
            dns.matchDomains = [""]
            settings.dnsSettings = dns
        }
        settings.mtu = NSNumber(value: interface.mtu)
        return settings
    }
}

/// Error returned to the system when the tunnel refuses to start. It carries only a fixed reason string.
///
/// The reason sits in `errorUserInfo` so it survives the trip to the app, which reads it with
/// `fetchLastDisconnectError`.
struct PacketTunnelFailure: LocalizedError, CustomNSError {
    static let errorDomain = "networkcore.packet-tunnel"

    let reason: String

    var errorDescription: String? {
        reason
    }

    var errorCode: Int {
        1
    }

    var errorUserInfo: [String: Any] {
        [NSLocalizedDescriptionKey: reason]
    }
}
