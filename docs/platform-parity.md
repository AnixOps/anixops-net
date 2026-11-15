# 全平台功能对照

本页对照 iOS、macOS、Windows、Linux、Android 五个客户端的功能，列出缺口和补齐计划。各平台实现细节分别见
[`debug-mode.md`](debug-mode.md)、[`phone-debugging.md`](phone-debugging.md) 和 `apps/*/README.md`。

- 界面：iOS 和 macOS 共用 SwiftUI（`apps/ios`），Windows、Linux、Android 共用 Tauri 界面（`apps/windows-gui/ui`）。
  后端分别是 `apps/windows-gui`（Windows）和 `apps/linux-gui`（Linux，Android 复用它）。
- 内核能力都在 networkcore（固定 `rev`），客户端只决定用不用、怎么呈现。
- Windows 和 Linux 正在改为原生界面（WinUI 3、GTK4 + libadwaita），方案和迁移清单见
  [`architecture/desktop-native.md`](architecture/desktop-native.md)。切换之前 Tauri 桌面版仍是正式版，本表的 Windows、
  Linux 两列指 Tauri 版；原生骨架合并时会加“Windows 原生”“Linux 原生”两列。
- 新增或改动功能时请同步更新本表。一个功能只在部分平台上线时，PR 描述写明其余平台的计划或系统限制。

图例：✅ 已有 · ◐ 部分（见备注） · ❌ 缺，计划补齐 · ⛔ 系统限制或产品决定不做（见“系统限制”） · — 不适用

核对基准：2026-11-14，main `1cc1921`，内核 `e5e158a`；进行中的 PR 单独标出。

## 调试（网络调试客户端计划）

| 功能 | iOS | macOS | Windows | Linux | Android | 备注 |
|---|---|---|---|---|---|---|
| 代理 / 调试双模式、首次启动选择 | ✅ | ✅ | ✅ | ✅ | ✅ | #119、#122 |
| 调试模式直连出口 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 开始 / 停止 / 清空抓包 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 连接抓包内容（文本 / 十六进制） | ✅ | ✅ | ◐ | ✅ | ✅ | 同上 |
| 导出 pcapng / HAR | ✅ | ✅ | ◐ | ✅ | ✅ | 同上 |
| HTTP 请求列表、快捷筛选、表达式搜索 | ⏳ | ⏳ | ◐ | ✅ | ✅ | Apple 见 #126（进行中） |
| 单条请求导出 cURL / 代码 / Postman / Insomnia，默认脱敏 | ❌ | ❌ | ◐ | ✅ | ✅ | Apple 由 Apple 调试线程在 #126 之后做（A1） |
| 抓包会话加密保存、7 天清理、载入回看 | ❌ | ❌ | ✅ | ✅ | ✅ | Tauri 见 `debug-mode.md`；Apple 见 A1 |
| 单条重放、编辑后重放 | ✅ | ✅ | ◐ | ✅ | ✅ | Android 经内核重放代理（#128） |
| Map Local / Map Remote | ✅ | ✅ | ✅ | ✅ | ✅ | 需开启 HTTPS 解密 |
| HTTP/2 MITM | ✅ | ✅ | ✅ | ✅ | ✅ | 内核 #233，各平台共用 |
| HTTPS 解密开关 + 首次开启确认条款 | ✅ | ✅ | ✅ | ✅ | ✅ | #117 |
| 根证书由用户手动信任、关闭解密保留证书 | ✅ | ✅ | ⏳ | ⏳ | ✅ | 桌面见 #123（进行中）；Android 本来就在系统设置里手动安装 |
| 手机连电脑抓包（局域网共享 + 证书页） | — | ✅ | ✅ | ✅ | — | 手机本身不需要 |
| 导入自有根证书（团队 CA） | ⛔ | ✅ | ✅ | ✅ | ⛔ | macOS 在扩展页“使用自己的根证书…”粘贴 PEM（A2），密钥须为 P-256；iOS / Android 见系统限制 |
| 解密对第三方 App 生效 | ✅ | ✅ | ✅ | ✅ | ⛔ | Android 7 起 App 默认不信任用户证书 |

## 代理与规则

| 功能 | iOS | macOS | Windows | Linux | Android | 备注 |
|---|---|---|---|---|---|---|
| 订阅导入、更新、自动更新、重命名 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 节点测速、选择最快、策略组切换 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 节点分享链接 | ✅ | ✅ | ✅ | ✅ | ✅ | Tauri 在节点卡片的“更多”菜单里；触屏上菜单按钮常显（T4） |
| 手动添加 / 编辑节点 | ✅ | ✅ | ✅ | ✅ | ✅ | 添加 = 粘贴分享链接导入；编辑名称、服务器、端口和删除节点只改本机，更新订阅后恢复，与 iOS 相同（`crates/node-edits`，T5）。策略组或规则用到的节点和最后一个节点不能删 |
| 扫码导入 | ✅ | — | — | — | ✅ | Android：订阅页“扫码”用相机读订阅或分享链接二维码，填进链接框后点“导入”（`tauri-plugin-barcode-scanner`，T5） |
| 分流模式（规则 / 全局 / 直连等） | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 我的规则（规则文本编辑、连接中原地换规则） | ✅ | ✅ | ◐ | ◐ | ◐ | 内核 `reload_rules`（T2）。桌面与 Android 不收 RULE-SET / GEOIP / GEOSITE 行（这些要下载规则集，分流模式已覆盖）；Windows TUN 模式下次连接生效 |
| 规则集查看、更新远程规则集 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| DNS 预设 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 自定义 DNS 服务器、分域 DNS、Hosts | ✅ | ✅ | ✅ | ✅ | ✅ | T3：桌面与 Android 用文本编辑（`名称 = 地址`、`DOMAIN-SUFFIX,cn,服务器`、`域名 = 地址`），iOS / macOS 是表单 |
| Fake-IP | ✅ | ✅ | ✅ | — | ✅ | 只在接管 IP 流量时有意义：Windows 在 TUN 模式下生效，Android 在 VPN 里生效（T3）；Linux 只有系统代理，代理拿到的就是域名 |
| 屏蔽 QUIC | ✅ | ✅ | ✅ | ✅ | ✅ | #106 |
| 流量重定向（TCP / UDP 劫持到开发服务器） | ✅ | ✅ | ✅ | ✅ | ✅ | 扩展 / 改写页“流量重定向”，与 iOS 相同的地址 / 域名 / 端口 / 协议 → 目标写法（引擎 `redirects`，`desktop_dns::redirects`）。Windows 和 Linux 不走系统代理的程序要开 TUN 模式（T6） |
| TUN（接管全部流量） | ✅ | ✅ | ✅ | ✅ | ✅ | Linux 每次连接经 pkexec（polkit）提权助手创建虚拟网卡，App 本身不提权，见系统限制（T14） |
| 系统代理 | — | — | ✅ | ✅ | — | |

## 扩展（插件与脚本）

| 功能 | iOS | macOS | Windows | Linux | Android | 备注 |
|---|---|---|---|---|---|---|
| 从网址导入插件 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 从文件 / 剪贴板导入插件 | ✅ | ✅ | ✅ | ✅ | ✅ | 插件卡片的“更多导入方式”：从文件导入、粘贴插件内容；同名插件再导入会替换（T7） |
| 插件开关、删除 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 插件内容查看（MITM 域名、改写、脚本列表） | ✅ | ✅ | ✅ | ✅ | ✅ | 插件行“更多 → 查看内容”：支持行数、解密 / 排除域名、脚本和数据文件、引擎逐行报告（T7） |
| 脚本（QuickJS） | ✅ | ✅ | ✅ | ✅ | ✅ | |

## 状态、诊断与系统集成

| 功能 | iOS | macOS | Windows | Linux | Android | 备注 |
|---|---|---|---|---|---|---|
| 概览：实时流量、连接列表、断开连接 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 出口地址探测 | ✅ | ✅ | ✅ | ✅ | ✅ | Android 的 VPN 不经过 App 自己的连接，探测时临时起一个本地代理（同一份配置），查完即停（T8） |
| 流量统计（按天） | ✅ | ✅ | ✅ | ✅ | ✅ | 活动页“流量统计”：今天 / 7 天，按小时（天）、主机、策略；与 iOS 相同每 5 秒从连接列表累计，保留 7 天（`crates/traffic-stats`，T9） |
| 导出日志 / 诊断报告 | ✅ | ✅ | ✅ | ✅ | ✅ | Android 报告存到「下载」目录；Linux / Android 报告附最近 80 行应用日志（T8） |
| 扩展日志级别与模块筛选、开发者模式 | ✅ | ✅ | ✅ | ✅ | ✅ | 活动页“日志”：级别（全部 / 警告 / 错误）、模块、文字筛选，拷贝与清空（`crates/app-log`，T10）。桌面和 Android 只有应用自己的日志：内核不单独记日志；Windows 读窗口、应用、HTTPS 解密三份日志，Linux / Android 记连接、断开、订阅更新、网络切换和出错原因，网址去掉查询参数 |
| 系统通知（脚本通知、意外断开、订阅更新失败） | ✅ | ✅ | ✅ | ✅ | ✅ | 设置页“通知”三个开关，默认全开（`crates/app-notifications`，tauri-plugin-notification，T11）。点击脚本通知不能打开它附带的链接：桌面通知插件不支持点击回调 |
| 网络变化后重建连接 | ✅ | ✅ | ✅ | ✅ | ◐ | Android 共用 `network_watch`，Wi-Fi / 移动网络切换待真机确认 |
| 按 Wi-Fi 自动连接 / 断开（On Demand） | ✅ | ✅ | ✅ | ⛔ | ✅ | 设置页“Wi-Fi 规则”：指定 Wi-Fi、其他 Wi-Fi、蜂窝（Android）、有线和其他网络各选连接 / 断开 / 保持不变；Windows、Android 由 App 运行时每 10–12 秒取样、换网络时执行一次（T12） |
| 开机 / 登录后启动、自动连接 | ✅ | ✅ | ✅ | ✅ | ⛔ | Android 每次连接要系统 VPN 授权，见系统限制 |
| `anixops://` 链接 | ✅ | ✅ | ✅ | ✅ | ✅ | 与 iOS 相同的动作：connect / disconnect / toggle、`mode?set=`、`select?node=&group=` 或 `policy=`、`import?url=`（只填进导入框，用户点“导入”才保存）。桌面没有 `global` 模式，`mode` 另外接受 `bypass_china` 等桌面模式名。Windows 写当前用户的 `HKCU\Software\Classes\anixops`，Linux 写 `~/.local/share/applications` 里的 handler 并调 `xdg-mime`，都在启动时注册；第二次启动把链接交给已运行的窗口（Linux 因此改为单实例）。Android 在 manifest 里声明 scheme（tauri-plugin-deep-link，T13） |
| 崩溃报告（本机、脱敏） | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 本机 Prometheus 指标 | ⛔ | ✅ | ✅ | ✅ | ✅ | macOS 在设置页打开，`127.0.0.1:9477/metrics`，默认关闭（A3）；iOS 见系统限制 |
| 用户协议同意、协议入口 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 简体 / 繁体中文 / 英文 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 应用内更新 | — | ✅ | ❌ | ❌ | ❌ | macOS 渠道未开；其余平台目前从下载页更新 |
| 命令行工具 | — | ✅ | ❌ | ❌ | — | 计划 D 决定桌面 UI 之后再做 |
| 小组件、灵动岛、快捷指令 | ✅ | ◐ | — | — | ❌ | 系统能力不同，不做对齐 |

## Windows 运行方式（W1）

Windows 只有一种运行方式：内核在客户端窗口进程里运行，关窗口即断开。服务模式（`networkcore-windows-service`）已删除：
MSI 升级时旧服务随旧版本一起卸载；便携版注册过的服务由客户端下次启动时删除，并恢复它改过的系统代理。卸载时的清理
（根证书、驱动）改由 `networkcore-windows-gui.exe --purge` 完成。

## 系统限制

- **iOS 导入自有根证书**：iOS 只信任用户通过描述文件安装、并在“证书信任设置”里打开的根证书。App 可以用团队 CA 签发，但要让
  用户自己安装描述文件，和现在的流程相同，所以不单独提供“导入”。
- **Android 解密第三方 App**：Android 7 起 App 默认只信任系统证书，用户安装的 CA 只对声明信任用户证书的 App（多数浏览器、
  调试构建）生效；装系统证书需要 root。
- **Android 自动连接**：VpnService 每次建立前要用户在系统对话框授权（或在系统设置里设为“始终开启的 VPN”）。App 不能自己在
  开机后连上。
- **Android 导入自有根证书**：解密在手机本机进行，不需要把团队 CA 装进别的设备；如需统一证书，见上一条，用户仍要在系统设置里装。
- **Linux TUN**：创建 TUN 需要 `CAP_NET_ADMIN`，改路由需要 root。App 本身不提权：每次在 TUN 模式下连接时，用 `pkexec`
  以 root 启动同一个程序的 `--tun-helper`，它只创建非持久的 `anxtun` 网卡、配地址和 `0.0.0.0/1`、`128.0.0.0/1`（IPv6
  同理）两半默认路由，把描述符交回 App 后退出；App 退出或断开时网卡随描述符消失，路由一起删除。所以每次连接会询问一次
  管理员密码（`.deb` 装的 polkit 规则在同一会话里短时间内不再询问）。引擎自己的连接用 `SO_BINDTODEVICE` 绑到原来的默认
  网卡避免回环，需要 Linux 5.7 以上。需要系统装有 `pkexec`；AppImage 运行在只有本用户可读的挂载点上，root 进程读不到，
  不支持 TUN 模式。Linux 的 TUN 模式暂不提供 Fake-IP 开关；打开“启动时自动连接”时，启动后也会先弹出一次授权。
- **Wi-Fi 规则（Windows、Android、Linux）**：iOS / macOS 由系统执行规则，App 不运行也生效；Windows 和 Android 没有对应的系统
  功能，规则只在 App 运行时（Android 包括后台）生效。读 Wi-Fi 名称都要位置权限：Android 在打开规则时询问一次，Windows 11 要在
  “隐私和安全性 > 位置”里允许桌面应用访问位置，否则只能区分“Wi-Fi / 有线”。Linux 桌面的网络管理器各不相同（NetworkManager、
  systemd-networkd、iwd），自带按连接执行脚本的办法，客户端不做。
- **iOS 指标端口**：Packet Tunnel 扩展的监听端口对其他 App 不可见，iOS 上用导出日志代替。
- **macOS 指标端口与自有根证书**：端口由 App 自己在 127.0.0.1 上监听（沙盒加了 `network.server`），每次抓取时向系统扩展里的
  引擎要指标，所以 App 退出后端口关闭；为免意外开放端口默认关闭，Windows / Linux 默认打开。引擎只用 ECDSA P-256 签发证书，
  所以自有根证书的私钥必须是 P-256。

## 补齐计划

每一项单独分支和 PR，按顺序推进；完成后把上表对应格子改成 ✅。

| 编号 | 内容 | 平台 | 状态 |
|---|---|---|---|
| A1 | 单条请求导出（cURL / 代码 / Postman / Insomnia）、抓包会话加密保存 | iOS、macOS | Apple 调试线程在 #126 之后做 |
| A2 | macOS 导入自有根证书 | macOS | 已完成 |
| A3 | macOS 本机指标端口 | macOS | 已完成 |
| T1 | 抓包会话加密保存、列表、载入、删除 | Windows、Linux、Android | 已完成 |
| T2 | 我的规则：规则文本编辑，连接中原地换规则 | Windows、Linux、Android | 已完成 |
| T3 | DNS：自定义服务器、分域 DNS、Hosts、Fake-IP | Windows、Linux、Android | 已完成 |
| T4 | 节点分享链接入口 | Windows、Linux、Android | 已完成 |
| T5 | 手动添加 / 编辑节点，Android 扫码导入 | Windows、Linux、Android | 已完成 |
| T6 | 流量重定向 | Windows、Linux（TUN 模式）、Android | 已完成 |
| T7 | 插件从文件 / 剪贴板导入、插件内容查看 | Windows、Linux、Android | 已完成 |
| T8 | Android 诊断报告保存到“下载”、出口地址 | Android | 已完成 |
| T9 | 按天流量统计 | Windows、Linux、Android | 已完成 |
| T10 | 日志查看（级别、模块筛选） | Windows、Linux、Android | 已完成 |
| T11 | 系统通知 | Windows、Linux、Android | 已完成 |
| T12 | 按 Wi-Fi 自动连接 / 断开 | Windows、Android | 已完成（Linux 桌面不做，见“系统限制”） |
| T13 | `anixops://` 链接 | Windows、Linux、Android | 已完成 |
| T14 | Linux TUN 模式（polkit 提权助手） | Linux | 已完成 |
| W1 | Windows 服务模式下的调试功能 | Windows | 已完成（删除服务模式，调试固定在窗口进程） |
