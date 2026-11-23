import Foundation
import NetworkCoreBridge
import NetworkExtension
import Observation
#if os(iOS)
import BackgroundTasks
import WidgetKit
#elseif os(macOS)
import AppKit
#endif

/// Owns the app's single VPN configuration: imports a node (a share link of any protocol the engine runs, a
/// subscription text or a WireGuard config), saves it as the
/// tunnel profile, and starts or stops the packet tunnel. Node secrets go only into the Keychain; the saved
/// profile holds a persistent reference to them and a config without them.
///
/// The widget, Control Center, the Live Activity, Shortcuts and Settings change the same VPN configuration from
/// other processes. Every change the app makes to it, and to the node keys, therefore runs in `profileWrites`, one
/// at a time, and starts from the configuration as saved at that moment (`writing(_:)`, `writeProfile`): a change
/// sets only what it owns, so it never puts back an On Demand switch or a mode another process changed. The app
/// also reads the configuration again when it changes (`NEVPNConfigurationChange`) and when the app becomes active.
///
/// Observable (not ObservableObject): a view redraws only when a property it actually read changes, so the speed
/// and latency updates no longer redraw every screen that holds the controller.
@MainActor
@Observable
final class TunnelController {
    private(set) var coordinator: AppCoordinator
    private(set) var nodeName: String?
    private(set) var connection: NEVPNStatus = .invalid
    /// The VPN configuration as the disconnected status line explains it (`TunnelProfileState`). Stored, since
    /// `manager` is not observed.
    private(set) var profileState = TunnelProfileState.missing
    /// Set once `load()` has read the saved profile and node library, so first-run screens do not flash for a
    /// user who already has nodes.
    private(set) var loaded = false
    /// The notice the home screen shows: what happened, optional technical detail, and what the user can do.
    private(set) var notice: AppNotice?
    /// The notice's text. Setting it shows a plain notice with no actions.
    private(set) var message: String? {
        get { notice?.text }
        set { notice = newValue.map { AppNotice(text: $0) } }
    }
    private(set) var runtimeStatus: String?
    /// Developer mode: the extension's diagnostics and the last stop error, kept after the message is replaced.
    private(set) var diagnostics: TunnelDiagnostics?
    private(set) var lastDisconnectError: String?
    /// When the current connection came up, for the elapsed time on the home screen.
    private(set) var connectedSince: Date?
    /// The engine's last connection list, kept after the tunnel stops so the request list still shows what went
    /// through it. In memory only: cleared when a new connection comes up or the app quits.
    private(set) var lastConnections: RuntimeControl.Connections?
    /// The library the tunnel runs: the active subscription group's nodes and rules, with the other groups its
    /// rules can use (`SubscriptionGroups.activeLibrary()`). Nil while a single pasted node is in use.
    private(set) var library: NodeLibrary?
    /// Every subscription group, from the Keychain.
    private(set) var subscriptions = SubscriptionGroups()
    /// Delay test results by `latencyKey(group:node:)`.
    private(set) var latencies: [String: NodeLatency] = [:]
    /// Groups being downloaded now.
    private(set) var updatingGroups: Set<String> = []
    /// Set once `subscriptions` holds what the Keychain has (`readSavedSubscriptions()`). Before that, or when the
    /// Keychain could not be read (before the first unlock, for example in a background launch), an empty list says
    /// nothing about the groups: the background refresh is not withdrawn for it (`scheduleBackgroundRefresh`), the
    /// app reads them again when it comes to the front (`RootView`, `reloadNow`), and `store` refuses to replace the
    /// saved groups with a list built on the empty one. `loaded` alone does not say the groups were read. Observed, so
    /// the welcome screen waits for it (`RootView`).
    private(set) var subscriptionsRead = false
    /// Whether the last `importLink` reconnected a running tunnel on what it imported, for the import sheet.
    @ObservationIgnored private(set) var importReconnected = false
    /// The saved tunnel's mode: by rules, everything through the selected node, or everything direct.
    private(set) var mode = ProxyMode.global
    /// DNS servers, hosts and per-domain servers, from `dns-settings.json`; the saved profile carries a copy.
    private(set) var dns = DnsSettings()
    /// Destination rewrites, from `redirects.json`; the saved profile carries the enabled lines.
    private(set) var redirects = TrafficRedirects()
    /// Map Local and Map Remote entries, from `debug-mappings.json`; the saved profile carries their plugin.
    private(set) var debugMappings = DebugMappings()
    /// The rule mode of a node list without subscription rules and the user's own rules, from `local-rules.json`;
    /// the saved profile carries a copy.
    private(set) var localRules = LocalRules()
    /// Reconnecting by itself and the Wi-Fi rules (`OnDemandSettings`), from the app's defaults; the saved profile
    /// carries the rules.
    private(set) var onDemand = OnDemandSettings.load()
    /// Whether the tunnel refuses UDP to port 443 so apps fall back from QUIC to TCP, where HTTPS decryption and
    /// domain rules see them. Kept in the app's defaults; the saved profile carries a copy
    /// (`TunnelProfileKeys.blockQuic`). Off by default.
    private(set) var blockQuic = UserDefaults.standard.bool(forKey: TunnelController.blockQuicKey)
    /// How the tunnel takes over traffic: the tunnel interface, the system proxy, or both (`TrafficTakeover`). Kept
    /// in the app's defaults; the saved profile carries a copy (`TunnelProfileKeys.takeover`).
    private(set) var takeover = TunnelController.savedTakeover()
    /// What the system proxy leaves alone; the saved profile carries a copy (`TunnelProfileKeys.proxyExceptions`).
    private(set) var proxyExceptions = ProxyExceptions.fromProfileValue(
        UserDefaults.standard.data(forKey: ProxyExceptions.storageKey)
    )
    /// Whether the Mac forwards other devices' traffic into the tunnel (`GatewayMode`); macOS only, and only in a
    /// mode that routes traffic. The saved profile carries a copy (`TunnelProfileKeys.gateway`).
    private(set) var gateway = TunnelController.isMac && UserDefaults.standard.bool(forKey: GatewayMode.storageKey)
    /// When the running tunnel last took new local rules in place, without a restart.
    private(set) var lastRulesReload: Date?
    /// Proxy client or debugging tool (`AppMode`); nil until the user picks one on first launch. Read in `load()`.
    private(set) var appMode: AppMode?
    /// Where debug mode sends traffic: straight out, or through the selected node (`DebugExit`).
    private(set) var debugExit = DebugExit(rawValue: UserDefaults.standard.string(forKey: DebugExit.storageKey) ?? "") ?? .direct
    /// The saved profile is the node-less one debug mode uses (`TunnelProfile.direct`).
    private(set) var runsDirectProfile = false

    /// Set while an action waits for the HTTPS decryption confirmation (`requireDecryptionConsent()`). The root view
    /// shows the alert, or the editor sheet in front of it (`decryptionConsentAlert(_:inSheet:)`).
    private(set) var asksDecryptionConsent = false
    /// The editor sheets that can show the confirmation, in the order they appeared. The last one shows it while
    /// any is open, so the alert never tries to present behind a sheet; with none, the root view shows it.
    private(set) var decryptionConsentSheets: [UUID] = []
    /// The actions waiting for the answer; all of them get the same one.
    @ObservationIgnored private var decryptionConsentWaiters: [CheckedContinuation<Bool, Never>] = []
    /// Set once this launch has asked about decryption saved before it was confirmed (`confirmSavedDecryption()`).
    @ObservationIgnored private var askedSavedDecryption = false

    /// The link an `anixops://import` asked for, until the import sheet takes it (`takePendingImportLink()`).
    @ObservationIgnored var pendingImportLink: String?

    /// The VPN configuration as last read from or saved to the system settings. Only `writeProfile` and the reloads
    /// replace it; nothing changes it in place.
    @ObservationIgnored private var manager: NETunnelProviderManager?
    @ObservationIgnored private var statusObserver: NSObjectProtocol?
    /// `NEVPNConfigurationChange`, and on macOS the app becoming active: both read the configuration again.
    @ObservationIgnored private var reloadObservers: [NSObjectProtocol] = []
    /// Every read-modify-write of the VPN configuration and the node keys, one at a time (`SerialAsyncQueue`).
    private let profileWrites = SerialAsyncQueue()
    /// Node key versions written by saves that have not finished; the clean-up after a save never removes them.
    @ObservationIgnored private var keyVersionsInFlight: Set<String> = []
    /// Set while a reload waits in `profileWrites`; the notifications that arrive meanwhile share it.
    @ObservationIgnored private var reloadQueued = false
    /// Set when `restart()` stopped the tunnel; the tunnel starts again once it is down.
    @ObservationIgnored private var restartAfterStop = false
    /// Set while the start after such a stop waits in `profileWrites`, so it starts on the profile the writes before
    /// it saved. The user's disconnect clears both.
    @ObservationIgnored private var restartStartQueued = false
    /// macOS: the start secrets last handed to the running System Extension (`handOverStartSecrets()`), encoded; nil
    /// while the tunnel is not connected, so the next connect hands them over again.
    @ObservationIgnored private var handedOverStartSecrets: Data?
    /// macOS: the profile (`TunnelStartSecrets.staleCopyProfileStamp`) the app last started again after a start
    /// failed on a stale copy of the start secrets (`startAfterStaleCopy`), so it tries once for each saved profile.
    /// Kept across launches: the system's record of the last stop outlives the app, and a retry already made for
    /// this profile is not made again at every launch.
    private var staleCopyRetriedProfile: String? {
        get { UserDefaults.standard.string(forKey: Self.staleCopyRetriedKey) }
        set { UserDefaults.standard.set(newValue, forKey: Self.staleCopyRetriedKey) }
    }
    private static let staleCopyRetriedKey = "networkcore.stale-copy-retried-profile"
    /// Capture as the app last set it in the running engine (`set_capture`; `load_capture` turns it off).
    @ObservationIgnored private var captureOn = false
    /// Set when the app restarts the tunnel itself (a new node, a decrypted domain) while capture is on: the new
    /// engine starts with capture off, so the app turns it back on once connected. A disconnect by the user, or by
    /// the system, ends capture with the tunnel.
    @ObservationIgnored private var resumeCapture = false
    /// The saved capture session the request and connection lists show instead of the live capture, opened in the
    /// app's own capture viewer (`CaptureViewer`); nil shows the live capture of the running tunnel.
    var viewedCapture: PacketCapture.Session?
    /// The app opened `viewedCapture` itself when the tunnel went down; the lists go back to the live capture when
    /// it connects again.
    var viewedOnStop = false
    /// The running tunnel's session id for its live capture (`PacketCapture.Status.runSession`): the app's copy of
    /// it opens when the tunnel goes down.
    @ObservationIgnored var captureRun: String?
    /// What the live capture held at the app's last copy of it, so an unchanged capture is not copied again.
    @ObservationIgnored var copiedCapture: PacketCapture.Status?
    /// Copies the live capture into the app every few seconds while connected (`runCaptureMirror`).
    @ObservationIgnored var captureMirror: Task<Void, Never>?

    init() {
        coordinator = AppCoordinator(status: Self.platformStatus(saved: false))
        statusObserver = NotificationCenter.default.addObserver(
            forName: .NEVPNStatusDidChange,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.refresh()
            }
        }
        // Another process changed the configuration (or the app saved it): read it again once the writes already
        // queued are done.
        reloadObservers.append(NotificationCenter.default.addObserver(
            forName: .NEVPNConfigurationChange,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.scheduleReload()
            }
        })
        #if os(macOS)
        // System Settings may have changed it while the app was in the background; iOS reloads on the scene becoming
        // active (`NetworkCoreAppMain`).
        reloadObservers.append(NotificationCenter.default.addObserver(
            forName: NSApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.scheduleReload()
                self?.checkStaleStartSecrets()
            }
        })
        // A new or deleted MITM root goes to the running System Extension's copy of the start secrets.
        reloadObservers.append(NotificationCenter.default.addObserver(
            forName: MitmCertificateStore.didChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.handOverStartSecrets()
            }
        })
        #endif
    }

    deinit {
        if let statusObserver = statusObserver {
            NotificationCenter.default.removeObserver(statusObserver)
        }
        for observer in reloadObservers {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    var isConnected: Bool {
        connection == .connected || connection == .connecting || connection == .reasserting
    }

    func load() async {
        await profileWrites.run {
            await loadNow()
        }
        // Not awaited: the answer can take as long as the person likes, and nothing else has to wait for it.
        Task {
            await confirmSavedDecryption()
        }
    }

    private func loadNow() async {
        do {
            manager = try await NETunnelProviderManager.loadAllFromPreferences().first
            message = nil
        } catch {
            message = String(localized: "无法读取 VPN 配置")
        }
        if let saved = try? TrafficRedirects.load(from: TrafficRedirects.defaultURL) {
            redirects = saved
        }
        if let saved = try? DnsSettings.load(from: DnsSettings.defaultURL) {
            dns = saved
        }
        if let saved = try? DebugMappings.load(from: DebugMappings.defaultURL) {
            debugMappings = saved
        }
        if let saved = try? LocalRules.load(from: LocalRules.defaultURL) {
            localRules = saved
        }
        readSavedSubscriptions()
        // Shortcuts or Control Center may have switched the mode while the app was not running.
        followSavedMode()
        refresh()
        let stored = UserDefaults.standard.string(forKey: AppMode.storageKey)
        appMode = AppMode.resolve(
            stored: stored,
            hasNodes: !hasNoNodes,
            onboardingDone: UserDefaults.standard.bool(forKey: AppMode.onboardingDoneKey)
        )
        if stored == nil, let appMode {
            UserDefaults.standard.set(appMode.rawValue, forKey: AppMode.storageKey)
        }
        loaded = true
        await repairMissingNodeKey()
        #if os(macOS)
        // On Demand may have failed to start the tunnel while the app was not running (`startAfterStaleCopy`).
        if manager?.connection.status == .disconnected {
            _ = startAfterStaleCopy(failure: await lastStopDescription(), requiresOnDemand: true)
        }
        #endif
    }

    /// Reads the subscription groups from the Keychain unless they were read already. Runs in `profileWrites`
    /// (`loadNow`, `reloadNow`), so no write is between building its groups and storing them while this replaces
    /// them. A Keychain that cannot be read now (before the first unlock) leaves `subscriptionsRead` false, and the
    /// next call tries again.
    private func readSavedSubscriptions() {
        guard !subscriptionsRead else {
            return
        }
        switch Result(catching: { try NodeSecretStore.value(account: SubscriptionGroups.keychainAccount) }) {
        case let .success(.some(value)):
            // Set before `store`, which refuses to write while the groups are unread.
            subscriptionsRead = true
            if let saved = try? SubscriptionGroups.fromKeychainValue(value) {
                // Saved again so the tunnel's library has what this version adds to it (the ANX license origin of
                // each node, anx spec §2.4.1) without waiting for a subscription update.
                store(saved)
            }
            // A value that does not decode will not decode on a later try either: the groups stay empty and the next
            // save replaces it, as before groups were guarded.
        case let .failure(error):
            if (error as? NodeSecretStore.Failure)?.isLockedUntilFirstUnlock == true {
                // Not readable before the first unlock: keep what is shown and read again later.
                break
            }
            // Any other failure repeats on every read; waiting for it would block every write for good. The groups
            // stay empty and the next save replaces the value, as before groups were guarded.
            subscriptionsRead = true
        case .success(.none):
            subscriptionsRead = true
            // A library saved before subscription groups existed becomes the first group.
            let previous = (try? NodeSecretStore.value(account: NodeLibrary.keychainAccount))
                .flatMap { $0 }
                .flatMap { try? NodeLibrary.fromKeychainValue($0) }
            store(SubscriptionGroups.migrating(previous, name: String(localized: "导入的节点")))
        }
    }

    /// Reads the VPN configuration again once the writes queued before are done: when the app comes back to the
    /// front, where Settings, the widget, Control Center, the Live Activity or Shortcuts may have changed it.
    func reloadProfile() async {
        await profileWrites.run {
            await reloadNow()
        }
    }

    /// `reloadProfile()` for a notification; the notifications that arrive while it waits share it.
    private func scheduleReload() {
        guard !reloadQueued else {
            return
        }
        reloadQueued = true
        Task {
            await profileWrites.run {
                reloadQueued = false
                await reloadNow()
            }
        }
    }

    /// Reads the VPN configuration as saved now; runs in `profileWrites`. A failed read keeps the copy shown.
    private func reloadNow() async {
        #if DEBUG
        if Self.isDemo {
            // The screenshots' made-up state stays; the demo reads no configuration.
            return
        }
        #endif
        // A launch in the background before the first unlock could not read the groups; every write and every
        // return to the front reads them once the Keychain allows it, so no write starts from the empty list.
        readSavedSubscriptions()
        guard let managers = try? await NETunnelProviderManager.loadAllFromPreferences() else {
            return
        }
        manager = managers.first
        followSavedMode()
        refresh()
    }

    /// The active group's rules switch follows the mode the VPN configuration is saved in
    /// (`SubscriptionGroups.followingSavedMode`), so the next profile the app saves keeps a mode that Shortcuts or
    /// Control Center picked.
    private func followSavedMode() {
        guard let configuration = (manager?.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration,
              let groups = subscriptions.followingSavedMode(ProxyMode.saved(in: configuration))
        else {
            return
        }
        store(groups)
    }

    /// Runs `operation` in `profileWrites` once the VPN configuration has been read again, so it starts from what
    /// is saved when its turn comes rather than from what the app read before.
    private func writing<T>(_ operation: () async -> T) async -> T {
        await profileWrites.run { () async -> T in
            await reloadNow()
            return await operation()
        }
    }

    /// One read-modify-write of the VPN configuration (`ConfigurationWrite`); runs in `profileWrites`. `change` sets
    /// the fields its write owns on a copy read right before, and returns false when the copy has them already.
    /// `creating` adds a configuration when there is none, which only an import or a connect does. `manager` becomes
    /// the configuration as saved afterwards, or as saved now when the write failed.
    @discardableResult
    private func writeProfile(
        creating: Bool = false,
        _ change: (NETunnelProviderManager) throws -> Bool
    ) async throws -> ConfigurationWrite.Outcome<NETunnelProviderManager> {
        let create: (() -> NETunnelProviderManager)? = creating ? { NETunnelProviderManager() } : nil
        do {
            let outcome = try await ConfigurationWrite.run(
                load: { try await NETunnelProviderManager.loadAllFromPreferences().first },
                create: create,
                save: { (target: NETunnelProviderManager) in
                    try await target.saveToPreferences()
                    // Reloaded so its connection object reflects the saved profile.
                    try await target.loadFromPreferences()
                },
                isStale: { Self.isStale($0) },
                change: change
            )
            manager = outcome.object
            if outcome.isSaved {
                // The System Extension's copy for starts without options follows the saved profile.
                handOverStartSecrets()
            }
            return outcome
        } catch {
            if let saved = try? await NETunnelProviderManager.loadAllFromPreferences() {
                manager = saved.first
            }
            throw error
        }
    }

    /// A save refused because another process saved the configuration after this copy was read.
    nonisolated private static func isStale(_ error: Error) -> Bool {
        (error as? NEVPNError)?.code == .configurationStale
    }

    /// A profile saved by 0.3.0-beta.11 or earlier may name a node key that another save removed while both ran, and
    /// the tunnel then stops at once with "node key is unavailable". Saving the profile again for the selected node
    /// writes the key anew. Only a key the Keychain reports missing counts (a Keychain that cannot be read yet, before
    /// the first unlock, is left alone), and only for the profile the system has turned on, so the save takes no other
    /// app's VPN configuration's place. A single pasted node lives only in the profile and has to be imported again.
    private func repairMissingNodeKey() async {
        guard let manager, manager.isEnabled,
              let configuration = (manager.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration,
              let version = configuration[TunnelProfileKeys.nodeKeyVersion] as? String,
              case .success(.none) = Result(catching: {
                  try NodeSecretStore.value(account: NodeSecretStore.versionedAccount(version))
              })
        else {
            return
        }
        if runsDirectProfile {
            _ = await saveDirectProfile()
        } else if subscriptions.activeID != nil, let groups = Self.withSelectedNode(subscriptions),
                  await saveActiveProfile(groups) {
            store(groups)
        }
    }

    /// Nothing imported yet: no node profile and no subscription groups. Debug mode's direct profile is no node.
    var hasNoNodes: Bool {
        (nodeName == nil || runsDirectProfile) && subscriptions.groups.isEmpty
    }

    /// Debug mode runs without a node: the tunnel goes straight out (`DebugExit.usesDirect`).
    var usesDirectExit: Bool {
        DebugExit.usesDirect(mode: appMode, exit: debugExit, hasNodes: !hasNoNodes)
    }

    /// Where debug mode's traffic goes out, for the connect switch's caption: direct, or the selected node. Exit
    /// "through a node" without any node runs direct for now (`DebugExit.usesDirect`).
    var debugExitSummary: String {
        if !usesDirectExit, !runsDirectProfile, let name = nodeName {
            return String(localized: "出口 · \(name)")
        }
        return String(localized: "出口 · 直连")
    }

    /// What the status line says while disconnected and the tunnel cannot start as saved: a sentence on what the
    /// connect does (`TunnelProfileState.offHint`), never the platform's developer diagnostic. Nil when the
    /// configuration is ready.
    var offHint: String? {
        guard case .blocked = coordinator.decision else {
            return nil
        }
        switch profileState.offHint {
        case .firstConnect?:
            #if os(macOS)
            return String(localized: "打开开关连接，首次需要允许添加 VPN 配置")
            #else
            return String(localized: "轻点连接，首次需要允许添加 VPN 配置")
            #endif
        case .reenable?:
            #if os(macOS)
            return String(localized: "已被其他 VPN 停用，打开开关重新启用")
            #else
            return String(localized: "已被其他 VPN 停用，轻点重新启用")
            #endif
        case nil:
            return String(localized: "无法启动隧道")
        }
    }

    /// The connect switch can start a tunnel: there is a node, or debug mode goes direct.
    var canConnect: Bool {
        !hasNoNodes || usesDirectExit
    }

    /// Switches between the proxy client and the debugging tool. The saved profile follows: debug mode with the
    /// direct exit runs the direct profile, the proxy client the selected node. A running tunnel reconnects.
    func setAppMode(_ next: AppMode) async {
        // Before `load()` a nil `appMode` only means the saved choice has not been read yet: writing one now would
        // replace it (a link that launched the app).
        guard loaded, next != appMode else {
            return
        }
        // A single pasted node lives only in the saved profile, so debug mode goes through it rather than
        // replacing it with the direct profile.
        if next == .debug, subscriptions.activeID == nil, nodeName != nil, !runsDirectProfile {
            setDebugExitValue(.node)
        }
        appMode = next
        UserDefaults.standard.set(next.rawValue, forKey: AppMode.storageKey)
        await applyExit()
    }

    /// Debug mode's exit: direct, or the selected node of the subscriptions. A running tunnel reconnects.
    func setDebugExit(_ next: DebugExit) async {
        guard next != debugExit else {
            return
        }
        setDebugExitValue(next)
        await applyExit()
    }

    private func setDebugExitValue(_ next: DebugExit) {
        debugExit = next
        UserDefaults.standard.set(next.rawValue, forKey: DebugExit.storageKey)
    }

    /// Saves the profile the mode and exit call for, when it is not the saved one, and reconnects a running tunnel.
    private func applyExit() async {
        await writing {
            let saved: Bool
            if usesDirectExit {
                // Without a saved profile nothing is saved yet: the system asks to add the VPN configuration on the
                // first connect (`toggle()`), not when a mode is picked.
                guard manager != nil, !runsDirectProfile else {
                    return
                }
                saved = await saveDirectProfile()
            } else if runsDirectProfile, let groups = Self.withSelectedNode(subscriptions) {
                saved = await saveActiveProfile(groups)
                if saved {
                    store(groups)
                }
            } else {
                return
            }
            if saved {
                restart()
            }
        }
    }

    /// Saves the direct profile (`TunnelProfile.direct`); runs in `profileWrites`.
    private func saveDirectProfile() async -> Bool {
        await save(TunnelProfile.direct(profileID: Self.profileID, displayName: String(localized: "直连")))
    }

    /// Parses pasted text for the import preview without saving anything. A subscription link is downloaded only
    /// when it is imported.
    func preview(_ link: String) -> Result<ImportPreview, ImportFailure> {
        if SubscriptionLink.isPlainHTTP(link) {
            return .failure(ImportFailure(message: Self.plainHTTPSubscriptionText))
        }
        if let url = SubscriptionLink.url(from: link) {
            let host = url.host ?? url.absoluteString
            return .success(ImportPreview(
                name: host,
                outboundKind: ImportPreview.subscriptionKind,
                maskedServer: ImportPreview.mask(host)
            ))
        }
        do {
            let profile = try TunnelNodeImport.parse(link, profileID: Self.profileID, native: NativeNodeImporterFFI.importNodes)
            return .success(ImportPreview(
                name: profile.displayName,
                outboundKind: profile.config.outboundKind,
                maskedServer: ImportPreview.mask(profile.serverAddress)
            ))
        } catch let error as TunnelNodeImportError {
            return .failure(ImportFailure(message: Self.describe(error)))
        } catch {
            return .failure(ImportFailure(message: String(localized: "无法解析节点")))
        }
    }

    /// Shows a plain notice, for example after an `anixops://` link could not run.
    func show(_ text: String) {
        message = text
    }

    func clearMessage() {
        message = nil
    }

    /// Saves the node as the tunnel profile. A subscription link is downloaded into its own subscription group (an
    /// existing group for that link is updated instead), and pasted multi-node text becomes a group too; either
    /// group becomes active. Returns whether it was saved; on failure `message` says why.
    @discardableResult
    func importLink(_ link: String) async -> Bool {
        importReconnected = false
        // ATS refuses plain HTTP downloads, so such a link would always fail as "the link may have expired".
        if SubscriptionLink.isPlainHTTP(link) {
            message = Self.plainHTTPSubscriptionText
            return false
        }
        // Importing in debug mode means going through what was imported.
        if appMode == .debug {
            setDebugExitValue(.node)
        }
        if let url = SubscriptionLink.url(from: link) {
            return await updateSubscription(
                url, groupID: subscriptions.group(url: url.absoluteString)?.id, activate: true, trigger: .user
            )
        }
        let imported: TunnelImport
        do {
            imported = try TunnelNodeImport.parseImport(
                link,
                profileID: Self.profileID,
                native: NativeNodeImporterFFI.importNodes
            )
        } catch let error as TunnelNodeImportError {
            message = Self.describe(error)
            return false
        } catch {
            message = String(localized: "无法解析节点")
            return false
        }
        return await writing { () async -> Bool in
            // Before the groups are read, a new group would replace every saved one.
            guard subscriptionsRead else {
                message = Self.subscriptionsUnreadText
                return false
            }
            var groups = subscriptions
            if let report = imported.report {
                let group = SubscriptionGroup(
                    name: String(localized: "导入的节点"),
                    library: NodeLibrary(report: report, subscriptionURL: nil, keeping: nil),
                    updateIntervalHours: 0
                )
                groups.upsert(group)
                groups.activeID = group.id
            } else {
                // A single node runs on its own; the subscription groups stay for switching back.
                groups.activeID = nil
            }
            guard await save(imported.profile, routed: groups.activeLibrary()?.routesByRules == true) else {
                return false
            }
            store(groups)
            // The extension reads the node and its library only at start: a running tunnel reconnects on what was
            // imported, after `store` put the library where the start reads it, so the home screen, the widget and
            // the Live Activity name the node the traffic really goes through.
            applyToRunningTunnel(.user)
            return true
        }
    }

    /// Downloads one subscription group again. The selected node, the rules switch and the group picks carry
    /// over where they still apply.
    @discardableResult
    func refreshSubscription(_ groupID: String, trigger: SubscriptionUpdateTrigger = .user) async -> Bool {
        guard let group = subscriptions.group(id: groupID), let link = group.url, let url = URL(string: link) else {
            return false
        }
        return await updateSubscription(url, groupID: groupID, activate: false, trigger: trigger)
    }

    /// Downloads every group that has a link, one after another.
    func refreshAllSubscriptions() async {
        for group in subscriptions.groups where group.isUpdatable {
            await refreshSubscription(group.id)
        }
    }

    /// Automatic update: downloads the groups whose interval has passed. Failures only mark the group, so a
    /// background run never leaves a message on screen. `trigger` is `.automatic` while the app runs and
    /// `.background` in the system's background refresh (`refreshInBackground()`).
    func refreshDueSubscriptions(at date: Date = Date(), trigger: SubscriptionUpdateTrigger = .automatic) async {
        // A group already downloading (the user pulled to refresh, or a background run that is still going) is not
        // started twice, which would count as a failure.
        let due = subscriptions.due(at: date).filter { !updatingGroups.contains($0.id) }
        var failed: [String] = []
        if !due.isEmpty {
            // The whole notice, with its detail and buttons: `message` would keep only the text. Nothing is touched
            // when no group is due, which is every time the app comes back to the front in between.
            let shown = notice
            for group in due {
                let updated = await refreshSubscription(group.id, trigger: trigger)
                if Task.isCancelled {
                    // The system ended the background time: not a failure of the subscription.
                    break
                }
                if !updated {
                    failed.append(group.name)
                }
            }
            // A stop notice that the tunnel has left behind meanwhile does not come back.
            notice = shown.flatMap { $0.origin.applies(whileConnected: isConnected) ? $0 : nil }
        }
        if !failed.isEmpty, AppNotifications.settings.subscriptions {
            // The locale's list format ("A and B", "A、B", "A 和 B"), not a Chinese separator in every language.
            let names = failed.formatted(.list(type: .and))
            AppNotifications.post(
                title: String(localized: "订阅更新失败"),
                body: String(localized: "\(names) 没有更新成功，仍在使用上次的节点。"),
                thread: "subscriptions",
                id: "subscriptions-failed"
            )
        }
        scheduleBackgroundRefresh()
    }

    /// The system's background app refresh. It can launch the app without a window, where `RootView`'s `.task`
    /// never runs, so the saved profile and the groups are read here first; without them nothing would be due and
    /// the next background run would not be asked for. It uses `RootView`'s condition: a run before the first unlock
    /// leaves `loaded` true with the groups unread (`subscriptionsRead`), and a later run in the same, only
    /// suspended process (the scheduled `.retry`) loads again. It calls `load()` rather than `reloadProfile()`, so
    /// the files `loadNow()` reads (local rules, DNS, redirects, debug mappings) and `appMode`, which were also read
    /// while protected data was unavailable, are read again too.
    func refreshInBackground() async {
        if !loaded || !subscriptionsRead {
            await load()
        }
        await refreshDueSubscriptions(trigger: .background)
    }

    /// A background update saved new nodes or rules while the tunnel kept running the old ones
    /// (`SubscriptionUpdateTrigger.background`): reconnects it now that the app is in front, unless the tunnel has
    /// come up again since, which already runs the saved profile (`SubscriptionRefresh.pendingReconnectApplies`).
    func reconnectIfPending() {
        let defaults = UserDefaults.standard
        guard let stamp = defaults.object(forKey: SubscriptionRefresh.pendingReconnectKey) as? Double else {
            return
        }
        defaults.removeObject(forKey: SubscriptionRefresh.pendingReconnectKey)
        if SubscriptionRefresh.pendingReconnectApplies(
            marked: Date(timeIntervalSince1970: stamp), connectedSince: connectedSince
        ) {
            restart()
        }
    }

    /// After a write changed what the running tunnel would run (nodes, rules, the active group): the extension reads
    /// them only at start, so a running tunnel reconnects. A background run does not stop the tunnel, as the app may
    /// be suspended before it could start it again; it marks the connection for `reconnectIfPending()` instead.
    private func applyToRunningTunnel(_ trigger: SubscriptionUpdateTrigger) {
        guard isConnected else {
            return
        }
        switch trigger {
        case .user, .automatic:
            if trigger == .user {
                importReconnected = true
            }
            restart()
        case .background:
            if let connectedSince {
                UserDefaults.standard.set(
                    connectedSince.timeIntervalSince1970, forKey: SubscriptionRefresh.pendingReconnectKey
                )
            }
        }
    }

    func renameSubscription(_ groupID: String, to name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard var group = subscriptions.group(id: groupID), !trimmed.isEmpty else {
            return
        }
        group.name = String(trimmed.prefix(SubscriptionGroup.maxNameLength))
        var groups = subscriptions
        groups.upsert(group)
        store(groups)
    }

    /// `hours` 0 updates by hand only.
    func setUpdateInterval(_ groupID: String, hours: Int) {
        guard let group = subscriptions.group(id: groupID) else {
            return
        }
        var groups = subscriptions
        groups.upsert(SubscriptionGroup(
            id: group.id,
            name: group.name,
            library: group.library,
            userInfo: group.userInfo,
            updateIntervalHours: hours,
            lastFailureAt: group.lastFailureAt
        ))
        store(groups)
        scheduleBackgroundRefresh()
    }

    /// Deletes a group. When it was active, the next group takes over and a running tunnel restarts on it.
    func deleteSubscription(_ groupID: String) async {
        await writing {
            var groups = subscriptions
            let wasActive = groups.activeID == groupID
            groups.remove(id: groupID)
            if wasActive, groups.active != nil {
                guard await saveActiveProfile(groups) else {
                    return
                }
            }
            store(groups)
            latencies = latencies.filter { !$0.key.hasPrefix(Self.latencyKey(group: groupID, node: "")) }
            if wasActive, groups.active != nil {
                restart()
            }
        }
    }

    /// Saves an edited node into its group. A running tunnel that may use the node, because its group is active or
    /// the active rules reach other groups, restarts on the edit.
    func updateNode(_ node: NativeImportReport.Node, inGroup groupID: String) async -> Bool {
        await writing { () async -> Bool in
            guard var group = subscriptions.group(id: groupID), let old = group.library.node(id: node.id) else {
                return false
            }
            guard old != node else {
                return true
            }
            group.library = group.library.replacing(node)
            return await changeGroup(group)
        }
    }

    /// Deletes a node the group's rules do not name, with the nodes that connect through it (`via`), which the user
    /// confirmed first (`NodeLibrary.removal(of:)`, `PendingNodeDeletion`).
    func deleteNode(_ nodeID: String, inGroup groupID: String) async {
        await writing {
            guard var group = subscriptions.group(id: groupID), let dependents = group.library.removal(of: nodeID) else {
                return
            }
            group.library = group.library.removingWithDependents(nodeID)
            for id in [nodeID] + dependents.map(\.id) {
                latencies.removeValue(forKey: Self.latencyKey(group: groupID, node: id))
            }
            _ = await changeGroup(group)
        }
    }

    /// Stores a changed group, saving the profile first when the tunnel may use it; runs in `profileWrites`.
    private func changeGroup(_ group: SubscriptionGroup) async -> Bool {
        var groups = subscriptions
        groups.upsert(group)
        let affectsTunnel = groups.activeID == group.id || library?.routesByRules == true
        if affectsTunnel, groups.active != nil {
            guard await saveActiveProfile(groups) else {
                return false
            }
        }
        store(groups)
        if affectsTunnel {
            restart()
        }
        return true
    }

    /// Switches between the active group's rules and groups and its selected node alone. A running tunnel
    /// restarts on the new setting.
    func setFollowsRules(_ on: Bool) async {
        await setMode(on ? .rule : .global)
    }

    /// Switches between the rule, global and direct modes. A running engine switches in place and drops its open
    /// connections so they reconnect in the new mode; an older engine restarts the tunnel instead.
    func setMode(_ next: ProxyMode) async {
        let saved = await writing { () async -> Bool in
            guard next != mode, manager != nil else {
                return false
            }
            if next == .rule, library == nil {
                message = String(localized: "单个节点没有节点列表，无法按规则分流。导入订阅或多个节点后可用")
                return false
            }
            let outcome: ConfigurationWrite.Outcome<NETunnelProviderManager>
            do {
                outcome = try await writeProfile { target in
                    guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                        return false
                    }
                    var configuration = tunnelProtocol.providerConfiguration ?? [:]
                    configuration[TunnelProfileKeys.mode] = next.rawValue
                    if next == .rule {
                        configuration[TunnelProfileKeys.routing] = true
                    } else if next == .global {
                        configuration.removeValue(forKey: TunnelProfileKeys.routing)
                    }
                    tunnelProtocol.providerConfiguration = configuration
                    target.protocolConfiguration = tunnelProtocol
                    return true
                }
            } catch {
                message = String(localized: "保存模式到 VPN 配置失败") + Self.errorCode(error)
                refresh()
                return false
            }
            // The rule and global modes are also the active subscription's rules switch.
            followSavedMode()
            refresh()
            return outcome.isSaved
        }
        guard saved, isConnected else {
            return
        }
        let answer = RuntimeControl.decode(RuntimeControl.ModeAnswer.self, from: await control(.setMode(next)))
        if answer?.mode == next {
            _ = await control(.closeAll)
        } else {
            restart()
        }
    }

    /// The app's defaults key of `blockQuic`.
    static let blockQuicKey = "networkcore.block-quic"

    static var isMac: Bool {
        #if os(macOS)
        true
        #else
        false
        #endif
    }

    private static func savedTakeover() -> TrafficTakeover {
        let raw = UserDefaults.standard.string(forKey: TrafficTakeover.storageKey) ?? ""
        return (TrafficTakeover(rawValue: raw) ?? .default).supported(macOS: isMac)
    }

    /// Switches between the tunnel interface, the system proxy, or both. The interface settings change with it, so
    /// a running tunnel restarts.
    func setTakeover(_ next: TrafficTakeover) async {
        let next = next.supported(macOS: Self.isMac)
        guard next != takeover else {
            return
        }
        takeover = next
        UserDefaults.standard.set(next.rawValue, forKey: TrafficTakeover.storageKey)
        await saveTakeoverSettings(failure: String(localized: "保存接管方式到 VPN 配置失败"))
    }

    /// Turns the gateway mode on or off (macOS). A running tunnel restarts so the extension turns IPv4 forwarding
    /// on or back off.
    func setGateway(_ on: Bool) async {
        let on = on && Self.isMac
        guard on != gateway else {
            return
        }
        gateway = on
        UserDefaults.standard.set(on, forKey: GatewayMode.storageKey)
        await saveTakeoverSettings(
            failure: String(localized: "保存网关模式到 VPN 配置失败"), restarts: takeover.routesTraffic
        )
    }

    /// Replaces what the system proxy leaves alone. A running tunnel that uses the proxy restarts on it.
    func setProxyExceptions(_ exceptions: ProxyExceptions) async {
        guard exceptions != proxyExceptions else {
            return
        }
        proxyExceptions = exceptions
        UserDefaults.standard.set(exceptions.profileValue, forKey: ProxyExceptions.storageKey)
        await saveTakeoverSettings(
            failure: String(localized: "保存跳过代理的地址到 VPN 配置失败"), restarts: takeover.usesSystemProxy
        )
    }

    /// Writes `takeover`, `proxyExceptions` and `gateway` into the saved profile and restarts a running tunnel on
    /// them.
    private func saveTakeoverSettings(failure: String, restarts: Bool = true) async {
        await profileWrites.run {
            let outcome: ConfigurationWrite.Outcome<NETunnelProviderManager>
            do {
                outcome = try await writeProfile { target in
                    guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                        return false
                    }
                    var configuration = tunnelProtocol.providerConfiguration ?? [:]
                    Self.applyTakeover(takeover, exceptions: proxyExceptions, gateway: gateway, to: &configuration)
                    tunnelProtocol.providerConfiguration = configuration
                    target.protocolConfiguration = tunnelProtocol
                    return true
                }
            } catch {
                message = failure + Self.errorCode(error)
                refresh()
                return
            }
            refresh()
            if restarts, outcome.isSaved {
                restart()
            }
        }
    }

    /// The profile entries for `takeover`, `exceptions` and `gateway`; the defaults are left out.
    static func applyTakeover(
        _ takeover: TrafficTakeover,
        exceptions: ProxyExceptions,
        gateway: Bool,
        to configuration: inout [String: Any]
    ) {
        configuration[TunnelProfileKeys.takeover] = takeover.rawValue
        if gateway {
            configuration[TunnelProfileKeys.gateway] = true
        } else {
            configuration.removeValue(forKey: TunnelProfileKeys.gateway)
        }
        if exceptions.isDefault {
            configuration.removeValue(forKey: TunnelProfileKeys.proxyExceptions)
        } else {
            configuration[TunnelProfileKeys.proxyExceptions] = exceptions.profileValue
        }
    }

    /// Turns QUIC blocking on or off. A running engine switches in place and closes its open UDP flows to port 443,
    /// so apps already on QUIC fall back to TCP at once; an older engine restarts the tunnel instead.
    func setBlockQuic(_ on: Bool) async {
        guard on != blockQuic else {
            return
        }
        blockQuic = on
        UserDefaults.standard.set(on, forKey: Self.blockQuicKey)
        let saved = await profileWrites.run { () async -> Bool in
            let outcome: ConfigurationWrite.Outcome<NETunnelProviderManager>
            do {
                outcome = try await writeProfile { target in
                    guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                        return false
                    }
                    var configuration = tunnelProtocol.providerConfiguration ?? [:]
                    if on {
                        configuration[TunnelProfileKeys.blockQuic] = true
                    } else {
                        configuration.removeValue(forKey: TunnelProfileKeys.blockQuic)
                    }
                    tunnelProtocol.providerConfiguration = configuration
                    target.protocolConfiguration = tunnelProtocol
                    return true
                }
            } catch {
                message = String(localized: "保存 QUIC 设置到 VPN 配置失败") + Self.errorCode(error)
                refresh()
                return false
            }
            refresh()
            return outcome.isSaved
        }
        guard saved, isConnected else {
            return
        }
        let answer = RuntimeControl.decode(RuntimeControl.BlockQuicAnswer.self, from: await control(.setBlockQuic(on)))
        guard answer?.blockQuic == on else {
            restart()
            return
        }
        if on, let list = RuntimeControl.decode(RuntimeControl.Connections.self, from: await control(.connections)) {
            for flow in list.active where flow.network == "udp" && flow.destination.hasSuffix(":443") {
                _ = await control(.closeConnection(flow.id))
            }
        }
    }

    /// Picks the member a `select` group uses. In the policy group of another subscription group this picks that
    /// group's selected node. A tunnel running by the rules restarts on it.
    func selectMember(_ member: String, inGroup groupID: String) async {
        // In `profileWrites`, so the pick does not go between another write's copy of the groups and that write
        // storing them.
        let picked = await profileWrites.run { () -> Bool in
            guard let current = library,
                  let group = current.routing?.groups.first(where: { $0.id == groupID }),
                  group.isSelectable, group.members.contains(member), current.selection(in: group) != member
            else {
                return false
            }
            var groups = subscriptions
            if let link = groups.linkedGroups().first(where: { $0.policyID == groupID }),
               var linked = groups.group(id: link.groupID), member.hasPrefix(link.prefix) {
                linked.library.selectedID = String(member.dropFirst(link.prefix.count))
                groups.upsert(linked)
            } else if var active = groups.active {
                active.library.groupSelections[groupID] = member
                groups.upsert(active)
            } else {
                return false
            }
            store(groups)
            return current.routesByRules
        }
        if picked, isConnected {
            // The engine switches select groups in place; an older one restarts on the new pick.
            let answer = RuntimeControl.decode(RuntimeControl.SelectAnswer.self, from: await control(.select(group: groupID, member: member)))
            if answer?.now != member {
                restart()
            }
        }
    }

    /// Makes a node of any group the tunnel's node; its group becomes active. A running tunnel restarts on it
    /// unless that group's rules pick the nodes. Another tap on the same node while its save waits or runs waits for
    /// that save instead of saving again (`SerialAsyncQueue.run(unlessRepeating:_:)`).
    func selectNode(_ nodeID: String, inGroup groupID: String) async {
        await profileWrites.run(unlessRepeating: Self.latencyKey(group: groupID, node: nodeID)) {
            await selectNodeNow(nodeID, inGroup: groupID)
        }
    }

    /// `selectNode` in `profileWrites`, on the configuration as saved when its turn comes.
    private func selectNodeNow(_ nodeID: String, inGroup groupID: String) async {
        await reloadNow()
        guard var group = subscriptions.group(id: groupID), group.library.node(id: nodeID) != nil else {
            return
        }
        let sameGroup = subscriptions.activeID == groupID
        if sameGroup, group.library.selectedID == nodeID {
            return
        }
        group.library.selectedID = nodeID
        var groups = subscriptions
        groups.upsert(group)
        groups.activeID = groupID
        guard await saveActiveProfile(groups) else {
            return
        }
        store(groups)
        if !(sameGroup && group.library.routesByRules) {
            restart()
        }
    }

    /// Stops a running tunnel and starts it again once it is down, so the extension reads the new profile and node
    /// list. Called after the profile is saved; a tunnel that is not running, or already stopping, is left alone.
    private func restart() {
        switch manager?.connection.status ?? .invalid {
        case .connected, .connecting, .reasserting:
            restartAfterStop = true
            // A second restart before the tunnel came back finds capture already off: keep the first one's wish.
            resumeCapture = resumeCapture || captureOn
            manager?.connection.stopVPNTunnel()
        default:
            break
        }
    }

    /// The `latencies` key of a node in a group; node ids repeat across subscriptions.
    static func latencyKey(group: String, node: String) -> String {
        "\(group)\u{1F}\(node)"
    }

    /// HTTP delay test of every node of every group through its own protocol, a few at a time (`NodeDelayFFI`).
    func testLatency() async {
        // Read once for the whole test: ANX nodes that need a license fail without it.
        let licenses = Self.delayTestLicenses()
        let nodes = subscriptions.groups.flatMap { group -> [(String, NativeImportReport.Node)] in
            let origin = AnxLicense.origin(ofSubscription: group.library.subscriptionURL)
            return group.library.nodes.map { node in
                (
                    Self.latencyKey(group: group.id, node: node.id),
                    node.standalone(among: group.library.nodes).licensed(by: licenses, origin: origin)
                )
            }
        }
        guard !nodes.isEmpty else {
            return
        }
        for (key, _) in nodes {
            latencies[key] = .measuring
        }
        let profileID = Self.profileID
        let batch = 8
        for start in stride(from: 0, to: nodes.count, by: batch) {
            let slice = nodes[start..<min(start + batch, nodes.count)]
            let results = await withTaskGroup(of: (String, NodeLatency).self, returning: [(String, NodeLatency)].self) { group in
                for (key, node) in slice {
                    group.addTask {
                        (key, await NodeDelayFFI.measureOffPool(node, profileID: profileID))
                    }
                }
                var results: [(String, NodeLatency)] = []
                for await result in group {
                    results.append(result)
                }
                return results
            }
            for (key, result) in results {
                latencies[key] = result
            }
        }
    }

    /// 节点体检 of one node of a group through its own protocol (`NodeCheckupFFI`); runs only when the user starts it.
    func checkup(
        _ nodeID: String,
        inGroup groupID: String,
        throughput: Bool
    ) async -> Result<NodeCheckupReport, NodeCheckupFailure> {
        guard let group = subscriptions.groups.first(where: { $0.id == groupID }),
              let node = group.library.nodes.first(where: { $0.id == nodeID })
        else {
            return .failure(.failed)
        }
        return await NodeCheckupFFI.runOffPool(
            node.standalone(among: group.library.nodes).licensed(
                by: Self.delayTestLicenses(),
                origin: AnxLicense.origin(ofSubscription: group.library.subscriptionURL)
            ),
            profileID: Self.profileID,
            throughput: throughput
        )
    }

    /// The ANX license fields for the app's own delay tests and checkups, which run outside the tunnel and so do not
    /// get the fields the tunnel adds at start: each subscription origin's license written to the app's own
    /// Application Support (the app cannot read the tunnel's container), with the device key from the Keychain. Nil
    /// before any subscription offered ANX licensing. The fields go only into the request sent at once
    /// (`NativeImportReport.Node.licensed`), never into the saved nodes.
    private static func delayTestLicenses() -> OutboundLicenses? {
        guard let value = try? NodeSecretStore.value(account: AnxLicense.keychainAccount),
              let state = try? AnxLicense.fromKeychainValue(value),
              state.offered,
              let support = try? FileManager.default
                  .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        else {
            return nil
        }
        let directory = support.appendingPathComponent("anx-licenses", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return OutboundLicenses(fieldsByOrigin: state.writeLicenseFiles(in: directory).fieldsByOrigin)
    }

    /// Downloads a subscription into the group `groupID`, or a new group when nil. `activate` makes the group the
    /// tunnel's; a refresh of the active group saves the profile only when its node or rules switch changed.
    private func updateSubscription(
        _ url: URL,
        groupID: String?,
        activate: Bool,
        trigger: SubscriptionUpdateTrigger
    ) async -> Bool {
        if let groupID {
            guard !updatingGroups.contains(groupID) else {
                return false
            }
            updatingGroups.insert(groupID)
        }
        defer {
            if let groupID {
                updatingGroups.remove(groupID)
            }
        }
        let text: String
        let response: SubscriptionResponseInfo
        do {
            let downloaded = try await Self.download(url)
            text = downloaded.0
            response = downloaded.1
            // The download may have saved a new ANX license.
            handOverStartSecrets()
        } catch {
            if Task.isCancelled || (error as? URLError)?.code == .cancelled {
                // The background time ran out, or the download was cancelled: the subscription did not fail.
                return false
            }
            await markFailure(groupID)
            if trigger == .user {
                notice = AppNotice(
                    text: Self.isPlainHTTPRefused(error)
                        ? Self.plainHTTPSubscriptionText
                        : String(localized: "订阅下载失败：链接可能已失效，或当前网络连不上订阅服务器。现有节点仍可使用。"),
                    detail: error.localizedDescription,
                    actions: groupID.map { [.refreshSubscription($0)] } ?? []
                )
            }
            return false
        }
        let report: NativeImportReport
        do {
            report = try NativeImportReport.decoded(from: NativeNodeImporterFFI.importNodes(text))
        } catch let failure as NativeImportFailure {
            await markFailure(groupID)
            if trigger == .user {
                message = Self.describe(.native(failure))
            }
            return false
        } catch {
            await markFailure(groupID)
            if trigger == .user {
                message = Self.describe(.native(.unavailable))
            }
            return false
        }
        return await writing { () async -> Bool in
            await merge(report, response: response, url: url, groupID: groupID, activate: activate, trigger: trigger)
        }
    }

    /// Puts a downloaded subscription into its group and saves the profile when the tunnel's node or rules switch
    /// changed; runs in `profileWrites`.
    private func merge(
        _ report: NativeImportReport,
        response: SubscriptionResponseInfo,
        url: URL,
        groupID: String?,
        activate: Bool,
        trigger: SubscriptionUpdateTrigger
    ) async -> Bool {
        // Before the groups are read, the downloaded group would replace every saved one.
        guard subscriptionsRead else {
            if trigger == .user {
                message = Self.subscriptionsUnreadText
            }
            return false
        }
        // A group deleted while it downloaded stays deleted: the result is dropped instead of coming back as a new
        // group (third-party review F08).
        if let groupID, subscriptions.group(id: groupID) == nil {
            return false
        }
        var groups = subscriptions
        let previous = groups.activeLibrary()
        let previousActiveID = groups.activeID
        let group = groupID.flatMap { groups.group(id: $0) }?.refreshed(with: report, response: response)
            ?? SubscriptionGroup(report: report, url: url.absoluteString, response: response)
        guard group.library.selected != nil else {
            failed(groupID)
            if trigger == .user {
                message = Self.describe(.native(.noSupportedNode(
                    protocols: report.unsupported.map(\.protocol),
                    reasons: []
                )))
            }
            return false
        }
        groups.upsert(group)
        if activate {
            groups.activeID = group.id
        }
        if groups.activeID == group.id {
            // A refresh that keeps the same node and rules switch leaves the saved profile and the running tunnel
            // alone.
            let next = groups.activeLibrary()
            let unchanged = previousActiveID == group.id
                && previous?.selected == next?.selected
                && previous?.routesByRules == next?.routesByRules
            // Without a saved profile an automatic update only keeps the nodes: the connect saves the profile
            // (`connectNow`), and no update in the background asks to add a VPN configuration.
            if !unchanged, manager != nil || trigger == .user {
                guard await saveActiveProfile(groups) else {
                    return false
                }
            }
        }
        // Other groups may have changed while this one downloaded or its profile was saved; one deleted meanwhile
        // stays deleted.
        var latest = subscriptions
        if let groupID, latest.group(id: groupID) == nil {
            return false
        }
        latest.upsert(group)
        latest.activeID = groups.activeID
        store(latest)
        latencies = latencies.filter { !$0.key.hasPrefix(Self.latencyKey(group: group.id, node: "")) }
        // The running tunnel took its node and library at start; one that would now run something else (another
        // node, changed nodes or rules, a new active group) reconnects, after `store` saved what the start reads.
        if SubscriptionRefresh.changesRunningTunnel(
            previous: previous,
            next: latest.activeLibrary(),
            previousActiveID: previousActiveID,
            nextActiveID: latest.activeID
        ) {
            applyToRunningTunnel(trigger)
        }
        return true
    }

    /// Marks a group whose update failed, in `profileWrites` so it does not go between another write's copy of the
    /// groups and that write storing them.
    private func markFailure(_ groupID: String?) async {
        await profileWrites.run {
            failed(groupID)
        }
    }

    /// `markFailure` for code already running in `profileWrites`.
    private func failed(_ groupID: String?) {
        guard let groupID, var group = subscriptions.group(id: groupID) else {
            return
        }
        group.lastFailureAt = Date()
        var groups = subscriptions
        groups.upsert(group)
        store(groups)
    }

    /// Saves the profile for the active group's selected node, routed when that group follows its rules.
    private func saveActiveProfile(_ groups: SubscriptionGroups) async -> Bool {
        guard let library = groups.activeLibrary(), let selected = library.selected else {
            message = String(localized: "没有可用的节点")
            return false
        }
        let profile: TunnelProfile
        do {
            profile = try TunnelProfile(node: selected.standalone(among: library.nodes), profileID: Self.profileID)
        } catch {
            message = String(localized: "无法生成隧道配置")
            return false
        }
        return await save(profile, routed: library.routesByRules)
    }

    /// Subscription bodies are text; the importer refuses anything larger than this.
    private static let maxSubscriptionBytes = 4 * 1024 * 1024

    /// A subscription body and its headers. A panel that licenses ANX (anx spec §2.4.1) gets this device's key and
    /// answers with its license (`AnxLicense`); the first time it says so, the request goes once more. A license
    /// problem never fails the subscription.
    private static func download(_ url: URL) async throws -> (String, SubscriptionResponseInfo) {
        let stored = (try? NodeSecretStore.value(account: AnxLicense.keychainAccount)).flatMap { try? AnxLicense.fromKeychainValue($0) }
        var license = stored
        var (data, http) = try await fetch(url, device: license?.requestHeader(for: url))
        if recordAnxLicense(&license, url: url, http: http),
           let retried = try? await fetch(url, device: license?.requestHeader(for: url))
        {
            (data, http) = retried
            _ = recordAnxLicense(&license, url: url, http: http)
        }
        if license != stored, let value = try? license?.keychainValue() {
            _ = try? NodeSecretStore.save(value, account: AnxLicense.keychainAccount)
        }
        let response = http
        return (String(decoding: data, as: UTF8.self), SubscriptionResponseInfo(header: { response.value(forHTTPHeaderField: $0) }))
    }

    private static func fetch(_ url: URL, device: String?) async throws -> (Data, HTTPURLResponse) {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 30)
        request.setValue("AnixOpsNet/1.0", forHTTPHeaderField: "User-Agent")
        if let device {
            request.setValue(device, forHTTPHeaderField: AnxLicense.headerDevice)
        }
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode),
              data.count <= maxSubscriptionBytes
        else {
            throw URLError(.badServerResponse)
        }
        return (data, http)
    }

    /// Takes in the license headers; the device key is made the first time a server offers licensing. True when
    /// this origin offered it for the first time.
    private static func recordAnxLicense(_ license: inout AnxLicense?, url: URL, http: HTTPURLResponse) -> Bool {
        let header = { (name: String) in http.value(forHTTPHeaderField: name) }
        if license == nil {
            guard let ca = header(AnxLicense.headerCA), !ca.trimmingCharacters(in: .whitespaces).isEmpty else {
                return false
            }
            license = AnxLicense()
        }
        return license?.record(url: url, header: header) ?? false
    }

    /// Saves the groups and the library the extension reads, both to the Keychain. Refused while the saved groups
    /// are unread (`subscriptionsRead`): `groups` was then built on an empty list and would overwrite them.
    private func store(_ groups: SubscriptionGroups) {
        guard subscriptionsRead else {
            message = Self.subscriptionsUnreadText
            return
        }
        let next = groups.activeLibrary()
        subscriptions = groups
        library = next
        if let value = try? groups.keychainValue() {
            _ = try? NodeSecretStore.save(value, account: SubscriptionGroups.keychainAccount)
        }
        if let next, let value = try? next.keychainValue() {
            _ = try? NodeSecretStore.save(value, account: NodeLibrary.keychainAccount)
        } else {
            try? NodeSecretStore.delete(account: NodeLibrary.keychainAccount)
        }
        handOverStartSecrets()
    }

    /// iOS runs `refreshDueSubscriptions` in the background around the next group's update time
    /// (`NetworkCoreAppMain`). Nothing to schedule when every group updates by hand.
    func scheduleBackgroundRefresh() {
        #if os(iOS)
        let begin: Date
        switch SubscriptionRefresh.backgroundPlan(
            groupsRead: subscriptionsRead, nextUpdate: subscriptions.nextUpdate, now: Date()
        ) {
        case .cancel:
            BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: Self.backgroundRefreshID)
            return
        case let .retry(date), let .submit(date):
            begin = date
        }
        // A new request replaces the one already submitted under the same identifier.
        let request = BGAppRefreshTaskRequest(identifier: Self.backgroundRefreshID)
        request.earliestBeginDate = begin
        try? BGTaskScheduler.shared.submit(request)
        #endif
    }

    /// Listed under `BGTaskSchedulerPermittedIdentifiers` in project.yml.
    static let backgroundRefreshID = "net.anixops.subscriptions.refresh"

    /// Saves `profile` as the tunnel profile; `routed` makes the extension follow the node library's rules. Runs in
    /// `profileWrites`.
    private func save(_ requested: TunnelProfile, routed requestedRouted: Bool = false) async -> Bool {
        let profileID = Self.profileID
        // Debug mode with the direct exit keeps the direct profile whichever node is picked or refreshed; the
        // picks are kept in the subscription groups for when the exit goes through a node. (Not `usesDirectExit`:
        // the first import is saved before its group is stored, while there are no nodes yet.)
        let direct = appMode == .debug && debugExit == .direct
        let profile = direct && !requested.isDirect
            ? TunnelProfile.direct(profileID: profileID, displayName: String(localized: "直连"))
            : requested
        let routed = requestedRouted && !profile.isDirect
        guard let providerBundleIdentifier = Self.tunnelBundleIdentifier() else {
            message = String(localized: "找不到隧道扩展")
            return false
        }

        let config: Data
        let interface: Data
        let keyReference: Data
        do {
            config = try profile.config.withoutSecret().encoded()
            interface = try profile.interface.encoded()
        } catch {
            message = String(localized: "无法生成隧道配置")
            return false
        }
        // Each save writes the key under a version of its own and the profile names that version
        // (`TunnelProfileKeys.nodeKeyVersion`). The profile in the system settings keeps reading its own key whatever
        // happens before the new profile is saved: a failure, or the app being stopped in between
        // (third-party review F05). Older versions are removed only once the new profile is saved.
        let keyValue: String
        let keyVersion = UUID().uuidString
        do {
            keyValue = try profile.config.secret.keychainValue()
            keyReference = try NodeSecretStore.save(keyValue, account: NodeSecretStore.versionedAccount(keyVersion))
        } catch {
            message = String(localized: "无法把节点密钥保存到钥匙串")
            return false
        }
        keyVersionsInFlight.insert(keyVersion)
        defer {
            keyVersionsInFlight.remove(keyVersion)
        }

        var providerConfiguration: [String: Any] = [
            TunnelProfileKeys.runtimeConfig: config,
            TunnelProfileKeys.tunnelInterface: interface,
            TunnelProfileKeys.nodeName: profile.displayName,
            TunnelProfileKeys.nodeKeyVersion: keyVersion,
        ]
        if routed {
            providerConfiguration[TunnelProfileKeys.routing] = true
        }
        if let value = try? dns.profileValue() {
            providerConfiguration[TunnelProfileKeys.dns] = value
        }
        if let value = try? redirects.profileValue() {
            providerConfiguration[TunnelProfileKeys.redirects] = value
        }
        if blockQuic {
            providerConfiguration[TunnelProfileKeys.blockQuic] = true
        }
        Self.applyTakeover(takeover, exceptions: proxyExceptions, gateway: gateway, to: &providerConfiguration)
        if let value = try? localRules.profileValue() {
            providerConfiguration[TunnelProfileKeys.localRules] = value
        }
        providerConfiguration[TunnelProfileKeys.notifications] = AppNotifications.settings.profileValue
        Self.applyMitmRootFingerprint(Self.currentMitmRootFingerprint(), to: &providerConfiguration)
        do {
            try Self.savedMitmBundle(mappings: debugMappings).apply(to: &providerConfiguration)
        } catch {
            message = String(localized: "保存插件到 VPN 配置失败") + Self.errorCode(error)
            return false
        }

        // Saving the profile does not need the extension to be running, only connecting does. While macOS waits for
        // the user to allow it, the import goes on and the notice says what to do.
        guard await activateSystemExtension(allowPending: true) else {
            return false
        }
        // A node list without subscription rules keeps the rule mode of its local rules (`ProxyMode.forNewProfile`).
        let localRuleList = library != nil && library?.routing == nil
        let outcome: ConfigurationWrite.Outcome<NETunnelProviderManager>
        do {
            outcome = try await writeProfile(creating: true) { target in
                // The mode and On Demand stay as saved now, whoever set them: the app, Shortcuts, Control Center,
                // the widget, the Live Activity or Settings.
                let current = ProxyMode.saved(
                    in: (target.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration
                )
                var configuration = providerConfiguration
                configuration[TunnelProfileKeys.mode] = ProxyMode.forNewProfile(
                    current: current,
                    routed: routed,
                    direct: profile.isDirect,
                    localRuleList: localRuleList
                ).rawValue
                let tunnelProtocol = NETunnelProviderProtocol()
                tunnelProtocol.providerBundleIdentifier = providerBundleIdentifier
                tunnelProtocol.serverAddress = profile.serverAddress
                tunnelProtocol.passwordReference = keyReference
                tunnelProtocol.providerConfiguration = configuration
                target.protocolConfiguration = tunnelProtocol
                target.localizedDescription = "AnixOps Net"
                target.isEnabled = true
                // A saved profile keeps running On Demand while it is on; a new one starts it with the first connect.
                onDemand.apply(to: target, active: target.isOnDemandEnabled)
                return true
            }
        } catch {
            message = String(localized: "保存 VPN 配置失败或未获授权") + Self.errorCode(error)
            refresh()
            return false
        }
        // The version the saved profile names, read back from the system settings.
        let savedVersion = (outcome.object?.protocolConfiguration as? NETunnelProviderProtocol)?
            .providerConfiguration?[TunnelProfileKeys.nodeKeyVersion] as? String
        if savedVersion == keyVersion {
            // The fixed account follows the saved profile for readers from before key versions.
            _ = try? NodeSecretStore.save(keyValue, account: profileID)
        }
        // Keys no saved profile names any more go: all but the saved profile's and those of saves still on their way.
        // A tunnel that is starting may still be reading the key of the profile it started with, so while it is,
        // they wait for the next save.
        if manager?.connection.status != .connecting {
            var kept = keyVersionsInFlight
            if let savedVersion {
                kept.insert(savedVersion)
            }
            NodeSecretStore.removeNodeKeys(keeping: kept)
        }
        // Keep the hint to allow the extension: the profile is saved, but connecting still needs it.
        message = systemExtensionPending ? Self.systemExtensionApprovalText : nil
        refresh()
        return true
    }

    /// The system error's domain and code after a failed save, so a screenshot of the notice says why
    /// (for example `NEVPNErrorDomain 5` is a read/write failure or a refused permission).
    private static func errorCode(_ error: Error) -> String {
        let error = error as NSError
        return " (\(error.domain) \(error.code))"
    }

    /// Puts the enabled HTTPS decryption plugins and the scripts the user allowed in them into the saved profile.
    /// Without a profile there is nothing to update: the next import picks them up. The extension reads them only
    /// at start, so a running tunnel restarts on them. Returns whether the profile changed.
    @discardableResult
    func applyMitmPlugins(
        _ texts: [String],
        scripts: [String: String] = [:],
        resources: [String: Data] = [:]
    ) async -> Bool {
        // The Map Local / Map Remote entries ride along as one more plugin (`DebugMappings.pluginText`), once
        // decryption has been confirmed when that plugin decrypts anything (`DecryptionConsent.profilePlugin`).
        let plugins = texts + [Self.mappingsPlugin(debugMappings)].compactMap { $0 }
        let next = MitmProfileBundle(plugins: plugins, scripts: scripts, resources: resources)
        return await profileWrites.run { () async -> Bool in
            let outcome: ConfigurationWrite.Outcome<NETunnelProviderManager>
            do {
                outcome = try await writeProfile { target in
                    guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                        return false
                    }
                    var configuration = tunnelProtocol.providerConfiguration ?? [:]
                    let carriesOldKeys = configuration[TunnelProfileKeys.mitmPlugins] != nil
                        || configuration[TunnelProfileKeys.mitmScripts] != nil
                        || configuration[TunnelProfileKeys.mitmResources] != nil
                    guard carriesOldKeys || MitmProfileBundle.fromProfile(configuration) != next else {
                        return false
                    }
                    try next.apply(to: &configuration)
                    tunnelProtocol.providerConfiguration = configuration
                    target.protocolConfiguration = tunnelProtocol
                    return true
                }
            } catch {
                message = String(localized: "保存插件到 VPN 配置失败") + Self.errorCode(error)
                refresh()
                return false
            }
            refresh()
            guard outcome.isSaved else {
                return false
            }
            restart()
            return true
        }
    }

    /// The plugins, allowed scripts and downloaded bodies saved on the device, with the debug mappings' plugin
    /// after the imported ones: what the tunnel profile carries.
    private static func savedMitmBundle(mappings: DebugMappings) -> MitmProfileBundle {
        let library = (try? MitmPluginLibrary.load(from: MitmPluginLibrary.defaultURL)) ?? MitmPluginLibrary()
        var bundle = MitmProfileBundle()
        if !library.activeTexts.isEmpty {
            let scripts = (try? MitmScriptLibrary.load(from: MitmScriptLibrary.defaultURL)) ?? MitmScriptLibrary()
            let resources = (try? MitmResourceLibrary.load(from: MitmResourceLibrary.defaultURL)) ?? MitmResourceLibrary()
            bundle = MitmProfileBundle(
                plugins: library.activeTexts,
                scripts: (try? scripts.sources(for: library.activeScriptReferences)) ?? [:],
                resources: resources.data(for: library.activeDataReferences.map(\.path))
            )
        }
        if let text = mappingsPlugin(mappings) {
            bundle.plugins.append(text)
        }
        return bundle
    }

    /// Whether HTTPS decryption as it is set up now (`library`'s enabled plugins under its switch, with the debug
    /// mappings' plugin as the profile carries it) is too large for the engine config of this version, so the
    /// tunnel will leave decryption out when it connects (`EngineBudget.decryptionExceedsConfig`). The plugins and
    /// mappings pages warn with it before connecting rather than only after.
    func decryptionExceedsConfig(with library: MitmPluginLibrary) -> Bool {
        let texts = library.activeTexts + [Self.mappingsPlugin(debugMappings)].compactMap { $0 }
        return EngineBudget.decryptionExceedsConfig(texts)
    }

    /// The plugins and mappings pages' warning for `decryptionExceedsConfig(with:)`.
    static var decryptionExceedsConfigWarning: String {
        String(localized: "已启用的插件和调试映射合计超过 64 KB，超出本版本隧道配置的上限：连接时 HTTPS 解密会被关闭，插件和映射都不会生效。请停用一些插件或减少映射响应体。")
    }

    /// The debug mappings' plugin as the profile may carry it: one that decrypts a domain only after the HTTPS
    /// decryption confirmation (`DecryptionConsent.profilePlugin`).
    private static func mappingsPlugin(_ mappings: DebugMappings) -> String? {
        DecryptionConsent.profilePlugin(
            for: mappings,
            accepted: UserDefaults.standard.string(forKey: DecryptionConsent.storageKey)
        )
    }

    // MARK: - MITM root changes

    /// The fingerprint of the MITM root in the Keychain now, or nil without a readable one.
    static func currentMitmRootFingerprint() -> String? {
        (try? MitmCertificateStore.load())?.sha256Fingerprint
    }

    /// Writes `fingerprint` as the profile's `TunnelProfileKeys.mitmRootFingerprint`, or removes it without a root.
    private static func applyMitmRootFingerprint(_ fingerprint: String?, to configuration: inout [String: Any]) {
        if let fingerprint {
            configuration[TunnelProfileKeys.mitmRootFingerprint] = fingerprint
        } else {
            configuration.removeValue(forKey: TunnelProfileKeys.mitmRootFingerprint)
        }
    }

    /// The MITM root was generated or replaced. The extension reads the root only at start, so a running tunnel
    /// would keep issuing certificates from the old one, which the person is about to stop trusting. This records
    /// the new root's fingerprint in the profile (a macOS start without the app's values then fails on an older kept
    /// copy, `TunnelStartSecrets.mitmRootFailureReason`, and the app starts it again with options), hands a running
    /// macOS extension the new values for the starts the system makes itself, and reconnects a running tunnel that
    /// decrypts anything. Call it after the new root is saved and before the old one's trust is taken back.
    func rootCertificateChanged() async {
        let fingerprint = Self.currentMitmRootFingerprint()
        let decrypts: Bool = await writing { () async -> Bool in
            do {
                // A saved write hands the new values over (`writeProfile`).
                try await writeProfile { target in
                    guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                        return false
                    }
                    var configuration = tunnelProtocol.providerConfiguration ?? [:]
                    guard configuration[TunnelProfileKeys.mitmRootFingerprint] as? String != fingerprint else {
                        return false
                    }
                    Self.applyMitmRootFingerprint(fingerprint, to: &configuration)
                    tunnelProtocol.providerConfiguration = configuration
                    target.protocolConfiguration = tunnelProtocol
                    return true
                }
            } catch {
                // Not fatal: the restart below still hands a running macOS tunnel the new root; only a later start
                // without the app's values could not tell its kept copy is older.
                message = String(localized: "无法把新根证书记录到 VPN 配置") + Self.errorCode(error)
            }
            refresh()
            let configuration = (manager?.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration
            return MitmProfileBundle.referencesPayload(configuration)
                || configuration?[TunnelProfileKeys.mitmPlugins] != nil
        }
        guard isConnected, decrypts else {
            return
        }
        #if os(macOS)
        await handOverStartSecretsBeforeRestart()
        #endif
        restart()
    }

    #if os(macOS)
    /// `handOverStartSecrets()`, waiting for the extension's answer, so that a start the system makes without the
    /// app's values right after `restart()` (On Demand winning the race against the app's own start) finds the new
    /// copy rather than failing on the old one and waiting for `startAfterStaleCopy`.
    private func handOverStartSecretsBeforeRestart() async {
        #if DEBUG
        if Self.isDemo {
            return
        }
        #endif
        guard let manager, let session = manager.connection as? NETunnelProviderSession, isConnected,
              let data = TunnelStartSecrets.fromKeychain(nodeKeyVersion: Self.nodeKeyVersion(of: manager)).encoded()
        else {
            return
        }
        let answer: Data? = await withCheckedContinuation { continuation in
            do {
                try session.sendProviderMessage(TunnelAppMessage.startSecrets(data).encoded) { answer in
                    continuation.resume(returning: answer)
                }
            } catch {
                continuation.resume(returning: nil)
            }
        }
        handedOverStartSecrets = TunnelStartSecrets.handOverAccepted(answer) ? data : nil
    }
    #endif

    // MARK: - HTTPS decryption confirmation

    /// Whether the HTTPS decryption terms were confirmed on this installation (`DecryptionConsent`).
    var decryptionConfirmed: Bool {
        !DecryptionConsent.needsConsent(accepted: UserDefaults.standard.string(forKey: DecryptionConsent.storageKey))
    }

    /// True at once when the HTTPS decryption terms were confirmed before; otherwise shows the confirmation (TERMS.md
    /// section 4) and returns the person's answer. Every way of starting decryption asks here first: the plugins'
    /// master switch, and the debug mappings through `allowsDecryption(of:)`.
    func requireDecryptionConsent() async -> Bool {
        if decryptionConfirmed {
            return true
        }
        return await withCheckedContinuation { continuation in
            decryptionConsentWaiters.append(continuation)
            asksDecryptionConsent = true
        }
    }

    /// The person's answer to the confirmation. Confirming stores the terms version in force and lets every waiting
    /// action go ahead; cancelling stops them all. Only the first answer counts.
    func answerDecryptionConsent(_ confirmed: Bool) {
        guard asksDecryptionConsent else {
            return
        }
        if confirmed {
            UserDefaults.standard.set(TermsConsent.version, forKey: DecryptionConsent.storageKey)
        }
        asksDecryptionConsent = false
        let waiters = decryptionConsentWaiters
        decryptionConsentWaiters = []
        for waiter in waiters {
            waiter.resume(returning: confirmed)
        }
    }

    /// Whether `next` may replace the saved debug mappings: asks for the confirmation when it decrypts a domain they
    /// do not and it was never given (`DecryptionConsent.isRequired`). An editor sheet calls it before it closes, so
    /// the alert shows over the sheet; `applyDebugMappings` calls it for every other change.
    func allowsDecryption(of next: DebugMappings) async -> Bool {
        guard DecryptionConsent.isRequired(
            from: debugMappings,
            to: next,
            accepted: UserDefaults.standard.string(forKey: DecryptionConsent.storageKey)
        ) else {
            return true
        }
        return await requireDecryptionConsent()
    }

    /// An editor sheet that can show the confirmation appeared (`decryptionConsentAlert(_:inSheet:)`).
    func addDecryptionConsentSheet(_ id: UUID) {
        if !decryptionConsentSheets.contains(id) {
            decryptionConsentSheets.append(id)
        }
    }

    /// That sheet went away.
    func removeDecryptionConsentSheet(_ id: UUID) {
        decryptionConsentSheets.removeAll { $0 == id }
    }

    /// Mappings saved by a build that did not ask (or before the confirmation was added) can name domains to
    /// decrypt while it was never given. Asks once per launch; confirming puts their plugin in the profile, and
    /// cancelling takes it out of a profile that still carries it, so nothing is decrypted without the confirmation.
    private func confirmSavedDecryption() async {
        guard !askedSavedDecryption, !decryptionConfirmed, !debugMappings.decryptedPatterns.isEmpty else {
            return
        }
        askedSavedDecryption = true
        // The user agreement comes first; until it is accepted the profile just drops the plugin.
        let termsAccepted = !TermsConsent.needsConsent(
            accepted: UserDefaults.standard.string(forKey: TermsConsent.storageKey)
        )
        if termsAccepted, await requireDecryptionConsent() {
            await applyDebugMappings(debugMappings)
            return
        }
        await reapplyMitmBundle()
    }

    /// Writes the plugins saved on the device and the mappings' plugin into the profile again.
    private func reapplyMitmBundle() async {
        // `applyMitmPlugins` adds the mappings' plugin itself.
        let imported = Self.savedMitmBundle(mappings: DebugMappings())
        await applyMitmPlugins(imported.plugins, scripts: imported.scripts, resources: imported.resources)
    }

    /// Throws a `DebugMappingsError` when `next`, replacing the saved mappings, makes their plugin larger and over
    /// its own limit or, with the enabled plugins, over the engine's decryption budget. An editor sheet calls it
    /// before it closes, so the reason shows in the sheet.
    func checkDebugMappingsBudget(_ next: DebugMappings) throws {
        // Only the plugins handed over now: with the switch off, turning it on checks the mappings in turn
        // (`PluginsModel.setMasterEnabled`).
        let library = (try? MitmPluginLibrary.load(from: MitmPluginLibrary.defaultURL)) ?? MitmPluginLibrary()
        try next.checkBudget(replacing: debugMappings, importedBytes: EngineBudget.decryptionBytes(library.activeTexts))
    }

    /// Saves the Map Local / Map Remote entries and puts their plugin in the saved profile; a running tunnel
    /// reconnects to use them. A change that decrypts a new domain first needs the HTTPS decryption confirmation
    /// (`allowsDecryption(of:)`); without it nothing is saved. Returns whether they were saved.
    @discardableResult
    func applyDebugMappings(_ next: DebugMappings) async -> Bool {
        // The mappings' plugin shares the engine's decryption budget with the enabled plugins; every change that
        // makes it larger is checked here, whichever view made it (toggles and moves included).
        do {
            try checkDebugMappingsBudget(next)
        } catch {
            message = (error as? DebugMappingsError)?.message ?? String(localized: "无法保存映射")
            return false
        }
        guard await allowsDecryption(of: next) else {
            return false
        }
        do {
            try next.save(to: DebugMappings.defaultURL)
        } catch {
            message = String(localized: "无法保存映射")
            return false
        }
        debugMappings = next
        await reapplyMitmBundle()
        return true
    }

    /// Saves the local rules now, so quick edits build on each other, then puts them in the saved profile. A running
    /// tunnel takes them in place when only rule lines changed (see `applyLocalRulesToProfile`), else reconnects.
    func updateLocalRules(_ next: LocalRules) {
        do {
            try next.save(to: LocalRules.defaultURL)
        } catch {
            message = String(localized: "无法保存本地规则")
            return
        }
        localRules = next
        Task {
            await applyLocalRulesToProfile()
        }
    }

    /// Puts the current local rules in the saved profile and reconnects a running tunnel when they changed.
    private func applyLocalRulesToProfile() async {
        // The rules as they are when this write's turn comes, so the last of quick edits is the one saved.
        let saved = await profileWrites.run { () async -> Data? in
            guard let value = try? localRules.profileValue() else {
                return nil
            }
            let outcome: ConfigurationWrite.Outcome<NETunnelProviderManager>
            do {
                outcome = try await writeProfile { target in
                    // Without a profile the next import picks them up.
                    guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                        return false
                    }
                    var configuration = tunnelProtocol.providerConfiguration ?? [:]
                    guard configuration[TunnelProfileKeys.localRules] as? Data != value else {
                        return false
                    }
                    configuration[TunnelProfileKeys.localRules] = value
                    tunnelProtocol.providerConfiguration = configuration
                    target.protocolConfiguration = tunnelProtocol
                    return true
                }
            } catch {
                message = String(localized: "保存本地规则到 VPN 配置失败") + Self.errorCode(error)
                refresh()
                return nil
            }
            refresh()
            return outcome.isSaved ? value : nil
        }
        guard let saved, isConnected else {
            return
        }
        // Rule lines alone swap into the running engine without dropping a connection; anything more (a new rule
        // list URL, an older extension, a refused line) restarts the tunnel as before.
        if let answer = await reloadLocalRules(saved), answer.reloaded {
            lastRulesReload = Date()
        } else {
            restart()
        }
    }

    /// The running tunnel's session, for messages that need no answer.
    var tunnelSession: NETunnelProviderSession? {
        manager?.connection as? NETunnelProviderSession
    }

    /// Puts the notification settings in the saved profile for the next start. No restart: a running extension
    /// gets them as a message (`applyNotificationSettings`).
    func saveNotificationSettingsToProfile(_ next: NotificationSettings) async {
        await profileWrites.run {
            _ = try? await writeProfile { target in
                guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                    return false
                }
                var configuration = tunnelProtocol.providerConfiguration ?? [:]
                guard configuration[TunnelProfileKeys.notifications] as? Data != next.profileValue else {
                    return false
                }
                configuration[TunnelProfileKeys.notifications] = next.profileValue
                tunnelProtocol.providerConfiguration = configuration
                target.protocolConfiguration = tunnelProtocol
                return true
            }
            refresh()
        }
    }

    /// Hands new local rules to the running extension (`TunnelAppMessage.reloadLocalRules`). Nil when the tunnel is
    /// not connected or the extension does not answer.
    private func reloadLocalRules(_ value: Data) async -> RuntimeControl.ReloadAnswer? {
        guard let session = manager?.connection as? NETunnelProviderSession, connection == .connected else {
            return nil
        }
        let data: Data? = await withCheckedContinuation { continuation in
            do {
                try session.sendProviderMessage(TunnelAppMessage.reloadLocalRules(value).encoded) { data in
                    continuation.resume(returning: data)
                }
            } catch {
                continuation.resume(returning: nil)
            }
        }
        return RuntimeControl.decode(RuntimeControl.ReloadAnswer.self, from: data)
    }

    /// Saves the DNS settings and puts them in the saved profile. A running tunnel reconnects to use them.
    /// Returns whether they were saved.
    @discardableResult
    func applyDns(_ next: DnsSettings) async -> Bool {
        do {
            try next.save(to: DnsSettings.defaultURL)
        } catch {
            message = String(localized: "无法保存 DNS 设置")
            return false
        }
        dns = next
        return await profileWrites.run { () async -> Bool in
            // The settings as they are when this write's turn comes, so the last of quick edits is the one saved.
            let value = try? dns.profileValue()
            let outcome: ConfigurationWrite.Outcome<NETunnelProviderManager>
            do {
                outcome = try await writeProfile { target in
                    // Without a profile the next import picks them up.
                    guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                        return false
                    }
                    var configuration = tunnelProtocol.providerConfiguration ?? [:]
                    guard configuration[TunnelProfileKeys.dns] as? Data != value else {
                        return false
                    }
                    if let value {
                        configuration[TunnelProfileKeys.dns] = value
                    } else {
                        configuration.removeValue(forKey: TunnelProfileKeys.dns)
                    }
                    tunnelProtocol.providerConfiguration = configuration
                    target.protocolConfiguration = tunnelProtocol
                    return true
                }
            } catch {
                message = String(localized: "保存 DNS 设置到 VPN 配置失败") + Self.errorCode(error)
                refresh()
                return false
            }
            refresh()
            if outcome.isSaved {
                restart()
            }
            return true
        }
    }

    /// Saves the destination rewrites and puts the enabled ones in the saved profile. A running tunnel reconnects to
    /// use them. Returns whether they were saved.
    @discardableResult
    func applyRedirects(_ next: TrafficRedirects) async -> Bool {
        do {
            try next.save(to: TrafficRedirects.defaultURL)
        } catch {
            message = String(localized: "无法保存重定向")
            return false
        }
        redirects = next
        return await profileWrites.run { () async -> Bool in
            // The rewrites as they are when this write's turn comes, so the last of quick edits is the one saved.
            let value = try? redirects.profileValue()
            let outcome: ConfigurationWrite.Outcome<NETunnelProviderManager>
            do {
                outcome = try await writeProfile { target in
                    // Without a profile the next import picks them up.
                    guard let tunnelProtocol = target.protocolConfiguration as? NETunnelProviderProtocol else {
                        return false
                    }
                    var configuration = tunnelProtocol.providerConfiguration ?? [:]
                    guard configuration[TunnelProfileKeys.redirects] as? Data != value else {
                        return false
                    }
                    if let value {
                        configuration[TunnelProfileKeys.redirects] = value
                    } else {
                        configuration.removeValue(forKey: TunnelProfileKeys.redirects)
                    }
                    tunnelProtocol.providerConfiguration = configuration
                    target.protocolConfiguration = tunnelProtocol
                    return true
                }
            } catch {
                message = String(localized: "保存重定向到 VPN 配置失败") + Self.errorCode(error)
                refresh()
                return false
            }
            refresh()
            if outcome.isSaved {
                restart()
            }
            return true
        }
    }

    /// Connects unless the tunnel is up, coming up or still stopping: what "Retry" and `anixops://connect` ask for.
    /// Never a disconnect, unlike `toggle()`: a stop card's Retry tapped after On Demand reconnected leaves the
    /// tunnel up.
    func connect() {
        guard !isConnected, connection != .disconnecting else {
            return
        }
        toggle()
    }

    /// Connects, or disconnects while the tunnel is up, as the switch shows it now. The connect or disconnect runs in
    /// `profileWrites`, on the VPN configuration as saved when its turn comes.
    func toggle() {
        let connects = !isConnected
        Task {
            await writing {
                if connects {
                    await connectNow()
                } else {
                    await disconnectNow()
                }
            }
        }
    }

    /// Starts the tunnel unless it is up already; runs in `profileWrites`.
    private func connectNow() async {
        guard !isConnected else {
            return
        }
        guard let manager else {
            if usesDirectExit {
                // Debug mode needs no node: save the direct profile, then connect.
                if await saveDirectProfile() {
                    await start()
                }
            } else if let groups = Self.withSelectedNode(subscriptions) {
                // Nodes are imported but no VPN profile is saved yet (or it was removed in Settings): save the
                // profile for the selected node, then connect.
                await saveProfileAndStart(groups)
            } else {
                notice = AppNotice(text: String(localized: "请先导入节点"), actions: [.importNode])
            }
            return
        }
        guard manager.isEnabled else {
            // Turning on another VPN app in Settings disables this profile. The coordinator then reports the
            // tunnel as unavailable, and the button used to do nothing at all: re-enable the profile first, the
            // same way the widget and Shortcuts paths do.
            await enableAndStart()
            return
        }
        guard coordinator.canStartTunnel else {
            message = String(localized: "无法启动隧道")
            return
        }
        await start()
    }

    /// Stops the tunnel; runs in `profileWrites`. On Demand would start it again right away, so the user's
    /// disconnect turns it off first, whoever turned it on: the app, the widget, Control Center or Shortcuts. A
    /// restart the app had begun ends here too.
    private func disconnectNow() async {
        restartAfterStop = false
        restartStartQueued = false
        resumeCapture = false
        // The tunnel seals its capture as it stops; a last copy now lets the app show all of it right away.
        await copyLiveCaptureBeforeStop()
        do {
            try await writeProfile { target in
                guard target.isOnDemandEnabled else {
                    return false
                }
                target.isOnDemandEnabled = false
                return true
            }
        } catch {
            message = String(localized: "无法关闭自动重连") + Self.errorCode(error)
        }
        manager?.connection.stopVPNTunnel()
    }

    /// Saves new reconnect and Wi-Fi settings and puts them in the VPN configuration. On Demand stays as it was: on
    /// while the user keeps the tunnel up, off after they disconnected.
    func setOnDemand(_ next: OnDemandSettings) async {
        guard next != onDemand else {
            return
        }
        onDemand = next
        next.save()
        await profileWrites.run {
            do {
                try await writeProfile { target in
                    next.apply(to: target, active: target.isOnDemandEnabled || isConnected)
                    return true
                }
            } catch {
                message = String(localized: "保存自动重连设置失败") + Self.errorCode(error)
            }
            refresh()
        }
    }

    /// Turns the profile on again, then starts it; runs in `profileWrites`.
    private func enableAndStart() async {
        do {
            try await writeProfile { target in
                target.isEnabled = true
                onDemand.apply(to: target, active: true)
                return true
            }
        } catch {
            message = String(localized: "无法启动隧道") + Self.errorCode(error)
            refresh()
            return
        }
        refresh()
        await start()
    }

    /// Saves the profile for the selected node, then starts it; runs in `profileWrites`.
    private func saveProfileAndStart(_ groups: SubscriptionGroups) async {
        guard await saveActiveProfile(groups) else {
            return
        }
        store(groups)
        guard manager != nil else {
            message = String(localized: "无法启动隧道")
            return
        }
        await start()
    }

    /// The groups with an active group that has a selected node: the active group's selection, else the first
    /// group that has one, else the first node of the first non-empty group. Nil when there is no node at all.
    static func withSelectedNode(_ subscriptions: SubscriptionGroups) -> SubscriptionGroups? {
        var groups = subscriptions
        if groups.active?.library.selected != nil {
            return groups
        }
        if let group = groups.groups.first(where: { $0.library.selected != nil }) {
            groups.activeID = group.id
            return groups
        }
        guard var group = groups.groups.first(where: { !$0.library.nodes.isEmpty }) else {
            return nil
        }
        group.library.selectedID = NodeLibrary.defaultNode(in: group.library.nodes)?.id
        groups.upsert(group)
        groups.activeID = group.id
        return groups
    }

    /// Starts the saved tunnel; runs in `profileWrites`.
    private func start() async {
        #if os(macOS)
        // A newer build may carry a newer System Extension; activation replaces the running one first. When the user
        // still has to allow the extension, the tunnel starts by itself once they do.
        guard await activateSystemExtension(afterApproval: { [weak self] in
            guard let self else {
                return
            }
            Task {
                await self.startAfterApproval()
            }
        }) else {
            return
        }
        #endif
        await startNow()
    }

    #if os(macOS)
    /// The connect that waited for the user to allow the System Extension, in `profileWrites` like any other.
    private func startAfterApproval() async {
        await writing {
            guard manager != nil, !isConnected else {
                return
            }
            await startNow()
        }
    }
    #endif

    /// Starts the tunnel as saved now; runs in `profileWrites`.
    private func startNow() async {
        // The user wants the tunnel up: On Demand keeps it up from here, including after the extension crashes.
        if onDemand.rules != nil, manager?.isOnDemandEnabled == false {
            do {
                try await writeProfile { target in
                    guard !target.isOnDemandEnabled else {
                        return false
                    }
                    onDemand.apply(to: target, active: true)
                    return true
                }
            } catch {
                // Connecting still works; only the automatic reconnect is missing.
                message = String(localized: "无法开启自动重连") + Self.errorCode(error)
            }
        }
        guard let manager else {
            message = String(localized: "无法启动隧道")
            return
        }
        startTunnel(manager)
    }

    private func startTunnel(_ manager: NETunnelProviderManager) {
        do {
            try manager.connection.startVPNTunnel(options: Self.startOptions(for: manager))
            message = nil
        } catch {
            message = String(localized: "无法启动隧道")
        }
    }

    /// Set once this run of the app has activated the tunnel System Extension.
    @ObservationIgnored private var systemExtensionActive = false
    /// Set while an activation request waits for the user to allow the extension in System Settings.
    @ObservationIgnored private var systemExtensionPending = false
    /// Runs once the waiting request is allowed; the last connect attempt sets it.
    @ObservationIgnored private var afterSystemExtensionApproval: (() -> Void)?

    /// Shown while macOS waits for the user to allow the extension.
    static let systemExtensionApprovalText = String(
        localized: "请在“系统设置 › 通用 › 登录项与扩展 › 网络扩展”中允许 AnixOps Net，然后再连接"
    )

    /// On macOS the tunnel may be a System Extension, which runs as root and cannot read the user's Keychain, so
    /// the app hands over the node key, node library and MITM root at start (`TunnelStartSecrets`). iOS shares the
    /// Keychain with the extension and passes nothing.
    private static func startOptions(for manager: NETunnelProviderManager) -> [String: NSObject]? {
        #if os(macOS)
        return TunnelStartSecrets.fromKeychain(nodeKeyVersion: nodeKeyVersion(of: manager)).options
        #else
        return nil
        #endif
    }

    /// The node key version the saved profile names (`TunnelProfileKeys.nodeKeyVersion`); nil for a profile from
    /// before key versions.
    private static func nodeKeyVersion(of manager: NETunnelProviderManager) -> String? {
        (manager.protocolConfiguration as? NETunnelProviderProtocol)?
            .providerConfiguration?[TunnelProfileKeys.nodeKeyVersion] as? String
    }

    /// macOS: hands the start secrets of the profile saved now to the running System Extension
    /// (`TunnelAppMessage.startSecrets`), which keeps them for the starts the system makes without options (On
    /// Demand, after a restart or a crash, at login, from System Settings). Without this, a profile saved again
    /// while the tunnel runs (a node picked by the rules, an import, a subscription update, new plugins) names a
    /// node key version the kept copy does not have, and the next such start fails, as a root process cannot read
    /// the Keychain. Called after every saved write, every change of the node library, a new MITM root or license,
    /// and when the tunnel comes up. Messages go out in call order, so the last one sent is the newest. While the
    /// tunnel is not running nothing can be sent: a start that then fails on the old copy is started again with
    /// options (`startAfterStaleCopy`). Nothing here is logged.
    private func handOverStartSecrets() {
        #if os(macOS)
        #if DEBUG
        if Self.isDemo {
            return
        }
        #endif
        guard let manager, let session = manager.connection as? NETunnelProviderSession else {
            handedOverStartSecrets = nil
            return
        }
        switch manager.connection.status {
        case .connected, .reasserting:
            break
        case .connecting:
            // The start's own options carry the values; `refresh()` hands over once it is connected.
            return
        default:
            handedOverStartSecrets = nil
            return
        }
        let secrets = TunnelStartSecrets.fromKeychain(nodeKeyVersion: Self.nodeKeyVersion(of: manager))
        guard let data = secrets.encoded(), data != handedOverStartSecrets else {
            return
        }
        handedOverStartSecrets = data
        do {
            try session.sendProviderMessage(TunnelAppMessage.startSecrets(data).encoded) { [weak self] answer in
                guard !TunnelStartSecrets.handOverAccepted(answer) else {
                    return
                }
                // Not kept (an extension older than the message, or the write failed): try again next time.
                Task { @MainActor in
                    if self?.handedOverStartSecrets == data {
                        self?.handedOverStartSecrets = nil
                    }
                }
            }
        } catch {
            handedOverStartSecrets = nil
        }
        #endif
    }

    #if os(macOS)
    /// The system's record of why the tunnel last stopped, with its underlying errors; nil when there is none.
    private func lastStopDescription() async -> String? {
        guard let connection = manager?.connection else {
            return nil
        }
        return await withCheckedContinuation { continuation in
            connection.fetchLastDisconnectError { error in
                continuation.resume(returning: error.map { Self.describe(disconnectError: $0 as NSError) })
            }
        }
    }

    /// When the app becomes active: a start the system made while the app was in the background may have failed on
    /// a stale copy of the start secrets (`startAfterStaleCopy`).
    private func checkStaleStartSecrets() {
        Task {
            await writing {
                guard manager?.connection.status == .disconnected else {
                    return
                }
                _ = startAfterStaleCopy(failure: await lastStopDescription(), requiresOnDemand: true)
            }
        }
    }

    /// Starts the tunnel with options once more after a start failed because the System Extension's copy of the start
    /// secrets belongs to an older profile (`TunnelStartSecrets.staleCopyReason`): the profile was saved again while
    /// the tunnel was not running (a new node key version, a new HTTPS decryption payload after a plugin or debug
    /// mapping change, or a new MITM root), and the system then started it without the app. Once for each saved
    /// profile, so a start that fails again does not loop; `requiresOnDemand` (on launch and on becoming active)
    /// leaves a tunnel the user turned off alone. Runs in `profileWrites`. Returns whether it started the tunnel.
    private func startAfterStaleCopy(failure: String?, requiresOnDemand: Bool) -> Bool {
        guard let manager, manager.isEnabled, manager.connection.status == .disconnected,
              !requiresOnDemand || manager.isOnDemandEnabled
        else {
            return false
        }
        let configuration = (manager.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration
        let profile = TunnelStartSecrets.staleCopyProfileStamp(
            nodeKeyVersion: Self.nodeKeyVersion(of: manager),
            mitmBundle: configuration?[TunnelProfileKeys.mitmBundle] as? Data,
            mitmRoot: configuration?[TunnelProfileKeys.mitmRootFingerprint] as? String
        )
        guard TunnelStartSecrets.retriesStaleCopy(
            failure: failure, profile: profile, retriedProfile: staleCopyRetriedProfile
        ) else {
            return false
        }
        staleCopyRetriedProfile = profile
        startTunnel(manager)
        return true
    }
    #endif

    /// Developer ID builds carry the tunnel as a System Extension, which macOS loads only after the app asks and the
    /// user allows it. Returns false, with the reason in `message`, when it is not active. While the user has not
    /// allowed it yet the request stays open: `allowPending` returns true anyway (saving a profile does not need
    /// the extension), and `afterApproval` runs once the user allows it. Builds whose tunnel is an app extension
    /// (iOS, Mac App Store) have nothing to activate.
    private func activateSystemExtension(allowPending: Bool = false, afterApproval: (() -> Void)? = nil) async -> Bool {
        #if os(macOS)
        guard !systemExtensionActive, let identifier = Self.systemExtensionIdentifier() else {
            return true
        }
        if afterApproval != nil {
            afterSystemExtensionApproval = afterApproval
        }
        if systemExtensionPending {
            message = Self.systemExtensionApprovalText
            return allowPending
        }
        let outcome = await SystemExtensionActivation.run(identifier) { [weak self] late in
            self?.systemExtensionFinished(late)
        }
        if outcome == .needsApproval {
            systemExtensionPending = true
            message = Self.systemExtensionApprovalText
            return allowPending
        }
        return systemExtensionFinished(outcome)
        #else
        return true
        #endif
    }

    #if os(macOS)
    /// Records the activation's final outcome, with the reason in `message` when it failed, and starts the tunnel
    /// when a connect waited for the user's approval. Returns whether the extension is active.
    @discardableResult
    private func systemExtensionFinished(_ outcome: SystemExtensionActivation.Outcome) -> Bool {
        let waited = systemExtensionPending
        systemExtensionPending = false
        let next = afterSystemExtensionApproval
        afterSystemExtensionApproval = nil
        switch outcome {
        case .activated:
            systemExtensionActive = true
            if message == Self.systemExtensionApprovalText {
                message = nil
            }
            if waited {
                next?()
            }
            return true
        case .needsApproval:
            systemExtensionPending = true
            afterSystemExtensionApproval = next
            message = Self.systemExtensionApprovalText
        case .needsRestart:
            message = String(localized: "网络扩展将在重启 Mac 后生效")
        case .notInApplications:
            message = String(localized: "请先把 AnixOps Net 拖进“应用程序”文件夹，从那里打开后再连接")
        case let .failed(code):
            message = String(localized: "无法启用网络扩展") + " (\(code))"
        }
        return false
    }
    #endif

    /// Nodes whose stop the user has already been told about; a node leaves the set once it dials again.
    @ObservationIgnored private var announcedStops: Set<String> = []
    @ObservationIgnored private var reporter: ClientReporter?

    /// Reads the engine's per-node dial health: tells the user once when a node stopped because its server kept
    /// rejecting this client (expired subscription or wrong device time), and, only when the user turned the panel
    /// report on in Settings, hands the counts to the panel reporter. `network` is `wifi`, `cellular`, `ethernet`
    /// or `other`.
    func checkOutboundHealth(network: String) async {
        guard let health = RuntimeControl.decode(OutboundHealth.self, from: await control(.outboundHealth)) else {
            return
        }
        let stopped = health.stopped
        announcedStops.formIntersection(stopped.map(\.node))
        if let entry = stopped.first(where: { !announcedStops.contains($0.node) }) {
            announcedStops.insert(entry.node)
            let origin = subscriptions.origin(ofRuntimeNode: entry.node)
            let name = origin.map { $0.node.name.isEmpty ? $0.node.id : $0.node.name } ?? entry.node
            var actions: [AppNotice.Action] = [.resumeNode(entry.node)]
            if let group = origin?.group, group.url != nil {
                actions.append(.refreshSubscription(group.id))
            }
            notice = AppNotice(
                text: ClientReport.stoppedText(node: name, health: entry.health),
                detail: "fallback=\(entry.health.counts.fallback) skew=\(entry.health.clockSkewSecs.map(String.init) ?? "-")",
                actions: actions
            )
        }
        let store = ClientReportStore()
        guard store.isEnabled else {
            self.reporter?.retire()
            self.reporter = nil
            store.forgetIfDisabled()
            return
        }
        let reporter = self.reporter ?? ClientReporter(store: store)
        self.reporter = reporter
        reporter.offer(health, groups: subscriptions, network: network)
    }

    /// The panel report setting (off by default). Turning it on starts from the counts at the next look, so the
    /// time it was off is never reported; turning it off deletes the saved baseline. The stopped-node notice above
    /// stays either way: it is shown on this device only.
    func setPanelReport(enabled: Bool) {
        reporter?.retire()
        reporter = nil
        ClientReportStore().setEnabled(enabled)
    }

    /// Lets a stopped node dial again after the user fixed the time or the subscription.
    func resumeNode(_ node: String) async {
        announcedStops.remove(node)
        _ = await control(.resumeOutbound(node: node))
    }

    /// Sends a control request to the running engine (live traffic, the connection list). Nil while the tunnel is
    /// not connected, or when the engine refused the request. While a saved capture session is shown
    /// (`viewedCapture`), the requests that read capture and the connection list go to the app's capture viewer
    /// instead, connected or not.
    func control(_ command: RuntimeControl) async -> Data? {
        #if DEBUG
        if Self.isDemo {
            return Self.demoControl(command)
        }
        #endif
        if viewedCapture != nil, Self.readsCapture(command) {
            return await CaptureViewer.shared.control(command)
        }
        return await tunnelControl(command)
    }

    /// `control` sent to the running engine whatever the lists show.
    func tunnelControl(_ command: RuntimeControl) async -> Data? {
        #if DEBUG
        if Self.isDemo {
            return Self.demoControl(command)
        }
        #endif
        guard let session = manager?.connection as? NETunnelProviderSession, connection == .connected else {
            return nil
        }
        let answer: Data? = await withCheckedContinuation { continuation in
            do {
                try session.sendProviderMessage(TunnelAppMessage.control(command.request).encoded) { data in
                    continuation.resume(returning: data)
                }
            } catch {
                continuation.resume(returning: nil)
            }
        }
        switch command {
        case .setCapture, .loadCapture:
            if let status = RuntimeControl.decode(PacketCapture.Status.self, from: answer) {
                captureOn = status.enabled
            } else if case .loadCapture = command {
                captureOn = false
            }
        default:
            break
        }
        return answer
    }

    /// Whether the running tunnel decrypts HTTPS, as its extension set it up; nil while it is not running or from
    /// an extension older than the question.
    func httpsDecryptionState() async -> HttpsDecryptionState? {
        #if DEBUG
        if Self.isDemo {
            return HttpsDecryptionState(status: .on, plugins: 1)
        }
        #endif
        guard let session = manager?.connection as? NETunnelProviderSession, connection == .connected else {
            return nil
        }
        let answer: Data? = await withCheckedContinuation { continuation in
            do {
                try session.sendProviderMessage(TunnelAppMessage.httpsDecryption.encoded) { data in
                    continuation.resume(returning: data)
                }
            } catch {
                continuation.resume(returning: nil)
            }
        }
        return HttpsDecryptionState.decoded(from: answer)
    }

    /// Whether the running tunnel routes by the rules, as its extension set it up; nil while it is not running or
    /// from an extension older than the question.
    func routingState() async -> RoutingState? {
        #if DEBUG
        if Self.isDemo {
            return nil
        }
        #endif
        guard let session = manager?.connection as? NETunnelProviderSession, connection == .connected else {
            return nil
        }
        let answer: Data? = await withCheckedContinuation { continuation in
            do {
                try session.sendProviderMessage(TunnelAppMessage.routing.encoded) { data in
                    continuation.resume(returning: data)
                }
            } catch {
                continuation.resume(returning: nil)
            }
        }
        return RoutingState.decoded(from: answer)
    }

    /// Keeps `list` as the last connection list read from the running engine.
    func remember(_ list: RuntimeControl.Connections) {
        guard connection == .connected, list != lastConnections else {
            return
        }
        lastConnections = list
    }

    /// The extension's hourly traffic by host and policy (`clear` empties it first). Nil while the tunnel is not
    /// running, or when the extension is older than the statistics page. Each answer is also kept as
    /// `TrafficStatsCache`, which the statistics page shows while the tunnel is off.
    func trafficStats(clear: Bool = false) async -> TrafficStats? {
        guard let session = manager?.connection as? NETunnelProviderSession, isConnected else {
            return nil
        }
        let message: TunnelAppMessage = clear ? .clearTrafficStats : .trafficStats
        let data: Data? = await withCheckedContinuation { continuation in
            do {
                try session.sendProviderMessage(message.encoded) { data in
                    continuation.resume(returning: data)
                }
            } catch {
                continuation.resume(returning: nil)
            }
        }
        guard let stats = data.flatMap({ try? TrafficStats.decoded(from: $0) }) else {
            return nil
        }
        TrafficStatsCache.store(stats)
        return stats
    }

    /// Asks the running extension for its sanitized runtime status.
    func refreshRuntimeStatus() {
        #if DEBUG
        if Self.isDemo {
            // The demo is connected without a tunnel; keep the made-up status so the page does not say it is not running.
            return
        }
        #endif
        guard let session = manager?.connection as? NETunnelProviderSession, connection == .connected else {
            runtimeStatus = nil
            return
        }
        do {
            try session.sendProviderMessage(TunnelAppMessage.runtimeStatus.encoded) { [weak self] data in
                let text = data.flatMap { String(data: $0, encoding: .utf8) }
                Task { @MainActor in
                    self?.runtimeStatus = text
                }
            }
        } catch {
            runtimeStatus = nil
        }
    }

    /// Asks the running extension for developer-mode diagnostics; clears them when it is not running.
    func refreshDiagnostics() {
        #if DEBUG
        if Self.isDemo {
            return
        }
        #endif
        guard let session = manager?.connection as? NETunnelProviderSession, isConnected else {
            diagnostics = nil
            return
        }
        do {
            try session.sendProviderMessage(TunnelAppMessage.diagnostics.encoded) { [weak self] data in
                let diagnostics = data.flatMap { try? TunnelDiagnostics.decoded(from: $0) }
                Task { @MainActor in
                    self?.diagnostics = diagnostics
                }
            }
        } catch {
            diagnostics = nil
        }
    }

    /// Reads the system's record of why the tunnel last stopped, for developer mode.
    func refreshLastDisconnectError() {
        manager?.connection.fetchLastDisconnectError { [weak self] error in
            let text = error.map { Self.describe(disconnectError: $0 as NSError) }
            Task { @MainActor in
                self?.lastDisconnectError = text
            }
        }
    }

    /// The saved profile's outbound protocol, without address or key.
    var outboundKind: String? {
        #if DEBUG
        if Self.isDemo {
            return "shadowsocks"
        }
        #endif
        let configuration = (manager?.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration
        guard let data = configuration?[TunnelProfileKeys.runtimeConfig] as? Data else {
            return nil
        }
        return (try? EmbeddedRuntimeConfig.decoded(from: data))?.outboundKind
    }

    private func refresh() {
        let saved = manager?.isEnabled == true && manager?.protocolConfiguration != nil
        coordinator.apply(Self.platformStatus(saved: saved))
        profileState = TunnelProfileState.of(
            hasConfiguration: manager?.protocolConfiguration != nil, isEnabled: manager?.isEnabled == true
        )
        let previous = connection
        connection = manager?.connection.status ?? .invalid
        connectedSince = connection == .connected ? manager?.connection.connectedDate : nil
        if isConnected, let notice, !notice.origin.applies(whileConnected: true) {
            // The tunnel is back (On Demand, the widget, Control Center): the stop card and its Retry are out of date.
            self.notice = nil
        }
        if connection == .connected || connection == .reasserting {
            if previous != .connected, previous != .reasserting {
                // Whatever the start was handed, the copy it keeps follows the profile saved now.
                handOverStartSecrets()
            }
        } else {
            handedOverStartSecrets = nil
        }
        if connection == .connected, previous != .connected {
            lastConnections = nil
            captureConnected()
            // The first connection is when notifications start to matter (an unexpected stop, a failed update):
            // ask for permission now, once, if it was never asked (`AppNotifications.requestIfUndetermined`).
            Task {
                await AppNotifications.requestIfUndetermined()
            }
            if resumeCapture {
                resumeCapture = false
                Task {
                    _ = await control(.setCapture(true))
                }
            }
        }
        if connection != .connected {
            captureOn = false
            if previous == .connected {
                captureStopped()
            }
        }
        if connection == .disconnected, !restartAfterStop, !restartStartQueued {
            // A stop the app did not ask for (the user, the system, a failure): capture ends with the tunnel.
            resumeCapture = false
        }
        if restartAfterStop, connection == .disconnected {
            // The app stopped the tunnel on purpose (`restart()`): no error to show. It starts again once the writes
            // queued before are done, on the profile they saved.
            restartAfterStop = false
            restartStartQueued = true
            Task {
                await profileWrites.run {
                    startAfterRestart()
                }
            }
        } else if connection == .disconnected, previous != .disconnected, previous != .invalid {
            showLastDisconnectError()
        }
        let configuration = (manager?.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration
        let previousNode = nodeName
        nodeName = configuration?[TunnelProfileKeys.nodeName] as? String
        runsDirectProfile = (configuration?[TunnelProfileKeys.runtimeConfig] as? Data)
            .flatMap { try? EmbeddedRuntimeConfig.decoded(from: $0) }?
            .outboundKind == TunnelProfile.directOutboundType
        let previousMode = mode
        mode = ProxyMode.saved(in: configuration)
        if connection != previous || nodeName != previousNode || mode != previousMode {
            Self.reloadWidgets()
        }
    }

    /// The start that ends a restart; runs in `profileWrites`, so `manager` is the profile the writes before it saved.
    /// Nothing starts when the user disconnected meanwhile (`disconnectNow()`), or when On Demand has started the
    /// tunnel already.
    private func startAfterRestart() {
        guard restartStartQueued else {
            return
        }
        restartStartQueued = false
        guard let manager else {
            // The configuration was removed meanwhile.
            resumeCapture = false
            return
        }
        guard manager.connection.status == .disconnected else {
            // On Demand started it already; capture comes back once it is connected.
            return
        }
        do {
            try manager.connection.startVPNTunnel(options: Self.startOptions(for: manager))
        } catch {
            resumeCapture = false
        }
    }

    /// The Home Screen widget and the Control Center switch read the VPN configuration themselves;
    /// this only tells them it changed.
    private static func reloadWidgets() {
        #if os(iOS)
        WidgetCenter.shared.reloadAllTimelines()
        if #available(iOS 18.0, *) {
            ControlCenter.shared.reloadAllControls()
        }
        #endif
    }

    /// Shows why the tunnel stopped, such as the extension's start failure, which iOS otherwise hides.
    private func showLastDisconnectError() {
        manager?.connection.fetchLastDisconnectError { [weak self] error in
            guard let error = error else {
                return
            }
            let text = Self.describe(disconnectError: error as NSError)
            #if os(macOS)
            if TunnelStartSecrets.isStaleCopyFailure(text) {
                Task { @MainActor in
                    await self?.recoverStaleCopy(text)
                }
                return
            }
            // The System Extension cannot post; on iOS the extension posts this itself.
            if AppNotifications.settings.connection {
                AppNotifications.post(
                    title: String(localized: "AnixOps Net 已断开"),
                    body: String(localized: "节点没有响应、被干扰，或网络刚切换。打开 App 重试或换一个节点。"),
                    thread: "connection",
                    id: "connection-stopped"
                )
            }
            #endif
            Task { @MainActor in
                guard let self else {
                    return
                }
                self.lastDisconnectError = text
                // The answer can arrive after On Demand already brought the tunnel back: no stop card then.
                guard NoticeOrigin.tunnelStopped.applies(whileConnected: self.isConnected) else {
                    return
                }
                self.notice = AppNotice(
                    text: String(localized: "连接已断开：节点没有响应、被干扰，或网络刚切换。可以重试，或换一个节点。"),
                    detail: text,
                    actions: [.retry, .chooseNode],
                    origin: .tunnelStopped
                )
            }
        }
    }

    #if os(macOS)
    /// A start failed because the System Extension's copy of the start secrets belongs to an older profile: starts
    /// the tunnel again with options (`startAfterStaleCopy`); when it already did for this profile, asks the user
    /// to connect from the app instead of blaming the node.
    private func recoverStaleCopy(_ text: String) async {
        let started = await writing {
            startAfterStaleCopy(failure: text, requiresOnDemand: false)
        }
        lastDisconnectError = text
        guard !started else {
            return
        }
        if AppNotifications.settings.connection {
            AppNotifications.post(
                title: String(localized: "AnixOps Net 已断开"),
                body: String(localized: "网络扩展保存的配置副本已过期。请打开 App 重新连接。"),
                thread: "connection",
                id: "connection-stopped"
            )
        }
        guard NoticeOrigin.tunnelStopped.applies(whileConnected: isConnected) else {
            return
        }
        notice = AppNotice(
            text: String(localized: "网络扩展保存的配置副本已过期，自动重连没有成功。请重新连接。"),
            detail: text,
            actions: [.retry],
            origin: .tunnelStopped
        )
    }
    #endif

    /// The error and its underlying errors, which carry the extension's fixed failure reason.
    nonisolated private static func describe(disconnectError: NSError) -> String {
        var lines = [String(localized: "隧道已停止：\(disconnectError.localizedDescription)")]
        var underlying = disconnectError.userInfo[NSUnderlyingErrorKey] as? NSError
        while let error = underlying {
            lines.append("\(error.localizedDescription) (\(error.domain) \(error.code))")
            underlying = error.userInfo[NSUnderlyingErrorKey] as? NSError
        }
        return lines.joined(separator: "\n")
    }

    /// The app cannot load the runtime itself: this build ships it inside the extension, which re-checks
    /// the live handshake on every start.
    private static func platformStatus(saved: Bool) -> IosPlatformStatus {
        IosPlatformFactsProbe.currentStatus(
            vpnConfiguration: saved ? .savedAndAuthorized : .notSaved,
            embeddedRuntime: .available
        )
    }

    private static let profileID = NodeSecretStore.tunnelProfileAccount

    private static func tunnelBundleIdentifier() -> String? {
        if let identifier = systemExtensionIdentifier() {
            return identifier
        }
        guard let plugins = Bundle.main.builtInPlugInsURL,
              let urls = try? FileManager.default.contentsOfDirectory(at: plugins, includingPropertiesForKeys: nil)
        else {
            return nil
        }
        for url in urls where url.pathExtension == "appex" {
            guard let bundle = Bundle(url: url),
                  let info = bundle.object(forInfoDictionaryKey: "NSExtension") as? [String: Any],
                  info["NSExtensionPointIdentifier"] as? String == "com.apple.networkextension.packet-tunnel"
            else {
                continue
            }
            return bundle.bundleIdentifier
        }
        return nil
    }

    /// The packet tunnel System Extension in `Contents/Library/SystemExtensions` of a Developer ID build, or nil.
    private static func systemExtensionIdentifier() -> String? {
        #if os(macOS)
        let folder = Bundle.main.bundleURL.appendingPathComponent("Contents/Library/SystemExtensions", isDirectory: true)
        guard let urls = try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil) else {
            return nil
        }
        for url in urls where url.pathExtension == "systemextension" {
            guard let bundle = Bundle(url: url),
                  let info = bundle.object(forInfoDictionaryKey: "NetworkExtension") as? [String: Any],
                  let providers = info["NEProviderClasses"] as? [String: Any],
                  providers["com.apple.networkextension.packet-tunnel"] != nil
            else {
                continue
            }
            return bundle.bundleIdentifier
        }
        return nil
        #else
        return nil
        #endif
    }

    /// Shown when a change to the subscription groups waits for them to be read: the Keychain cannot be read before
    /// the device is first unlocked after a restart.
    static let subscriptionsUnreadText = String(
        localized: "还没读到已保存的订阅，请先输入锁屏密码，再重试。"
    )

    /// Shown for an `http://` subscription link: iOS and macOS refuse plain HTTP downloads (App Transport Security).
    static let plainHTTPSubscriptionText = String(
        localized: "只支持 HTTPS 订阅链接：系统不允许 App 用不加密的 http:// 下载订阅。请向服务商索取 https:// 链接。"
    )

    /// A download App Transport Security refused because it was not HTTPS (a group saved by an older version with an
    /// `http://` link).
    private static func isPlainHTTPRefused(_ error: Error) -> Bool {
        (error as? URLError)?.code == .appTransportSecurityRequiresSecureConnection
    }

    private static func describe(_ error: TunnelNodeImportError) -> String {
        switch error {
        case .unrecognized:
            return String(localized: "请粘贴节点链接、订阅内容或 WireGuard 配置")
        case let .native(failure):
            switch failure {
            case .unrecognized:
                return String(localized: "无法识别的节点链接或订阅格式")
            case .empty:
                return String(localized: "内容为空")
            case .tooLarge:
                return String(localized: "订阅内容过大")
            case let .noSupportedNode(protocols, _):
                // The locale's own list punctuation ("a、b" in Chinese, "a, b" in English).
                let names = Array(Set(protocols)).sorted().formatted(.list(type: .and))
                return names.isEmpty
                    ? String(localized: "没有可用的节点")
                    : String(localized: "没有可用的节点（暂不支持：\(names)）")
            case .unavailable:
                return String(localized: "导入组件不可用，请更新 App")
            }
        case let .shadowsocks(error):
            switch error {
            case .notShadowsocksLink, .malformed:
                return String(localized: "节点链接格式不正确")
            case .unsupportedMethod:
                return String(localized: "目前只支持 Shadowsocks-2022 加密方式")
            case .unsupportedPlugin:
                return String(localized: "暂不支持带插件的节点")
            }
        case let .vless(error):
            switch error {
            case .notVlessLink, .malformed:
                return String(localized: "节点链接格式不正确")
            case .invalidUUID:
                return String(localized: "VLESS 用户 ID 格式不正确")
            case .unsupportedSecurity:
                return String(localized: "目前只支持 REALITY 安全类型的 VLESS 节点")
            case .unsupportedTransport:
                return String(localized: "目前只支持 TCP 传输的 VLESS 节点")
            case .unsupportedFlow:
                return String(localized: "目前只支持 xtls-rprx-vision 流控或不使用流控")
            case .invalidRealitySettings:
                return String(localized: "REALITY 参数（sni、pbk、sid）不正确")
            }
        case let .wireguard(error):
            switch error {
            case .malformed:
                return String(localized: "WireGuard 配置格式不正确")
            case let .missingField(field):
                return String(localized: "WireGuard 配置缺少 \(field)")
            case .invalidKey:
                return String(localized: "WireGuard 密钥格式不正确")
            case .invalidAddress:
                return String(localized: "WireGuard 地址或 AllowedIPs 格式不正确")
            case .invalidEndpoint:
                return String(localized: "WireGuard Endpoint 格式不正确")
            case .unsupportedPeerCount:
                return String(localized: "目前只支持一个 Peer 的 WireGuard 配置")
            }
        }
    }
}

/// A delay test result for one node.
enum NodeLatency: Equatable, Sendable {
    case measuring
    case millis(Int)
    /// No HTTP answer within `NodeDelayFFI.timeoutMillis`.
    case timeout
    /// The node refused, the handshake failed or the answer was not HTTP.
    case failed
    /// The outbound cannot carry one HTTP request, for example WireGuard.
    case unsupported
}

/// What the import sheet shows before saving: no password, key or full server address.
struct ImportPreview: Equatable {
    /// `outboundKind` of a subscription link, which is only downloaded on import.
    static let subscriptionKind = "subscription"

    let name: String
    let outboundKind: String
    let maskedServer: String

    /// Keeps the first two and last four characters of a host so the user can recognize it in a screenshot-safe way.
    static func mask(_ host: String) -> String {
        guard host.count > 8 else {
            return "••••"
        }
        return "\(host.prefix(2))••••\(host.suffix(4))"
    }
}

struct ImportFailure: Error, Equatable {
    let message: String
}

/// Who started a subscription update, which decides what it may show and do to a running tunnel.
enum SubscriptionUpdateTrigger: Equatable {
    /// The user: an import, "Update now", pull to refresh, a notice's button, the command line. A failure shows a
    /// notice; a running tunnel reconnects when what it runs changed.
    case user
    /// The automatic update while the app runs (in front on iOS, any time on macOS). A failure only marks the group;
    /// a running tunnel reconnects when what it runs changed.
    case automatic
    /// The automatic update in the system's background refresh on iOS. A failure only marks the group; the app may be
    /// suspended right after, so a running tunnel is not stopped here but reconnects when the app is next in front
    /// (`TunnelController.reconnectIfPending()`).
    case background
}

/// A notice on the home screen: one plain sentence on what happened and what it means, the raw error behind it
/// for the curious (shown folded), and the actions that fix it.
struct AppNotice: Equatable {
    enum Action: Equatable, Hashable {
        /// Start the tunnel again.
        case retry
        /// Open the node picker.
        case chooseNode
        /// Download this subscription group again.
        case refreshSubscription(String)
        /// Open the import sheet.
        case importNode
        /// Let an engine node that stopped after repeated rejections dial again.
        case resumeNode(String)
    }

    var text: String
    var detail: String?
    var actions: [Action] = []
    /// What made it, so a status change can drop it once it no longer applies (`NoticeOrigin`).
    var origin: NoticeOrigin = .other
}

#if DEBUG
/// Demo data for screenshots: a debug build launched with `-AnixOpsDemo YES` shows a connected client with a
/// made-up subscription instead of reading the VPN configuration and the Keychain. Nothing is saved and no
/// tunnel starts, so it runs unsigned on a CI machine.
extension TunnelController {
    static var isDemo: Bool {
        UserDefaults.standard.bool(forKey: "AnixOpsDemo")
    }

    /// The time of day the demo's requests end at, today: CI compares screenshots pixel by pixel, so what a
    /// picture shows must not depend on when it was taken (.github/scripts/mac-screenshot-compare.sh).
    static var demoClock: Date {
        Calendar.current.startOfDay(for: Date()).addingTimeInterval(12 * 3600 + 36 * 60)
    }

    /// When the demo subscription expires: a fixed day, for the same reason.
    static let demoExpiry = Date(timeIntervalSince1970: 1_796_731_200)

    func loadDemo() {
        let credentials = "YWVzLTI1Ni1nY206ZGVtby1wYXNzd29yZA"
        let names = [("hk1", "香港 01"), ("hk2", "香港 02"), ("jp1", "日本 01"), ("sg1", "新加坡 01"), ("us1", "美国 01"), ("de1", "德国 01")]
        let links = names.map { host, name in
            "ss://\(credentials)@\(host).example.com:8388#\(name.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? host)"
        }
        let delays = [38, 45, 72, 86, 164, 213]
        if let data = try? NativeNodeImporterFFI.importNodes(links.joined(separator: "\n")),
           let report = try? NativeImportReport.decoded(from: data) {
            let group = SubscriptionGroup(
                name: "AnixOps",
                library: NodeLibrary(
                    report: report,
                    subscriptionURL: "https://example.com/subscription",
                    keeping: nil,
                    at: Date().addingTimeInterval(-2 * 3600)
                ),
                userInfo: SubscriptionUserInfo(
                    upload: 6_400_000_000,
                    download: 61_800_000_000,
                    total: 200_000_000_000,
                    expire: Self.demoExpiry
                )
            )
            subscriptions = SubscriptionGroups(groups: [group], activeID: group.id)
            library = subscriptions.activeLibrary()
            nodeName = group.library.selected?.name
            for (index, node) in group.library.nodes.enumerated() {
                latencies[Self.latencyKey(group: group.id, node: node.id)] = .millis(delays[index % delays.count])
            }
        }
        // The made-up subscription has no rules, so the mode switch offers global and direct.
        mode = .global
        connection = .connected
        connectedSince = Date().addingTimeInterval(-4521)
        lastConnections = Self.demoConnections()
        runtimeStatus = Self.demoRuntimeStatus
        diagnostics = Self.demoDiagnostics()
        // Screenshots of debug mode pass `-AnixOpsMode debug`; `choose` shows the first-launch mode choice.
        let demoMode = UserDefaults.standard.string(forKey: "AnixOpsMode") ?? ""
        appMode = demoMode == "choose" ? nil : AppMode(rawValue: demoMode) ?? .proxy
        // The UI tests pass `-AnixOpsNotice <text>` to check that a failure shows on every page.
        if let text = UserDefaults.standard.string(forKey: "AnixOpsNotice"), !text.isEmpty {
            message = text
        }
        // The made-up groups stand for the saved ones: the demo's changes are stored as before.
        subscriptionsRead = true
        loaded = true
    }

    /// A running engine's sanitized status (`engine-embedded` `RuntimeStatus`), so the demo's diagnostics agree with
    /// its "connected" state.
    static let demoRuntimeStatus = #"{"abi_version":1,"schema_version":1,"profile_id":"demo","outbound":"shadowsocks","state":"running","last_error":null,"stats":{"packets_in":48210,"packets_in_dropped":0,"packets_out":51377,"tcp_active":14,"tcp_total":1286,"tcp_rejected":0,"tcp_connect_failed":3,"udp_dropped":0,"udp_flows":212,"udp_failed":0,"dns_queries":904,"dns_failed":2}}"#

    /// The extension's diagnostics for the demo: the status above, memory and a few log lines.
    static func demoDiagnostics() -> TunnelDiagnostics {
        let now = Date()
        let lines = [
            (4521.0, "startTunnel"),
            (4520.0, "mode global"),
            (4519.0, "startTunnel succeeded"),
            (1803.0, "bound interface en0 (code 0)"),
        ]
        return TunnelDiagnostics(
            runtimeStatus: demoRuntimeStatus,
            memoryFootprintBytes: 18_400_000,
            peakMemoryFootprintBytes: 23_100_000,
            memoryAvailableBytes: 31_600_000,
            uptimeSeconds: 4521,
            handshake: .current,
            events: lines.map { TunnelDiagnosticEvent(time: now.addingTimeInterval(-$0.0), message: $0.1) }
        )
    }

    /// Made-up capture answers for the debug-mode screenshots: capture on, a dozen decrypted requests, each the only
    /// request of its connection. Other commands get no answer.
    static func demoControl(_ command: RuntimeControl) -> Data? {
        guard let request = try? JSONSerialization.jsonObject(with: command.request) as? [String: Any],
              let cmd = request["cmd"] as? String
        else {
            return nil
        }
        let requests = demoRequests()
        let answer: [String: Any]
        switch cmd {
        case "capture":
            answer = ["enabled": true, "max_total_bytes": 2_097_152, "used_bytes": 812_000, "connections": requests.count]
        case "capture_requests":
            let since = (request["since_seq"] as? NSNumber)?.intValue ?? 0
            answer = [
                "requests": since == 0 ? requests.map { $0.row } : [],
                "last_seq": requests.count,
                "dropped_before_seq": 0,
            ]
        case "capture_connection":
            let id = (request["id"] as? NSNumber)?.intValue ?? 0
            guard let found = requests.first(where: { ($0.row["connection_id"] as? Int) == id }) else {
                return try? JSONSerialization.data(withJSONObject: ["capture": NSNull()])
            }
            let totals: [String: Any] = ["seen": 2_400, "captured": 2_400, "truncated": false]
            answer = ["capture": [
                "id": id,
                "network": "tcp",
                "decrypted": true,
                "upload": totals,
                "download": totals,
                "chunks": [Any](),
                "http": [found.exchange],
            ]]
        default:
            return nil
        }
        return try? JSONSerialization.data(withJSONObject: answer)
    }

    private static func demoRequests() -> [(row: [String: Any], exchange: [String: Any])] {
        let items: [(method: String, url: String, status: Int?, type: String, size: Int, took: Int)] = [
            ("GET", "https://api.example.com/v2/users/me", 200, "application/json", 1_840, 86),
            ("POST", "https://api.example.com/v2/orders", 201, "application/json", 412, 214),
            ("GET", "https://api.example.com/v2/orders?page=2&limit=20", 200, "application/json", 18_320, 142),
            ("GET", "https://cdn.example.com/img/banner@3x.png", 200, "image/png", 284_000, 391),
            ("GET", "https://api.example.com/v2/feature-flags", 304, "", 0, 41),
            ("PUT", "https://api.example.com/v2/users/me/avatar", 413, "application/json", 96, 77),
            ("GET", "https://api.example.com/v2/notifications", 500, "application/json", 128, 1_204),
            ("POST", "https://events.example.com/v1/batch", 204, "", 0, 63),
            ("GET", "https://api.example.com/v2/search?q=coffee", 200, "application/json", 42_600, 288),
            ("OPTIONS", "https://api.example.com/v2/orders", 204, "", 0, 38),
            ("GET", "https://ws.example.com/socket", nil, "", 0, 0),
            ("GET", "https://api.example.com/v2/config", 200, "application/json", 3_120, 95),
        ]
        let now = Int(demoClock.timeIntervalSince1970 * 1000)
        return items.enumerated().map { index, item in
            let seq = index + 1
            let started = now - (items.count - index) * 1_700
            let host = URL(string: item.url)?.host ?? ""
            let body = "{\"id\":\"usr_29\",\"name\":\"Demo\",\"plan\":\"pro\"}"
            var row: [String: Any] = [
                "seq": seq,
                "connection_id": seq,
                "index": 0,
                "method": item.method,
                "url": item.url,
                "host": host,
                "http_version": index % 3 == 0 ? "HTTP/1.1" : "HTTP/2",
                "started_at_ms": started,
                "request_size": item.method == "GET" ? 0 : 240,
                "response_size": item.size,
            ]
            var exchange: [String: Any] = [
                "seq": seq,
                "started_at_ms": started,
                "method": item.method,
                "url": item.url,
                "request": [
                    "headers": [[":authority", host], ["user-agent", "DemoApp/4.2 (iPhone; iOS 18.1)"], ["accept", "application/json"], ["authorization", "Bearer eyJhbGciOi…"]],
                    "body": ["size": 0, "truncated": false, "data_base64": ""],
                ],
                "response": [
                    "headers": item.type.isEmpty ? [[String]]() : [["content-type", item.type], ["cache-control", "no-store"]],
                    "body": [
                        "size": item.type == "application/json" ? body.utf8.count : 0,
                        "truncated": false,
                        "data_base64": item.type == "application/json" ? Data(body.utf8).base64EncodedString() : "",
                    ],
                ],
            ]
            if let status = item.status {
                row["status"] = status
                row["ended_at_ms"] = started + item.took
                exchange["status"] = status
                exchange["ended_at_ms"] = started + item.took
            }
            if !item.type.isEmpty {
                row["response_content_type"] = item.type
            }
            return (row, exchange)
        }
    }

    /// A request list as the engine would answer it: some connections still open, some ended, one rejected.
    private static func demoConnections() -> RuntimeControl.Connections? {
        let rows: [(host: String, port: Int, network: String, rule: String, outbound: String, bytes: Int, ended: Bool)] = [
            ("www.youtube.com", 443, "udp", "DOMAIN-SUFFIX,youtube.com", "香港 01", 412_000_000, false),
            ("rr3---sn.googlevideo.com", 443, "udp", "DOMAIN-SUFFIX,googlevideo.com", "香港 01", 828_000_000, false),
            ("github.com", 443, "tcp", "DOMAIN-SUFFIX,github.com", "日本 01", 96_000_000, false),
            ("objects.githubusercontent.com", 443, "tcp", "DOMAIN-SUFFIX,githubusercontent.com", "日本 01", 216_000_000, false),
            ("gateway.icloud.com", 443, "tcp", "DOMAIN-SUFFIX,icloud.com", "DIRECT", 148_000_000, false),
            ("api.openai.com", 443, "tcp", "DOMAIN-SUFFIX,openai.com", "美国 01", 96_000_000, false),
            ("web.telegram.org", 443, "tcp", "DOMAIN-KEYWORD,telegram", "新加坡 01", 41_000_000, false),
            ("www.apple.com", 443, "tcp", "DOMAIN-SUFFIX,apple.com", "DIRECT", 3_200_000, false),
            ("ads.example.net", 443, "tcp", "DOMAIN-SUFFIX,example.net", "REJECT", 0, true),
            ("www.google.com", 443, "tcp", "DOMAIN-SUFFIX,google.com", "香港 01", 1_800_000, true),
            ("api.github.com", 443, "tcp", "DOMAIN-SUFFIX,github.com", "日本 01", 640_000, true),
            ("time.apple.com", 123, "udp", "FINAL", "DIRECT", 96, true),
        ]
        let now = Int(Date().timeIntervalSince1970 * 1000)
        let list = rows.enumerated().map { index, row -> [String: Any] in
            var entry: [String: Any] = [
                "id": index + 1,
                "network": row.network,
                "host": row.host,
                "destination": "\(row.host):\(row.port)",
                "rule": row.rule,
                "chain": row.outbound == "DIRECT" || row.outbound == "REJECT" ? [row.outbound] : ["节点选择", row.outbound],
                "outbound": row.outbound,
                "started_at_ms": now - (index + 1) * 47_000,
                "upload": row.bytes / 24,
                "download": row.bytes,
            ]
            if row.ended {
                entry["ended_at_ms"] = now - (index + 1) * 9_000
            }
            if row.outbound == "REJECT" {
                entry["error"] = "rejected"
            }
            return entry
        }
        let answer: [String: Any] = [
            "traffic": ["upload_total": 86_400_000, "download_total": 1_932_000_000, "upload_rate": 184_000, "download_rate": 3_870_000, "active": 8],
            "active": list.filter { $0["ended_at_ms"] == nil },
            "closed": list.filter { $0["ended_at_ms"] != nil },
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: answer) else {
            return nil
        }
        return RuntimeControl.decodeConnections(data)
    }
}
#endif
