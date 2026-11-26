import Foundation
import NetworkCoreBridge
import Observation

/// One JSON object of the mesh status or of a `MeshView` answer, read by key. Missing keys and `null` read as nil.
struct MeshObject {
    let raw: [String: Any]

    init(_ value: Any?) {
        raw = value as? [String: Any] ?? [:]
    }

    func string(_ key: String) -> String? {
        raw[key] as? String
    }

    func int(_ key: String) -> Int? {
        guard let number = raw[key] as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else {
            return nil
        }
        return number.intValue
    }

    func double(_ key: String) -> Double? {
        guard let number = raw[key] as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else {
            return nil
        }
        return number.doubleValue
    }

    func bool(_ key: String) -> Bool {
        (raw[key] as? Bool) ?? false
    }

    func object(_ key: String) -> MeshObject? {
        (raw[key] as? [String: Any]).map { MeshObject($0) }
    }

    func objects(_ key: String) -> [MeshObject] {
        (raw[key] as? [Any] ?? []).map { MeshObject($0) }
    }

    func strings(_ key: String) -> [String] {
        (raw[key] as? [Any] ?? []).compactMap { $0 as? String }
    }

    func values(_ key: String) -> [Any] {
        raw[key] as? [Any] ?? []
    }
}

/// The mesh (组网) status the overview entry and the mesh page show: the engine's `mesh_status` snapshot
/// (`docs/architecture/mesh-client.md`), read every 2 s while one of them is on screen. Nil while the tunnel is not
/// connected, and when the engine has no mesh module (it answers `unsupported`): the entry is then hidden.
@MainActor
@Observable
final class MeshModel {
    /// How often the snapshot is read while shown (R23: 2 s in the foreground).
    static let interval: Duration = .seconds(2)

    private(set) var status: [String: Any]?
    /// Why the last join, leave or route change did not take effect; nil once it did.
    private(set) var refusal: RuntimeControl.Refusal?
    private(set) var busy = false
    /// The networks saved on this device (`MeshNetworkLibrary`).
    private(set) var saved = MeshNetworkLibrary.load()

    /// The snapshot the entry and page show: the engine's while it runs; otherwise, with an engine that runs mesh
    /// networks, one made from the saved networks (the joined one, else the first), without members.
    private var shown: [String: Any]? {
        if let status {
            return status
        }
        guard MeshSupport.available else {
            return nil
        }
        guard let network = saved.first(where: \.joined) ?? saved.first else {
            return [:]
        }
        var shown: [String: Any] = [
            "id": network.name,
            "name": network.name,
            // Joined while the engine is down: the VPN is on its way up for it.
            "state": network.joined ? "joining" : "not_joined",
        ]
        if let security = network.security {
            shown["security"] = security
        }
        return ["network": shown]
    }

    var entry: MeshObject {
        MeshObject(MeshView.entry(status: shown))
    }

    var available: Bool {
        shown != nil && entry.bool("available")
    }

    var network: MeshObject? {
        shown.flatMap { MeshObject($0).object("network") }
    }

    var own: MeshObject? {
        status.flatMap { MeshObject($0).object("self") }
    }

    var notices: [MeshObject] {
        MeshView.notices(status: status).map { MeshObject($0) }
    }

    /// Reads the snapshot while the VPN runs, for the proxy or for the mesh alone.
    func poll(_ tunnel: TunnelController) async {
        saved = MeshNetworkLibrary.load()
        while !Task.isCancelled, tunnel.vpnConnection == .connected {
            await refresh(tunnel)
            try? await Task.sleep(for: Self.interval)
        }
        if tunnel.vpnConnection != .connected {
            status = nil
        }
    }

    func refresh(_ tunnel: TunnelController) async {
        let data = await tunnel.control(.meshStatus)
        guard let data, RuntimeControl.Refusal.from(data) == nil,
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            status = nil
            return
        }
        status = object
    }

    /// Joins or leaves the network the snapshot names. Joining and leaving do not touch the proxy (R23); the saved
    /// network and the profile follow once the engine took it.
    func setJoined(_ joined: Bool, tunnel: TunnelController) async {
        guard !joined || MeshSupport.enabled else {
            return
        }
        guard let id = network?.string("id") else {
            return
        }
        if status != nil {
            await send(joined ? .meshJoin(network: id) : .meshLeave(network: id), tunnel: tunnel)
            guard refusal == nil else {
                return
            }
        }
        guard saved.contains(where: { $0.name == id }) else {
            return
        }
        MeshNetworkLibrary.setJoined(id, joined)
        saved = MeshNetworkLibrary.load()
        _ = await tunnel.applyMeshNetworks()
        // The VPN is up while the proxy or the mesh is on.
        await tunnel.meshJoinedChanged()
    }

    /// Saves a network from 添加网络 with its secret and joins it: in the running engine at once (`mesh_save_network`,
    /// then `mesh_join`, leaving the network joined before, since a phone joins one at a time), and in the profile
    /// for the next start. False when the secret could not be saved; the engine's refusal shows on the page.
    func addNetwork(_ added: MeshNetwork, secret: String, tunnel: TunnelController) async -> Bool {
        // The release gate holds here too, not only at the entry (`MeshSupport.enabled`).
        guard MeshSupport.enabled else {
            return false
        }
        do {
            try MeshNetworkLibrary.save(added, secret: secret)
        } catch {
            return false
        }
        saved = MeshNetworkLibrary.load()
        if tunnel.vpnConnection == .connected, let config = added.runtimeConfigJSON(secret: secret) {
            if let current = network, let id = current.string("id"), id != added.name,
               ["joined", "joining", "partial"].contains(current.string("state") ?? "")
            {
                await send(.meshLeave(network: id), tunnel: tunnel)
            }
            await send(.meshSaveNetwork(configJSON: config), tunnel: tunnel)
            if refusal == nil {
                await send(.meshJoin(network: added.name), tunnel: tunnel)
            }
        }
        _ = await tunnel.applyMeshNetworks()
        await tunnel.meshJoinedChanged()
        return true
    }

    /// Removes the network the snapshot names: from the running engine, the saved networks, the Keychain and the
    /// profile.
    func removeNetwork(tunnel: TunnelController) async {
        guard let id = network?.string("id") else {
            return
        }
        if status != nil {
            await send(.meshRemoveNetwork(network: id), tunnel: tunnel)
        }
        MeshNetworkLibrary.remove(id)
        saved = MeshNetworkLibrary.load()
        _ = await tunnel.applyMeshNetworks()
        await tunnel.meshJoinedChanged()
    }

    /// Whether a subnet that overlaps this device's network goes through the mesh (`mesh`) or stays local (`local`).
    func setRoute(_ route: String, cidr: String, tunnel: TunnelController) async {
        guard let id = network?.string("id") else {
            return
        }
        await send(.meshSetSubnetRoute(network: id, cidr: cidr, route: route), tunnel: tunnel)
    }

    /// The network's default path policy: `latency` or `hops` (kernel 0.5).
    func setPolicy(_ policy: String, tunnel: TunnelController) async {
        guard let id = network?.string("id") else {
            return
        }
        await send(.meshSetPolicy(network: id, policy: policy), tunnel: tunnel)
    }

    /// The custom paths before the last removal, for 撤销 (T0); nil when there is nothing to undo.
    private(set) var removedPaths: [MeshRoutingPath]?

    /// Removes a custom path, by id; the list before it can be put back with `undoPathRemoval`.
    func removePath(_ pathID: String, tunnel: TunnelController) async {
        guard let id = network?.string("id"),
              let before = MeshPaths.routing(status: status),
              let after = MeshPaths.routing(status: status, remove: pathID)
        else {
            return
        }
        await send(.meshSetPaths(network: id, paths: after), tunnel: tunnel)
        removedPaths = refusal == nil ? before : nil
    }

    func undoPathRemoval(tunnel: TunnelController) async {
        guard let id = network?.string("id"), let paths = removedPaths else {
            return
        }
        removedPaths = nil
        await send(.meshSetPaths(network: id, paths: paths), tunnel: tunnel)
    }

    /// Adds a custom path, replacing one to the same target. False when a member on it has no virtual address.
    func addPath(_ draft: MeshPaths.Draft, tunnel: TunnelController) async -> Bool {
        guard let id = network?.string("id"), let paths = MeshPaths.routing(status: status, add: draft) else {
            return false
        }
        removedPaths = nil
        await send(.meshSetPaths(network: id, paths: paths), tunnel: tunnel)
        return true
    }

    func clearRefusal() {
        refusal = nil
    }

    private func send(_ command: RuntimeControl, tunnel: TunnelController) async {
        busy = true
        defer { busy = false }
        let answer = await tunnel.control(command)
        refusal = answer == nil ? RuntimeControl.Refusal(code: "other", detail: nil) : RuntimeControl.Refusal.from(answer)
        await refresh(tunnel)
    }
}

/// The words for the mesh status values (`docs/mesh-ui.md`, 用词 and 状态).
enum MeshText {
    static func state(_ state: String?) -> String {
        switch state {
        case "joined":
            return String(localized: "已加入")
        case "joining":
            return String(localized: "正在加入")
        case "partial":
            return String(localized: "部分不可达")
        case "failed":
            return String(localized: "无法加入")
        default:
            return String(localized: "未加入")
        }
    }

    static func security(_ security: String?) -> String? {
        switch security {
        case "forward_secrecy":
            return String(localized: "前向保密")
        case "standard":
            return String(localized: "未启用前向保密")
        case "none":
            return String(localized: "未加密")
        default:
            return nil
        }
    }

    static func role(_ role: String?) -> String? {
        switch role {
        case "border":
            return String(localized: "边界")
        case "gateway":
            return String(localized: "网关")
        case "host":
            return String(localized: "宿主")
        case "server":
            // 主机 alone is a host name elsewhere; the role has its own key.
            return String(localized: "mesh.role.server")
        case "client":
            return String(localized: "接入端")
        default:
            return nil
        }
    }

    static func connection(_ connection: String?, hops: Int) -> String {
        switch connection {
        case "self":
            return String(localized: "本机")
        case "direct":
            // 直连 between members is Peer-to-peer; the rule policy 直连 (Direct) has its own key.
            return String(localized: "mesh.connection.direct")
        case "relay":
            return String(localized: "中转 \(hops) 跳")
        case "cross_region":
            return String(localized: "跨区 \(hops) 跳")
        case "via_host":
            return String(localized: "经宿主")
        case "via_gateway":
            return String(localized: "经网关")
        default:
            return String(localized: "不可达")
        }
    }

    static func latency(_ milliseconds: Int?) -> String {
        guard let milliseconds else {
            return String(localized: "超时")
        }
        return "\(milliseconds) ms"
    }

    static func mark(_ mark: String) -> String? {
        switch mark {
        case "duplicate":
            return String(localized: "重名")
        case "unmanaged":
            return String(localized: "地址未受管")
        case "fallback":
            return String(localized: "已回落")
        case "first_hop_only":
            return String(localized: "仅首跳生效")
        case "multi_source":
            return String(localized: "多源")
        case "overlap":
            return String(localized: "重叠")
        case "local_overlap":
            return String(localized: "与本机网络重叠")
        case "refused":
            return String(localized: "已拒绝")
        default:
            return nil
        }
    }

    static func pathState(_ state: String?) -> String {
        switch state {
        case "custom", "active":
            return String(localized: "自定义")
        case "fallback":
            return String(localized: "已回落")
        case "first_hop_only":
            return String(localized: "仅首跳生效")
        default:
            return String(localized: "自动")
        }
    }

    static func policy(_ policy: String?) -> String {
        policy == "hops" ? String(localized: "跳数最低") : String(localized: "延迟最低")
    }

    static func joiningStage(_ stage: String?) -> String? {
        switch stage {
        case "connecting":
            return String(localized: "正在连接对端")
        case "assigning_address":
            return String(localized: "正在取得虚拟地址")
        case "measuring_regions":
            return String(localized: "正在测量延迟以确定地区")
        default:
            return nil
        }
    }

    /// A failure reason (`network.failure.code`, or the code of a refused command), in one sentence.
    static func failure(_ code: String?) -> String {
        switch code {
        case "unencrypted_refused":
            return String(localized: "已拒绝加入：网络未加密。在网络设置中允许后方可加入。")
        case "secret_mismatch":
            return String(localized: "加入失败：密钥不匹配。核对网络名称与密钥。")
        case "security_mismatch":
            return String(localized: "加入失败：加密模式与网络不一致。")
        case "port_in_use":
            return String(localized: "无法启动组网：监听端口已被占用。")
        case "tun_in_use":
            return String(localized: "无法启动组网：虚拟网卡已被占用。")
        case "route_overlap":
            return String(localized: "无法启动组网：虚拟网段与现有路由重叠。")
        case "address_in_use":
            return String(localized: "无法加入：虚拟地址已被其他成员使用。")
        case "permission_denied":
            return String(localized: "无法启动组网：缺少系统权限。")
        case "identity_online":
            return String(localized: "无法加入：本机身份已在其他设备上线。")
        case "existing_program":
            return String(localized: "无法启动组网：本机正在运行现有组网程序。")
        case "unreachable_peers":
            return String(localized: "无法加入：对端均不可达。")
        case "peer_not_native":
            return String(localized: "已拒绝对端连接：对端非本系统节点。")
        case "native_keys_pending":
            return String(localized: "暂未加入：本机公钥尚未列入网络名单。")
        case "unsupported":
            return String(localized: "当前内核不支持此操作。")
        case "invalid_argument":
            return String(localized: "操作未完成：参数无效。")
        case "not_found":
            return String(localized: "操作未完成：网络不存在。")
        case "not_allowed":
            return String(localized: "操作未完成：网络不允许此操作。")
        default:
            return String(localized: "操作未完成。")
        }
    }

    /// The text of a `view.mesh_notices` entry (`docs/mesh-ui.md`, 提示).
    static func notice(_ notice: MeshObject) -> String {
        let values = notice.values("values")
        func text(_ index: Int) -> String {
            guard index < values.count else {
                return ""
            }
            return values[index] as? String ?? "\(values[index])"
        }
        func number(_ index: Int) -> Int? {
            guard index < values.count, let value = values[index] as? NSNumber else {
                return nil
            }
            return value.intValue
        }
        let id = notice.string("id") ?? ""
        switch id {
        case "mesh.notice.path_fallback":
            return String(localized: "自定义路径已回落：\(text(0)) 不可达。\(text(1)) 使用默认路径。")
        case "mesh.notice.path_unsupported":
            return String(localized: "自定义路径已回落：\(text(0)) 不支持按此路径转发。\(text(1)) 使用默认路径。")
        case "mesh.notice.path_pending_timeout":
            return String(localized: "自定义路径已回落：\(text(0)) 确认超时。\(text(1)) 使用默认路径。")
        case "mesh.notice.path_no_echo":
            return String(localized: "自定义路径已回落：\(text(0)) 探测无响应。\(text(1)) 使用默认路径。")
        case "mesh.notice.path_hop_limit":
            return String(localized: "自定义路径已回落：跳数超出上限。\(text(0)) 使用默认路径。")
        case "mesh.notice.detour_loss":
            let loss = number(3) ?? 0
            return String(localized: "\(text(0))至\(text(1))已改经\(text(2))：直连链路丢包 \(loss)%。")
        case "mesh.notice.detour_latency":
            if let latency = number(3) {
                return String(localized: "\(text(0))至\(text(1))已改经\(text(2))：直连链路延迟 \(latency) ms。")
            }
            return String(localized: "\(text(0))至\(text(1))已改经\(text(2))：直连链路延迟过高。")
        case "mesh.notice.partial":
            let names = (values.count > 1 ? values[1] as? [String] : nil) ?? []
            let list = names.formatted(.list(type: .and))
            return String(localized: "\(names.count) 台成员不可达：\(list)。")
        default:
            // `failure:<code>`, or `failure:<code>:<peer>` for a refused peer.
            let code = notice.string("key").flatMap {
                $0.hasPrefix("failure:") ? $0.dropFirst(8).split(separator: ":", maxSplits: 1).first.map(String.init) : nil
            }
            return failure(code)
        }
    }

    /// The raw detail of a failure notice (the port, the overlapping network), shown under 技术详情.
    static func noticeDetail(_ notice: MeshObject) -> String? {
        guard notice.string("key")?.hasPrefix("failure:") == true, let value = notice.values("values").first else {
            return nil
        }
        return value as? String ?? "\(value)"
    }
}

#if DEBUG
/// The mesh status of the demo (`-AnixOpsDemo YES`, screenshots): a joined network with members in two regions.
enum MeshDemo {
    static let status = #"{"statusVersion":1,"available":true,"network":{"id":"office","name":"office","state":"joined","security":"forward_secrecy","failure":null,"joining":null,"routingPolicy":"latency","dnsSuffix":"mesh.internal","dnsEnabled":true},"self":{"id":"m-self","name":"par-phone","role":"client","address":"10.144.144.5/24","addressManaged":true,"domain":"par-phone.eu.mesh.internal","region":"eu","regionBasis":{"border":"fra-border","latencyMs":12},"regionState":"auto"},"regions":[{"code":"eu","name":"欧洲","local":true},{"code":"ap","name":"亚太","local":false}],"members":[{"id":"m-1","name":"fra-border","role":"border","region":"eu","state":"online","address":"10.144.144.1/24","domain":"fra-border.eu.mesh.internal","addressManaged":true,"connection":"direct","hops":0,"transport":"udp","latencyMs":11,"path":["m-1"],"pathState":"auto","subnets":[],"vms":[],"duplicateOf":[]},{"id":"m-2","name":"ams-gw","role":"gateway","region":"eu","state":"online","address":"10.144.144.2/24","domain":"ams-gw.eu.mesh.internal","addressManaged":true,"connection":"relay","hops":1,"transport":"udp","latencyMs":24,"path":["m-1","m-2"],"pathState":"auto","subnets":["192.168.20.0/24"],"vms":[],"duplicateOf":[]},{"id":"m-3","name":"lon-host","role":"host","region":"eu","state":"online","address":"10.144.144.3/24","domain":"lon-host.eu.mesh.internal","addressManaged":true,"connection":"direct","hops":0,"transport":"tcp","latencyMs":9,"path":["m-3"],"pathState":"auto","subnets":[],"vms":[{"name":"build-1","domain":"build-1.eu.mesh.internal","address":"10.144.150.2"},{"name":"db-1","domain":"db-1.eu.mesh.internal","address":"10.144.150.3"}],"duplicateOf":[]},{"id":"m-4","name":"tyo-border","role":"border","region":"ap","state":"online","address":"10.144.146.1/24","domain":"tyo-border.ap.mesh.internal","addressManaged":true,"connection":"cross_region","hops":1,"transport":"udp","latencyMs":228,"path":["m-1","m-4"],"pathState":"auto","subnets":["192.168.30.0/24"],"vms":[],"duplicateOf":[]}],"subnets":[{"cidr":"192.168.20.0/24","region":"eu","advertisers":[{"member":"m-2","inUse":true}],"latencyMs":26,"path":["m-1","m-2"],"state":"ok","overlaps":[],"localOverlap":false,"route":"mesh"},{"cidr":"192.168.30.0/24","region":"ap","advertisers":[{"member":"m-4","inUse":true}],"latencyMs":231,"path":["m-1","m-4"],"state":"ok","overlaps":[],"localOverlap":false,"route":"mesh"}],"links":[{"from":"eu","to":"ap","via":null,"borders":["m-1","m-4"],"latencyMs":214,"loss":0.0,"state":"ok","reason":null}],"refusedAdvertisements":0}"#
}
#endif
