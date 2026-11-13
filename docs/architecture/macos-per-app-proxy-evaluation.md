# 按 App 代理与 TUN 按进程分流评估（2026-11-12）

完备度评估第二版的两项：macOS 按 App 代理（`NEAppProxyProvider`）是否可行，以及 TUN 模式下按进程分流（S5）。

## 现状

- 内核已有 `PROCESS-NAME` 规则，但只用在桌面「系统代理」模式的本地代理入口：`engine-embedded/src/process.rs` 用连接的
  四元组反查进程（Linux `/proc/net/tcp{,6}` + `/proc/<pid>/fd`，Windows `GetExtendedTcpTable`，macOS 遍历各进程的
  `proc_pidfdinfo`），只查 TCP。
- TUN 模式（Windows 服务、Linux、macOS 系统扩展、iOS）的连接不查进程，`PROCESS-NAME` 规则在 TUN 下不生效。
- macOS 隧道是 Developer ID 分发的 System Extension（`NEPacketTunnelProvider`），进沙盒、以 root 运行。

## 1. NEAppProxyProvider（每 App VPN）：不建议做

- 每 App VPN 的「哪些 App 走这个代理」（`NEAppRule` / app-to-VPN 映射）来自配置描述文件里的 VPN 载荷，按 Apple 的设计由
  MDM 下发；iOS 上明确要求设备被管理，开发期只能用 `NETestAppMapping` 测试，仍要一份带 `VPNUUID` 的描述文件。
- 我们的用户是机场用户的个人设备，没有 MDM。让用户手装描述文件既不是正常发布路径，也给不了「在 App 里勾选 App」的体验。
- 结论：不做。按 App 分流在 macOS 上改用下面的透明代理实现。

## 2. macOS 按 App 分流：可行，走 NETransparentProxyProvider

- `NETransparentProxyProvider`（macOS 11+）可以由 Developer ID 签名、公证的 System Extension 提供，不需要 MDM。
- 它按网络规则（`NETransparentProxyNetworkSettings.includedNetworkRules`）认领 TCP 和 UDP 流；每个流带
  `metaData.sourceAppSigningIdentifier` 和 `sourceAppAuditToken`（可得 pid 和可执行文件路径）。`handleNewFlow` 返回 `false`
  时（macOS 11+），系统按没有代理处理这个流，所以「只代理选中的 App，其余直连」或反过来都能做。
- 多个透明代理同时存在时，来源 App 可能被别的代理改写成它自己；我们自己发出的连接要把原流的元数据带上
  （`NWParameters.setMetadata(on:)`，macOS 15；C API 自 10.15.4）。
- 代价：
  - 系统扩展里加第二个 provider 类，`NEProviderClasses` 加 `com.apple.networkextension.app-proxy`；Developer ID 描述文件要包含
    `app-proxy-provider-systemextension`（App ID 已勾 Network Extensions，重新生成描述文件即可，需要用户操作）。
  - 内核要多一个「按流」入口：直接收 TCP 字节流和 UDP 报文（带目的主机），不经过 netstack。引擎内部本来就是按连接路由，
    这部分改动集中在 iOS/macOS 运行时 FFI。
  - 和现有包隧道同时开启时的优先级、DNS 和 fake-ip 的配合，只能真机验证。
- 规模估计：内核按流 FFI 1–2 个 PR，Mac 系统扩展 provider + 管理器 1–2 个 PR，App 里的 App 选择界面 1 个 PR，真机验收。

## 3. TUN 模式按进程分流（S5）

### Windows、Linux：可行，低风险

TUN 模式下应用的 socket 本地地址是 TUN 网卡地址，远端是目的地址（或 fake-ip），netstack 收到的第一个包就有这对地址，
用现有 `process.rs` 的反查就能找到进程。要做的是：

1. netstack 接受 TCP 连接时，在配置有 `PROCESS-NAME` 规则时调用反查（放到阻塞线程，和本地代理入口一样）。
2. 补 UDP：Linux `/proc/net/udp{,6}`，Windows `GetExtendedUdpTable`，按本地地址匹配（UDP 没有连接，只按本地端口）。
3. 连接列表显示进程名，规则测试覆盖 TUN 路径。

Windows 服务以 SYSTEM、Linux 以 root 运行，读得到其他用户进程的表。估计 networkcore 2 个 PR。

### macOS：需要先真机验证

包隧道收到的是 IP 包，不带来源进程。只能在系统扩展里按四元组反查，而系统扩展在沙盒里，沙盒很可能不允许读其他进程的
文件描述符信息（`proc_pidfdinfo`）。这一点只能在真机上确认：

- 先做一个只读探测：开发者模式里显示「进程识别：可用 / 被沙盒拒绝」，并统计命中率（1 个 PR）。
- 可用：直接复用 Windows/Linux 的做法。
- 被拒绝：按进程分流改走第 2 节的透明代理（按 App 识别比按进程更准，也是 Apple 支持的路径）。

### iOS：不做

iOS 没有进程表可查，系统也不给包隧道来源 App；按 App 只有 MDM 下的每 App VPN。

## 结论

| 项 | 结论 |
| --- | --- |
| macOS `NEAppProxyProvider`（每 App VPN） | 不做：需要 MDM 下发 App 映射，不适合个人用户 |
| macOS 按 App 分流 | 可行，用 `NETransparentProxyProvider`，约 4–5 个 PR + 用户重新生成描述文件 + 真机验收 |
| S5 Windows / Linux TUN 按进程分流 | 可行、低风险，复用现有 `PROCESS-NAME` 反查，约 2 个 PR |
| S5 macOS TUN 按进程分流 | 先做沙盒内反查的真机探测，再决定复用反查还是走透明代理 |
| iOS 按 App / 按进程 | 不做 |

## 参考

- Apple Developer Forums：[Which NetworkExtension API to use](https://developer.apple.com/forums/thread/671743)（透明代理的 TCP/UDP
  与 `handleNewFlow` 返回 `false` 的行为）
- [test NEAppProxyProvider without MDM?](https://origin-devforums.apple.com/forums/thread/820572)（每 App VPN 需要被管理的设备，开发期
  `NETestAppMapping`）
- [Transparent Proxy overrides sourceApplicationSigningIdentifier](https://developer.apple.com/forums/thread/763912)（来源元数据与
  `setMetadata(on:)`）
- [Surge Mac 进程规则](https://manual.nssurge.com/rules/process.html)（同类产品只在 Mac 上支持进程规则）
