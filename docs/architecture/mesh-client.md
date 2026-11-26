# 组网：客户端与内核的接口

状态：草案（2026-11-25）。界面说明见 [`../mesh-ui.md`](../mesh-ui.md)，内核模块见 networkcore_anixops
`docs/architecture/mesh-module.md`。本页约定客户端从内核读什么、对内核做什么，以及各平台共用的界面逻辑放在哪里。
内核接口的正本是内核仓库的 [`docs/architecture/mesh-status.md`](https://github.com/AnixOps/AnixOps-core/blob/main/docs/architecture/mesh-status.md)（AnixOps-core #296），字段以那份为准；本页只记客户端怎么用。

## 分版本

| 客户端 | 内核 | 界面内容 |
|---|---|---|
| 1.1 | 0.3 | 加入开关（独立于连接开关）、网络状态与加密模式、成员与子网视图、内网域名、添加与移除网络 |
| 1.2 | 0.4 | 地区分组与归属依据、边界与宿主组、地址分配（固定 / 自动）、跨区链路状态 |
| 1.3 | 0.5 | 自定义路径与回落提示、个人电脑用入网凭据加入 |

内核没有组网能力（`capabilities.mesh` 不存在或为 `false`）时，各平台不显示组网入口。版本内尚未提供的字段缺省即可，
界面按缺省值降级：没有 `regions` 时成员不分组，没有 `links` 时路径页不显示跨区链路，没有 `paths` 时不显示自定义路径。

## 读取：状态快照

内核给出一份完整快照，界面不自行拼接多次查询。字段名用 lowerCamelCase，未知字段忽略，枚举遇到未知取值按 `other` 处理。

```json
{
  "statusVersion": 1,
  "network": {
    "id": "office",
    "name": "office",
    "state": "joined",
    "security": "standard",
    "failure": null,
    "joining": null,
    "routingPolicy": "latency",
    "dnsSuffix": "mesh.internal",
    "dnsEnabled": true
  },
  "self": {
    "id": "m-self",
    "name": "par-laptop",
    "role": "client",
    "address": "10.144.144.5/24",
    "addressV6": null,
    "addressManaged": true,
    "domain": "par-laptop.eu.mesh.internal",
    "region": "eu",
    "regionBasis": { "border": "fra-border", "latencyMs": 12 },
    "regionState": "auto",
    "relay": false,
    "hostMode": false,
    "listen": [{ "transport": "tcp", "port": 11010 }],
    "advertised": [],
    "suggestedSubnets": [{ "cidr": "192.168.10.0/24", "interface": "en0", "public": false }],
    "ipForwarding": "not_needed"
  },
  "regions": [{ "code": "eu", "name": "欧洲", "local": true }],
  "members": [
    {
      "id": "m-1",
      "name": "fra-border",
      "role": "border",
      "region": "eu",
      "state": "online",
      "address": "10.144.144.1/24",
      "domain": "fra-border.eu.mesh.internal",
      "addressManaged": true,
      "connection": "direct",
      "hops": 0,
      "transport": "udp",
      "latencyMs": 11,
      "path": ["m-1"],
      "pathState": "auto",
      "unreachable": null,
      "subnets": [],
      "vms": [],
      "duplicateOf": []
    }
  ],
  "subnets": [
    {
      "cidr": "192.168.20.0/24",
      "region": "eu",
      "advertisers": [{ "member": "m-3", "inUse": true }],
      "latencyMs": 24,
      "path": ["m-1", "m-3"],
      "state": "ok",
      "overlaps": [],
      "localOverlap": false,
      "route": "mesh"
    }
  ],
  "links": [
    {
      "from": "eu", "to": "cn", "via": null, "borders": ["m-1", "m-9"],
      "latencyMs": 180, "loss": 0.0, "state": "ok", "reason": null
    }
  ],
  "paths": [
    { "id": "p-1", "target": { "subnet": "192.168.20.0/24" }, "via": ["m-4"], "state": "active", "unreachable": null }
  ],
  "refusedAdvertisements": 0
}
```

### 取值

| 字段 | 取值 |
|---|---|
| `network.state` | `joined`、`joining`、`partial`（部分不可达）、`not_joined`、`failed` |
| `network.security` | `forward_secrecy`、`standard`（未启用前向保密）、`none`（未加密） |
| `network.failure` | `{ "code": …, "detail": "…" }`；`code` 为 `secret_mismatch`、`security_mismatch`、`unencrypted_refused`、`port_in_use`（`detail` 为端口）、`tun_in_use`、`route_overlap`（`detail` 为网段）、`address_in_use`、`identity_online`、`permission_denied`、`unreachable_peers`、`peer_not_native`（managed 网络只接受 `network.native_keys` 中的静态公钥，对端非本系统节点时拒绝；`detail` 为对端节点 ID）、`native_keys_pending`（managed 网络的名单尚未列入本机公钥，网络暂缓，状态为 `not_joined`；`detail` 不显示），与安装器的冲突码一致。`failure` 不依赖 `state`：`unreachable_peers`、`peer_not_native` 出现时网络照常运行，提示仍按 `failure` 生成。`existing_program` 不由内核给出，桌面客户端或安装器检测到现有组网程序时自行填入 |
| `network.joining` | `{ "stage": "connecting" \| "assigning_address" \| "measuring_regions", "peersConnected": 2 }` |
| `routingPolicy` | `latency`、`hops` |
| `role` | `border`（边界）、`gateway`（网关）、`host`（宿主）、`server`（主机）、`client`（接入端）；内核 0.3 只有 `server`（完整节点）和 `client`（接入节点） |
| `member.state` | `online`、`unreachable` |
| `connection` | `self`、`direct`（直连）、`relay`（中转，`hops` 为跳数）、`cross_region`（跨区，`hops`）、`via_host`、`via_gateway`、`unreachable` |
| `transport` | `tcp`、`udp` |
| `latencyMs` | 整数；测不通为 `null` |
| `pathState` | `auto`、`custom`、`fallback`（已回落，`unreachable` 为不可达成员的 id）、`first_hop_only` |
| `vms` | `[{ "name", "domain", "address" }]`，宿主行的虚拟机 |
| `duplicateOf` | 同一地区内重名成员的 id |
| `subnet.state` | `ok`、`unreachable`、`refused`（公网网段，未安装路由） |
| `subnet.overlaps` | 与之重叠的其他通告网段 |
| `subnet.localOverlap` | 与本机所在网络重叠；此时 `route` 默认为 `local`（使用本机网络），用户可改为 `mesh` |
| `link.state` | `ok`、`detour`（已自动切换，`via` 为绕行地区，`reason` 为 `{ "loss": 0.12 }` 或 `{ "latencyMs": 400 }`）、`down` |
| `path.state` | `active`、`fallback`（`unreachable` 为不可达成员）、`first_hop_only` |
| `regionState` | `auto`、`pinned`、`pending`（变更申请待批准） |
| `member.id` | 不透明字符串（对端实例 UUID，重启不变）；不得用数字对端编号 |
| `ipForwarding` | `on`、`off`、`not_needed`；仅桌面与服务器的完整节点给出 |
| `self.suggestedSubnets` | 完整节点自动发现的本机局域网网段，作为建议列出，用户确认后经 `mesh_advertise` 通告；`public` 为 `true` 的不可通告（R17）；仅完整节点给出 |

多源子网用 `advertisers` 的多项表示，`inUse` 标出使用中的一项。子网内手动命名的设备只存在客户端本机，不经内核。

## 控制

| 命令 | 参数 | 版本 |
|---|---|---|
| `mesh_status` | 无 | 1.1 |
| `mesh_join` / `mesh_leave` | `network` | 1.1 |
| `mesh_save_network` | 网络配置（内核 `type: mesh` 的 `config_version: 1` 结构，`network.name` 即网络 id；可选 `node.hostname`、`node.mtu`、`node.relay`、`routing.system_routes`；密钥由钥匙串注入） | 1.1 |
| `mesh_remove_network` | `network` | 1.1 |
| `mesh_set_dns` | `enabled`、`suffix` | 1.1 |
| `mesh_set_subnet_route` | `cidr`、`route`（`local` \| `mesh`）；写入本机覆盖设置，不改网络配置 | 1.1 |
| `mesh_set_relay` / `mesh_set_host_mode` | `enabled`（仅完整节点） | 1.2 |
| `mesh_advertise` / `mesh_withdraw` | `cidr`（仅完整节点） | 1.2 |
| `mesh_request_region` | `region`（交由网络管理者批准） | 1.2 |
| `mesh_set_policy` | `policy` | 1.3 |
| `mesh_set_paths` | `paths`（与内核 `routing.paths` 同结构） | 1.3 |
| `mesh_join_credential` | 一次性入网凭据（个人电脑） | 1.3 |

命令失败时返回 `{ "ok": false, "error": { "code": …, "detail": … } }`，`code` 为 `unsupported`、`invalid_argument`、`not_found`、`not_allowed` 或上文的某个失败原因；界面按「错误」状态写明原因和下一步。

各平台的通道（命令名在三条通道上相同，未编进组网时返回 `unsupported`）：

- iOS、macOS：组网与代理共用一个 VPN 和一个引擎实例，命令走现有的 `networkcore_ios_runtime_control`，
  请求体 `{ "cmd": "mesh_status", … }`；引擎带组网时 `networkcore_ios_feature_set_version` 为 7。
  App 经 NetworkExtension 消息转发。
- Android：JNI 桥转到 `client-core` 的同名命令。
- Windows、Linux：窗口经 `client-ipc` 帧发同名命令到后台服务。

## 保存的网络与配置

网络配置（含 `network.secret`）由客户端保存，引擎每次启动时随配置下发；运行中的增删与加入退出同时直接发给引擎，不重启。
同一设备同时只加入一个网络，加入一个即退出其他网络。

- 配置写法：每个已加入的网络是 `routing.outbounds` 的一项，值为 `{"type":"mesh", …config_version 1 配置}`。
  键只含 `[A-Za-z0-9._-]`：名称本身合规时为 `mesh-<名称>`，否则为 `mesh-x` 加名称 UTF-8 字节的十六进制，
  各平台相同。规则不必提到它们：引擎把各网络的 `tunnel.routes` 先于规则送入对应出站（R23）。
  配置只有单个出站时，改写为路由表：该出站为 `node`，`rules` 为 `["FINAL,node"]`，`global` 为 `node`。
  引擎不接受带组网出站的配置时（未编进组网），照常不带组网启动，并记入日志或诊断。
- iOS、macOS：App 设置保存不含密钥的配置，密钥在共享钥匙串（账户 `mesh-network.<名称>`）；
  隧道配置的 `networkcore.mesh-networks` 只带已加入的网络，扩展启动时补上密钥；macOS 系统扩展经启动密钥取得。
- iOS、macOS 只开组网：代理断开而有网络已加入时，隧道配置标记 `networkcore.mesh-only`，VPN 重启为只带组网出站的配置
  （`rules` 为 `["FINAL,DIRECT"]`，`mode` 为 `rule`，不需要节点密钥）；`includedRoutes` 只放 `tunnel.routes`，
  内网域名开启时 DNS 只对 `tunnel.dns.matchDomains` 生效。扩展每 2 秒读一次 `mesh_status`，`routesVersion` 变化时重新应用，
  间隔至少 1 秒。代理连上时去掉标记并重启；退出最后一个网络时关闭 VPN。入口在 `feature_set_version` ≥ 7 时显示。
- Android 只开组网：代理断开（或连接失败、Wi-Fi 规则断开）而有网络已加入时，`client-core` 不关 VPN，改为以只带组网的接口
  重新打开（地址与 MTU 同代理，起初没有路由），再在该描述符上启动只带组网出站的引擎（`rules` 为 `["FINAL,DIRECT"]`），
  与代理引擎分开，连接状态仍为未连接。每 2 秒读一次 `mesh_status`，接口所需的路由或 DNS 变化时重新打开接口，间隔至少 1 秒；
  新描述符在运行中的引擎下替换旧的，组网连接不断。内网域名开启时 DNS 服务器为隧道地址并加主机路由；Android 的 VPN
  没有按后缀的 DNS，此时所有域名由引擎解析，内网后缀走组网。代理连上时停掉只开组网的引擎，接口按代理重新打开；
  退出最后一个网络时关闭 VPN。未运行引擎时 `mesh_status` 由保存的网络作答（加入的网络，否则第一个，无成员），
  入口照常显示；引擎是否支持组网由能否接受只开组网的配置判断。断开时加入网络先取得 VPN 授权。
- Windows 只开组网：代理断开而有网络已加入时，窗口在自己的虚拟网卡上启动只带组网出站的引擎（与代理共用固定 GUID，
  两者不同时运行）。网卡起初没有路由，每 2 秒读一次 `mesh_status`，只在 `tunnel.routes` 变化时差量替换；网卡不向系统登记 DNS，
  也不拦截外部 DNS；内网域名开启时，每个 `tunnel.dns.matchDomains` 后缀以名称解析策略表（NRPT）规则指向
  `tunnel.dns.servers`（IPv4 地址在前；规则注释固定为 `AnixOps Net mesh`，其余域名仍用本机 DNS），引擎停止时删除。
  规则对整机所有用户和程序生效，因此：
  - 后缀须至少两段且以 `internal` 结尾（如 `mesh.internal`、`eu.mesh.internal`）；单段名、公共后缀、`local`、`arpa`
    及根一律不写规则，被拒的后缀写入日志并在组网页提示（与内核侧 F-03 的限制一致）。
  - 本机组网网段（`self.address`、`self.addressV6`）的反向解析区（`*.in-addr.arpa`、`*.ip6.arpa`）同样指向组网 DNS；
    前缀不在标签边界时展开为下一级的各区，超过 16 个区的网段不写（写入日志）。
  - 同一地区只写主机名：本机所在地区的后缀（`self.domain` 去掉主机名，如 `eu.mesh.internal`）设为组网网卡的连接后缀，
    Windows 对单段名称追加该后缀后按规则解析；组策略下发了全局后缀搜索列表时，连接后缀不生效。
  - 每次写入或删除规则后清空 DNS 缓存。写入失败时按 5、10、20、40 秒、之后每 60 秒重试，组网页提示「内网域名暂不可用」。
  - 组策略下发了名称解析策略表规则时（`HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DNSClient\DnsPolicyConfig`
    下有规则），本机规则被系统忽略，组网页提示由组策略管理。
  - 规则只在本进程持有下述网卡锁时改动：只开组网的引擎运行期间持锁；崩溃残留由首次 `settle` 在取得锁后清理，
    另一用户的窗口占用网卡时不清理（规则属于对方），留待下次。卸载（MSI `--purge`）时同样按注释删除。
  - PowerShell 与 rundll32 以系统目录下的绝对路径启动。
  - 同一时间只加入一个网络，所有后缀对应同一组解析地址；以后允许同时加入多个网络时，须改为按网络分别对应。
  网卡 GUID 全机唯一，而每个登录用户各运行一个窗口：
  打开网卡前独占 `%ProgramData%` 数据目录下的 `tun-adapter.lock`，另一用户的窗口已占用时报 `tun_in_use`，不替换对方网卡。
  启动失败时组网页按系统错误码给出提示（拒绝访问为 `permission_denied`，上述占用为 `tun_in_use`，其余为通用失败，
  不按错误文本猜测），并按 5、10、20、40 秒、之后每 60 秒重试；网络变化时立即重试；
  引擎自行退出也按此重试。窗口退出、注销、关机或重启时显式停止该引擎并撤下网卡与路由；注销被取消时恢复。崩溃时网卡随进程
  移除，下次启动按同一 GUID 替换。
- 组网路由的范围（所有平台，`client-core` `mesh::too_wide`，Apple `MeshTunnel.tooWide`）：隧道不承载宽于 IPv4 /8、
  IPv6 /7 的组网路由，包括默认路由及其两半（`0.0.0.0/1`、`128.0.0.0/1`、`::/1`、`8000::/1`），无论引擎给出什么，
  与内核规范 intranet-mesh §15.4（U2）一致。被拒的路由写入日志。
- Android、Windows、Linux：`client-core` 把配置连同密钥存在数据目录的 `mesh-networks.json`（仅所有者可读），
  窗口经以下命令管理，任何回答都不含密钥：

| 命令 | 参数 | 回答 |
|---|---|---|
| `mesh_networks` | 无 | `[{name, joined, security, peers, address?}]` |
| `save_mesh_network` | `config`（含密钥） | 引擎的回答；引擎未运行时 `{"ok":true}` |
| `set_mesh_joined` | `network`、`joined` | 同上；引擎拒绝时不改保存的状态 |
| `remove_mesh_network` | `network` | 同上 |

## 发布闸门

组网在内核 0.3 完成并通过审计之前不进入发布版：

- `client-core`：`mesh::ENABLED` 由构建变量 `ANIXOPS_MESH_PREVIEW=1` 打开。关闭时 `mesh::supported()` 为假（入口不显示），
  `mesh_*` 控制命令被拒绝，保存或加入网络返回 `mesh.disabled`，配置不带组网出站；退出和移除网络不受限制。
- Apple：`MeshSupport.enabled` 由编译条件 `MESH_PREVIEW` 打开；关闭时入口不显示，隧道配置不带已加入的网络，也不取密钥。
- CI 只在测试步骤里打开（`ci.yml`）。`.github/scripts/mesh-release-gate.sh` 在 `release.yml`、`ios-testflight.yml`、
  `play-bundle.yml` 与 `ci.yml` 中运行：运行环境设了该变量，或发布构建读取的文件（其余 workflow、composite action、
  `.github/scripts`、`.cargo`、各 `build.rs`、Tauri 配置、Apple 工程与 Package、Android 构建脚本）在非注释行打开闸门时失败。
- 解除闸门：内核线程确认内核 0.3 完成并通过审计后，随下一次内核固定一起修改。

## 共用的界面逻辑

成员、子网、路径和入口分区的取舍规则只写一次，在 `crates/client-view/src/mesh.rs`，以 `view.mesh_*` 命令提供：

| 命令 | 参数 | 答复 |
|---|---|---|
| `view.mesh_entry` | `status` | 概览与侧栏入口：名称、状态、在线数 / 总数、加密模式标记、是否有提示 |
| `view.mesh_members` | `status`、`groupBy`（`region` \| `role` \| `none`）、`filter`、`sort`、`query` | 分组后的成员行，含折叠、所含数量、最差状态和标记 |
| `view.mesh_subnets` | `status`、`query` | 子网行与标注 |
| `view.mesh_paths` | `status` | 默认选路、跨区链路、自定义路径 |
| `view.mesh_notices` | `status` | 页面提示条，按 `mesh-ui.md`「提示」表的顺序 |
| `view.mesh_latency_tone` | `latencyMs` | `fast`（< 120 ms）、`medium`（< 250 ms）、`slow`、`timeout` |

Windows、Linux 的组网页在共用界面里（`apps/windows-gui/ui/src/pages/MeshPage.tsx`，文案与表单规则在 `mesh.ts`）：
侧栏「组网」在「代理」之后，引擎回答 `mesh_status` 时显示；页面顶部是网络选择、状态条与加入开关，分段「成员 · 子网 · 路径 · 本机」，
成员表右侧是详情栏；「更多」菜单有「添加网络」和「移除网络」（按名称确认）。桌面是完整节点，添加网络时 `node.role` 为 `full`，
`node.listen` 为所填监听端口（默认 11010）的 TCP 与 UDP。Linux 窗口经 `client-core` 取得这些命令；Windows 窗口在后台服务接上前不显示入口。

样例在 `crates/client-view/fixtures/cases.json`，Rust、Kotlin、C# 的测试读同一份文件。Apple 端用 Swift 实现同样的规则，
测试读同一份样例。
