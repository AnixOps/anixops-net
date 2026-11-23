# 全平台功能对照

本页对照 iOS、macOS、Windows、Linux、Android 五个客户端的功能，列出缺口和补齐计划。各平台实现细节分别见
[`debug-mode.md`](debug-mode.md)、[`phone-debugging.md`](phone-debugging.md) 和 `apps/*/README.md`。

- 界面：iOS 和 macOS 共用 SwiftUI（`apps/ios`），Windows、Linux 共用 Tauri 界面（`apps/windows-gui/ui`），Android 是
  原生 Kotlin + Jetpack Compose（`apps/android-native`，见 [`architecture/android-native-compose.md`](architecture/android-native-compose.md)）。
  后端分别是 `apps/windows-gui`（Windows）和 `crates/client-core`（Linux 与 Android 共用，Android 经 JNI 调用）。
- 内核能力都在 networkcore（固定 `rev`），客户端只决定用不用、怎么呈现。
- Windows 和 Linux 正在改为原生界面（WinUI 3、GTK4 + libadwaita），方案和迁移清单见
  [`architecture/desktop-native.md`](architecture/desktop-native.md)。切换之前 Tauri 桌面版仍是正式版，本表的 Windows、
  Linux 两列指 Tauri 版；原生骨架合并时会加“Windows 原生”“Linux 原生”两列。
- 新增或改动功能时请同步更新本表。一个功能只在部分平台上线时，PR 描述写明其余平台的计划或系统限制。

图例：✅ 已有 · ◐ 部分（见备注） · ❌ 缺，计划补齐 · ⛔ 系统限制或产品决定不做（见“系统限制”） · — 不适用

核对基准：2026-11-15，main `2731671`，内核 `e5e158a`；进行中的 PR 单独标出。

## 调试（网络调试客户端计划）

| 功能 | iOS | macOS | Windows | Linux | Android | 备注 |
|---|---|---|---|---|---|---|
| 代理 / 调试双模式、首次启动选择 | ✅ | ✅ | ✅ | ✅ | ✅ | #119、#122 |
| 调试模式直连出口 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 开始 / 停止 / 清空抓包 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 连接抓包内容（文本 / 十六进制） | ✅ | ✅ | ◐ | ✅ | ✅ | 同上 |
| 导出 pcapng / HAR | ✅ | ✅ | ◐ | ✅ | ✅ | 同上 |
| HTTP 请求列表、快捷筛选、表达式搜索 | ✅ | ✅ | ◐ | ✅ | ✅ | Apple #126 |
| 单条请求导出 cURL / 代码 / Postman / Insomnia，默认脱敏 | ✅ | ✅ | ◐ | ✅ | ✅ | Apple #137（A1） |
| 抓包会话加密保存、7 天清理、载入回看 | ✅ | ✅ | ✅ | ✅ | ✅ | Tauri 见 `debug-mode.md`；Apple #137（A1） |
| 断开或重连时自动保存抓包、重连后恢复抓包 | ✅ | ✅ | ✅ | ✅ | ✅ | 内核没有解密策略热替换，改解密域名、映射、插件会重启引擎；Tauri / Android 见 `debug-mode.md`“抓包跟引擎同生命周期” |
| 单条重放、编辑后重放 | ✅ | ✅ | ◐ | ✅ | ✅ | Android 经内核重放代理（#128） |
| Map Local / Map Remote | ✅ | ✅ | ✅ | ✅ | ✅ | 桌面和 Android 需开启 HTTPS 解密；Apple 端不受插件总开关影响，首次新增要解密的主机时先确认条款（docs/debug-mode.md） |
| HTTP/2 MITM | ✅ | ✅ | ✅ | ✅ | ✅ | 内核 #233，各平台共用 |
| HTTPS 解密开关 + 首次开启确认条款 | ✅ | ✅ | ✅ | ✅ | ✅ | #117；Apple 端解密域名、“解密此域名”和映射也先确认（docs/debug-mode.md） |
| 根证书由用户手动信任、关闭解密保留证书 | ✅ | ✅ | ✅ | ✅ | ✅ | 桌面 #123；Android 本来就在系统设置里手动安装 |
| 手机连电脑抓包（局域网共享 + 证书页） | — | ✅ | ✅ | ✅ | — | 手机本身不需要；默认要求随机生成的代理用户名和密码（存系统密钥库），可关闭 |
| 导入自有根证书（团队 CA） | ⛔ | ✅ | ✅ | ✅ | ⛔ | macOS 在扩展页“使用自己的根证书…”粘贴 PEM（A2），密钥须为 P-256；iOS / Android 见系统限制 |
| 解密对第三方 App 生效 | ✅ | ✅ | ✅ | ✅ | ⛔ | Android 7 起 App 默认不信任用户证书 |

## 代理与规则

| 功能 | iOS | macOS | Windows | Linux | Android | 备注 |
|---|---|---|---|---|---|---|
| 订阅导入、更新、自动更新、重命名 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 节点测速、选择最快、策略组切换 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 节点体检（几轮建连、TLS 握手和请求，可选测下载速度） | ✅ | ✅ | ❌ | ❌ | ✅ | `client-core` 有体检命令，Android 在节点列表里打开；Tauri 桌面界面还没有入口（原生 WinUI / GTK 版已有，见 [`architecture/desktop-native.md`](architecture/desktop-native.md)） |
| 节点分享链接 | ✅ | ✅ | ✅ | ✅ | ✅ | Tauri 在节点卡片的“更多”菜单里；触屏上菜单按钮常显（T4） |
| 手动添加 / 编辑节点 | ✅ | ✅ | ✅ | ✅ | ✅ | 添加 = 粘贴分享链接导入；编辑名称、服务器、端口和删除节点只改本机，更新订阅后恢复，与 iOS 相同（`crates/node-edits`，T5）。策略组或规则用到的节点和最后一个节点不能删；iOS / macOS 还不能删其他节点经它连接的前置节点（`via`），桌面与 Android 的 `crates/node-edits` 暂未检查这一项 |
| 扫码导入 | ✅ | — | — | — | ✅ | Android：订阅页“扫码”用相机读订阅或分享链接二维码，填进链接框后点“导入”（CameraX + ZXing，T5） |
| 分流模式：规则、直连（不带预设分流模式，见 [no-presets.md](no-presets.md)） | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 分流模式：全局 | ✅ | ✅ | ❌ | ❌ | ❌ | 内核运行时支持 `global`，但桌面与 Android 用的 profile 辅助层（`engine_embedded::profile::RoutingMode`）和 `client-core` 的 `RoutingMode` 只有规则、直连，界面不显示全局，`anixops://mode?set=global` 被拒绝。替代办法：只有一个节点的配置在规则模式下本来就全部走该节点；有订阅规则时，在“我的规则”里为要排查的站点加一条走代理的规则（排在订阅规则前面） |
| 我的规则（规则文本编辑、连接中原地换规则） | ✅ | ✅ | ◐ | ◐ | ◐ | 内核 `reload_rules`（T2）。桌面与 Android 用 `RULE-SET,https://…,策略` 导入规则集（经 PROXY 每天下载，上次成功下载的内容保存在本机，重启后直接使用），GEOSITE,cn / GEOIP,CN 用内置公开列表，其他 GEOSITE 分类要订阅自带同名规则集；Windows TUN 模式下次连接生效 |
| 规则测试、规则命中次数、GeoIP / ASN 数据（按 URL 添加，缺数据逐条显示） | ✅ | ✅ | ✅ | ✅ | ✅ | 内核 `test_route`、`rule_hits`、`rule_sets.missing`。PROCESS-NAME 只在 Windows 和 Linux 的 TUN 生效，iOS、macOS、Android 在规则页提示不生效 |
| SRC-PORT、HOSTNAME-TYPE、SUBNET、PROTOCOL STUN / DOH / DOQ 规则，FINAL dns-failed | ✅ | ✅ | ✅ | ✅ | ✅ | 内核 networkcore #259。SUBNET 按客户端上报的网络（`set_network`）匹配，iOS / macOS 读不到 Wi-Fi 名 |
| CELLULAR-RADIO 规则 | ✅ | — | — | — | — | iOS 隧道上报蜂窝制式（Core Telephony）；macOS、Android 在规则页提示不生效 |
| DEVICE-NAME 规则与设备名 | — | ✅ | ✅ | ✅ | — | Mac 作网关、桌面局域网共享时按来源 IP 给设备起名（引擎 `routing.devices`）；iOS、Android 在规则页提示不生效 |
| 规则集查看、更新远程规则集 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| DNS 预设 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 自定义 DNS 服务器、分域 DNS、Hosts | ✅ | ✅ | ✅ | ✅ | ✅ | T3：桌面与 Android 用文本编辑（`名称 = 地址`、`DOMAIN-SUFFIX,cn,服务器`、`域名 = 地址`），iOS / macOS 是表单 |
| DNS 上游 UDP / DoQ、默认竞速、备用与系统 DNS | ✅ | ✅ | ✅ | ✅ | ✅ | 上游可写 `udp://`、`quic://`（DoQ）、`system`（本机网络的 DNS，总是直连）；最多 4 个 `default` 同时查询取最先的应答，`fallback` 在默认或规则的服务器失败时再查（公司 VPN 内部域名）。桌面与 Android 在服务器行尾写 `default` / `fallback`，iOS / macOS 长按服务器设置。Windows / Linux 的 TUN 把系统 DNS 指向隧道，客户端读物理网卡的 DNS 填给内核（`desktop_dns::system`），iOS 与 Android 的隧道进程本身不走隧道，用系统解析 |
| Fake-IP | ✅ | ✅ | ✅ | ❌ | ✅ | IPv4 用 198.18.0.0/15，IPv6（AAAA）用 fc00::/18。只在接管 IP 流量时有意义：Windows 在 TUN 模式下生效，Android 在 VPN 里生效（T3）。Linux 的 TUN 模式（T14）暂不提供 Fake-IP 开关（见系统限制）；系统代理模式下代理拿到的本来就是域名 |
| 屏蔽 QUIC | ✅ | ✅ | ✅ | ✅ | ✅ | #106 |
| 流量重定向（TCP / UDP 劫持到开发服务器） | ✅ | ✅ | ✅ | ✅ | ✅ | 扩展 / 改写页“流量重定向”，与 iOS 相同的地址 / 域名 / 端口 / 协议 → 目标写法（引擎 `redirects`，`desktop_dns::redirects`）。Windows 和 Linux 不走系统代理的程序要开 TUN 模式（T6） |
| TUN（接管全部流量） | ✅ | ✅ | ✅ | ✅ | ✅ | Linux 每次连接经 pkexec（polkit）提权助手创建虚拟网卡，App 本身不提权，见系统限制（T14） |
| 系统代理 | ✅ | ✅ | ✅ | ✅ | — | 苹果平台的“接管方式”：iOS 默认 VPN + 系统代理（隧道里同时注册 `NEProxySettings`，指向内核 `127.0.0.1` 的 `system_proxy` 监听），可改仅 VPN；macOS 另有仅系统代理（不建默认路由）。跳过代理的地址可编辑；macOS 还有网关模式（局域网设备把路由器设为 Mac），见 [`system-proxy.md`](system-proxy.md) |

## 扩展（插件与脚本）

| 功能 | iOS | macOS | Windows | Linux | Android | 备注 |
|---|---|---|---|---|---|---|
| 从网址导入插件 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 从文件 / 剪贴板导入插件 | ✅ | ✅ | ✅ | ✅ | ✅ | 插件卡片的“更多导入方式”：从文件导入、粘贴插件内容；同名插件再导入会替换（T7） |
| 插件开关、删除 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 脚本运行前确认 | ✅ | ✅ | ✅ | ✅ | ✅ | 写法不同：Windows、Linux、Android 导入的插件先关着，导入后立即打开审阅（解密域名、改写行数、脚本和数据文件，`*` 等解密所有网站的通配醒目警告），点“打开插件”才生效；打开插件的开关也先经过审阅。再导入同一插件时，只有文本和每个脚本的内容都没变才保持打开，否则关掉重新审阅；升级前已导入的插件保持原来的开关（`crates/desktop-mitm` `plugins::save`）。iOS / macOS 导入后插件的解密域名和改写立即生效，脚本逐个允许后才下载运行 |
| 插件内容查看（MITM 域名、改写、脚本列表） | ✅ | ✅ | ✅ | ✅ | ✅ | 插件行“更多 → 查看内容”：支持行数、解密 / 排除域名、脚本和数据文件、引擎逐行报告（T7） |
| 脚本（QuickJS） | ✅ | ✅ | ✅ | ✅ | ✅ | |

## 状态、诊断与系统集成

| 功能 | iOS | macOS | Windows | Linux | Android | 备注 |
|---|---|---|---|---|---|---|
| 概览：实时流量、连接列表、断开连接 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 出口地址探测 | ✅ | ✅ | ✅ | ✅ | ✅ | Android 的 VPN 不经过 App 自己的连接，探测时临时起一个本地代理（同一份配置），查完即停（T8） |
| 流量统计（按天） | ✅ | ✅ | ✅ | ✅ | ✅ | 活动页“流量统计”：今天 / 7 天，按小时（天）、主机、策略；与 iOS 相同每 5 秒从连接列表累计，保留 7 天（`crates/traffic-stats`，T9） |
| 导出日志 / 诊断报告 | ✅ | ✅ | ✅ | ✅ | ✅ | Android 报告存到「下载」目录；Linux / Android 报告附最近 80 行应用日志（T8） |
| 扩展日志级别与模块筛选、开发者模式 | ✅ | ✅ | ✅ | ✅ | ✅ | 活动页“日志”：级别（全部 / 警告 / 错误）、模块、文字筛选，拷贝与清空（`crates/app-log`，T10）。桌面和 Android 只有应用自己的日志：内核不单独记日志；Windows 读窗口、应用、HTTPS 解密三份日志，Linux / Android 记连接、断开、订阅更新、网络切换和出错原因，网址只保留协议和域名（路径、查询参数、分享链接的密码与 UUID 都去掉），订阅下载失败的原因、订阅卡片上的错误和系统通知也不含网址 |
| 系统通知（脚本通知、意外断开、订阅更新失败） | ✅ | ✅ | ✅ | ✅ | ✅ | 设置页“通知”三个开关，默认全开（`crates/app-notifications`，tauri-plugin-notification，T11）。桌面点击脚本通知不能打开它附带的链接：桌面通知插件不支持点击回调；Android 点击通知打开脚本附带的 http / https 链接，没有链接时打开 App，长文字可展开，“意外断开”“订阅更新失败”各只保留最新一条，脚本通知逐条叠加 |
| 网络变化后重建连接 | ✅ | ✅ | ✅ | ✅ | ◐ | Android 共用 `network_watch`，Wi-Fi / 移动网络切换待真机确认 |
| 按 Wi-Fi 自动连接 / 断开（On Demand） | ✅ | ✅ | ✅ | ⛔ | ✅ | 设置页“Wi-Fi 规则”：指定 Wi-Fi、其他 Wi-Fi、蜂窝（Android）、有线和其他网络各选连接 / 断开 / 保持不变；Windows、Android 由 App 运行时每 10–12 秒取样、换网络时执行一次（T12） |
| 开机 / 登录后启动、自动连接 | ✅ | ✅ | ✅ | ✅ | ◐ | Android 在系统设置里把本 App 设为“始终开启的 VPN”后，开机由系统启动并自动连接；App 自己不能开机连接，见系统限制 |
| `anixops://` 链接 | ✅ | ✅ | ✅ | ✅ | ✅ | 与 iOS 相同的动作：connect / disconnect / toggle、`mode?set=`、`select?node=&group=` 或 `policy=`、`import?url=`（只填进导入框，用户点“导入”才保存）。Windows、Linux、Android 没有 `global` 模式（见“分流模式：全局”一行），这些平台拒绝 `mode?set=global`；公开版不带预设分流模式（[no-presets.md](no-presets.md)），旧版预设模式名一律拒绝。Windows 写当前用户的 `HKCU\Software\Classes\anixops`，命令是不需要管理员权限的 `anixops-net-link.exe`：客户端在运行时经窗口消息把链接交给它，不弹 UAC；没运行时才以管理员身份启动客户端（一次 UAC），拒绝 UAC 时链接保留一小时、下次启动客户端时处理（`installer/windows/README.md`）。Linux 写 `~/.local/share/applications` 里的 handler 并调 `xdg-mime`，都在启动时注册；第二次启动先把链接交给已运行的窗口再退出，不打开数据目录、不碰系统代理（Linux 因此改为单实例，并以数据目录里的 `instance.lock` 兜底）。冷启动收到的链接排队，等节点和状态读完、模式选定后再执行。Android 在 manifest 里声明 scheme，由共用的 `view.parse_app_link` 解析（T13） |
| 崩溃报告（本机、脱敏） | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 本机 Prometheus 指标 | ⛔ | ✅ | ✅ | ✅ | ⛔ | `127.0.0.1:9477/metrics`，各端都默认关闭，在设置页打开（macOS A3；Windows / Linux 在“诊断与隐私”）；只回答 `Host` 为本机回环地址的请求。iOS 见系统限制；Android 上本机任何 App 都能连回环端口，不提供 |
| 用户协议同意、协议入口 | ✅ | ✅ | ✅ | ✅ | ✅ | |
| 简体 / 繁体中文 / 英文 | ✅ | ✅ | ✅ | ✅ | ✅ | 跟随系统语言；其他语言一律显示英文（[ui-ux.md](ui-ux.md)“界面语言”） |
| 应用内更新 | — | ✅ | ❌ | ❌ | ❌ | macOS 渠道未开；其余平台目前从下载页更新 |
| 命令行工具 | — | ✅ | ❌ | ❌ | — | 计划 D 决定桌面 UI 之后再做 |
| 小组件、灵动岛、快捷指令 | ✅ | ◐ | — | — | ◐ | Android 有快捷设置磁贴（连接 / 断开，显示所用节点）；系统能力不同，其余不做对齐 |

## Windows 运行方式（W1）

Windows 只有一种运行方式：内核在客户端窗口进程里运行，关窗口即断开。服务模式（`networkcore-windows-service`）已删除：
MSI 升级时旧服务随旧版本一起卸载；便携版注册过的服务由客户端下次启动时删除，并恢复它改过的系统代理。卸载时的清理
（卸载时已登录用户信任过的 HTTPS 解密根证书、它的私钥、设置页安装的驱动）改由 `networkcore-windows-gui.exe --purge` 完成，
升级时不执行；升级保留托管配置、登录后启动任务和信任过的根证书（`installer/windows/README.md`）。数据目录
`%ProgramData%\AnixOps\NetworkCore` 只有 SYSTEM 和 Administrators 可以访问。

## 系统限制

- **iOS 导入自有根证书**：iOS 只信任用户通过描述文件安装、并在“证书信任设置”里打开的根证书。App 可以用团队 CA 签发，但要让
  用户自己安装描述文件，和现在的流程相同，所以不单独提供“导入”。
- **Android 解密第三方 App**：Android 7 起 App 默认只信任系统证书，用户安装的 CA 只对声明信任用户证书的 App（多数浏览器、
  调试构建）生效；装系统证书需要 root。
- **Android 自动连接**：VpnService 第一次建立前要用户在系统对话框授权。App 不能自己在开机后连上；用户在系统设置里把它设为
  “始终开启的 VPN”后，由系统在开机时启动并连接。
- **Android 导入自有根证书**：解密在手机本机进行，不需要把团队 CA 装进别的设备；如需统一证书，见上一条，用户仍要在系统设置里装。
- **Linux TUN**：创建 TUN 需要 `CAP_NET_ADMIN`，改路由需要 root。App 本身不提权：每次在 TUN 模式下连接时，用 `pkexec`
  以 root 启动同一个程序的 `--tun-helper`，它只创建非持久的 `anxtun` 网卡、配地址和 `0.0.0.0/1`、`128.0.0.0/1`（IPv6
  同理）两半默认路由，把描述符交回 App 后退出；App 退出或断开时网卡随描述符消失，路由一起删除。所以每次连接会询问一次
  管理员密码（`.deb` 装的 polkit 规则在同一会话里短时间内不再询问）。引擎自己的连接用 `SO_BINDTODEVICE` 绑到原来的默认
  网卡避免回环，需要 Linux 5.7 以上。需要系统装有 `pkexec`；AppImage 运行在只有本用户可读的挂载点上，root 进程读不到，
  不支持 TUN 模式。Linux 的 TUN 模式暂不提供 Fake-IP 开关；打开“启动时自动连接”时，启动后也会先弹出一次授权。
- **Wi-Fi 规则（Windows、Android、Linux）**：iOS / macOS 由系统执行规则，App 不运行也生效；Windows 和 Android 没有对应的系统
  功能，规则只在 App 运行时（Android 包括后台）生效。Android 打开规则后，VPN 断开时服务留在前台并显示“等待网络”通知，
  以便在后台按规则重新连接；规则发起的连接失败时按 30 秒起、最长 10 分钟的间隔重试，并发一条通知。读 Wi-Fi 名称都要位置
  权限：Android 在打开规则时询问精确位置；Android 10 起后台读名称还要“始终允许”位置（设置页说明用途后再跳到系统设置），
  没有时 App 在后台读不到名称，按名称写的规则这段时间保持不变，而不是当成“其他 Wi-Fi”。Windows 11 要在
  “隐私和安全性 > 位置”里允许桌面应用访问位置，否则只能区分“Wi-Fi / 有线”。Linux 桌面的网络管理器各不相同（NetworkManager、
  systemd-networkd、iwd），自带按连接执行脚本的办法，客户端不做。
- **iOS 指标端口**：Packet Tunnel 扩展的监听端口对其他 App 不可见，iOS 上用导出日志代替。
- **macOS 指标端口与自有根证书**：端口由 App 自己在 127.0.0.1 上监听（沙盒加了 `network.server`），每次抓取时向系统扩展里的
  引擎要指标，所以 App 退出后端口关闭；为免意外开放端口默认关闭，Windows / Linux 同样默认关闭。引擎只用 ECDSA P-256 签发证书，
  所以自有根证书的私钥必须是 P-256。

## 补齐计划

每一项单独分支和 PR，按顺序推进；完成后把上表对应格子改成 ✅。

| 编号 | 内容 | 平台 | 状态 |
|---|---|---|---|
| A1 | 单条请求导出（cURL / 代码 / Postman / Insomnia）、抓包会话加密保存 | iOS、macOS | 已完成（#137） |
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
