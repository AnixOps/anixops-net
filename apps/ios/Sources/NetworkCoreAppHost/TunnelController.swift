import Foundation
import NetworkCoreBridge
import NetworkExtension
import Observation
#if os(iOS)
import BackgroundTasks
import WidgetKit
#endif

/// Owns the app's single VPN configuration: imports a node (a share link of any protocol the engine runs, a
/// subscription text or a WireGuard config), saves it as the
/// tunnel profile, and starts or stops the packet tunnel. Node secrets go only into the Keychain; the saved
/// profile holds a persistent reference to them and a config without them.
///
/// Observable (not ObservableObject): a view redraws only when a property it actually read changes, so the speed
/// and latency updates no longer redraw every screen that holds the controller.
@MainActor
@Observable
final class TunnelController {
    private(set) var coordinator: AppCoordinator
    private(set) var nodeName: String?
    private(set) var connection: NEVPNStatus = .invalid
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
    /// The saved tunnel's mode: by rules, everything through the selected node, or everything direct.
    private(set) var mode = ProxyMode.global
    /// DNS servers, hosts and per-domain servers, from `dns-settings.json`; the saved profile carries a copy.
    private(set) var dns = DnsSettings()
    /// Destination rewrites, from `redirects.json`; the saved profile carries the enabled lines.
    private(set) var redirects = TrafficRedirects()
    /// The rule mode of a node list without subscription rules and the user's own rules, from `local-rules.json`;
    /// the saved profile carries a copy.
    private(set) var localRules = LocalRules()

    @ObservationIgnored private var manager: NETunnelProviderManager?
    @ObservationIgnored private var statusObserver: NSObjectProtocol?
    /// Set when a node switch stopped the tunnel; the tunnel starts again once it is down.
    @ObservationIgnored private var restartAfterStop = false

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
    }

    deinit {
        if let statusObserver = statusObserver {
            NotificationCenter.default.removeObserver(statusObserver)
        }
    }

    var isConnected: Bool {
        connection == .connected || connection == .connecting || connection == .reasserting
    }

    func load() async {
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
        if let saved = try? LocalRules.load(from: LocalRules.defaultURL) {
            localRules = saved
        }
        switch Result(catching: { try NodeSecretStore.value(account: SubscriptionGroups.keychainAccount) }) {
        case let .success(.some(value)):
            if let saved = try? SubscriptionGroups.fromKeychainValue(value) {
                subscriptions = saved
                library = saved.activeLibrary()
            }
        case .failure:
            // The Keychain is not readable now (for example before the first unlock); keep what is shown.
            break
        case .success(.none):
            // A library saved before subscription groups existed becomes the first group.
            let previous = (try? NodeSecretStore.value(account: NodeLibrary.keychainAccount))
                .flatMap { $0 }
                .flatMap { try? NodeLibrary.fromKeychainValue($0) }
            store(SubscriptionGroups.migrating(previous, name: String(localized: "导入的节点")))
        }
        refresh()
        loaded = true
    }

    /// Nothing imported yet: no saved profile and no subscription groups.
    var hasNoNodes: Bool {
        nodeName == nil && subscriptions.groups.isEmpty
    }

    /// Parses pasted text for the import preview without saving anything. A subscription link is downloaded only
    /// when it is imported.
    func preview(_ link: String) -> Result<ImportPreview, ImportFailure> {
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

    func clearMessage() {
        message = nil
    }

    /// Saves the node as the tunnel profile. A subscription link is downloaded into its own subscription group (an
    /// existing group for that link is updated instead), and pasted multi-node text becomes a group too; either
    /// group becomes active. Returns whether it was saved; on failure `message` says why.
    @discardableResult
    func importLink(_ link: String) async -> Bool {
        if let url = SubscriptionLink.url(from: link) {
            return await updateSubscription(url, groupID: subscriptions.group(url: url.absoluteString)?.id, activate: true)
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
        return true
    }

    /// Downloads one subscription group again. The selected node, the rules switch and the group picks carry
    /// over where they still apply.
    @discardableResult
    func refreshSubscription(_ groupID: String) async -> Bool {
        guard let group = subscriptions.group(id: groupID), let link = group.url, let url = URL(string: link) else {
            return false
        }
        return await updateSubscription(url, groupID: groupID, activate: false)
    }

    /// Downloads every group that has a link, one after another.
    func refreshAllSubscriptions() async {
        for group in subscriptions.groups where group.isUpdatable {
            await refreshSubscription(group.id)
        }
    }

    /// Automatic update: downloads the groups whose interval has passed. Failures only mark the group, so a
    /// background run never leaves a message on screen.
    func refreshDueSubscriptions(at date: Date = Date()) async {
        let shown = message
        for group in subscriptions.due(at: date) {
            await refreshSubscription(group.id)
        }
        message = shown
        scheduleBackgroundRefresh()
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
        var groups = subscriptions
        let wasActive = groups.activeID == groupID
        groups.remove(id: groupID)
        let wasRunning = isConnected
        if wasActive, groups.active != nil {
            guard await saveActiveProfile(groups) else {
                return
            }
        }
        store(groups)
        latencies = latencies.filter { !$0.key.hasPrefix(Self.latencyKey(group: groupID, node: "")) }
        if wasActive, wasRunning, groups.active != nil {
            restart()
        }
    }

    /// Saves an edited node into its group. A running tunnel that may use the node, because its group is active or
    /// the active rules reach other groups, restarts on the edit.
    func updateNode(_ node: NativeImportReport.Node, inGroup groupID: String) async -> Bool {
        guard var group = subscriptions.group(id: groupID), let old = group.library.node(id: node.id) else {
            return false
        }
        guard old != node else {
            return true
        }
        group.library = group.library.replacing(node)
        return await changeGroup(group)
    }

    /// Deletes a node that the group's rules do not name (`NodeLibrary.canRemove`).
    func deleteNode(_ nodeID: String, inGroup groupID: String) async {
        guard var group = subscriptions.group(id: groupID), group.library.canRemove(nodeID) else {
            return
        }
        group.library = group.library.removing(nodeID)
        latencies.removeValue(forKey: Self.latencyKey(group: groupID, node: nodeID))
        _ = await changeGroup(group)
    }

    private func changeGroup(_ group: SubscriptionGroup) async -> Bool {
        var groups = subscriptions
        groups.upsert(group)
        let affectsTunnel = groups.activeID == group.id || library?.routesByRules == true
        let wasRunning = isConnected
        if affectsTunnel, groups.active != nil {
            guard await saveActiveProfile(groups) else {
                return false
            }
        }
        store(groups)
        if affectsTunnel, wasRunning {
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
        guard next != mode, let manager, let tunnelProtocol = manager.protocolConfiguration as? NETunnelProviderProtocol else {
            return
        }
        if next == .rule, library == nil {
            message = String(localized: "单个节点没有节点列表，无法按规则分流。导入订阅或多个节点后可用")
            return
        }
        // The rule and global modes are also the active subscription's rules switch.
        if next != .direct, var group = subscriptions.active, group.library.routing != nil,
           group.library.followsRules != (next == .rule) {
            group.library.followsRules = next == .rule
            var groups = subscriptions
            groups.upsert(group)
            store(groups)
        }
        var configuration = tunnelProtocol.providerConfiguration ?? [:]
        configuration[TunnelProfileKeys.mode] = next.rawValue
        if next == .rule {
            configuration[TunnelProfileKeys.routing] = true
        } else if next == .global {
            configuration.removeValue(forKey: TunnelProfileKeys.routing)
        }
        tunnelProtocol.providerConfiguration = configuration
        manager.protocolConfiguration = tunnelProtocol
        do {
            self.manager = try await persist(manager)
        } catch {
            message = String(localized: "保存模式到 VPN 配置失败") + Self.errorCode(error)
            refresh()
            return
        }
        refresh()
        guard isConnected else {
            return
        }
        let answer = RuntimeControl.decode(RuntimeControl.ModeAnswer.self, from: await control(.setMode(next)))
        if answer?.mode == next {
            _ = await control(.closeAll)
        } else {
            restart()
        }
    }

    /// Picks the member a `select` group uses. In the policy group of another subscription group this picks that
    /// group's selected node. A tunnel running by the rules restarts on it.
    func selectMember(_ member: String, inGroup groupID: String) async {
        guard let current = library,
              let group = current.routing?.groups.first(where: { $0.id == groupID }),
              group.isSelectable, group.members.contains(member), current.selection(in: group) != member
        else {
            return
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
            return
        }
        store(groups)
        if current.routesByRules, isConnected {
            // The engine switches select groups in place; an older one restarts on the new pick.
            let answer = RuntimeControl.decode(RuntimeControl.SelectAnswer.self, from: await control(.select(group: groupID, member: member)))
            if answer?.now != member {
                restart()
            }
        }
    }

    /// Makes a node of any group the tunnel's node; its group becomes active. A running tunnel restarts on it
    /// unless that group's rules pick the nodes.
    func selectNode(_ nodeID: String, inGroup groupID: String) async {
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
        let wasRunning = isConnected
        guard await saveActiveProfile(groups) else {
            return
        }
        store(groups)
        if wasRunning, !(sameGroup && group.library.routesByRules) {
            restart()
        }
    }

    /// Stops the tunnel and starts it again once it is down, so the extension reads the new profile and node list.
    private func restart() {
        restartAfterStop = true
        manager?.connection.stopVPNTunnel()
    }

    /// The `latencies` key of a node in a group; node ids repeat across subscriptions.
    static func latencyKey(group: String, node: String) -> String {
        "\(group)\u{1F}\(node)"
    }

    /// HTTP delay test of every node of every group through its own protocol, a few at a time (`NodeDelayFFI`).
    func testLatency() async {
        let nodes = subscriptions.groups.flatMap { group in
            group.library.nodes.map { node in
                (Self.latencyKey(group: group.id, node: node.id), node.standalone(among: group.library.nodes))
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

    /// Downloads a subscription into the group `groupID`, or a new group when nil. `activate` makes the group the
    /// tunnel's; a refresh of the active group saves the profile only when its node or rules switch changed.
    private func updateSubscription(_ url: URL, groupID: String?, activate: Bool) async -> Bool {
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
        } catch {
            markFailure(groupID)
            notice = AppNotice(
                text: String(localized: "订阅下载失败：链接可能已失效，或当前网络连不上订阅服务器。现有节点仍可使用。"),
                detail: error.localizedDescription,
                actions: groupID.map { [.refreshSubscription($0)] } ?? []
            )
            return false
        }
        let report: NativeImportReport
        do {
            report = try NativeImportReport.decoded(from: NativeNodeImporterFFI.importNodes(text))
        } catch let failure as NativeImportFailure {
            markFailure(groupID)
            message = Self.describe(.native(failure))
            return false
        } catch {
            markFailure(groupID)
            message = Self.describe(.native(.unavailable))
            return false
        }
        var groups = subscriptions
        let previous = groups.activeLibrary()
        let previousActiveID = groups.activeID
        let group = groupID.flatMap { groups.group(id: $0) }?.refreshed(with: report, response: response)
            ?? SubscriptionGroup(report: report, url: url.absoluteString, response: response)
        guard group.library.selected != nil else {
            markFailure(groupID)
            message = Self.describe(.native(.noSupportedNode(
                protocols: report.unsupported.map(\.protocol),
                reasons: []
            )))
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
            if !unchanged {
                guard await saveActiveProfile(groups) else {
                    return false
                }
            }
        }
        // Other groups may have changed while this one downloaded.
        var latest = subscriptions
        latest.upsert(group)
        latest.activeID = groups.activeID
        store(latest)
        latencies = latencies.filter { !$0.key.hasPrefix(Self.latencyKey(group: group.id, node: "")) }
        return true
    }

    private func markFailure(_ groupID: String?) {
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

    private static func download(_ url: URL) async throws -> (String, SubscriptionResponseInfo) {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 30)
        request.setValue("AnixOpsNet/1.0", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode),
              data.count <= maxSubscriptionBytes
        else {
            throw URLError(.badServerResponse)
        }
        return (String(decoding: data, as: UTF8.self), SubscriptionResponseInfo(header: { http.value(forHTTPHeaderField: $0) }))
    }

    /// Saves the groups and the library the extension reads, both to the Keychain.
    private func store(_ groups: SubscriptionGroups) {
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
    }

    /// iOS runs `refreshDueSubscriptions` in the background around the next group's update time
    /// (`NetworkCoreAppMain`). Nothing to schedule when every group updates by hand.
    func scheduleBackgroundRefresh() {
        #if os(iOS)
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: Self.backgroundRefreshID)
        guard let next = subscriptions.nextUpdate else {
            return
        }
        let request = BGAppRefreshTaskRequest(identifier: Self.backgroundRefreshID)
        request.earliestBeginDate = max(next, Date().addingTimeInterval(15 * 60))
        try? BGTaskScheduler.shared.submit(request)
        #endif
    }

    /// Listed under `BGTaskSchedulerPermittedIdentifiers` in project.yml.
    static let backgroundRefreshID = "net.anixops.subscriptions.refresh"

    /// Saves `profile` as the tunnel profile; `routed` makes the extension follow the node library's rules.
    private func save(_ profile: TunnelProfile, routed: Bool = false) async -> Bool {
        let profileID = Self.profileID
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
        do {
            keyReference = try NodeSecretStore.save(profile.config.secret.keychainValue(), account: profileID)
        } catch {
            message = String(localized: "无法把节点密钥保存到钥匙串")
            return false
        }

        let tunnelProtocol = NETunnelProviderProtocol()
        tunnelProtocol.providerBundleIdentifier = providerBundleIdentifier
        tunnelProtocol.serverAddress = profile.serverAddress
        tunnelProtocol.passwordReference = keyReference
        var providerConfiguration: [String: Any] = [
            TunnelProfileKeys.runtimeConfig: config,
            TunnelProfileKeys.tunnelInterface: interface,
            TunnelProfileKeys.nodeName: profile.displayName,
        ]
        if routed {
            providerConfiguration[TunnelProfileKeys.routing] = true
        }
        // With subscription rules the rules switch decides between the rule and global modes; a node list without
        // them keeps the mode the user picked (local rules), and a single node has no rule mode.
        let savedMode: ProxyMode
        if mode == .direct {
            savedMode = .direct
        } else if routed || (mode == .rule && library != nil && library?.routing == nil) {
            savedMode = .rule
        } else {
            savedMode = .global
        }
        providerConfiguration[TunnelProfileKeys.mode] = savedMode.rawValue
        if let value = try? dns.profileValue() {
            providerConfiguration[TunnelProfileKeys.dns] = value
        }
        if let value = try? redirects.profileValue() {
            providerConfiguration[TunnelProfileKeys.redirects] = value
        }
        if let value = try? localRules.profileValue() {
            providerConfiguration[TunnelProfileKeys.localRules] = value
        }
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
        do {
            try bundle.apply(to: &providerConfiguration)
        } catch {
            message = String(localized: "保存插件到 VPN 配置失败") + Self.errorCode(error)
            return false
        }
        tunnelProtocol.providerConfiguration = providerConfiguration

        guard await activateSystemExtension() else {
            return false
        }
        let target = manager ?? NETunnelProviderManager()
        target.protocolConfiguration = tunnelProtocol
        target.localizedDescription = "AnixOps Net"
        target.isEnabled = true
        do {
            manager = try await persist(target)
            message = nil
        } catch {
            message = String(localized: "保存 VPN 配置失败或未获授权") + Self.errorCode(error)
            refresh()
            return false
        }
        refresh()
        return true
    }

    /// Saves `target` and reloads it so its connection object reflects the saved profile. When the save fails, for
    /// example because the object went stale after a widget, the Control Center switch or Settings changed the
    /// configuration, or because the configuration was removed in Settings, the saved configuration is read again
    /// and the same settings are saved once more on that fresh object. Returns the object that was saved.
    private func persist(_ target: NETunnelProviderManager) async throws -> NETunnelProviderManager {
        do {
            try await target.saveToPreferences()
            try await target.loadFromPreferences()
            return target
        } catch {
            let fresh = (try? await NETunnelProviderManager.loadAllFromPreferences().first) ?? NETunnelProviderManager()
            fresh.protocolConfiguration = target.protocolConfiguration?.copy() as? NEVPNProtocol
            fresh.localizedDescription = target.localizedDescription
            fresh.isEnabled = target.isEnabled
            try await fresh.saveToPreferences()
            try await fresh.loadFromPreferences()
            return fresh
        }
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
        guard let manager,
              let tunnelProtocol = manager.protocolConfiguration as? NETunnelProviderProtocol
        else {
            return false
        }
        var configuration = tunnelProtocol.providerConfiguration ?? [:]
        let next = MitmProfileBundle(plugins: texts, scripts: scripts, resources: resources)
        let carriesOldKeys = configuration[TunnelProfileKeys.mitmPlugins] != nil
            || configuration[TunnelProfileKeys.mitmScripts] != nil
            || configuration[TunnelProfileKeys.mitmResources] != nil
        guard carriesOldKeys || MitmProfileBundle.fromProfile(configuration) != next else {
            return false
        }
        do {
            try next.apply(to: &configuration)
        } catch {
            message = String(localized: "保存插件到 VPN 配置失败") + Self.errorCode(error)
            return false
        }
        tunnelProtocol.providerConfiguration = configuration
        manager.protocolConfiguration = tunnelProtocol
        do {
            self.manager = try await persist(manager)
        } catch {
            message = String(localized: "保存插件到 VPN 配置失败") + Self.errorCode(error)
            refresh()
            return false
        }
        refresh()
        if isConnected {
            restart()
        }
        return true
    }

    /// Saves the DNS settings and puts them in the saved profile. A running tunnel reconnects to use them.
    /// Returns whether they were saved.
    @discardableResult
    /// Saves the local rules now, so quick edits build on each other, then puts them in the saved profile. A running
    /// tunnel reconnects to use them.
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
        guard let manager, let tunnelProtocol = manager.protocolConfiguration as? NETunnelProviderProtocol,
              let value = try? localRules.profileValue()
        else {
            // The next import picks them up.
            return
        }
        var configuration = tunnelProtocol.providerConfiguration ?? [:]
        guard configuration[TunnelProfileKeys.localRules] as? Data != value else {
            return
        }
        configuration[TunnelProfileKeys.localRules] = value
        tunnelProtocol.providerConfiguration = configuration
        manager.protocolConfiguration = tunnelProtocol
        do {
            self.manager = try await persist(manager)
        } catch {
            message = String(localized: "保存本地规则到 VPN 配置失败") + Self.errorCode(error)
            refresh()
            return
        }
        refresh()
        if isConnected {
            restart()
        }
    }

    func applyDns(_ next: DnsSettings) async -> Bool {
        do {
            try next.save(to: DnsSettings.defaultURL)
        } catch {
            message = String(localized: "无法保存 DNS 设置")
            return false
        }
        dns = next
        guard let manager, let tunnelProtocol = manager.protocolConfiguration as? NETunnelProviderProtocol else {
            // The next import picks them up.
            return true
        }
        var configuration = tunnelProtocol.providerConfiguration ?? [:]
        let value = try? next.profileValue()
        guard configuration[TunnelProfileKeys.dns] as? Data != value else {
            return true
        }
        if let value {
            configuration[TunnelProfileKeys.dns] = value
        } else {
            configuration.removeValue(forKey: TunnelProfileKeys.dns)
        }
        tunnelProtocol.providerConfiguration = configuration
        manager.protocolConfiguration = tunnelProtocol
        do {
            self.manager = try await persist(manager)
        } catch {
            message = String(localized: "保存 DNS 设置到 VPN 配置失败") + Self.errorCode(error)
            refresh()
            return false
        }
        refresh()
        if isConnected {
            restart()
        }
        return true
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
        guard let manager, let tunnelProtocol = manager.protocolConfiguration as? NETunnelProviderProtocol else {
            // The next import picks them up.
            return true
        }
        var configuration = tunnelProtocol.providerConfiguration ?? [:]
        let value = try? next.profileValue()
        guard configuration[TunnelProfileKeys.redirects] as? Data != value else {
            return true
        }
        if let value {
            configuration[TunnelProfileKeys.redirects] = value
        } else {
            configuration.removeValue(forKey: TunnelProfileKeys.redirects)
        }
        tunnelProtocol.providerConfiguration = configuration
        manager.protocolConfiguration = tunnelProtocol
        do {
            self.manager = try await persist(manager)
        } catch {
            message = String(localized: "保存重定向到 VPN 配置失败") + Self.errorCode(error)
            refresh()
            return false
        }
        refresh()
        if isConnected {
            restart()
        }
        return true
    }

    func toggle() {
        guard let manager = manager else {
            if let groups = Self.withSelectedNode(subscriptions) {
                // Nodes are imported but no VPN profile is saved yet (or it was removed in Settings): save the
                // profile for the selected node, then connect.
                Task { await saveProfileAndStart(groups) }
            } else {
                notice = AppNotice(text: String(localized: "请先导入节点"), actions: [.importNode])
            }
            return
        }
        if isConnected {
            manager.connection.stopVPNTunnel()
            return
        }
        guard manager.isEnabled else {
            // Turning on another VPN app in Settings disables this profile. The coordinator then reports the
            // tunnel as unavailable, and the button used to do nothing at all: re-enable the profile first, the
            // same way the widget and Shortcuts paths do.
            Task { await enableAndStart(manager) }
            return
        }
        guard coordinator.canStartTunnel else {
            message = String(localized: "无法启动隧道")
            return
        }
        start(manager)
    }

    private func enableAndStart(_ manager: NETunnelProviderManager) async {
        do {
            manager.isEnabled = true
            let saved = try await persist(manager)
            self.manager = saved
            refresh()
            start(saved)
        } catch {
            message = String(localized: "无法启动隧道") + Self.errorCode(error)
        }
    }

    private func saveProfileAndStart(_ groups: SubscriptionGroups) async {
        guard await saveActiveProfile(groups) else {
            return
        }
        store(groups)
        guard let manager = manager else {
            message = String(localized: "无法启动隧道")
            return
        }
        start(manager)
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

    private func start(_ manager: NETunnelProviderManager) {
        #if os(macOS)
        // A newer build may carry a newer System Extension; activation replaces the running one first.
        Task {
            guard await activateSystemExtension() else {
                return
            }
            startNow(manager)
        }
        #else
        startNow(manager)
        #endif
    }

    private func startNow(_ manager: NETunnelProviderManager) {
        do {
            try manager.connection.startVPNTunnel(options: Self.startOptions())
            message = nil
        } catch {
            message = String(localized: "无法启动隧道")
        }
    }

    /// Set once this run of the app has activated the tunnel System Extension.
    @ObservationIgnored private var systemExtensionActive = false

    /// On macOS the tunnel may be a System Extension, which runs as root and cannot read the user's Keychain, so
    /// the app hands over the node key, node library and MITM root at start (`TunnelStartSecrets`). iOS shares the
    /// Keychain with the extension and passes nothing.
    private static func startOptions() -> [String: NSObject]? {
        #if os(macOS)
        return TunnelStartSecrets.fromKeychain().options
        #else
        return nil
        #endif
    }

    /// Developer ID builds carry the tunnel as a System Extension, which macOS loads only after the app asks and the
    /// user allows it. Returns false, with the reason in `message`, when it is not active. Builds whose tunnel is
    /// an app extension (iOS, Mac App Store) have nothing to activate.
    private func activateSystemExtension() async -> Bool {
        #if os(macOS)
        guard !systemExtensionActive, let identifier = Self.systemExtensionIdentifier() else {
            return true
        }
        let outcome = await SystemExtensionActivation.run(identifier) { [weak self] in
            Task { @MainActor in
                self?.message = String(localized: "请在“系统设置 › 通用 › 登录项与扩展 › 网络扩展”中允许 AnixOps Net，然后再连接")
            }
        }
        switch outcome {
        case .activated:
            systemExtensionActive = true
            return true
        case .needsRestart:
            message = String(localized: "网络扩展将在重启 Mac 后生效")
        case .notInApplications:
            message = String(localized: "请先把 AnixOps Net 拖进“应用程序”文件夹，从那里打开后再连接")
        case let .failed(code):
            message = String(localized: "无法启用网络扩展") + " (\(code))"
        }
        return false
        #else
        return true
        #endif
    }

    /// Nodes whose stop the user has already been told about; a node leaves the set once it dials again.
    @ObservationIgnored private var announcedStops: Set<String> = []
    @ObservationIgnored private var reporter: ClientReporter?

    /// Reads the engine's per-node dial health: tells the user once when a node stopped because its server kept
    /// rejecting this client (expired subscription or wrong device time), and hands the counts to the panel
    /// reporter. `network` is `wifi`, `cellular`, `ethernet` or `unknown`.
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
        let reporter = self.reporter ?? ClientReporter()
        self.reporter = reporter
        reporter.offer(health, groups: subscriptions, network: network)
    }

    /// Lets a stopped node dial again after the user fixed the time or the subscription.
    func resumeNode(_ node: String) async {
        announcedStops.remove(node)
        _ = await control(.resumeOutbound(node: node))
    }

    /// Sends a control request to the running engine (live traffic, the connection list). Nil while the tunnel is
    /// not connected, or when the engine refused the request.
    func control(_ command: RuntimeControl) async -> Data? {
        guard let session = manager?.connection as? NETunnelProviderSession, connection == .connected else {
            return nil
        }
        return await withCheckedContinuation { continuation in
            do {
                try session.sendProviderMessage(TunnelAppMessage.control(command.request).encoded) { data in
                    continuation.resume(returning: data)
                }
            } catch {
                continuation.resume(returning: nil)
            }
        }
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
        let previous = connection
        connection = manager?.connection.status ?? .invalid
        connectedSince = connection == .connected ? manager?.connection.connectedDate : nil
        if connection == .connected, previous != .connected {
            lastConnections = nil
        }
        if restartAfterStop, connection == .disconnected {
            // The node switch stopped the tunnel on purpose: no error to show, just start it on the new node.
            restartAfterStop = false
            try? manager?.connection.startVPNTunnel(options: Self.startOptions())
        } else if connection == .disconnected, previous != .disconnected, previous != .invalid {
            showLastDisconnectError()
        }
        let configuration = (manager?.protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration
        let previousNode = nodeName
        nodeName = configuration?[TunnelProfileKeys.nodeName] as? String
        let previousMode = mode
        mode = ProxyMode.saved(in: configuration)
        if connection != previous || nodeName != previousNode || mode != previousMode {
            Self.reloadWidgets()
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
            Task { @MainActor in
                self?.notice = AppNotice(
                    text: String(localized: "连接已断开：节点没有响应、被干扰，或网络刚切换。可以重试，或换一个节点。"),
                    detail: text,
                    actions: [.retry, .chooseNode]
                )
                self?.lastDisconnectError = text
            }
        }
    }

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
                let names = Array(Set(protocols)).sorted().joined(separator: "、")
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
}

#if DEBUG
/// Demo data for screenshots: a debug build launched with `-AnixOpsDemo YES` shows a connected client with a
/// made-up subscription instead of reading the VPN configuration and the Keychain. Nothing is saved and no
/// tunnel starts, so it runs unsigned on a CI machine.
extension TunnelController {
    static var isDemo: Bool {
        UserDefaults.standard.bool(forKey: "AnixOpsDemo")
    }

    func loadDemo() {
        let credentials = "YWVzLTI1Ni1nY206ZGVtby1wYXNzd29yZA"
        let names = [("hk1", "香港 01"), ("hk2", "香港 02"), ("jp1", "日本 01"), ("sg1", "新加坡 01"), ("us1", "美国 01"), ("de1", "德国 01")]
        let links = names.map { host, name in
            "ss://\(credentials)@\(host).example.com:8388#\(name.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? host)"
        }
        let delays = [38, 45, 72, 86, 164, 213]
        if let data = try? NativeNodeImporterFFI.importNodes(links.joined(separator: "\n")),
           let report = try? NativeImportReport.decoded(from: data) {
            let day: TimeInterval = 24 * 3600
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
                    expire: Date().addingTimeInterval(23 * day)
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
        loaded = true
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
