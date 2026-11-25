# 组网：客户端与内核的接口

状态：草案（2026-11-25）。界面说明见 [`../mesh-ui.md`](../mesh-ui.md)，内核模块见 networkcore_anixops
`docs/architecture/mesh-module.md`。本页约定客户端从内核读什么、对内核做什么，以及各平台共用的界面逻辑放在哪里。
内核接口的正本是内核仓库的 `docs/architecture/mesh-status.md`（撰写中），字段以那份为准；本页只记客户端怎么用。

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
| `network.failure` | `{ "code": …, "detail": "…" }`；`code` 为 `secret_mismatch`、`security_mismatch`、`unencrypted_refused`、`port_in_use`（`detail` 为端口）、`tun_in_use`、`route_overlap`（`detail` 为网段）、`address_in_use`、`identity_online`、`permission_denied`、`unreachable_peers`，与安装器的冲突码一致。`existing_program` 不由内核给出，桌面客户端或安装器检测到现有组网程序时自行填入 |
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

各平台的通道（命令名在三条通道上相同，未编进组网时返回 `unsupported`）：

- iOS、macOS：组网与代理共用一个 VPN 和一个引擎实例，命令走现有的 `networkcore_ios_runtime_control`，
  请求体 `{ "cmd": "mesh_status", … }`；引擎带组网时 `networkcore_ios_feature_set_version` 为 7。
  App 经 NetworkExtension 消息转发。
- Android：JNI 桥转到 `client-core` 的同名命令。
- Windows、Linux：窗口经 `client-ipc` 帧发同名命令到后台服务。

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

样例在 `crates/client-view/fixtures/cases.json`，Rust、Kotlin、C# 的测试读同一份文件。Apple 端用 Swift 实现同样的规则，
测试读同一份样例。
