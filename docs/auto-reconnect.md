# 自动重连：逐步降速，连续 5 次失败后停止

所有平台共用一条规则：自动重连每次比上一次等得更久，连续 5 次没有连上就停止，并告诉用户（重试或换节点）；
重试也解决不了的错误（比如引擎与本机版本不兼容、配置被引擎拒绝）不再重试，直接停止。用户自己点“连接”
总会立即执行，并从 1 重新计数。

| 平台 | 谁在自动重连 | 等待 | 停止条件 | 代码 |
| --- | --- | --- | --- | --- |
| iOS / macOS | 系统 VPN On Demand（失败后立刻再启动隧道） | 隧道扩展自己等：第 1 次立即，之后 2、4、8、16 秒，各 ±20% 抖动 | 连续 5 次启动失败，或一次不可恢复的失败；扩展关闭 On Demand，App 也会关（macOS System Extension 无法自己关） | `apps/ios/Sources/NetworkCoreBridge/AutoReconnect.swift`、`PacketTunnelProvider.swift` |
| Android、Linux（Wi-Fi 规则） | 共享 Rust 层 `client-core` 的 Wi-Fi 规则 | 30、60、120、240 秒（每 10 秒取样一次） | 同一网络上连续 5 次失败；换网络或手动连接重新计数 | `crates/on-demand` 的 `retry`、`crates/client-core/src/client.rs` |
| Android（进程被系统杀掉后） | 前台服务的 sticky 重启 | Android 自己对反复崩溃的服务逐次延长 | 10 分钟内 5 次 | `AnxVpnService.kt` 的 `MAX_RESTARTS` |
| Windows / Linux 原生桌面 | 没有自动重连 | — | — | — |

## iOS / macOS 细节

- App、小组件、快捷指令、控制中心开关发起的启动带 `net.anixops.user-start` 选项，算用户启动；系统的 On Demand、
  崩溃后重启不带，算自动启动。
- 计数存在隧道扩展自己的 UserDefaults（macOS System Extension 没有 App Group）。隧道连上并保持 60 秒后清零。
- 停止后再来的自动启动会被拒绝；如果此时 On Demand 已经是关的，说明是用户在系统设置里打开的，照常连接。
  扩展无法关闭 On Demand 时（macOS System Extension），拒绝会先等 30 秒，避免系统再次快速循环，直到 App 关掉它。
- 扩展在失败原因末尾加 `[auto-reconnect stopped]` 或 `[auto-reconnect stopped: retrying cannot fix this]`，
  App 读系统记录的断开原因（`fetchLastDisconnectError`）后显示“已停止自动重连”卡片（重试 / 换节点），并发通知。
- 不可恢复的失败：隧道或嵌入引擎不可用（含 ABI 不兼容）、配置缺失或损坏、节点密钥缺失、配置超出引擎上限、
  引擎拒绝配置、网卡地址无效。钥匙串暂时读不到（开机后尚未解锁）等仍按普通失败重试。
