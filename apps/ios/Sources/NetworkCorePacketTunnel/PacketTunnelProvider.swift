import Foundation
import NetworkCoreBridge
import NetworkExtension
import UserNotifications
import os
import Security

/// Packet tunnel backed by the embedded Rust runtime.
///
/// It re-reads platform facts and the runtime handshake on every start and fails closed when either is
/// unavailable. It never starts another process and never logs the tunnel profile.
final class PacketTunnelProvider: NEPacketTunnelProvider {
    /// The node key for this profile. A profile saved with a key version reads that version's key and no other: a
    /// key saved for a later profile that never made it into the system settings must not pair with this one. A
    /// profile from before key versions reads the fixed account the app updated in place, then the persistent
    /// reference: builds that deleted and re-added the item on every save left references that point at nothing,
    /// or at another item once the Keychain reused its row.
    private func nodeKey(for reference: Data, version: String?) throws -> String {
        if let value = startSecrets?.nodeKey(forVersion: version) {
            return value
        }
        if let version {
            guard let value = try NodeSecretStore.value(account: NodeSecretStore.versionedAccount(version)) else {
                throw NodeSecretStore.Failure(status: errSecItemNotFound)
            }
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
    /// What the running config was built from before the subscription and local rules went in, and the config the
    /// engine runs, so new local rules can be swapped in place (`reloadLocalRules`).
    private var reloadBase: ReloadBase?
    private var runningConfig: EmbeddedRuntimeConfig?

    private struct ReloadBase {
        let config: EmbeddedRuntimeConfig
        let mode: ProxyMode
        let dns: DnsSettings?
        let redirects: [String]
        let blockQuic: Bool
        /// The license fields of each ANX outbound by its node's origin (`OutboundLicenses`), or nil without a
        /// license state.
        let anxLicense: OutboundLicenses?
        /// The node library (its Keychain value) this start read, or why it could not. New local rules are built on
        /// this copy, not on the Keychain as it is later: the app saves a new policy group pick there while the
        /// engine switches it in place, and a background subscription update saves new nodes without a restart;
        /// building on those would differ from the running config in more than the rule lines and turn every
        /// later rule edit into a restart.
        let nodeLibrary: Result<String?, Error>
    }
    private var memoryPressure: DispatchSourceMemoryPressure?
    /// Recent events for the app's developer mode. Same rule as the system log: fixed reasons and codes only.
    private let events = TunnelDiagnosticLog()
    /// Whether the running engine decrypts HTTPS, set by `makeRuntimeSession` (`TunnelAppMessage.httpsDecryption`).
    private var httpsDecryption = HttpsDecryptionState(status: .unused)
    /// Whether the running engine routes by the rules, set by `makeSession` (`TunnelAppMessage.routing`).
    private var routingState = RoutingState(status: .unused)
    /// The rule sets the last routing section built left out (`NodeLibrary.RoutingBuild.droppedRuleSets`).
    private var droppedRuleSets: [String] = []
    /// Hourly traffic by host and policy for the app's statistics page.
    private let trafficStats = TrafficStatsRecorder()
    /// Network changes in, script notifications out.
    private let scriptEvents = ScriptEventRelay()
    /// Which notifications to post (`NotificationSettings`), from the profile and then from the app's messages.
    private var notificationSettings = NotificationSettings()
    private var pathObservation: NSKeyValueObservation?
    private var lastPath: NWPath?
    /// Rebuilds the engine's connections when the physical network changes (`NetworkRecovery`).
    private let physicalNetwork = PhysicalNetworkMonitor()
    /// When the device went to sleep, to decide on wake whether open connections are still worth keeping.
    private var sleptAt: Date?
    /// Set when the tunnel is stopped while a start waits to reconnect (`AutoReconnect`).
    private let startGate = StartGate()
    /// The start whose stable connection resets the reconnect count; cleared when the tunnel stops.
    private let stableToken = Locked<UUID?>(nil)

    #if os(macOS)
    /// Pins the engine's sockets to the physical interface (see `BoundInterfaceMonitor`).
    private let boundInterface = BoundInterfaceMonitor()
    /// Set when this tunnel turned IPv4 forwarding on for the gateway mode, so stopping turns it back off.
    private var turnedForwardingOn = false
    #endif

    override func startTunnel(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        let user = AutoReconnect.isUserStart(options)
        var state = AutoReconnect.load()
        let decision = AutoReconnect.begin(&state, user: user, unit: Double.random(in: 0..<1))
        AutoReconnect.save(state)
        switch decision {
        case .refuse:
            refuseStart(options: options, completionHandler: completionHandler)
        case let .attempt(attempt, delay):
            events.record("start \(attempt) of at most \(AutoReconnect.maxAttempts)\(user ? ", asked by the user" : "")")
            let counted = countedCompletion(completionHandler)
            guard delay > 0 else {
                launchTunnel(options: options, completionHandler: counted)
                return
            }
            // The system starts a failed tunnel again at once; each start after the first waits longer.
            events.record("reconnecting in \(Int(delay.rounded())) s")
            startGate.arm()
            DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + delay) { [weak self] in
                guard let self else {
                    completionHandler(PacketTunnelFailure(reason: "tunnel provider was released"))
                    return
                }
                guard !self.startGate.cancelled else {
                    // Stopped while waiting (the user disconnected): not a failure of the start.
                    self.events.record("reconnect cancelled while waiting")
                    completionHandler(PacketTunnelFailure(reason: "start cancelled"))
                    return
                }
                self.launchTunnel(options: options, completionHandler: counted)
            }
        }
    }

    /// Counts the end of a start (`AutoReconnect`): a failure makes the next start wait longer, and the last one
    /// allowed, or one no retry can fix, turns reconnecting off before the system hears of it, so it does not start
    /// the tunnel again. A start that worked counts from one again once the tunnel stayed up `stableAfter`.
    private func countedCompletion(_ completionHandler: @escaping (Error?) -> Void) -> (Error?) -> Void {
        { [weak self] error in
            guard let error else {
                self?.watchStable()
                completionHandler(nil)
                return
            }
            var state = AutoReconnect.load()
            let failure = error as? PacketTunnelFailure
            let outcome = AutoReconnect.failed(&state, fatal: failure?.fatal == true)
            AutoReconnect.save(state)
            guard case let .stopped(fatal) = outcome, let self else {
                completionHandler(error)
                return
            }
            let reason = failure?.reason ?? error.localizedDescription
            self.events.record(fatal
                ? "auto-reconnect stopped: cannot be fixed by retrying"
                : "auto-reconnect stopped after \(AutoReconnect.maxAttempts) failed starts")
            self.postReconnectStopped(fatal: fatal)
            self.turnOffOnDemand { _ in
                completionHandler(PacketTunnelFailure(
                    reason: AutoReconnect.stoppedReason(reason, fatal: fatal), fatal: fatal
                ))
            }
        }
    }

    /// A start while reconnecting is stopped. With On Demand already off it cannot be the system's own start: the user
    /// switched the VPN on in Settings or Control Center, so it goes ahead as theirs. Otherwise it fails, after
    /// turning On Demand off; when that is not possible here (the macOS System Extension), it fails only after
    /// `refusalHold`, so the system's restarts stay slow until the app turns it off.
    private func refuseStart(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        events.record("start refused: auto-reconnect stopped")
        ownManager { [weak self] manager in
            guard let self else {
                completionHandler(PacketTunnelFailure(reason: "tunnel provider was released"))
                return
            }
            if let manager, !manager.isOnDemandEnabled {
                var state = AutoReconnect.load()
                _ = AutoReconnect.begin(&state, user: true, unit: 0)
                AutoReconnect.save(state)
                self.events.record("start 1 of at most \(AutoReconnect.maxAttempts), from the system settings")
                self.launchTunnel(options: options, completionHandler: self.countedCompletion(completionHandler))
                return
            }
            let refused = PacketTunnelFailure(
                reason: AutoReconnect.stoppedReason("auto-reconnect is stopped; connect from the app", fatal: false)
            )
            self.turnOffOnDemand(manager: manager) { turnedOff in
                guard !turnedOff else {
                    completionHandler(refused)
                    return
                }
                DispatchQueue.global().asyncAfter(deadline: .now() + AutoReconnect.refusalHold) {
                    completionHandler(refused)
                }
            }
        }
    }

    /// Resets the count once the tunnel that just started has stayed up `stableAfter`.
    private func watchStable() {
        let token = UUID()
        stableToken.set(token)
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + AutoReconnect.stableAfter) { [weak self] in
            guard let self, self.stableToken.get() == token else {
                return
            }
            var state = AutoReconnect.load()
            AutoReconnect.stable(&state)
            AutoReconnect.save(state)
            self.events.record("connection stable: reconnect count reset")
        }
    }

    /// This tunnel's own VPN configuration, or nil when the extension cannot read it (the macOS System Extension, a
    /// root process, has none). Answers within five seconds whatever the system does.
    private func ownManager(_ completion: @escaping (NETunnelProviderManager?) -> Void) {
        let once = Once(completion)
        DispatchQueue.global().asyncAfter(deadline: .now() + 5) {
            once.call(nil)
        }
        let bundle = Bundle.main.bundleIdentifier
        NETunnelProviderManager.loadAllFromPreferences { managers, _ in
            let own = managers?.first {
                ($0.protocolConfiguration as? NETunnelProviderProtocol)?.providerBundleIdentifier == bundle
            }
            once.call(own ?? (managers?.count == 1 ? managers?.first : nil))
        }
    }

    /// Turns On Demand off so the system stops starting the tunnel; the app turns it on again with the user's next
    /// connect. Answers whether it is off now.
    private func turnOffOnDemand(manager known: NETunnelProviderManager? = nil, _ completion: @escaping (Bool) -> Void) {
        let save: (NETunnelProviderManager?) -> Void = { [events] manager in
            guard let manager else {
                completion(false)
                return
            }
            guard manager.isOnDemandEnabled else {
                completion(true)
                return
            }
            let once = Once(completion)
            DispatchQueue.global().asyncAfter(deadline: .now() + 5) {
                once.call(false)
            }
            manager.isOnDemandEnabled = false
            manager.saveToPreferences { error in
                events.record(error == nil ? "auto-reconnect turned off" : "auto-reconnect could not be turned off")
                once.call(error == nil)
            }
        }
        if let known {
            save(known)
        } else {
            ownManager(save)
        }
    }

    /// Tells the user the tunnel stopped reconnecting by itself (iOS only, as `postStopNotification`).
    private func postReconnectStopped(fatal: Bool) {
        #if os(iOS)
        let settings = NotificationSettings.fromProfileValue(
            (protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration?[TunnelProfileKeys.notifications] as? Data
        )
        guard settings.connection else {
            return
        }
        let content = UNMutableNotificationContent()
        content.title = String(localized: "AnixOps Net 未能连接")
        content.body = fatal
            ? String(localized: "这个问题重试也无法解决，已停止自动重连。打开 App 查看原因。")
            : String(localized: "连续 5 次没有连上，已停止自动重连。打开 App 重试或换一个节点。")
        content.threadIdentifier = "connection"
        UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: "connection-stopped", content: content, trigger: nil)
        )
        #endif
    }

    /// The start itself, after any wait.
    private func launchTunnel(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
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
        // macOS: the copy kept from an earlier start, when the app passed nothing this time (On Demand, login,
        // System Settings); nil when it passed fresh values, and always nil on iOS, which keeps no copy.
        let keptCopy = TunnelStartSecrets(options: options) == nil ? startSecrets : nil
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
            completionHandler(PacketTunnelFailure(
                reason: status.tunnel.unavailableReason ?? "iOS tunnel is unavailable", fatal: true
            ))
            return
        }
        if !status.embeddedRuntime.available {
            completionHandler(
                PacketTunnelFailure(
                    reason: status.embeddedRuntime.unavailableReason ?? "iOS embedded runtime is unavailable", fatal: true
                )
            )
            return
        }
        guard let tunnelProtocol = protocolConfiguration as? NETunnelProviderProtocol,
              let template = tunnelProtocol.providerConfiguration?[TunnelProfileKeys.runtimeConfig] as? Data
        else {
            completionHandler(PacketTunnelFailure(reason: "tunnel profile is missing", fatal: true))
            return
        }
        guard let keyReference = tunnelProtocol.passwordReference else {
            completionHandler(PacketTunnelFailure(reason: "tunnel profile has no node key; import the node again", fatal: true))
            return
        }
        var config: EmbeddedRuntimeConfig
        let interface: TunnelInterface
        do {
            let version = tunnelProtocol.providerConfiguration?[TunnelProfileKeys.nodeKeyVersion] as? String
            let secret = NodeSecret.fromKeychainValue(try nodeKey(for: keyReference, version: version))
            config = try EmbeddedRuntimeConfig.decoded(from: template).withSecret(secret)
            interface = try (tunnelProtocol.providerConfiguration?[TunnelProfileKeys.tunnelInterface] as? Data)
                .map(TunnelInterface.decoded(from:)) ?? .proxy
        } catch let failure as NodeSecretStore.Failure {
            // On macOS a copy handed over for an older profile says so, so the app knows to start the tunnel once
            // with options rather than report a Keychain problem.
            let version = tunnelProtocol.providerConfiguration?[TunnelProfileKeys.nodeKeyVersion] as? String
            completionHandler(PacketTunnelFailure(reason: TunnelStartSecrets.nodeKeyFailureReason(
                status: failure.status, handedOver: startSecrets, version: version
            )))
            return
        } catch {
            completionHandler(PacketTunnelFailure(reason: "tunnel profile is malformed", fatal: true))
            return
        }
        // The subscription's rules and groups go in whatever the mode, so the app can switch modes without a restart.
        let mode = ProxyMode.saved(in: tunnelProtocol.providerConfiguration)
        // Rules an earlier version saved over the engine's domain limit are left out (`withinDomainLimit()`): the
        // engine would refuse the whole config.
        let dnsLimit = DnsSettings.fromProfileValue(tunnelProtocol.providerConfiguration?[TunnelProfileKeys.dns] as? Data)?
            .withinDomainLimit()
        let dns = dnsLimit?.settings
        if let dropped = dnsLimit?.droppedRules, dropped > 0 {
            events.record("dns: \(dropped) rules skipped (over \(DnsSettings.maxRuleDomains) domains)")
        }
        // Lines an earlier version saved without checking them all are skipped (`TrafficRedirects.validLines`): the
        // engine would refuse the whole config for one of them.
        let savedRedirects = TrafficRedirects.lines(
            fromProfileValue: tunnelProtocol.providerConfiguration?[TunnelProfileKeys.redirects] as? Data
        )
        let redirects = TrafficRedirects.validLines(savedRedirects)
        if redirects.count < savedRedirects.count {
            events.record("redirects: \(savedRedirects.count - redirects.count) skipped (invalid)")
        }
        let localRules = LocalRules.fromProfileValue(
            tunnelProtocol.providerConfiguration?[TunnelProfileKeys.localRules] as? Data
        )
        applyNotificationSettings(NotificationSettings.fromProfileValue(
            tunnelProtocol.providerConfiguration?[TunnelProfileKeys.notifications] as? Data
        ))
        let blockQuic = tunnelProtocol.providerConfiguration?[TunnelProfileKeys.blockQuic] as? Bool == true
        let takeover = TrafficTakeover.saved(in: tunnelProtocol.providerConfiguration, macOS: Self.isMac)
        let gateway = GatewayMode.saved(in: tunnelProtocol.providerConfiguration, takeover: takeover, macOS: Self.isMac)
        let proxyExceptions = ProxyExceptions.fromProfileValue(
            tunnelProtocol.providerConfiguration?[TunnelProfileKeys.proxyExceptions] as? Data
        )
        let nodeLibrary = Result<String?, Error> {
            try startSecrets?.nodeLibrary ?? NodeSecretStore.value(account: NodeLibrary.keychainAccount)
        }
        let base = ReloadBase(
            config: config, mode: mode, dns: dns, redirects: redirects, blockQuic: blockQuic,
            anxLicense: anxLicenseFields(library: nodeLibrary), nodeLibrary: nodeLibrary
        )
        config = Self.assemble(
            base, routing: withSubscriptionRules(base.config, localRules: localRules, dns: dns, library: base.nodeLibrary)
        )
        events.record("mode \(mode.rawValue)")
        if !redirects.isEmpty {
            events.record("redirects \(redirects.count)")
        }
        if blockQuic {
            events.record("block quic")
        }
        if let dns {
            events.record("dns \(dns.servers.count) servers, \(dns.rules.count) rules, \(dns.hosts.count) hosts")
        }
        events.record("profile loaded, mtu \(interface.mtu), \(interface.addresses.count) addresses, \(interface.includedRoutes.count) routes, \(interface.dnsServers.count) dns servers")

        // A kept copy without the profile's HTTPS decryption payload would start with decryption silently off; fail
        // with the stale-copy reason instead, so the app starts the tunnel once more with options.
        if let reason = TunnelStartSecrets.mitmBundleFailureReason(
            profileValue: tunnelProtocol.providerConfiguration?[TunnelProfileKeys.mitmBundle] as? Data,
            keptCopy: keptCopy
        ) {
            events.record("https decryption payload missing from the kept start secrets")
            completionHandler(PacketTunnelFailure(reason: reason))
            return
        }
        // The same for a kept copy with an older MITM root than the profile names: the root was generated or
        // replaced while the tunnel was not running, and certificates from the old one must not be issued.
        let decrypts = MitmProfileBundle.referencesPayload(tunnelProtocol.providerConfiguration)
            || tunnelProtocol.providerConfiguration?[TunnelProfileKeys.mitmPlugins] != nil
        if let reason = TunnelStartSecrets.mitmRootFailureReason(
            profileFingerprint: tunnelProtocol.providerConfiguration?[TunnelProfileKeys.mitmRootFingerprint] as? String,
            decrypts: decrypts,
            keptCopy: keptCopy
        ) {
            events.record("root certificate in the kept start secrets is older than the profile")
            completionHandler(PacketTunnelFailure(reason: reason))
            return
        }

        let session: EmbeddedRuntimeSession
        do {
            let mitm = MitmProfileBundle.fromProfile(
                tunnelProtocol.providerConfiguration, chunks: startSecrets?.mitmBundleChunks
            )
            // The config the engine actually runs: without the rules when it could not take them, so a later
            // local-rules change restarts rather than swapping lines into an engine that has no rules.
            let started = try makeSession(
                config,
                mitmPlugins: mitm.plugins,
                mitmScripts: mitm.scripts,
                mitmResources: mitm.resources
            )
            session = started.0
            config = started.1
            events.record("runtime initialized")
        } catch let tooLarge as EngineBudget.TooLarge {
            completionHandler(PacketTunnelFailure(
                reason: "tunnel config is too large for the engine (\(tooLarge.bytes) of \(tooLarge.limit) bytes); remove DNS rules or redirects",
                fatal: true
            ))
            return
        } catch let failure as EmbeddedRuntimeSession.Failure {
            completionHandler(PacketTunnelFailure(
                reason: "embedded runtime rejected the tunnel profile (code \(failure.code))", fatal: true
            ))
            return
        } catch {
            completionHandler(PacketTunnelFailure(reason: "embedded runtime rejected the tunnel profile", fatal: true))
            return
        }

        let proxy = startSystemProxy(session, takeover: takeover, exceptions: proxyExceptions)
        // Without the listener (a whole-device WireGuard tunnel, or the port could not be bound) the system proxy
        // mode falls back to routing through the tunnel, so the connection still goes where the user expects.
        let routesTraffic = takeover.routesTraffic || proxy == nil
        if !takeover.routesTraffic, proxy == nil {
            events.record("system proxy mode unavailable: routing through the tunnel")
        }
        guard let settings = Self.networkSettings(
            interface,
            remoteAddress: IPAddressText.tunnelRemoteAddress(for: tunnelProtocol.serverAddress),
            routesTraffic: routesTraffic,
            proxy: proxy
        ) else {
            completionHandler(PacketTunnelFailure(reason: "tunnel profile has invalid interface addresses", fatal: true))
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
                self.reloadBase = base
                self.runningConfig = config
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
                self.watchPhysicalNetwork(session)
                self.setUpCaptureStorage(session)
                if gateway, routesTraffic {
                    self.enableGatewayForwarding()
                }
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
        startGate.cancel()
        stableToken.set(nil)
        postStopNotification(reason)
        trafficStats.stop()
        scriptEvents.stop()
        pathObservation?.invalidate()
        pathObservation = nil
        lastPath = nil
        physicalNetwork.stop()
        if let session {
            saveCaptureOnStop(session)
        }
        session?.stop()
        session = nil
        startedAt = nil
        reloadBase = nil
        runningConfig = nil
        #if os(macOS)
        boundInterface.stop()
        if turnedForwardingOn {
            turnedForwardingOn = false
            let restored = IPv4Forwarding.set(false)
            events.record("gateway: ip forwarding \(restored ? "off again" : "could not be turned off")")
        }
        #endif
        memoryPressure?.cancel()
        memoryPressure = nil
        completionHandler()
    }

    override func sleep(completionHandler: @escaping () -> Void) {
        events.record("device sleep")
        sleptAt = Date()
        // Fewer timer wakes while the screen is off (iOS heat and battery).
        trafficStats.setSleeping(true)
        scriptEvents.setSleeping(true)
        completionHandler()
    }

    /// Connections from before the sleep may point at a path that is gone (the NAT mapping expired, the device
    /// moved): pooled sessions are always dropped, open connections too after a long sleep.
    override func wake() {
        let slept = sleptAt.map { Date().timeIntervalSince($0) } ?? 0
        sleptAt = nil
        events.record("device wake after \(Int(slept)) s")
        trafficStats.setSleeping(false)
        scriptEvents.setSleeping(false)
        resetEngine(NetworkRecovery.afterWake(sleptFor: slept), reason: "wake")
        scriptEvents.poll()
    }

    /// Resets the engine's connections whenever the network under the tunnel changes (another Wi-Fi, Wi-Fi to
    /// cellular, back online), so the next flows dial on the new path instead of stalling on the old one.
    private func watchPhysicalNetwork(_ session: EmbeddedRuntimeSession) {
        var recovery = NetworkRecovery()
        var reported: NetworkFacts??
        physicalNetwork.start { [weak self, weak session] key, facts in
            guard let self, let session else {
                return
            }
            // The first path too, so `subnet` groups know the network the tunnel started on; before the reset, so
            // the rebuilt connections pick by the new one.
            if reported != .some(facts), facts != nil || reported != nil {
                reported = .some(facts)
                self.reportNetwork(facts, session: session)
            }
            guard let request = recovery.pathChanged(to: key) else {
                return
            }
            self.resetEngine(request, reason: "network change", session: session)
        }
    }

    /// Tells the engine the network under the tunnel for its `subnet` policy groups. Only the type is logged: the
    /// router and addresses identify the place.
    private func reportNetwork(_ facts: NetworkFacts?, session: EmbeddedRuntimeSession) {
        let answer = RuntimeControl.decode(
            RuntimeControl.NetworkSet.self, from: session.control(RuntimeControl.setNetwork(facts).request)
        )
        if let answer, answer.changed {
            events.record("network for subnet groups: \(facts?.kind ?? "unknown")")
        }
    }

    private func resetEngine(_ request: RuntimeControl, reason: String, session: EmbeddedRuntimeSession? = nil) {
        guard let session = session ?? self.session else {
            return
        }
        let began = Date()
        let answer = RuntimeControl.decode(RuntimeControl.NetworkReset.self, from: session.control(request.request))
        let millis = Int(Date().timeIntervalSince(began) * 1000)
        if let answer {
            events.record("\(reason): reset \(answer.reset) sessions, closed \(answer.closed) connections in \(millis) ms")
        } else {
            events.record("\(reason): engine has no network reset")
        }
    }

    /// Lets the engine seal saved capture sessions: a folder in this extension's container and a key that never
    /// leaves this device (`CaptureSessionStorage.key()`: the Keychain on iOS, a root-only file on macOS, where the
    /// System Extension cannot use the data protection keychain). Without either the tunnel saves nothing on stop; the
    /// app's own copies of the live capture (`copy_capture`) need no storage here. Only fixed reasons and codes are
    /// logged, never the key.
    private func setUpCaptureStorage(_ session: EmbeddedRuntimeSession) {
        do {
            if session.control(try CaptureSessionStorage.setupRequest().request) == nil {
                events.record("capture storage refused")
            }
        } catch let failure as NodeSecretStore.Failure {
            events.record("capture storage unavailable (keychain status \(failure.status))")
        } catch let failure as CaptureSessionKeyFile.Failure {
            switch failure {
            case .unreadable:
                events.record("capture storage unavailable: key file unreadable")
            case let .malformed(length):
                events.record("capture storage unavailable: key file holds \(length) bytes")
            case let .notWritten(code):
                events.record("capture storage unavailable: key file not written (errno \(code))")
            }
        } catch {
            events.record("capture storage unavailable")
        }
    }

    /// Capture lives in the engine's memory and ends with the tunnel. When it holds something, it is sealed into this
    /// run's session first (`save_capture` with `run`, 7 days), so disconnecting stops capture without losing what it
    /// caught. The app keeps copies of the run under the same session id while connected (`copyLiveCapture`); this
    /// seal is the complete one, and the app copies it at the next connect.
    private func saveCaptureOnStop(_ session: EmbeddedRuntimeSession) {
        guard let status = RuntimeControl.decode(PacketCapture.Status.self, from: session.control(RuntimeControl.captureStatus.request)),
              status.connections > 0
        else {
            return
        }
        let answer = RuntimeControl.decode(
            PacketCapture.SavedAnswer.self,
            from: session.control(RuntimeControl.saveCapture(name: String(localized: "断开时自动保存"), run: true).request)
        )
        events.record(answer != nil ? "capture saved on stop, \(status.connections) connections" : "capture not saved on stop")
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

    private func applyNotificationSettings(_ settings: NotificationSettings) {
        notificationSettings = settings
        scriptEvents.setPostsNotifications(settings.scripts)
        events.record("notifications scripts \(settings.scripts) connection \(settings.connection)")
    }

    /// Tells the user the tunnel stopped on its own (`NotificationSettings.notifiesStop`). Only on iOS: the macOS
    /// System Extension runs as root and cannot post, so the macOS app posts this from the stop error instead.
    private func postStopNotification(_ reason: NEProviderStopReason) {
        #if os(iOS)
        guard notificationSettings.connection, startedAt != nil,
              NotificationSettings.notifiesStop(reasonCode: reason.rawValue)
        else {
            return
        }
        let content = UNMutableNotificationContent()
        content.title = String(localized: "AnixOps Net 已断开")
        content.body = String(localized: "节点无响应、被干扰，或网络已切换。请打开 App 重试或更换节点。")
        content.threadIdentifier = "connection"
        UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: "connection-stopped", content: content, trigger: nil)
        )
        #endif
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
        case .httpsDecryption:
            completionHandler?(session == nil ? nil : httpsDecryption.encoded())
        case .routing:
            completionHandler?(session == nil ? nil : routingState.encoded())
        case let .notificationSettings(data):
            applyNotificationSettings(NotificationSettings.fromProfileValue(data))
            completionHandler?(Data("ok".utf8))
        case let .reloadLocalRules(value):
            completionHandler?(reloadLocalRules(value))
        case let .startSecrets(value):
            completionHandler?(keepStartSecrets(value))
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

    /// The engine config of a start: the subscription and local rules (`withRouting` already applied by
    /// `withSubscriptionRules`), then the mode, DNS, destination rewrites, QUIC blocking and the rule set cache.
    private static func assemble(_ base: ReloadBase, routing config: EmbeddedRuntimeConfig) -> EmbeddedRuntimeConfig {
        let assembled = config.withMode(base.mode).withDns(base.dns).withRedirects(base.redirects)
            .withBlockQuic(base.blockQuic).withRuleSetCache(ruleSetCachePath())
        return base.anxLicense.map { assembled.withAnxLicense($0) } ?? assembled
    }

    /// Where the engine keeps the remote rule sets' last good downloads between starts: a folder in the
    /// extension's own Application Support, as for the script store. Only the extension reads it, and on macOS the
    /// tunnel may run as a root System Extension that has no access to the user's app group container. The lists
    /// are downloaded again when lost, so the folder is left out of backups. Nil (no cache) when it cannot be made.
    private static func ruleSetCachePath() -> String? {
        guard var url = try? FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("rule-sets", isDirectory: true)
        else {
            return nil
        }
        do {
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        } catch {
            return nil
        }
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? url.setResourceValues(values)
        return url.path
    }

    /// Builds the config a start with `value` (new local rules) would run. When only its rule lines differ from the
    /// running config, swaps them into the engine in place (`reload_rules`, a double-buffered swap that keeps every
    /// open connection); otherwise, or when the engine refuses them, answers `restart` so the app restarts the
    /// tunnel as before.
    private func reloadLocalRules(_ value: Data) -> Data? {
        func answer(_ reply: RuntimeControl.ReloadAnswer) -> Data? {
            try? JSONEncoder().encode(reply)
        }
        guard let session, let base = reloadBase, let running = runningConfig else {
            return answer(.init(reloaded: false, restart: true))
        }
        // On the node library this start read (`ReloadBase.nodeLibrary`): new nodes or picks saved since are not
        // what the engine runs, and a pick made since is kept by the engine across the swap.
        let next = Self.assemble(
            base,
            routing: withSubscriptionRules(
                base.config, localRules: LocalRules.fromProfileValue(value), dns: base.dns, library: base.nodeLibrary
            )
        )
        guard let rules = running.reloadableRules(to: next) else {
            events.record("local rules changed beyond the rule lines: restart")
            return answer(.init(reloaded: false, restart: true))
        }
        // The same size check as at start: a swap is refused by nothing, so a config the engine could not have
        // started with restarts instead, and the start says what it left out.
        if (try? EngineBudget.encoded(next.withoutMitm())) == nil {
            events.record("local rules over the config budget: restart")
            return answer(.init(reloaded: false, restart: true))
        }
        if rules == running.routing?.rules {
            runningConfig = next
            routingState.droppedRuleSets = droppedRuleSets
            return answer(.init(reloaded: true, rules: rules.count))
        }
        let reply = session.control(RuntimeControl.reloadRules(rules, dryRun: false).request)
        guard let done = RuntimeControl.decode(RuntimeControl.ReloadAnswer.self, from: reply), done.reloaded else {
            let code = RuntimeControl.decode(RuntimeControl.ReloadAnswer.self, from: reply)?.error?.code ?? "unavailable"
            events.record("rules reload refused (\(code)): restart")
            return answer(.init(reloaded: false, error: RuntimeControl.decode(RuntimeControl.ReloadAnswer.self, from: reply)?.error, restart: true))
        }
        runningConfig = next
        routingState.droppedRuleSets = droppedRuleSets
        events.record("rules reloaded in place, \(rules.count) rules, generation \(done.generation ?? 0)")
        return reply
    }

    /// Adds the subscription's rules and groups from `library` (the node library the start read from the Keychain or
    /// the start secrets), with the user's local rules in front; a node list without subscription rules gets the
    /// local rules, then everything else through the selected node. They are an extra like HTTPS decryption: without
    /// a readable library the selected node carries all traffic, and the reason goes to the diagnostics.
    private func withSubscriptionRules(
        _ config: EmbeddedRuntimeConfig,
        localRules: LocalRules,
        dns: DnsSettings?,
        library: Result<String?, Error>
    ) -> EmbeddedRuntimeConfig {
        droppedRuleSets = []
        do {
            guard let value = try library.get(),
                  let build = try NodeLibrary.fromKeychainValue(value).routingBuild(
                      localRules: localRules, dnsRuleSets: dns?.ruleSetNames ?? []
                  )
            else {
                events.record("subscription rules off: no node list")
                return config
            }
            let routing = build.routing
            droppedRuleSets = build.droppedRuleSets
            events.record("subscription rules on, \(routing.outbounds.count) nodes, \(routing.rules.count) rules, \(routing.groups.count) groups")
            if !build.droppedRuleSets.isEmpty {
                events.record("rule sets over the engine's \(NodeLibrary.maxRuleSets): \(build.droppedRuleSets.count) left out")
            }
            return config.withRouting(routing)
        } catch let failure as NodeSecretStore.Failure {
            events.record("subscription rules off: node list unavailable (keychain status \(failure.status))")
        } catch {
            events.record("subscription rules off: node list is malformed")
        }
        return config
    }

    /// Gives each ANX outbound the key and license of its node's subscription origin (anx spec §2.4.1), once some
    /// subscription offered ANX licensing. Each origin's license goes to its own file in the extension's container,
    /// which the outbound reads at each connection. Without a license state there is nothing and the config stays
    /// unchanged; a node without an origin, or whose origin offers no license, gets nothing, never another origin's.
    private func anxLicenseFields(library snapshot: Result<String?, Error>) -> OutboundLicenses? {
        guard let value = startSecrets?.anxLicense ?? (try? NodeSecretStore.value(account: AnxLicense.keychainAccount)),
              let state = try? AnxLicense.fromKeychainValue(value),
              state.offered,
              let directory = try? FileManager.default
                  .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        else {
            return nil
        }
        let licenses = state.writeLicenseFiles(in: directory)
        if licenses.failures > 0 {
            events.record("anx license not written")
        }
        let fieldsByOrigin = licenses.fieldsByOrigin
        let written = licenses.written
        // The single license file of versions before 2026-11-19.
        try? FileManager.default.removeItem(at: directory.appendingPathComponent("anx-license.cert"))
        // The same copy of the library the routing is built from.
        let library = (try? snapshot.get()).flatMap { try? NodeLibrary.fromKeychainValue($0) }
        events.record("anx licenses \(written) of \(fieldsByOrigin.count) origins")
        return OutboundLicenses(fieldsByOrigin: fieldsByOrigin, nodes: library?.nodes ?? [])
    }

    /// Creates the runtime and returns it with the config it runs. When the config with the subscription's rules is
    /// over the engine's size budget (`EngineBudget`) or the engine refuses it, or there are none, the selected node
    /// runs alone (`EmbeddedRuntimeConfig.selectedNodeAlone()`); `routingState` says which, for the app.
    private func makeSession(
        _ config: EmbeddedRuntimeConfig,
        mitmPlugins: [String],
        mitmScripts: [String: String],
        mitmResources: [String: Data]
    ) throws -> (EmbeddedRuntimeSession, EmbeddedRuntimeConfig) {
        let resourceAssets = Self.resourceAssets(mitmResources)
        guard config.routing != nil else {
            routingState = RoutingState(status: .unused)
            let alone = config.selectedNodeAlone()
            let session = try makeRuntimeSession(
                alone, mitmPlugins: mitmPlugins, mitmScripts: mitmScripts, resourceAssets: resourceAssets
            )
            return (session, alone)
        }
        do {
            // Measured without decryption first: the rules go before the plugins, which `makeRuntimeSession`
            // leaves out when only they do not fit.
            _ = try EngineBudget.encoded(config.withoutMitm())
            let session = try makeRuntimeSession(
                config, mitmPlugins: mitmPlugins, mitmScripts: mitmScripts, resourceAssets: resourceAssets
            )
            routingState = RoutingState(status: .on, droppedRuleSets: droppedRuleSets)
            return (session, config)
        } catch let tooLarge as EngineBudget.TooLarge {
            events.record("subscription rules off: config too large (\(tooLarge.bytes) of \(tooLarge.limit) bytes)")
            routingState = RoutingState(status: .off, reason: .tooLarge, bytes: tooLarge.bytes)
        } catch let failure as EmbeddedRuntimeSession.Failure {
            events.record("subscription rules off: runtime refused them (code \(failure.code))")
            routingState = RoutingState(status: .off, reason: .refused)
        } catch {
            events.record("subscription rules off: runtime refused them")
            routingState = RoutingState(status: .off, reason: .refused)
        }
        let alone = config.selectedNodeAlone()
        let session = try makeRuntimeSession(
            alone, mitmPlugins: mitmPlugins, mitmScripts: mitmScripts, resourceAssets: resourceAssets
        )
        return (session, alone)
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
                // Over the engine's plugin budget it would refuse the whole decryption section.
                try EngineBudget.checkDecryption(mitmPlugins)
                if let authority = try mitmAuthority() {
                    if !mitmScripts.isEmpty {
                        do {
                            let session = try EmbeddedRuntimeSession(
                                config: EngineBudget.encoded(config.withMitm(
                                    plugins: mitmPlugins,
                                    authority: authority,
                                    scriptAssets: try Self.scriptAssets(mitmScripts),
                                    resourceAssets: resourceAssets,
                                    scriptStorePath: try Self.scriptStoreURL().path
                                )),
                                packetFlow: packetFlow
                            )
                            events.record("https decryption on, \(mitmPlugins.count) plugins, \(mitmScripts.count) scripts")
                            httpsDecryption = HttpsDecryptionState(
                                status: .on, plugins: mitmPlugins.count, rootFingerprint: authority.sha256Fingerprint
                            )
                            return session
                        } catch let failure as EmbeddedRuntimeSession.Failure {
                            events.record("scripts off: runtime refused them (code \(failure.code))")
                        } catch let tooLarge as EngineBudget.TooLarge {
                            events.record("scripts off: config too large (\(tooLarge.bytes) of \(tooLarge.limit) bytes)")
                        } catch {
                            events.record("scripts off: could not write them")
                        }
                    }
                    let session = try EmbeddedRuntimeSession(
                        config: EngineBudget.encoded(config.withMitm(
                            plugins: mitmPlugins, authority: authority, resourceAssets: resourceAssets
                        )),
                        packetFlow: packetFlow
                    )
                    events.record("https decryption on, \(mitmPlugins.count) plugins")
                    httpsDecryption = HttpsDecryptionState(
                        status: .on, plugins: mitmPlugins.count, rootFingerprint: authority.sha256Fingerprint
                    )
                    return session
                }
                events.record("https decryption off: no root certificate")
                httpsDecryption = HttpsDecryptionState(status: .off, reason: .noRoot, plugins: mitmPlugins.count)
            } catch let failure as MitmCertificateStore.Failure {
                events.record("https decryption off: root certificate unavailable (keychain status \(failure.status))")
                httpsDecryption = HttpsDecryptionState(status: .off, reason: .rootUnavailable, plugins: mitmPlugins.count)
            } catch let tooLarge as EngineBudget.TooLarge {
                events.record("https decryption off: plugins too large (\(tooLarge.bytes) of \(tooLarge.limit) bytes)")
                httpsDecryption = HttpsDecryptionState(
                    status: .off, reason: .tooLarge, plugins: mitmPlugins.count, bytes: tooLarge.bytes
                )
            } catch let failure as EmbeddedRuntimeSession.Failure {
                events.record("https decryption off: runtime refused the plugins (code \(failure.code))")
                httpsDecryption = HttpsDecryptionState(status: .off, reason: .refused, plugins: mitmPlugins.count)
            } catch {
                events.record("https decryption off: setup failed")
                httpsDecryption = HttpsDecryptionState(status: .off, reason: .setupFailed, plugins: mitmPlugins.count)
            }
        } else {
            httpsDecryption = HttpsDecryptionState(status: .unused)
        }
        return try EmbeddedRuntimeSession(config: EngineBudget.encoded(config.withoutMitm()), packetFlow: packetFlow)
    }

    /// The MITM root: the one the macOS app handed over, else the shared Keychain's.
    private func mitmAuthority() throws -> MitmCertificateAuthority? {
        if let stored = startSecrets?.mitmAuthority {
            return try MitmCertificateStore.authority(fromStoredValue: stored)
        }
        return try MitmCertificateStore.load()
    }

    /// Where the extension keeps the last start secrets, in its own Application Support directory (macOS).
    private static func startSecretsURL() -> URL? {
        try? FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent(TunnelStartSecrets.fileName)
    }

    /// Keeps the start secrets the macOS app handed over after saving the profile again
    /// (`TunnelAppMessage.startSecrets`), for the next start without options. Only the kept copy changes: the
    /// running tunnel goes on with what it started with, so its node library still matches its outbound. Answers
    /// `ok` once the copy is written, else nil. iOS shares the Keychain and keeps no copy.
    private func keepStartSecrets(_ value: Data) -> Data? {
        #if os(macOS)
        guard let secrets = TunnelStartSecrets.decoded(from: value), let url = Self.startSecretsURL() else {
            events.record("start secrets not kept: unreadable hand-over")
            return nil
        }
        do {
            try secrets.save(to: url)
        } catch {
            events.record("start secrets not kept: could not write them")
            return nil
        }
        events.record("start secrets updated by the app")
        return TunnelStartSecrets.handOverAnswer
        #else
        return nil
        #endif
    }

    /// On macOS the tunnel may run as a System Extension, a root process that cannot read the user's Keychain, so
    /// the app passes the Keychain values in the start options. They are kept in the extension's own container for
    /// starts without options (on demand, after sleep), and the app replaces that copy whenever it saves the profile
    /// again while the tunnel runs (`keepStartSecrets`). iOS shares the Keychain and never passes them.
    private static func resolveStartSecrets(options: [String: NSObject]?, events: TunnelDiagnosticLog) -> TunnelStartSecrets? {
        #if os(macOS)
        let url = startSecretsURL()
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

    /// Gateway mode: other devices route through this Mac, which needs IPv4 forwarding. Turned on here when the
    /// system lets the extension (and off again on stop); otherwise the app shows the user the command to run.
    private func enableGatewayForwarding() {
        #if os(macOS)
        switch IPv4Forwarding.isOn() {
        case true?:
            events.record("gateway: ip forwarding already on")
        default:
            if IPv4Forwarding.set(true) {
                turnedForwardingOn = true
                events.record("gateway: ip forwarding on")
            } else {
                events.record("gateway: ip forwarding is off and the extension may not turn it on (errno \(errno))")
            }
        }
        #endif
    }

    /// Where the system proxy points and what it leaves alone.
    struct SystemProxyEndpoint {
        let port: UInt16
        let exceptions: ProxyExceptions
    }

    static var isMac: Bool {
        #if os(macOS)
        true
        #else
        false
        #endif
    }

    /// Starts the engine's `127.0.0.1` listener when the takeover mode registers a system proxy. It runs before
    /// the interface settings, which carry its port, and stops with the session. Nil when the mode has no proxy or
    /// the engine refused it; the tunnel then runs without one.
    private func startSystemProxy(
        _ session: EmbeddedRuntimeSession,
        takeover: TrafficTakeover,
        exceptions: ProxyExceptions
    ) -> SystemProxyEndpoint? {
        guard takeover.usesSystemProxy else {
            events.record("takeover \(takeover.rawValue)")
            return nil
        }
        let request = RuntimeControl.startSystemProxy(port: RuntimeControl.systemProxyPort).request
        guard let answer = RuntimeControl.decode(RuntimeControl.SystemProxy.self, from: session.control(request)),
              answer.running, let port = answer.port
        else {
            events.record("takeover \(takeover.rawValue): system proxy off, the engine refused it")
            return nil
        }
        events.record("takeover \(takeover.rawValue): system proxy on port \(port), \(exceptions.entries.count) exceptions")
        return SystemProxyEndpoint(port: port, exceptions: exceptions)
    }

    /// The system HTTP and HTTPS proxy for `proxy`, for every domain (`matchDomains = [""]`), leaving plain host
    /// names and the exceptions alone.
    static func proxySettings(_ proxy: SystemProxyEndpoint) -> NEProxySettings {
        let settings = NEProxySettings()
        let server = NEProxyServer(address: "127.0.0.1", port: Int(proxy.port))
        settings.httpEnabled = true
        settings.httpServer = server
        settings.httpsEnabled = true
        settings.httpsServer = server
        settings.excludeSimpleHostnames = true
        settings.exceptionList = proxy.exceptions.entries
        settings.matchDomains = [""]
        return settings
    }

    /// Interface settings from the saved profile. The provider's own sockets (to the proxy server or the
    /// WireGuard endpoint) are never routed into the tunnel by the system. Returns nil for an address or route
    /// that does not parse.
    ///
    /// Without `routesTraffic` (the macOS system proxy mode) the interface takes no routes and no DNS: only the
    /// system proxy, when there is one, sends traffic to the engine.
    static func networkSettings(
        _ interface: TunnelInterface,
        remoteAddress: String,
        routesTraffic: Bool = true,
        proxy: SystemProxyEndpoint? = nil
    ) -> NEPacketTunnelNetworkSettings? {
        let addresses = interface.addresses.map(CIDR.parse)
        let routes = interface.includedRoutes.map(CIDR.parse)
        guard addresses.allSatisfy({ $0 != nil }), routes.allSatisfy({ $0 != nil }) else {
            return nil
        }
        let v4Addresses = addresses.compactMap { $0 }.filter { !$0.isIPv6 }
        let v6Addresses = addresses.compactMap { $0 }.filter { $0.isIPv6 }
        let v4Routes = routesTraffic ? routes.compactMap { $0 }.filter { !$0.isIPv6 } : []
        let v6Routes = routesTraffic ? routes.compactMap { $0 }.filter { $0.isIPv6 } : []

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
        if routesTraffic, !interface.dnsServers.isEmpty {
            let dns = NEDNSSettings(servers: interface.dnsServers)
            dns.matchDomains = [""]
            settings.dnsSettings = dns
        }
        if let proxy {
            settings.proxySettings = proxySettings(proxy)
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
    /// No retry can fix it (an engine this build cannot run, a profile the engine refuses): the tunnel stops
    /// reconnecting by itself at once (`AutoReconnect`).
    var fatal = false

    init(reason: String, fatal: Bool = false) {
        self.reason = reason
        self.fatal = fatal
    }

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

/// Whether the tunnel was stopped while a start waited (`AutoReconnect`).
final class StartGate: @unchecked Sendable {
    private let lock = NSLock()
    private var stopped = false

    func arm() {
        lock.lock()
        stopped = false
        lock.unlock()
    }

    func cancel() {
        lock.lock()
        stopped = true
        lock.unlock()
    }

    var cancelled: Bool {
        lock.lock()
        defer { lock.unlock() }
        return stopped
    }
}

/// A value behind a lock.
final class Locked<Value>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Value

    init(_ value: Value) {
        self.value = value
    }

    func get() -> Value {
        lock.lock()
        defer { lock.unlock() }
        return value
    }

    func set(_ next: Value) {
        lock.lock()
        value = next
        lock.unlock()
    }
}

/// Calls a completion once, whichever answer comes first.
final class Once<Value>: @unchecked Sendable {
    private let lock = NSLock()
    private var completion: ((Value) -> Void)?

    init(_ completion: @escaping (Value) -> Void) {
        self.completion = completion
    }

    func call(_ value: Value) {
        lock.lock()
        let pending = completion
        completion = nil
        lock.unlock()
        pending?(value)
    }
}
