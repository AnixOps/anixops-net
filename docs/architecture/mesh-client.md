# 组网：客户端与内核的接口

状态：草案（2026-11-25）。界面说明见 [`../mesh-ui.md`](../mesh-ui.md)，内核模块见 networkcore_anixops
`docs/architecture/mesh-module.md`。本页约定客户端从内核读什么、对内核做什么，以及各平台共用的界面逻辑放在哪里。
内核接口的正本是内核仓库的 [`docs/architecture/mesh-status.md`](https://github.com/AnixOps/AnixOps-core/blob/main/docs/architecture/mesh-status.md)（AnixOps-core #296），字段以那份为准；本页只记客户端怎么用。

## 分版本

| 客户端 | 内核 | 界面内容 |
|---|---|---|
| 1.1 | 0.3 | 加入开关（独立于连接开关）、网络状态与加密模式、成员与子网视图、内网域名、添加与移除网络 |
| 1.2 | 0.4 | 地区分组与归属依据、迁区建议与申请（R24）、边界与宿主组（组地址段，R21、R26）、地址分配（固定 / 自动）、其他地区与骨干的可达状态（R19）、外部网络（R36） |
| 1.3 | 0.5 | 自定义路径与回落提示、个人电脑用入网凭据加入 |

内核没有组网能力（`capabilities.mesh` 不存在或为 `false`，见「发布闸门」）时，各平台不显示组网入口。版本内尚未提供的字段缺省即可，
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

### 1.2 内核 0.4 的字段

以下字段的名字与形态取自内核 `mesh-status.md`（AnixOps-core #384）；客户端按夹具实现，缺省时界面按「没有该信息」降级，不推断。
`links[]` 只在边界成员上给出，普通成员的快照没有 `links`，其他地区的状态改由 `regions[].reachable` 和汇总网段推出（见下）。

| 字段 | 形态 | 用途 |
|---|---|---|
| `subnets[].kind` | `lan`（缺省）、`host_group`、`external`、`summary`；未知按 `lan` | 子网类型标记 |
| `subnets[].hostGroup` | `{ "gateway": 实例 id, "backupGateway": 实例 id 或 null }`，仅 `host_group`；取自配置 `host_groups` | 「网关 X 通告的组地址段」，X 取 `advertisers` 中 `inUse` 的成员（备用网关只在主网关离线时通告） |
| `members[].vms[].cidr` | 所属组地址段；`vms` 来自面板下发的名单，挂在当前通告该组地址段的成员下 | 组地址段行展开列出虚拟机 |
| `subnets[].bridge`、`subnets[].name` | 桥接成员 id；面板给出的名称（可缺省），仅 `external` | 「经 X 桥接」 |
| `members[].bridgesExternal` | 布尔，缺省 `false`（顶层的 `externalBridge` 是另一个对象） | 「外部网络桥接」标记 |
| `self.regionRequest` | `null` 或 `{ "id", "region", "state": "pending" \| "rejected", "reason", "rejectReason" }`；批准后为 `null`。`rejected` 与撤回只在内核 `features` 列出 `mesh_region_request_state` 时出现 | 待批准的目标地区、已拒绝及管理者原因 |
| `self.regionSuggestion` | `null` 或 `{ "region", "currentMedianMs", "suggestedMedianMs", "since" }`（中位 RTT） | 迁区建议 |
| `self.regionState` | `auto`、`pinned`、`pending`；可能另有 `migrating`、`measuring`、`unassigned`，一律按 `other` | 地区行 |
| `self.regionBasis` | `{ "border", "latencyMs" }`，可能另加 `kind` | 归属依据 |
| `self.backbone` | `null` 或 `{ "state": "ok" \| "partial" \| "down" \| "unavailable", "linksUp", "linksTotal", "failure" }`，仅边界成员 | 本机页「骨干」行 |
| `regions[]` | `{ "code", "name", "local" }`，另有可选的 `prefixes[]`、`borders[]`（本地区边界成员 id）、`remoteBorders[]`、`reachable`（布尔） | 其他地区的可达状态（没有 `links` 时） |
| `links[]` | `{ "from", "to", "via", "borders", "latencyMs", "loss", "state", "reason" }`，另有 `localBorder`、`remoteBorder`；仅边界成员给出 | 其他地区的状态、出口边界成员、绕行 |
| `mesh_withdraw_region_request` | 命令，参数 `network`；需 `mesh_region_request_state` | 「撤回申请」 |

其他地区的摘要行（`view.mesh_members` 的 `groups[].summary`）按以下顺序取值：

- 有 `links[]` 中本地区与它之间的一项：`ok` 为可达，`detour` 为已绕行，`down` 为中断；出口边界成员取 `localBorder`，没有则取 `borders`
  中属于本地区的第一个，再没有则取其汇总网段中 `inUse` 的通告者；延迟、丢包、绕行地区取自该项。
- 没有 `links[]`（普通成员）：`regions[].reachable` 为 `false` 是中断、`true` 是可达；该字段缺省时，汇总网段全部不可达为中断，否则可达；
  延迟取汇总网段中最低的，出口边界成员取汇总网段 `inUse` 的通告者，没有绕行与丢包。
- 三者都没有的地区不出现摘要行。
- 汇总网段为 `kind` 为 `summary` 的子网，按其 `region` 归入对应地区的摘要行。

连接 `cross_region` 保留，内核 0.4 不给出；界面照旧处理。

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
| `mesh_request_region` | `region`、可省的 `reason`（交由网络管理者批准） | 1.2 |
| `mesh_withdraw_region_request` | `network`（撤回待批准或已拒绝的申请；需内核 `features` 列出 `mesh_region_request_state`） | 1.2 |
| `mesh_set_policy` | `policy` | 1.3 |
| `mesh_set_paths` | `paths`（与内核 `routing.paths` 同结构） | 1.3 |
| `mesh_join_credential` | 一次性入网凭据（个人电脑） | 1.3 |

命令失败时返回 `{ "ok": false, "error": { "code": …, "detail": … } }`，`code` 为 `unsupported`、`invalid_argument`、`not_found`、`not_allowed` 或上文的某个失败原因；界面按「错误」状态写明原因和下一步。

各平台的通道（命令名在三条通道上相同，未编进组网时返回 `unsupported`）：

- iOS、macOS：组网与代理共用一个 VPN 和一个引擎实例，命令走现有的 `networkcore_ios_runtime_control`，
  请求体 `{ "cmd": "mesh_status", … }`；`networkcore_ios_feature_set_version` 为 7，与是否带组网无关；组网是否可用问引擎（`capabilities.mesh`，见发布闸门）。
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
  每个网络另有各自的安全模式静态私钥（`node.private_key`）：由网络扩展用系统随机数生成 32 字节，以标准 base64（带填充）
  保存在扩展专用的钥匙串访问组 `networkcore.tunnel-private`（账户 `mesh-network-key.<名称>`，不与主 App 共享；macOS 系统扩展使用系统钥匙串，
  失败时退为扩展数据容器内仅 root 可读的文件，遵循 core#406）。主 App 不生成、不读取、不持有私钥，只读引擎快照里的公钥。
  扩展在启动组网和处理 `mesh_save_network` 时读取，没有则生成，读取出错或已存的值不合法时报错、不重新生成；
  App 随 `mesh_save_network` 发出的配置不含 `node.private_key`，由扩展注入。网络被移除时，扩展在收到 `mesh_remove_network` 或下次启动时
  （隧道配置的 `networkcore.mesh-saved-names` 列出全部已保存网络）删除不再保存的网络的私钥；App 只清理旧版本留在共享钥匙串（账户 `mesh-network-key.<名称>`）的副本。
  旧版本存入共享钥匙串的私钥，由扩展首次启动时迁入专用访问组并删除共享副本，面板已登记的公钥保持有效。
  设备实例 id（`node.instance_id`，`^[0-9A-Za-z][0-9A-Za-z-]{0,63}$`）同样由扩展在首次使用组网时生成（小写 UUID v4）、
  保存在同一专用存储（账户 `mesh-instance-id`），各网络的配置使用同一个值；已存的值不合法时报错、不替换。
  隧道配置的 `networkcore.mesh-networks` 只带已加入的网络，扩展启动时补上密钥。
- iOS、macOS 只开组网：代理断开而有网络已加入时，隧道配置标记 `networkcore.mesh-only`，VPN 重启为只带组网出站的配置
  （`rules` 为 `["FINAL,DIRECT"]`，`mode` 为 `rule`，不需要节点密钥）；`includedRoutes` 只放 `tunnel.routes`，
  内网域名开启时 DNS 只对 `tunnel.dns.matchDomains` 生效。扩展每 2 秒读一次 `mesh_status`，`routesVersion` 变化时重新应用，
  间隔至少 1 秒。代理连上时去掉标记并重启；退出最后一个网络时关闭 VPN。入口在 `MeshSupport.available`（发布门打开且引擎回答 `capabilities.mesh` 为 `true`）时显示。
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
  - 后缀须至少两段且以 `internal` 结尾（如 `mesh.internal`、`eu.mesh.internal`），并符合 core#370：ASCII 标签
    （允许 Punycode `xn--`），每段 1–63 字节且不以连字符开头或结尾，全长不超过 172 字节，只去掉末尾一个点，前导点、
    空标签和多个末尾点均拒绝；单段名、公共后缀、`local`、`arpa` 及根一律不写规则，被拒的后缀写入日志并在组网页提示
    （与内核侧 F-03 的限制一致）。
  - 反向解析区（`*.in-addr.arpa`、`*.ip6.arpa`）只取 `tunnel.dns.matchDomains` 中由内核给出的（core#375：内核取网络
    地址范围与内网白名单的交集，按标签边界展开），同样指向组网 DNS；客户端不自行推导。客户端只接受 `in-addr.arpa` 下
    1–4 段 0–255 的十进制标签、`ip6.arpa` 下 2–32 段单个十六进制标签，`arpa`、`in-addr.arpa`、`ip6.arpa` 本身及更宽的区
    一律拒绝并写入日志。此外，区须整个落在内网白名单（intranet-mesh §15.4：IPv4 10.0.0.0/8、172.16.0.0/12、
    192.168.0.0/16、100.64.0.0/10，IPv6 fc00::/7）之内：标签倒序即前缀，前缀长度不短于白名单范围且前导位一致，
    如 `16.172.in-addr.arpa`、`c.f.ip6.arpa`、`d.f.ip6.arpa` 接受，`172.in-addr.arpa`（比 172.16/12 更宽）、`15.172.in-addr.arpa`、
    `b.f.ip6.arpa`、`e.f.ip6.arpa` 拒绝，作为纵深防御，防止公网地址的反向查询被指向组网 DNS。
  - 同一地区只写主机名：本机所在地区的后缀（`self.domain` 去掉主机名，如 `eu.mesh.internal`）设为组网网卡的连接后缀，
    Windows 对单段名称追加该后缀后按规则解析；组策略下发了全局后缀搜索列表时，连接后缀不生效。连接后缀按网卡 GUID
    保存，网卡不存在时也保留；删除规则时同时按固定 GUID 清除注册表中的这项设置，崩溃后下次启动即清除，
    同一 GUID 的代理网卡不会继承。
  - 解析地址 IPv4 在前；只有 IPv6 解析地址而本机无法经 IPv6 到达时，组网页提示。
  - 每次写入或删除规则后清空 DNS 缓存。写入失败时按 5、10、20、40 秒、之后每 60 秒重试，组网页提示「内网域名暂不可用」。
  - 组策略下发了名称解析策略表规则时（`HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DNSClient\DnsPolicyConfig`
    下有规则），本机规则被系统忽略，组网页提示由组策略管理。
  - 规则只在本进程持有下述网卡锁时改动：只开组网的引擎运行期间持锁；崩溃残留由首次 `settle` 在取得锁后清理，
    另一用户的窗口占用网卡时不清理（规则属于对方），留待下次。卸载（MSI `--purge`）时同样按注释删除。
  - PowerShell 与 rundll32 以系统目录下的绝对路径启动，系统目录由 `GetSystemDirectoryW` 取得，不读环境变量。
  - CI 的 Windows 任务以管理员身份实际写入并删除规则一次（`the_rules_are_written_and_removed_on_windows`），
    核对只删除带本程序注释的规则、并清除按 GUID 保存的连接后缀。
  - 网卡 GUID `{4E3C5B8A-7D21-4F0B-9A6E-1C2D3E4F5A6B}` 抄自内核 `platform-windows` 的 `tun::ADAPTER_GUID`（内核未导出）；
    单元测试 `the_adapter_guid_is_the_kernels` 核对其各段，内核钉版（根 `Cargo.toml` 的 `rev`）每次升级时必须对照新的
    `tun.rs` 重新核对。
  - 同一时间只加入一个网络，所有后缀对应同一组解析地址；以后允许同时加入多个网络时，须改为按网络分别对应。
  网卡 GUID 全机唯一，而每个登录用户各运行一个窗口：
  打开网卡前独占 `%ProgramData%` 数据目录下的 `tun-adapter.lock`，另一用户的窗口已占用时报 `tun_in_use`，不替换对方网卡。
  启动失败时组网页按系统错误码给出提示（拒绝访问为 `permission_denied`，上述占用为 `tun_in_use`，其余为通用失败，
  不按错误文本猜测），并按 5、10、20、40 秒、之后每 60 秒重试；网络变化时立即重试；
  引擎自行退出也按此重试。窗口退出、注销、关机或重启时显式停止该引擎并撤下网卡与路由；注销被取消时恢复。崩溃时网卡随进程
  移除，下次启动按同一 GUID 替换。
- Windows 防火墙（需求 R5）：窗口的引擎（TUN 模式的代理引擎或只开组网的引擎）运行已加入的网络期间，对每个网络持有内核
  `platform_windows::mesh_host` 的防火墙守卫：放行该网络 `node.listen` 各端口的入站（TCP 与 UDP），并拦截经组网网卡
  （`AnixOps Net`）到达本程序的入站流量，使组网成员够不到窗口的其他服务（代理监听、控制端口）。网络退出、被移除或引擎停止时
  放下守卫，由内核在后台线程删除规则；窗口退出时（两个引擎都停止之后）最多等待 10 秒让删除完成，超时写入警告日志，残留规则由下次启动的
  `recover_mesh_host` 清理（卸载 `--purge` 同样调用）。同一主机上只有一个进程可持有此类改动：另一进程（组网节点服务）已持有时，
  客户端不添加规则，记录一次日志，并在组网页提示「入站保护未生效」（`network.local_dns` 的 `inbound_unprotected`，持有者在时 detail 为 `busy`；规则添加失败包括 `AlreadyExists` 同样提示）；每次更新都重新尝试恢复，只开组网的引擎的路由监视与代理引擎的恢复监视器也约每 30 秒同步一次，持有者退出后自动补上规则。窗口退出时先停止全部引擎再放下守卫并等待删除；会话结束时不放下守卫也不等待（进程随会话结束，规则由下次启动恢复，期间组网网卡的拦截规则仍然生效）。完整卸载时窗口仍持有主机改动，`--purge` 把它记为错误行。非 TUN 模式的代理引擎没有组网网卡，不添加规则。客户端尚未通告子网，因此不开转发、不做地址转换（NAT）；规则一律由内核写入，不使用 `netsh`。
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

- 每个网络有各自的安全模式静态私钥：添加网络时由客户端用操作系统的密码学安全随机数生成 32 字节，以标准 base64（带填充）
  存入该网络的配置，作为 `node.private_key` 注入内核配置；Windows 随配置文件用数据保护（DPAPI）封存，Linux 为仅所有者可读的文件，
  安卓用 Android Keystore 包装的密钥以 AES-GCM 封存（`mesh-networks.sealed`），旧的明文文件在首次读取后转为封存并删除；密钥不可用时不读不写。已知偏差（core#406）：安卓界面与 VpnService、引擎同在一个进程，严格说不满足「界面进程不接触私钥」；计划在 1.1 之后把 VpnService、引擎与 Keystore 解封移到单独进程（`android:process=":vpn"`），界面只经 IPC 读状态。同名网络再次保存时沿用已存的私钥（面板的 `native_keys` 列出对应公钥，私钥变化会使设备失去接入），
  仅在网络被移除后重新添加时才生成新的；此前保存、没有私钥的网络在下次保存或加入时补上。引擎从不写出或输出该私钥，窗口也看不到它
  （`mesh_networks` 与离线状态都不含配置）。Apple 由网络扩展生成并保存在扩展专用的钥匙串访问组（不与主 App 共享），主 App 只读引擎快照里的公钥；macOS 系统扩展使用系统钥匙串（失败时退为 root 专用文件，遵循 core#406）。设备实例 id 在 Apple 同样由扩展生成并保存。
- 设备实例 ID：设备首次使用组网时由客户端用密码学安全随机数生成 16 字节，格式化为小写 UUID v4 文本，存入数据目录的
  `mesh-instance-id`（仅所有者可读；不是机密，不封存），此后不再改变。每个网络的配置都带同一个 `node.instance_id`
  （符合 `^[0-9A-Za-z][0-9A-Za-z-]{0,63}$`），状态快照的 `self.id` 与之相等；保存或加入网络时，以及生成引擎配置时，
  配置中不同的 `node.instance_id` 一律以设备的为准。该文件存在但内容不合格式时报错，不替换，以免面板看到的设备标识变化。
  `members[].id` 会随对端重启而变化，客户端不得以它作为任何持久化内容的键。

## 发布闸门

组网在内核 0.3 完成并通过审计之前不进入发布版：

- `client-core`：`mesh::ENABLED` 由构建变量 `ANIXOPS_MESH_PREVIEW=1` 打开。关闭时 `mesh::supported()` 为假（入口不显示），
  `mesh_*` 控制命令被拒绝，保存或加入网络返回 `mesh.disabled`，配置不带组网出站；退出和移除网络不受限制。
- Apple：`MeshSupport.enabled` 由编译条件 `MESH_PREVIEW` 打开；关闭时入口不显示，隧道配置不带已加入的网络，也不取密钥。
- CI 只在测试步骤里打开（`ci.yml`）。`.github/scripts/mesh-release-gate.sh` 在 `release.yml`、`ios-testflight.yml`、
  `play-bundle.yml` 与 `ci.yml` 中运行：运行环境设了该变量，或发布构建读取的文件（其余 workflow、composite action、
  `.github/scripts`、`.cargo`、各 `build.rs`、Tauri 配置、Apple 工程与 Package、Android 构建脚本）在非注释行打开闸门时失败。
- 发布门关闭时，客户端拒绝启动包含 `"type":"mesh"` 出站的配置（含用户手写或导入的配置），因为内核默认编入组网且没有运行时开关。
  检测口径与内核构建出站的位置一致，共三处：顶层 `outbound`；`routing.outbounds` 的每个条目（对象或数组形式）；以及前两者的内联
  `via` 对象（递归）。拒绝不删除（规则可能引用该 tag），错误码固定为 `error.mesh_gated`，各端自行用本地语言措辞，不含配置内容。
  每个启动入口都调用同一道门：client-core 的 `Engine::start` 与 `MeshOnly::start`（安卓、Linux、Windows 原生）、Windows 桌面端的
  `DesktopTun::start_with`（TUN 模式与仅组网）、Apple 扩展的 `launchTunnel` 与 `beginMeshOnly`；被拒绝的重启先停掉旧引擎，不留旧连接；
  按需连接把它当作不可重试的失败。门检查的调用点由 `mesh-release-gate.sh` 断言存在。Windows 桌面的系统代理模式（本地代理）同样拒绝
  （`start_embedded`），与 TUN 模式一致。内联 `via` 的递归最多跟 64 层，更深的按「含组网」处理（内核本来就拒绝超过 4 层的链）。
- 内核层的第二道门（内核固定在 v0.3.0-rc.2（`ce0cb5765a0aeb94845c0cfef9d71bbf39ba028e`，release/0.3），core#424、#425）：内核的信封字段
  `allow_mesh`（布尔，缺省 `false`）为 `false` 或缺省时，`RuntimeConfig::from_json` 以 `config_rejected`、固定消息
  `mesh outbounds are not allowed by this config` 拒绝任何位置的组网出站，该运行实例 `capabilities.mesh` 为 `false`，所有
  `mesh_*` 命令返回 `unsupported`。两层都保留，不得以「内核已有开关」为由删掉客户端门：客户端门先拒绝，保证各端错误码都是
  `error.mesh_gated`、文案一致；`allow_mesh` 覆盖客户端解析与内核解析的差异（审计 #351 L-f）、将来的新入口和新的出站构建位置。
  交给引擎的每一份最终信封都带 `"allow_mesh": <发布门是否打开>`（client-core 为 `mesh::ENABLED`，Apple 为 `MeshSupport.enabled`），
  用户手写或导入的配置里的 `allow_mesh` 一律被覆盖，不存在写死 `true` 的位置：client-core 的 `mesh::with_allow_mesh`
  （文本与字节形式，`Engine::start`、`MeshOnly::start`、`through_temporary_proxy`、Windows 桌面的 `DesktopTun::start_with` 与本地代理的
  `start_embedded`）；Apple 的 `EmbeddedRuntimeConfig.withAllowMesh()`，由 `EngineBudget.encoded` 统一调用，扩展里每一次交给引擎的编码都经过它。
  带取值的变体（Rust 的 `prepared_start`、`allow_mesh_text`、`start_gated`、`through_proxy_gated`，Swift 的 `settingAllowMesh`、`MeshSupport.available(enabled:)`）
  不公开（Rust 最多 `pub(crate)`，Swift 为 internal，测试用 `@testable import`），调用方只能走绑定发布门的包装；测试用它们验证门打开时 `true` 确实到达引擎信封。
  信封无法解析而含 `mesh` 时，发布门关闭则拒绝，但按「配置无效」报（无错误码的普通英文），不报 `error.mesh_gated`。client-core 与 Windows 中
  内核自己的 `MESH_NOT_ALLOWED` 拒绝映射为同一条 `error.mesh_gated` 文案（`map_kernel_refusal`）；Apple 不映射：C ABI 的
  `networkcore_ios_runtime_init` 只返回 `CONFIG_REJECTED` 而不带消息，漏过客户端门的配置在 Apple 上显示为通用的「配置被拒绝」。
  只做校验的 `from_json`（保存 DNS、规则、数据列表、设备名、重定向，Windows 的「检查配置」）经 `mesh::check_envelope`，同样写入 `allow_mesh`。
  Windows 的本地代理模式与 client-core 的本地代理不带组网（`capabilities.mesh` 恒为 `false`）：运行的引擎是本地代理时，`mesh_*` 答
  `unsupported`（`Backend::runtime_control` 的捷径，引擎自己也这样答）；`Backend::mesh_supported` 只在没有引擎运行时被问到，那时等于 `mesh::supported()`。
  `mesh-release-gate.sh` 断言各启动文件里这些调用都在预期的函数内（不计注释行），锁定两处写入点的取值（`set_allow_mesh(config, ENABLED)`、
  Swift 的 `settingAllowMesh(MeshSupport.enabled)`），并在非测试源码里拦截把 `allow_mesh` 写成 `true` 的字面写法（同一行出现 `allow_mesh`、`allowMesh` 或 `AllowMesh` 与单词 `true`，含带参数的助手、
  `Value::Bool(true)`、下标赋值与转义的 JSON 文本）。拦不住的是变量传值、`!false`、常量、宏、折行的 `insert`、改名的 serde 字段；所以脚本另外按整个仓库
  统计各写入助手（`allow_mesh_text`、`prepared_start`、`check_envelope`、`withAllowMesh` 等）在非测试代码里的调用次数，要求与清单**恰好相等**，新增启动路径必须同时改脚本。
  测试代码的边界以脚本为准：Rust 跳过每个 `#[cfg(test)]` 条目（`mod`、`use`、`fn`、`impl`），从该条目起到与它配对的右花括号（无花括号的到分号）为止，
  花括号在字符串和注释之外计数，之后的生产代码照常扫描；`#![cfg(test)]` 的文件整个跳过。Swift、Kotlin、TypeScript 不按条目区分，只排除路径中含
  `test`、`tests`、`Tests`、`androidTest` 的目录。行尾 `//` 注释在字符串字面量之外才剥离（只认单行的 `"..."`，TypeScript 另认 `'...'`；原始字符串与跨行字符串不处理）。
- 组网是否可用由引擎回答，不由特性集版本推断：内核固定后 `networkcore_ios_feature_set_version` 为 7，且不随 `mesh` feature 变化。Apple 的
  `MeshSupport.available` 为 `enabled`（发布门）且引擎对 `{"cmd":"capabilities"}` 回答 `capabilities.mesh` 为 `true`；应用在启动时
  （`EngineCapabilities.install`）装上探测：在应用进程内以带发布门 `allow_mesh` 的最小配置建一个不启动的运行时，在后台线程问，成功的回答才缓存，失败（无应答或不是应答）不缓存，之后的读取隔 5 秒再问；不在主线程等引擎。
  client-core 与引擎同进程，`mesh::supported()` 用同一份配置解析探测（带发布门 `allow_mesh` 的仅组网配置能否被引擎构建）。
- 发布门关闭时，Apple 扩展的控制通道拒绝除 `mesh_engine_stop` 以外的所有 `mesh_*` 命令，请求含 `mesh_` 而无法解析时同样拒绝，答
  `error.mesh_gated`；client-core 的窗口请求在 `activity_request` 中同样拒绝所有 `mesh_*`。
- 解除闸门：内核线程确认内核 0.3 完成并通过审计后，随下一次内核固定一起修改。

客户端 1.2 的界面另有一道闸门，在内核 0.4 完成并通过审计之前不进入发布版（先开上一道闸门，这一道才有意义）：

- `client-core`：`mesh::V04_ENABLED` 由构建变量 `ANIXOPS_MESH04_PREVIEW=1` 打开；Apple 为编译条件 `MESH04_PREVIEW`。
- 关闭时 `mesh_status` 的回答在交给界面前去掉内核 0.4 的字段（`regions`、`links`、`regionRequests`、`externalBridge`，
  `self` 的 `region`、`regionBasis`、`regionState`、`regionRequest`、`regionSuggestion`、`backbone`、`hostMode`，
  成员的 `region`、`vms`、`externalBridge`、`bridgesExternal`，子网的 `region`、`kind`、`hostGroup`、`bridge`、`name`），界面按 0.3 降级；
  `mesh_set_relay`、`mesh_set_host_mode`、`mesh_advertise`、`mesh_withdraw`、`mesh_request_region`、`mesh_withdraw_region_request`
  被拒绝（`unsupported`）。
- `mesh-release-gate.sh` 对两个变量做同样的检查；CI 测试步骤两道都打开。

## 共用的界面逻辑

成员、子网、路径和入口分区的取舍规则只写一次，在 `crates/client-view/src/mesh.rs`，以 `view.mesh_*` 命令提供：

| 命令 | 参数 | 答复 |
|---|---|---|
| `view.mesh_entry` | `status` | 概览与侧栏入口：名称、状态、在线数 / 总数、加密模式标记、是否有提示 |
| `view.mesh_members` | `status`、`groupBy`（`region` \| `role` \| `none`）、`filter`、`sort`、`query` | 分组后的成员行，含折叠、所含数量、最差状态和标记 |
| `view.mesh_subnets` | `status`、`query` | 子网行与标注；含 `kind`、`display`、`label`、`hostGroup`、`bridge`、`vms` |
| `view.mesh_paths` | `status` | 默认选路、跨区链路、自定义路径 |
| `view.mesh_notices` | `status` | 页面提示条，按 `mesh-ui.md`「提示」表的顺序 |
| `view.mesh_region` | `status` | 地区面板：本地区与归属依据、变更申请、迁区建议、可选目标地区（含延迟）、能否申请与撤回、骨干 |
| `view.mesh_latency_tone` | `latencyMs` | `fast`（< 120 ms）、`medium`（< 250 ms）、`slow`、`timeout` |

Windows、Linux 的组网页在共用界面里（`apps/windows-gui/ui/src/pages/MeshPage.tsx`，文案与表单规则在 `mesh.ts`）：
侧栏「组网」在「代理」之后，引擎回答 `mesh_status` 时显示；页面顶部是网络选择、状态条与加入开关，分段「成员 · 子网 · 路径 · 本机」，
成员表右侧是详情栏；「更多」菜单有「添加网络」和「移除网络」（按名称确认）。桌面是完整节点，添加网络时 `node.role` 为 `full`，
`node.listen` 为所填监听端口（默认 11010）的 TCP 与 UDP。Linux 窗口经 `client-core` 取得这些命令；Windows 窗口在后台服务接上前不显示入口。

样例在 `crates/client-view/fixtures/cases.json`，Rust、Kotlin、C# 的测试读同一份文件。Apple 端用 Swift 实现同样的规则，
测试读同一份样例。
