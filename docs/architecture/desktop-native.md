# Windows / Linux 原生界面方案

状态：方案已确认（2026-11-14，取舍见文末）。核对基准：main `d9d33a2`。

2026-11-14 用户决定：每个平台一套原生客户端。Apple 端已是 SwiftUI，安卓改用 Compose（见
[`android-native-compose.md`](android-native-compose.md)），本页是 Windows 和 Linux：

- Windows：WinUI 3（Windows App SDK）+ C#。
- Linux：GTK4 + libadwaita + Rust（gtk4-rs，界面结构用 relm4）。
- 视觉跟随系统设计语言（Fluent、libadwaita），套 AnixOps 品牌色。
- Tauri 桌面版在原生版功能全部对齐之前仍是正式版，对齐后一次切换并删除。

代价：以后每个界面功能要做四遍（SwiftUI、Compose、WinUI、GTK）。所以本方案的重点是先把界面以外的东西全部收进一份
Rust 核心，让四套界面尽量薄。

## 现状

| 项目 | Windows | Linux |
|---|---|---|
| 界面 | `apps/windows-gui/ui`，React，约 9300 行 | 同一套 |
| 后端 | `apps/windows-gui/src/gui`，约 7000 行，其中 `tauri_ui.rs` 约 4950 行和 Tauri 绑在一起 | `apps/linux-gui`，`backend` 不依赖 Tauri；安卓复用它 |
| 权限 | 整个程序 `requireAdministrator`，每次启动弹 UAC；内核在窗口进程里，关窗口即断开 | 普通用户；系统代理，TUN 由 pkexec 辅助进程建网卡和路由（T14），内核在窗口进程里 |
| 打包 | MSI（WiX 4）+ 便携 ZIP，没有 Authenticode 签名 | tar 包 + deb，依赖 WebKitGTK 4.1 |
| 截图 | 无头 Chromium 渲染，没有真实 WebView2 / WebKitGTK 窗口 | 同左 |

两套后端各写各的，是 `platform-parity.md` 里 Windows 列出现“部分”的原因。

## 技术选型

| 平台 | 候选 | 结论 | 理由 |
|---|---|---|---|
| Windows | WinUI 3 + C# | 采用 | XAML 的工具、文档和社区控件以 C# 为主；UI Automation、Mica、贴靠布局、系统标题栏由框架提供 |
| Windows | WinUI 3 + C++/WinRT | 不采用 | 同一个框架，开发和测试成本更高，用户看不到差别 |
| Windows | Rust（windows-rs）直接写 WinUI | 不采用 | 没有 XAML 编译和工具支持 |
| Windows | WPF | 不采用 | 拿不到 Windows 11 的材质和控件 |
| Linux | GTK4 + libadwaita + Rust | 采用 | 界面和核心同一种语言，直接调用；自适应布局、深浅色、AT-SPI 由框架提供 |
| Linux | GTK4 + C / Vala | 不采用 | 多一种语言，还要给核心写 C 接口 |
| Linux | Qt 6 | 不采用 | 要引入 C++ 或 cxx-qt；多数发行版默认桌面是 GNOME |

最低系统版本：

- Windows 10 1809 起可运行，Mica 等材质只在 Windows 11 生效。.NET 和 Windows App SDK 运行时随安装包自带。
- Linux 需要 GTK 4.14、libadwaita 1.5 以上（Ubuntu 24.04、Debian 13、Fedora 40 起）。Ubuntu 22.04 和 Debian 12 停在
  最后一个 Tauri 版。具体版本线在骨架 PR 里按实际用到的控件核定。

颜色、间距、动效时长从同一份设计 tokens 生成：`.github/scripts/design_tokens.py` 在生成 `tokens.css`、`Tokens.kt` 之外，
再生成 XAML 资源字典和 GTK CSS，CI 检查生成物与源一致。

## 架构：一份核心，四套薄界面

```text
界面（普通用户权限）                          核心（有权限）
Windows  WinUI 3 / C#  ── 命名管道 ────┐
Linux    GTK4 / Rust   ── Unix 套接字 ─┤── client-ipc（帧化 JSON）── client-core ── networkcore 内核
Android  Compose       ── JNI（同进程）─┘                              │
Tauri（过渡期）        ── 同进程调用                             Platform trait
                                                     Windows：Wintun、系统代理、证书库
                                                     Linux：TUN、路由、系统代理、信任库
                                                     Android：VpnService
```

1. **Windows 后端并入共用核心。** 安卓线已把 Linux 后端的命令分发抽成不依赖 Tauri 的 `Client`
   （`Client::invoke(command, json)`，命令名与 `bridge.ts` 一致）和 `Platform` trait。本线把它挪成独立的
   `crates/client-core`，把系统代理、证书信任、开机启动提成 `Platform` 方法，并给 Windows 写一个实现（Wintun、
   系统代理、证书库、开机启动、Wi-Fi 取样）。`tauri_ui.rs` 缩成薄包装。Tauri 版行为不变，两套后端变一套。
2. **`crates/client-ipc`。** 把 `Client::invoke` 和事件流通过本机通道传输：长度前缀的 JSON 帧，请求带编号，事件单向
   推送。Windows 命名管道按安装用户的 SID 设 ACL；Linux 用 `SO_PEERCRED` 核对对端用户，特权动作再过 polkit。
   线上格式（第一版，协议号 1）：每条消息一帧，4 字节大端长度加 UTF-8 JSON，单帧上限 64 MiB。消息有四种 `type`：
   `hello`（双方各先发一条，协议号不同或对端角色不对就断开）、`request`（带 `id`、`command`、`args`）、`response`
   （同 `id`，带 `ok` 或 `error`）、`event`（`name`、`payload`，不需要应答）。两边都可以发请求：界面请求核心执行
   命令，核心请求界面做只有用户会话能做的事（写当前用户的系统代理等）。样例在 `crates/client-ipc/fixtures/`，C# 那边
   用同一批样例测试；改格式就改协议号。
   C# 一侧是 `apps/windows-native/src/AnixOps.Net.Ipc`（`Channel`）：对方的请求各自在线程池上应答，事件按到达顺序
   交给界面，读取不被挡住。它和真实服务（`anixops-net-service --console`）的端到端测试随界面外壳一起加。
   打完招呼之后两边都用 `client_ipc::Peer`：对方的请求每个在自己的线程上应答（核心应答界面的命令时，可能反过来
   问界面要系统代理的写入结果，读取不能被挡住），自己的请求按编号等应答，带超时；事件按发送顺序交给处理方。通道断开时
   所有还在等的请求立刻失败。
   通道本身在 `client_ipc::transport`：Linux 是 Unix 套接字 `/run/anixops-net/core.sock`，核心从内核读对端的用户
   编号（`SO_PEERCRED`），哪个用户可以控制核心在守护进程里决定；Windows 是命名管道 `\\.\pipe\AnixOpsNet.Core`，
   访问表只放行系统、管理员和在本机登录的用户，拒绝来自网络的调用方，第一个实例带"首个实例"标志，名字被别的程序占了
   核心会启动失败而不是悄悄共用。界面连接时只让核心知道自己是谁，不允许核心以界面用户的身份做事。管道的所有句柄都按
   重叠 I/O 打开，否则一个线程在读的时候别的线程写不进去。
   核心和界面怎么接上通道在 `crates/client-service`：服务一侧的 `Windows` 记着当前连着的界面，把界面的命令交给
   `Client::invoke`，同时就是核心的 `UserSession`——系统代理和登录启动项的读写都转成 `session.*` 请求发给界面
   （名字和样例在 `crates/client-service/fixtures/`，C# 界面按同一批样例实现）。
   界面一侧（Rust 写的窗口用）单独放在两个不带内核的 crate 里，窗口进程只链接它们，不链接 `client-core`：
   `crates/client-session` 是"属于当前用户会话的事"（`UserSession`、`LocalSession`、系统代理、登录启动项，
   `client_core::backend` 下的同名模块就是它）；`crates/client-window` 是 `session.*` 的名字、应答方 `WindowEnd`
   和常驻连接 `CoreLink`（连接、握手、断开后每 2 秒重连，状态变化通知窗口；核心一直不在时只通知一次）。规则：
   - 界面连上后核心先问 `session.describe`，答得上来才算用户会话；只发命令的程序（命令行工具）不会被问。
   - 同时连着几个界面时问最后连上的那个，它离开后回到前一个。
   - 没有界面连着时，改用户设置的请求直接失败并说明原因；开机自动连接时系统代理先不设，界面连上后核心补设
     （`Client::session_arrived`）。
   - 通知作为 `notice` 事件发给界面，由界面弹出。
   - 界面不能反过来向核心发 `session.*`。
   还没解决：系统代理的备份只存一份，Windows 快速用户切换时两个用户先后连上会把备份还给错的人；做服务时按用户
   分开存。
   Linux 守护进程是 `apps/linux-daemon`（程序名 `anixops-netd`，systemd 单元在它的 `packaging/` 里）：以 root 运行，
   数据放 `/var/lib/anixops-net`（只有 root 能进），套接字放 `/run/anixops-net/core.sock`。谁可以控制它：root，以及
   在本机有登录会话的用户（登录管理器在 `/run/systemd/users/` 或 `/run/user/` 下有这个用户的记录）；其他账号连上后
   每个命令都得到一句说明原因的拒绝。这条线和 Windows 命名管道的"本机登录用户"一致，不区分坐在屏幕前和远程登录，
   需要区分的动作以后走 polkit。收到 SIGTERM 时先停内核、让界面把系统代理还原，再删套接字。
   守护进程还没做的：TUN 模式现在仍走 `pkexec` 助手，守护进程里要改成直接打开设备；Linux Tauri 版放在用户目录下的
   数据搬到 `/var/lib/anixops-net`；deb / rpm 里装单元文件并启用。
   Windows 服务是 `apps/windows-service`（程序 `anixops-net-service.exe`，服务名 `AnixOpsNetCore`，以 LocalSystem 运行）：
   数据放 `%ProgramData%\AnixOps\Net`，和旧版的 `%ProgramData%\AnixOps\NetworkCore` 分开，两个版本可以同时装着。
   `%ProgramData%` 下任何用户都能建目录，所以数据目录建的时候就带自己的访问表（只有系统和管理员，不继承上级）；目录
   已经存在而所有者不是系统或管理员时服务拒绝启动，防止普通用户抢先放一个自己的目录。目录关好之后才搬旧版数据
   （`legacy_windows::migrate`，搬不动不阻止启动）。服务管理器要求停止或关机时先停内核、让界面还原系统代理再报告
   已停止。`--console` 参数可以不经服务管理器在前台运行，排查用。
   服务还没做的：注册服务和失败后自动重启的设置由 MSI 完成；机器级的 WinHTTP 代理；Wintun 驱动；系统代理备份按用户
   分开存。
3. **契约有类型。** 每个命令的请求和应答样例放在 `client-core/tests/fixtures/*.json`。C# 和 Kotlin 的数据类型必须
   无损解析同一批样例；命令改字段时 Rust、Kotlin、C# 三边测试一起失败。GTK 界面直接用 Rust 类型。
4. **`crates/client-view`。** 现在 TypeScript 里有行为的纯逻辑（抓包筛选表达式、请求表排序、节点筛选、`anixops://`
   解析）在 Rust 里写一次，作为命令暴露，各端调用，样例测试只保留一份。字节、时长、日期这类显示格式由各端用平台的
   区域设置格式化。
   已有的命令（名字都以 `view.` 开头，和核心的命令走同一个入口 `Client::invoke`，所以经通道、JNI 都能调）：
   `parse_app_link`、`find_link_node`、`find_link_member`、`combine_filter`、`next_since`、`merge_requests`、
   `sort_requests`、`describe_request`、`filter_nodes`、`group_nodes`、`sort_nodes`、`delay_tone`。约定：
   - 行（请求、节点）原样传入传出，这个 crate 不认识的字段保留。
   - 被拒绝的链接返回稳定的原因 id（`invalid`、`not_app_link`、`too_long`、`missing_action`、`unknown_action`、
     `missing_parameter`、`unknown_mode`），句子由各端自己的文案给出，不再用简体原文当 key。
   - 名称排序不区分大小写，不按某一种语言的规则；要按用户语言排序的界面自己排。
   - `crates/client-view/fixtures/cases.json` 是唯一一份样例（命令、参数、应答），Rust 测试跑全部，并检查每个命令都有
     样例；其他语言的界面可以拿同一个文件对着自己的通道跑。
   网页界面里的 TypeScript 版本在 Tauri 版删除前保持原样，不改成调命令：它只修 bug，行为以这里为准。
5. **文案单一来源。** 简体、繁体、英文只保留一份目录：`crates/client-strings/catalogue.json`，每句话一个稳定 id
   （小写单词用点连接，如 `nav.settings`），下面是三种语言的句子，语言 id 沿用偏好设置里已有的 `zh`、`zh-Hant`、`en`。
   - 原生的两个桌面窗口不生成副本，直接用这一个文件：GTK 窗口经 `client-strings`（`include_str!`），Windows 窗口把同一
     个文件作为资源编进 `AnixOps.Net.Presentation`。没有生成物，也就没有“生成物与源不一致”这种问题。
   - 占位符写作 `{0}`、`{1}`，三种语言里含义相同。数量、字节、时长由各端按平台的区域设置格式化后再填进去。
   - `client-strings` 的测试检查目录本身：每个 id 三种语言齐全且不为空、占位符一致且不跳号、id 按顺序且不重复
     （两个分支各加句子时只在真正重叠的地方冲突）、繁体里没有留下常见的简体字、没有多余空格和三个点的省略号。
     C# 一侧再检查它读到的 key 三种语言一致、读坏的目录会被拒绝。
   - 系统语言的判断两边一致：区域名里写了字体（`Hans`、`Hant`）就按字体；没写时台湾、香港、澳门用繁体，其余中文
     区域用简体；其他语言用英文。
   - 句子随页面一起加：做哪一页，就把那一页的句子从 TypeScript 搬进目录并换成稳定 id，不一次性搬完没人用的句子。
   - 安卓和 iOS 现在各自有平台格式的文案文件（`strings.xml`、`Localizable.xcstrings`），这一步没有动。它们要并进来时
     由 CI 从目录生成平台文件并检查与源一致。Tauri 版的 TypeScript 文案（key 是简体原文）不迁移，随 Tauri 版一起删除。

## 服务、TUN 与提权

界面以普通用户运行，内核和 TUN 在一个有权限的后台进程里，安装时提权一次。

| 项目 | Windows | Linux |
|---|---|---|
| 后台进程 | Windows 服务 `AnixOpsNetCore`，LocalSystem，MSI 安装时注册 | systemd 单元 `anixops-netd`，只给 `CAP_NET_ADMIN` 和 `CAP_NET_BIND_SERVICE`，由 deb / rpm 安装 |
| TUN | Wintun，沿用 `platform-windows` | 沿用 T14 的网卡、路由和 `SO_BINDTODEVICE` 代码 |
| 界面权限 | 普通用户，启动不弹 UAC | 普通用户；特权动作第一次由 polkit 询问 |
| 关窗口 | 连接保持，托盘图标留着 | 连接保持 |
| 开机自动连接 | 服务随系统启动 | 单元随系统启动 |
| 便携版 | ZIP 版没有服务：只有系统代理，核心作为界面的子进程运行 | tar 包同理 |

`platform-parity.md` 的 W1 删除了旧的服务模式，原因是调试功能固定在窗口进程。这次整个 `client-core`（含抓包、解密、
重放）都在服务里，只有一种运行方式。以管理员身份运行的 WinUI 3 窗口不能接收从资源管理器拖入的文件，这是界面不提权
的另一个理由。

Linux 守护进程相对 T14 辅助进程要补的地方：

- T14 每次连接授权一次，自动连接启动时也会弹框；守护进程安装后不再询问。
- T14 在连接时取一次默认网卡并用 `SO_BINDTODEVICE` 绑定，运行中换网络（Wi-Fi 与有线之间）不跟随；守护进程要监听
  路由变化并更新绑定。
- 没有图形 polkit agent 的会话（纯 SSH、部分精简窗口管理器）弹不出授权；守护进程的日常操作不依赖它，只有安装信任
  证书这类动作需要。
- T14 不改 systemd-resolved，DNS 走默认路由进 TUN 由引擎处理，Linux TUN 没有 Fake-IP 开关；守护进程版沿用，Fake-IP
  随规则与设置的 PR 补上。
- 依赖 iproute2 配置地址和路由（IPv6 尽力而为），守护进程版改用 netlink，去掉对外部命令的依赖。

### 服务和用户会话各做什么

后台进程以系统身份运行，碰不到“当前用户”的设置；有些事必须留在界面所在的用户会话里做。核心里这些事收在一个接口后面
（`client_core::backend::session::UserSession`，不动安卓实现的 `Platform`）：系统代理的写入和恢复、登录启动项已经
接上，根证书信任随 Windows 实现一起接。在同进程形态（Tauri Linux、安卓、便携版）里它的实现是 `LocalSession`，直接调
现在 `backend` 里的模块，行为不变；在后台进程形态里由界面进程经本机通道应答。

Windows 上现有的代理写入（内核仓库 `platform-windows` 的 `apply_system_proxy`）同时写当前用户的 WinINet 设置和整机的
WinHTTP 设置，后者需要管理员权限。原生版里拆开：界面进程只写 WinINet，WinHTTP 由后台进程写。内核仓库已经提供分开的
函数（`apply_current_user_system_proxy`、`apply_winhttp_proxy`）。共用核心的 Windows 代理实现
（`crates/client-session/src/system_proxy_windows.rs`）只写当前用户的设置；恢复前读一次当前值，已经不是客户端写入的那一份就不动它。
WinHTTP 的写入随后台服务一起做（PR 2）。

Windows 的登录启动项还没有接：内核仓库现有的函数建的是提权的计划任务（给需要管理员权限的 Tauri 版用），原生版的
托盘不提权，要用当前用户的 Run 项，随 Windows 的其余系统集成一起补。

| 事情 | 后台进程（核心） | 用户会话（界面进程） |
|---|---|---|
| 内核、TUN、路由、本机代理监听 | 做 | — |
| 订阅、节点、规则、设置的存储 | 做。Windows 在 `%ProgramData%\AnixOps Net`，Linux 在 `/var/lib/anixops-net`，界面不直接读写 | — |
| 抓包会话和它的密钥、MITM 根证书私钥 | 做，界面进程读不到 | — |
| 从文件导入订阅 | 只收内容（`import_subscription_text`），不打开界面给的任何路径：核心以 SYSTEM / root 运行，替用户读文件会读到用户自己读不到的文件，UNC 路径还会让机器账户去做网络认证 | 以当前用户身份读文件（只收普通文件，上限 16 MiB），把文件名和内容发给核心 |
| 系统代理 | 告知端口，核对归属 | 写入和恢复当前用户的代理设置（Windows 的 WinINet 设置，Linux 的 gsettings / kwriteconfig）。恢复前核对当前值仍是客户端写入的 |
| 根证书信任 | 导出证书文件 | 打开系统的证书对话框由用户手动信任（Windows）；写 NSS 数据库（Linux）；读“是否已信任” |
| 当前网络和 Wi-Fi 名称 | 按规则决定连接或断开 | 取样并上报（读 Wi-Fi 名称的位置权限属于用户会话） |
| 系统通知、`anixops://` 链接、托盘 | 发事件 | 做 |
| 登录后启动 | 开机自动连接由后台进程自己完成 | 托盘程序的登录启动项（Windows 当前用户的 Run 项，Linux 的 XDG autostart） |

由此带来的两条规则：

- 系统代理模式需要界面（托盘）进程在运行；TUN 模式不需要。开机自动连接而用户尚未登录时，只有 TUN 模式生效，用户
  登录、托盘启动后再写系统代理。
- 一台机器一份配置。本机通道只接受交互登录的本机用户；多个用户同时登录时，后登录的用户看到同一份连接状态。按用户
  分开配置不在本方案范围内。

## MITM 根证书

- 根证书和私钥由核心生成和保管。Windows 上私钥用 DPAPI 加密存在服务的数据目录；Linux 存在
  `/var/lib/anixops-net`，权限 0600，界面进程读不到。
- 信任仍由用户手动完成，关闭解密保留证书。Windows 装入当前用户的受信任根，系统弹自己的确认框；Linux 按发行版写入
  系统信任库（过 polkit），并保留现在对 NSS 数据库的处理。
- 卸载时清理根证书、服务和虚拟网卡，沿用 `--purge` 的职责。
- 导入团队 CA、手机连电脑抓包的证书页照常保留。

## 打包与签名

| 格式 | 结论 | 说明 |
|---|---|---|
| Windows MSI（WiX 4） | 主安装包 | 注册服务、`anixops://`、开始菜单；升级码不变，从 Tauri 版直接升级，数据目录不变 |
| Windows 便携 ZIP | 保留 | 无服务，只有系统代理 |
| Windows MSIX / 商店 | 暂不做 | MSIX 必须有受信任的代码签名证书才能安装；商店还要审核打包服务的权限 |
| Linux deb、rpm | 主安装包 | 带 systemd 单元和 polkit 策略；rpm 是新增 |
| Linux tar 包 | 保留 | 无守护进程，只有系统代理 |
| Linux Flatpak | 暂不做 | 沙箱里装不了系统服务，只能是无 TUN 的版本；原生版稳定后再评估 |

Windows 发布物现在没有 Authenticode 签名（发布清单里是 `unsigned-no-authenticode-with-github-attestation`）。未签名的
系统服务更容易被安全软件拦截，正式切换前需要代码签名证书，见 `docs/manual-intervention.md`。应用内更新在切换之后单独
做，复用 macOS 的签名更新清单。

## CI、截图基线与无障碍

| 项目 | Windows（`windows-latest`） | Linux（`ubuntu-24.04`） |
|---|---|---|
| 构建与单元测试 | `dotnet build`、`dotnet test`（视图模型、契约样例）；Rust 服务 `cargo test` | `cargo test`，界面状态逻辑和控件分开测 |
| 截图基线 | 在 runner 的桌面会话里启动真实窗口，用演示数据按页面截取；浅色 / 深色 × 简体 / 英文 | GTK 离屏渲染成 PNG；同样的组合，加 1× 和 2× 缩放 |
| 多缩放 | 100 / 125 / 150%。托管 runner 能否切换缩放在验证 PR 里确认，不行就列入人工验收 | 无头 Wayland 合成器补拍 125% 和 150% |
| 界面自动化冒烟 | UI Automation：启动、同意协议、调试模式直连、连接、本机请求出现在列表、断开 | AT-SPI 驱动同一条路径 |
| 无障碍检查 | 遍历 UIA 树：每个可操作控件有名称和角色，Tab 顺序覆盖全部控件；高对比度主题截图一组 | 遍历 AT-SPI 树做同样检查；文字缩放 1.5 截图一组 |

- 基线图提交在仓库里，只能随界面改动的 PR 一起更新，PR 描述附前后对照。比对脚本和目录约定与 Tauri、Apple、安卓的
  基线共用。
- 演示数据用同一份（核心的演示模式），各平台截图内容一致，方便横向对比。
- 每个操作都能只用键盘完成，快捷键表以 `docs/ui-ux.md` 为准。
- 缩放、输入法、触控板滚动手感等 CI 验不了的项目写进 `docs/manual-intervention.md`。

## Windows 验证 PR 的过关标准

WinUI 3 没有自带的数据表格控件，请求列表要自己做虚拟化表格。骨架 PR 同时是一道门，下面三项在 CI 上量出来，有一项
不过就停下来报告，不降低标准继续：

1. **请求表格。** 5 万行演示数据，滚动时不出现空白行，输入筛选条件到列表更新不超过 100 毫秒。
2. **资源占用。** 同一台 runner 上，冷启动到可操作的时间和空闲内存都低于当时的 Tauri 版。
3. **服务链路。** 不提权的界面经命名管道控制服务完成连接、TUN、解密；MSI 安装、升级、卸载各跑一遍，卸载后不留服务、
   网卡和证书。

第 1 项的量法（4c）：程序带 `--page activity --demo-rows 50000 --measure <文件>` 启动时自己测量并退出，
`AnixOps.Net.App.UiTests` 读结果并卡阈值，CI 日志里打印原始数字。

- 表格是 `ListView`（`ItemsStackPanel` 虚拟化，视口外前后各留一屏，关闭占位行），表头和行共用一组列宽。只有视口附近
  的行有控件，行数只影响数据，不影响控件数量。
- 筛选：依次输入 6 个条件（含全部命中和零命中），从写入筛选框到列表换成结果后的第二帧为止，每个都不得超过 100 毫秒。
- 滚动：240 次移动，一半从顶部按 0.9 屏逐页下翻，一半跳到随机位置；每次移动后的下一帧检查视口内每一行都已有内容，
  空白行数必须为 0。
- 这里量的是界面一侧（表格换数据和绘制）。演示数据和筛选在界面进程里（`DemoRequests`、`RequestFilter`），正式版的
  筛选由引擎做，往返服务的耗时在 4d 接上服务后另量。触控板惯性滚动的手感 CI 量不了，列入人工验收。

第 2 项的量法（4d）：CI 在同一台 runner 上下载最新发布的 Tauri 便携版，和刚构建的原生版交替各启动 6 次
（`StartupTests`），第一次单列（会建缓存），后 5 次取中位数，原始数字打印在 CI 日志里。

- 启动时间：从创建进程到 UI Automation 能找到内容为止。原生版找页面标题，Tauri 版找网页文档里第一个有名称的按钮。
  两边都是 10 毫秒轮询一次。UI Automation 查询网页内容本身有开销，这个量法对 Tauri 版略微不利，差距小的时候不能
  只看结论，要看原始数字。
- 空闲内存：可操作后静置 10 秒，把整棵进程树（Tauri 版含 WebView2 的子进程）的专用内存和工作集分别相加。
- 三个中位数（启动时间、专用内存、工作集）原生版都必须更低。
- 原生版现在是普通 JIT 构建，没有做 ReadyToRun 或裁剪；发布打包时再定，届时重新量。
- 上面“原生版”是只有窗口、没有核心的数字。4f 起同一个测试多量一组“原生版加服务”：服务先启动好（相当于开机后
  已在运行），从创建窗口进程到窗口显示“核心服务 <版本>”为止算启动时间，空闲内存把服务进程一起加进去。
  Tauri 版的核心在自己进程里，所以这一组才是对等的比较，三个中位数同样必须更低。服务用 release 构建。
- 仍然不对等的地方：页面还是占位，没有节点列表、图表这些真正吃内存的内容；每做完一批真实页面都要看这组数字。

界面和服务的连接（4f）：

- `AnixOps.Net.Session.CoreLink` 保持窗口到服务的连接，服务不在就每 2 秒再试，服务停了（升级、崩溃）之后自己
  连回去。标题栏显示状态：正在连接、核心服务 <版本>、核心服务没有运行。
- 窗口用 `SessionHandler` 回答服务的 `session.*` 请求，所以系统代理和登录启动改的是当前用户自己的设置，
  窗口和服务都不需要为此提权。
- CI 的“Windows native app”任务构建真实的 `anixops-net-service.exe`（release），测试用 `--console` 在独立的
  管道和数据目录上启动它：保存“登录后启动”会写到用户的 Run 键；导入节点后连接，用户的系统代理指向本地端口，
  断开后恢复成用户原来的代理；服务被杀掉再启动，窗口自动连回并且数据还在。这些测试用测试自己的注册表键，
  不动 runner 的真实设置。
- 还没做：服务发来的通知（`notice`）窗口收到了但还不弹出；窗口里还没有 TUN 模式的开关（设置页，PR 6 之后），
  HTTPS 解密的根证书信任见 4i。

第 3 项的量法（4g，MSI 部分）：CI 在 runner 上用同一批文件打两个版本号的 MSI，然后
（`.github/scripts/native-msi-lifecycle.ps1`）：

- 安装：服务已注册、正在运行、随系统启动、以 LocalSystem 运行；装好的窗口不带任何参数启动后显示
  “核心服务 <版本>”（UI Automation 检查）。
- 升级：窗口开着、用户有登录启动项、数据目录里有文件时装第二个版本。退出码必须是 0（3010 表示要重启才干净，
  算不通过）；旧窗口已关闭，只登记了一个产品，登录启动项和数据都还在，新窗口能连上新服务。
- 卸载：窗口开着时卸载。之后服务不存在、没有残留进程、安装目录不存在、开始菜单快捷方式不存在、登录启动项
  不存在、产品登记不存在。`%ProgramData%\AnixOps\Net` 里的数据（订阅、设置）有意保留。
- 顺序：服务先停（停的过程中让还开着的窗口把用户的系统代理恢复），然后安装器以 LocalSystem 运行
  `anixops-net-service --close-windows`（升级）或 `--purge`（卸载）关掉窗口，再删文件。
- CI 的 runner 是管理员身份，“普通用户不提权也能用”这一点由管道的访问控制测试（Rust）保证，真机上还要用
  标准用户账号实测一次，列入人工验收。
- 第 3 项里的 HTTPS 解密见下面 4i 的量法。

第 3 项的量法（4h，TUN 部分）：

- 做法：`set_tun_mode` 在 Windows 上可用。连接时由核心（也就是服务，LocalSystem）用安装目录里的 `wintun.dll`
  建网卡“AnixOps Net”、加路由，内核自己的连接绑在物理网卡上；断开或服务停止时网卡连同路由一起消失
  （`crates/client-core/src/backend/tun_windows.rs`，网卡和路由的代码沿用 `platform-windows`）。窗口不需要任何
  权限，TUN 模式下也不改用户的系统代理。
- CI 单独一步（会改 runner 的路由几秒钟，所以不和计时的测试一起跑）：窗口这一侧连上真实的服务，分流模式设为
  直连，打开 TUN 模式后连接。检查：网卡出现且已启用；本机发往公网的出口地址变成网卡的地址（路由确实指向
  网卡）；不经任何代理的 HTTPS 请求成功（进了网卡的包确实从另一头出去）；用户的系统代理没被动；连接中不允许
  切换 TUN 模式。断开后：网卡不存在，出口地址和原来一样，网络仍然可用。
- MSI 里带 `wintun.dll`（WireGuard 签名的原版）和它的许可证。安装后对装好的服务再跑一遍上面的测试；卸载后
  检查没有“AnixOps Net”网卡，驱动库里没有 `wintun.inf`。驱动由 `anixops-net-service --purge` 调用 Wintun 自己的
  `WintunDeleteDriver` 移除；别的程序（包括还没下线的 Tauri 版）的网卡还在用时 Wintun 不会删，这不算残留。
- 没量到的：分流模式不是直连时经节点的流量（CI 没有可用的节点）；标准用户账号下的实测（runner 是管理员），
  都列入人工验收。

第 3 项的量法（4i，HTTPS 解密的根证书）：

- 做法：根证书只信任到**当前用户**自己的“受信任的根证书颁发机构”，不装进整机的信任库。原因：服务以
  LocalSystem 运行，任何登录用户的窗口都能向它发命令，而根证书可以由用户导入（`import_mitm_root`）；如果让服务
  往整机信任库里装，普通用户就能借它让同一台电脑上的其他用户信任一个私钥在自己手里的根证书。所以核心把这件事
  交给用户自己的窗口：`session.root_trusted`、`session.trust_root`、`session.untrust_root`
  （`crates/client-service`，样例在 `fixtures/`）。窗口不需要管理员权限；添加和删除时 Windows 自己弹确认框，
  用户选“否”时请求失败并说明原因。核心在数据目录的 `trusted-roots.txt` 里记下被信任过的根证书指纹。
- 没有窗口连接时（比如开机自动连接、用户还没登录）状态显示为未信任，信任请求会说明需要打开窗口。
- Linux 和 Tauri 版不变：会话不回答这几个问题时，核心仍按原来的方式处理（`backend::trust`）。
- 卸载：`anixops-net-service --purge` 按 `trusted-roots.txt` 把这些根证书从当前已登录用户的证书库里删掉
  （删除用户设置里对应的键，不需要确认框）。卸载时没登录的用户会留下这张根证书；它的私钥在只有系统和管理员
  能读的数据目录里。
- CI 单独一步，用真实的证书库，Windows 的确认框由测试通过 UI Automation 点按钮：添加时 Windows 弹
  “Security Warning”，选“是”后证书在用户的证书库里，Windows 验证证书链时接受它；按卸载的做法删键后，证书库里
  没有它，Windows 也不再接受它；用户主动取消信任时 Windows 弹“Root Certificate Store”，确认后同样生效。
  实测到的两点：同一张根证书用户同意过一次后，再次添加 Windows 不再询问（卸载只删证书、不删这条同意记录，
  所以重装后再信任同一张根证书不会再弹框）；每次弹没弹、按的是哪个窗口都打印在 CI 日志里。
  另有不弹框的测试走真实的服务：`open_mitm_root` 让窗口信任服务生成的
  根证书、状态变为已信任、指纹被记下，`remove_mitm_root_trust` 取消信任。
- MSI：对装好的服务请求信任，确认证书只出现在当前用户的证书库、不在整机的；卸载后两处都没有。
- 没量到的：解密后的流量本身（设置页和抓包页接上之后再测）；Firefox 默认不读 Windows 的证书库，需要用户在
  Firefox 里打开“企业根证书”或手动导入，界面要给出说明（PR 8）。

骨架（4b）已有的做法，后面的页面照此继续：

- 界面不带窗口的部分（页面清单、文案、启动参数）在 `AnixOps.Net.Presentation`，普通单元测试覆盖；`AnixOps.Net.App`
  只放 XAML 和接线。
- 控件样式和字号用系统自带的，品牌色和间距来自生成的 `Theme/BrandTokens.xaml`（`Brand…Brush`、`Brand…Radius`、
  `BrandSpacing…`）。高对比度主题不套品牌色。系统强调色（按钮、开关的填充）目前仍是 Windows 的设置，换成品牌色需要
  一组深浅色阶，AnixOps Design 里还没有，补上之后再接。
- 每个可操作控件有 `AutomationId`（侧栏是 `Nav.<页面 id>`，页面标题是 `Page.Title`）。`AnixOps.Net.App.UiTests` 启动
  构建出的程序，经 UI Automation 逐页切换、核对名称，并检查能接收键盘焦点的控件都有名称。
- 截图由同一批测试按 页面 × 语言 × 深浅色 生成，作为 CI 产物上传。入库做基线比对要等页面有了真实内容。
- 启动参数 `--language zh|zh-Hant|en`、`--theme light|dark`、`--page <id>` 只为测试和截图固定环境，不认识的参数忽略。

Linux 骨架（PR 5，`apps/linux-native`）的做法：

- 结构和 Windows 一致：不带窗口的部分（页面清单 `sections`、启动参数 `launch`）不依赖 GTK，所有系统的 workspace 任务
  都测；窗口（`window.rs`，relm4 + gtk4-rs + libadwaita）在 `gui` feature 后面，只在 Linux 上、装了 GTK 4 和 libadwaita
  开发包时构建。有一个测试直接读 `AnixOps.Net.Presentation/Section.cs`，两个窗口的页面清单不一致时失败。
- 布局用 libadwaita 自己的：`AdwNavigationSplitView`，窗口窄于 640sp 时折成一栏；侧栏是 `navigation-sidebar` 列表，
  设置固定在底部。窗口、卡片、文字的颜色都是 libadwaita 的，只把强调色和状态色换成品牌色
  （`data/brand-light.css`、`brand-dark.css`，由 `design_tokens.py` 生成）。GTK 4.14 的样式表里不能判断深浅色，所以
  生成两份、由窗口按当前配色加载；高对比度下不加载。
- 截图不用抓屏：`--screenshot <文件>` 让窗口用自己的渲染器把自己画进 PNG 后退出，CI 在 Xvfb 里用 Cairo 渲染器跑，
  每台机器画出来一样。组合：6 页 × 3 种语言 × 深浅色，加一张窄窗口、一张 2× 缩放；脚本检查尺寸，并检查任意两张
  都不相同（参数没生效或什么都没画时会撞图）。入库做基线比对同样等页面有真实内容。
- 无障碍检查从外面做：`.github/scripts/linux-native-a11y.py` 启动真实窗口，经 AT-SPI（读屏软件用的接口）核对窗口名、
  侧栏两个列表的名称和六个页面的顺序，逐页经无障碍接口选中并等页面内容出现，检查两个列表里始终只有一行被选中、
  每个可操作控件都有名称。三种语言各跑一遍。
- 这一步没有的：和守护进程的连接（标题栏状态）、托盘、deb / rpm 包、Wayland 下的实测（CI 是 X11）。

Linux 窗口接上守护进程（PR 5b）：

- 窗口进程里没有内核。它经 `client-window` 的 `CoreLink` 连守护进程的套接字（默认 `/run/anixops-net/core.sock`，
  `--socket <路径>` 给测试换一个），CI 用 `cargo tree` 检查窗口的依赖里没有 `client-core` 和 `engine-embedded`。
- 侧栏底部一行小字显示连接状态，文案和 Windows 标题栏用同一批（`core.connecting`、`core.connected`、
  `core.unavailable`）。连不上的具体原因只进日志，不直接显示。
- 窗口同时是守护进程的"用户会话"：`session.*` 由 `WindowEnd` 加 `LocalSession` 应答，系统代理按当前桌面
  （GNOME、KDE）写。预览版不写登录启动项（登录后启动的仍是旧窗口），通知等页面做出来时一起接。
- 检查：`client-window` 的测试用真实套接字和一个假核心，覆盖"核心没启动、启动后连上、双向请求和事件、核心离开、
  回来后自动重连、套接字上不是核心的程序不会被当成核心"。CI 的"Linux native app"任务再用真实的 `anixops-netd`
  跑一遍：窗口先显示核心服务没有运行，守护进程启动后显示版本，停掉后回到没有运行，再启动后重新连上；都经 AT-SPI
  读窗口上的文字判断。连上时的窗口另存一张截图。
- 守护进程拒绝当前用户时（没有在本机登录的账号）握手仍然成功，侧栏显示已连上核心；概览页的第一个请求会被拒绝，
  拒绝的原因显示在概览页上，按钮不可用。

概览页（PR 6a Linux，PR 6b Windows）：

- 页面内容：连接状态、连接 / 断开按钮、当前节点、上下行速率和累计、活动连接数；连不上时显示核心给的原因。
- "核心状态对这一页意味着什么"只写一份，在 `crates/client-view` 的 `view.overview`：入参是核心的 `runtime_snapshot`、
  `list_nodes` 和引擎的流量计数，出参是状态（`connected`、`disconnected`、`failed`、`core_error`）、出口（节点或直连）、
  当前节点、能不能连接、原因和流量。样例在 `fixtures/cases.json`。
- 窗口不自己凑这三样：核心有一个 `overview` 命令（`crates/client-core`），拿自己的状态、选中的节点和引擎的流量计数
  （只在已连接时问引擎）调用 `view.overview`，一次往返给出整页内容。两个窗口每秒问一次这个命令，得到的是同一个回答。
- 把回答变成页面上的每一句话、每个数字和按钮能不能按，各窗口各写一份、测试对齐：Linux 在
  `apps/linux-native/src/overview.rs`（控件在 `overview_page.rs`），Windows 在
  `apps/windows-native/src/AnixOps.Net.Presentation/Overview.cs`（控件在 `AnixOps.Net.App/Pages/OverviewPage.xaml`）。
  两边读同一份 `fixtures/cases.json`，数字写法一致：按 1024 进位，一位小数（`1.5 MB/s`），正好落在两个一位小数
  中间时取偶数那个（1280 字节是 `1.2 KB`）。
- Windows 页面按系统设置页的样子排：标题、状态卡片（状态和按钮）、当前节点卡片、三张流量卡片。"连接"用强调色按钮，
  "断开"用普通按钮。
- 向核心发请求都在后台线程，界面线程不等；按钮按下后到核心答复前显示"正在连接 / 正在断开"并禁用。核心离开时
  清空已知状态。
- CI 用真实的 `anixops-netd`：新装的守护进程没有节点，窗口显示"还没有选择节点"、按钮不可用；脚本以另一个程序的身份
  让守护进程改成直连出口，然后经 AT-SPI 按下"连接"，等窗口显示"已连接"和两个速率，截图，再按"断开"。
  守护进程只应答在本机登录的用户，runner 的任务不是登录会话，所以 CI 里先按登录管理器的做法建好 `/run/user/<uid>`。
- Windows 的 CI 用真实的服务（前台运行）做同一件事，经 UI Automation 读字和按按钮，两种语言各一遍并截图。服务把
  "改这个用户的系统代理"交给最新打开的窗口去做，所以测试在应用之后再连一个自己的窗口，代理写进测试自己的注册表键，
  不动 runner 的真实代理设置；测试同时检查连接后这个键指向引擎、断开后恢复。
- 连接列表（概览页"活动连接"旁的按钮打开的对话框）：打开着的连接有哪些、每条去哪、命中哪条规则、经过哪些策略组和
  出口、搜索留下哪些，都由共享部分决定。`crates/client-view` 的 `view.connections` 把引擎的 `connections` 回答变成
  `{ running, total, connections: [{ id, target, network, rule, route, upload, download, startedAtMs }] }`：
  `target` 有域名就是"域名:端口"，否则是目的地址；`route` 把节点 id 换成节点名；最新的在前；搜索词（不分大小写）
  在目标、规则、路线里找。核心的 `connections` 命令（参数是搜索词）一次往返给出这份回答，`close_connection`
  （参数 `id`）关一条，`close_connections` 全部关掉。对话框里：搜索框、"关闭全部"、一行计数（"3 个连接"，搜索时
  "1 / 3 个连接"）、每条连接一行（目标；下面是 UDP 才写的协议、规则、路线、上下行流量）和行末的关闭按钮。没连接、
  搜索没结果、引擎没在运行各有一句话。对话框开着时每秒问一次，同样的连接保持同一行，键盘焦点不跳。
  Linux：`src/connections.rs`（无 GTK，读 `view.connections` 样例的测试）和 `src/connection_list.rs`（`AdwDialog`）。
  Windows：`Presentation/Connections.cs`（同样的测试）和 `Pages/ConnectionsDialog`（`ContentDialog`）。两端的界面测试
  都在连接后经引擎的本机代理向测试自己起的本机服务器保持两条连接（一条按地址、一条按 `localhost`），核对对话框
  列出的和核心列出的一致、搜索只留下 `localhost` 那条、搜不到时的那句话、关掉一条后那条连接在测试这一端确实断了
  而另一条没断、"关闭全部"后另一条也断了且列表为空；并截图（Linux 用 `--show connections`）。
- 还没有的：流量曲线、出口地址、订阅用量；核心的错误原因目前是英文原文。

代理页（PR 6c Linux，PR 6d Windows）：

- 第一版的内容：导入订阅或分享链接、按订阅分组的节点列表、搜索、选择节点、测速。订阅管理（更新、改名、删除、用量）、
  策略组、节点编辑和分享在后面的 PR。
- 和概览页同一个做法：`crates/client-view` 的 `view.proxies` 决定"搜索留下哪些节点、怎么按订阅分段、每个节点上次
  测速的结果算快还是慢"；核心的 `proxies` 命令（参数是搜索词）拿自己的节点列表和记住的测速结果调用它，一次往返给出
  整页内容。`test_proxies`（同样的搜索词）测这些节点并记住结果，`test_node_delays` 的结果也记住。测速结果记在核心
  的内存里，核心重启后清空；这样两个窗口、以及关掉再打开的窗口看到的是同一份结果。
- Linux 窗口不带 GTK 的部分在 `apps/linux-native/src/proxies.rs`，控件在 `proxies_page.rs`。列表用 `GtkListView`，
  只有视口附近的行有控件，几千个节点的订阅和十个节点的一样快；订阅名是列表的分段标题。选中的行就是正在使用的节点，
  选另一行就是让核心换节点。导入、测速、换节点三件事互不等待：长列表测速期间仍然可以换节点。
- 每秒问一次核心（只在代理页显示时）；列表只改有变化的行，不动滚动位置。搜索词变了而上一个回答还没到时，
  到了的旧回答丢掉重新问。
- CI 用真实的 `anixops-netd`：经 AT-SPI 把一条分享链接写进输入框、按"导入"，等节点出现；另一个程序再导入第二个
  节点，窗口不用通知自己显示出来；搜索后的数量和核心说的一致；经无障碍接口选中另一个节点，核心确认已换；按"测速"，
  每一行（地址是文档保留段，没有服务器）都显示"超时"；截图。
- Windows 窗口不带控件的部分在 `AnixOps.Net.Presentation/Proxies.cs`，和 Linux 的 `proxies.rs` 逐项对应，两边读同一批
  `view.proxies` 样例；控件在 `Pages/ProxiesPage`。列表是按订阅分组的 `ListView`（`ItemsStackPanel` 虚拟化，分组标题
  吸顶），选中的行就是正在使用的节点。读屏时每行的名字是节点名，说明是"协议、延迟、是否在用"。
- Windows 的 CI 用真实的服务做同一遍：经 UI Automation 把链接写进输入框、按"导入"，导入成功后输入框清空；测试自己的
  第二个窗口再导入，页面不用通知自己显示出来；搜索、搜不到时的提示、清空搜索；经 `SelectionItemPattern` 选中另一个节点，
  核心确认已换且只有这一行标为在用；按"测速"后每行都说"超时"，核心给第二个窗口的也是同一份结果；两种语言各一遍并截图。
- 分组按订阅的 id 分，不按名字：两个同名的订阅各自一组。粘贴进来的分享链接在核心里有一个固定的中文名字，
  `view.proxies` 给这一组标上 `pasted`，窗口用自己语言的"粘贴的链接"做标题（搜索仍然按核心里的名字匹配）。
- 订阅列表的内容也在共享部分：核心的 `subscriptions` 命令经 `view.subscriptions` 给出每个订阅一行要说的话
  （名字、节点数、上次更新、上次失败的原因、已用和总流量、到期时间和是否已到期、能不能更新）。
- 订阅管理放在代理页上的一个对话框里（页面上的"订阅"按钮），不放在节点列表的分组标题上：一个节点都没有的订阅、
  被搜索滤掉的订阅也要能管。每个订阅一行：名字，下面一行是节点数、上次更新、用量、到期，上次更新失败时再加一行
  原因；右边是"更新"（粘贴的链接没有）、改名、删除。改名和删除先问一次，删除的问题里回车是"取消"。核心正在处理的
  那一行按钮不能再按，其他行不受影响。核心不在时按钮不可用，已打开的对话框关闭。
- Linux：`src/subscriptions.rs`（无 GTK，读 `view.subscriptions` 样例的测试）和 `src/subscription_list.rs`
  （`AdwDialog` 里的 `AdwActionRow`，问题是 `AdwAlertDialog`）。CI 里脚本自己起一个本机订阅服务（带用量和到期的
  响应头，第二次下载多给一个节点），经 AT-SPI 核对这一行说的用量和到期日、按"更新"后节点数变了、改名框里先是
  原名、改名后核心和列表都是新名字、删除先点"取消"什么都不变、再点"删除"后订阅和它的节点都没了；并截图。
- Windows：`Presentation/Subscriptions.cs`（同一批样例的测试）和 `Pages/SubscriptionsDialog`（`ContentDialog` 里的
  列表）。Windows 同一时间只能开一个对话框，所以改名和删除的问题不再弹一层，而是在这个对话框里把列表换成问题，
  Esc 或"取消"回到列表；删除的问题里焦点先落在"取消"上。行是同一个对象原地更新，按过的按钮不会因为刷新丢掉焦点。
  界面测试自己起一个本机订阅服务，对真实服务把同样的步骤走一遍（用量和到期、更新后节点数、改名、取消删除、删除），
  中文浅色和英文深色各一遍并截图。
- 策略组的内容也在共享部分：核心的 `groups` 命令经 `view.groups` 给出每个组的名字、种类（手动选择、自动选最快的、
  故障转移、其他）、能不能由用户选、成员（id 和显示名）和正在用的成员。只有正在使用的订阅自带规则和策略组时才有。
- 策略组放在代理页上的一个对话框里（页面上的"策略组"按钮，没有策略组时不显示）。每个组一行：名字，下面一行是
  种类和"正在用 X"或"还没有选定"。能由用户选的组可以点进去，第二页是成员列表，选中的那一行就是正在用的成员，
  和代理页节点列表的规矩一样；选另一行就是换成员，核心处理期间保留用户选的那一行。自己选成员的组只读。
  换了没有策略组的订阅、或核心不在时，对话框关闭。
- Linux：`src/groups.rs`（无 GTK，读 `view.groups` 样例的测试）和 `src/group_list.rs`（`AdwDialog` 里的
  `AdwNavigationView`，成员页是 `GtkListView`）。CI 里脚本自己起一个带规则和策略组的本机订阅，经 AT-SPI 核对
  "Proxy" 一行说"手动选择 · 正在用 Auto"、自动选的组不能点进去、成员页里选 HK 后核心确认已换且只有这一行选中、
  删掉订阅后对话框和按钮都没了；并截图。
- Windows：`Presentation/Groups.cs`（同一批样例的测试）和 `Pages/GroupsDialog`（`ContentDialog`，成员列表在同一个
  对话框里替换掉分组列表，有"返回"按钮；"关闭"和 Esc 在两页上都是关掉对话框）。界面测试自己起同样的本机订阅，
  对真实服务走同样的步骤（行上说的话、自动选的组没有按钮、经 `SelectionItemPattern` 选 HK、返回后这一行说
  "正在用 HK"、删掉订阅后对话框和按钮都没了），中文浅色和英文深色各一遍并截图。
- 节点详情也由核心决定：`node_details` 给出一个节点的名字、协议、服务器、端口、能不能改（只有正在使用的订阅里的
  节点能改）、不能删的原因（`other` 别的订阅的节点、`last` 只剩这一个、`used` 策略组或规则用到了它）、分享链接和
  二维码（每行一个字符串，`#` 是深色块，不带留白；窗口自己画，不引入 SVG 依赖）。`check_node` 判断填的名字、服务器、
  端口能不能保存，回答的是原因代号（`server.empty`、`server.invalid`、`port.invalid`），窗口按代号用自己的语言说；
  规则只有 `crates/node-edits` 一份，和 iOS 的编辑表单一致。保存是 `edit_node`，删除是 `remove_node`，都只保留到
  下次更新订阅，对话框里写明了这一点。
- 节点详情是每行末尾的一个按钮打开的对话框（按它不会选中这个节点）：名称、服务器、端口三个框，"保存"和"删除节点"，
  下面是分享：二维码、可选中的链接、"复制链接"（复制后按钮写"已复制"）。保存前先问核心能不能保存，不能就在框下面
  写原因，填的内容留着；回车等于按"保存"。删除先问一次，回车是"取消"；删掉后对话框关闭。别的订阅里的节点三个框
  只读，没有按钮，只能分享。核心处理期间两个按钮都不能按。
- Linux：`src/node.rs`（无 GTK，带测试）和 `src/node_dialog.rs`（`AdwDialog` 里的 `AdwEntryRow`，问题是
  `AdwAlertDialog`，二维码是 `GdkMemoryTexture`）。CI 里经 AT-SPI 对真实守护进程核对：对话框里是节点的名字、服务器、
  端口、链接和二维码；端口填 0 被拒绝且核心没变；改名、换服务器和端口后核心和列表都是新的、链接也变了；删除先点
  "取消"什么都不变，再点"删除"后节点没了、对话框关了；并截图（`--show node`）。
- Windows：`Presentation/Node.cs`（同样的测试）和 `Pages/NodeDialog`（`ContentDialog`，二维码是 `WriteableBitmap`；
  删除的问题在同一个对话框里替换掉表单）。界面测试对真实服务走同样的步骤，另外核对打开详情不会切换节点、
  "复制链接"后按钮写"已复制"、只剩一个节点时"删除节点"不可用并说明原因；中文浅色和英文深色各一遍并截图。
- 粘贴的分享链接只有一个"粘贴的链接"：再粘贴时并进已有的那一个，已经有的链接不重复加（共享核心 `Backend::import`）。
  用户给它起了名字之后它就是一个普通订阅，下次粘贴另起一个。英文里一个节点写"1 node"（`proxies.count.one`）。
- 还没有的：测速进度（现在等全部测完才显示）、按延迟排序、出口地址。

## 迁移清单与 PR 顺序

原生版在 `apps/windows-native` 和 `apps/linux-native` 平行开发，以“预览”名称并存安装。`platform-parity.md` 里该平台
每一格都对齐、有截图基线和测试之后，一个 PR 切换并删除该平台的 Tauri 版。哪个平台先对齐就先切哪个。

| PR | 内容 | 前置 |
|---|---|---|
| 0 | 本方案 | — |
| 1a | `crates/client-core`：`backend` 和 `client` 从 `apps/linux-gui` 挪成独立 crate，行为不变 | 安卓线的 `Client` 模块合并 |
| 1b | Windows 后端并入 `client-core`，Tauri Windows 行为不变 | 1a |
| 2 | `client-ipc`、Windows 服务、Linux 守护进程，MSI / deb / rpm 安装与卸载 | 1 |
| 3 | `client-view`；文案单一来源（`crates/client-strings`，原生 Windows 窗口已改为读它，并补上繁体） | 1 |
| 4a | `apps/windows-native/src/AnixOps.Net.Ipc`：界面一侧的通道（C#），帧、四种消息、握手、双向请求；测试读 `crates/client-ipc/fixtures/` 的同一批样例，CI 任务“Windows native app” | 2 |
| 4b | Windows 验证：WinUI 3 工程骨架（`AnixOps.Net.App`）、自绘标题栏、侧栏导航、Mica、设计 tokens 生成 XAML 资源字典、CI 里启动真实窗口做 UIA 检查并截图 | 4a |
| 4c | Windows 验证：请求列表原型（虚拟化表格、筛选），量“过关标准”第 1 项 | 4b |
| 4d | Windows 验证：量“过关标准”第 2 项（启动时间和空闲内存对比已发布的 Tauri 版） | 4c |
| 4e | Windows 验证：`AnixOps.Net.Session`，界面一侧的 `session.*` 处理方（当前用户的系统代理、登录启动、通知）；测试读 `crates/client-service/fixtures/` 的同一批样例 | 4d |
| 4f | Windows 验证：界面接上服务（`CoreLink`、标题栏状态），和真实的 `anixops-net-service --console` 做端到端测试（登录启动、连接时设置和恢复系统代理、服务重启后重连）；第 2 项加量“原生版加服务” | 4e |
| 4g | Windows 验证：原生版 MSI（`installer/windows-native`，WiX 5）：注册并启动服务；CI 里安装、带着打开的窗口升级、带着打开的窗口卸载，卸载后不残留服务、进程、文件、快捷方式和登录启动项。“过关标准”第 3 项里除 TUN 和解密以外的部分 | 4f |
| 4h | Windows 验证：TUN 模式经服务（Wintun 由服务加载，MSI 带上 `wintun.dll`，卸载后不残留网卡和驱动），第 3 项的 TUN 部分 | 4g |
| 4i | Windows 验证：HTTPS 解密的根证书信任（只信任到当前用户的证书库，由窗口添加、Windows 弹确认框；卸载时移除），第 3 项的解密部分 | 4g |
| 5 | Linux 骨架（`apps/linux-native`）：导航、主题、三种语言、CI 截图与 AT-SPI 检查。和守护进程的连接、打包在后面的 PR | 2 |
| 5b | Linux 窗口接上守护进程：`crates/client-session`、`crates/client-window`（`CoreLink`、`WindowEnd`，不带内核），侧栏底部的连接状态，CI 里和真实的 `anixops-netd` 跑"没启动、连上、停掉、重连" | 5 |
| 6a | 概览页（Linux）：`view.overview`、连接 / 断开、当前节点、速率和流量；CI 里对真实守护进程连接和断开 | 5b |
| 6b | 概览页（Windows）：同样的内容；核心新增 `overview` 命令，两个窗口都改用它 | 6a |
| 6c | 代理页（Linux）第一版：`view.proxies`、核心的 `proxies` 和 `test_proxies`、导入、搜索、选节点、测速；CI 里对真实守护进程做一遍 | 6b |
| 6d | 代理页（Windows）第一版：同样的内容 | 6c |
| 6 | 代理 › 首页与代理：连接、状态、流量、连接列表、出口地址、订阅、节点、测速、策略组、分享、节点编辑 | 4、5 |
| 7 | 规则与设置：分流模式、我的规则、规则集、DNS、Fake-IP、屏蔽 QUIC、流量重定向、通知、Wi-Fi 规则、日志、流量统计、诊断、协议、语言 | 6 |
| 8 | 扩展与改写：插件导入 / 开关 / 查看，HTTPS 解密与根证书，解密域名，Map Local / Remote，团队 CA，手机连电脑抓包 | 6 |
| 9 | 调试 › 请求与出口：请求表、筛选、检查器、导出、重放、连接视图、会话保存、检查清单 | 6 |
| 10 | 系统集成：托盘、`anixops://`、系统通知、开机启动、本机指标、命令面板与快捷键 | 6 |
| 11 | 切换：发布流程改出原生包，删除该平台的 Tauri 版，更新文档和下载页 | 全部打勾 |

### PR 1b 的步骤

Windows 后端不是 Linux 后端的变体，而是另一套实现：`tauri_ui.rs` 里 79 个命令，自己的状态结构，存储走
`platform_windows::managed`（受管配置和状态、内嵌配置、`source.txt`、节点修改、Wi-Fi 规则、通知、DNS、我的规则、流量
统计），系统代理恢复前会核对当前值是否仍是客户端写入的，根证书由用户在系统对话框里手动信任。并入时这些行为和用户
已保存的数据都要保留，所以分步做，每一步合并后 Tauri Windows 版都照常可用：

2026-11-15 定下：不做 1b-4。现有 Windows Tauri 版保持自己的后端、只修缺陷，切换到原生版时整体删除；1b-1 到 1b-3
的成果由原生预览版和 CI 验证。所以 1b-1 往共用核心里放 Windows 实现时，不再要求 `apps/windows-gui` 改为调用它们
（已经改过去的抓包会话密钥保持现状）。

| 步骤 | 内容 | 验证 |
|---|---|---|
| 1b-1 | `client_core::backend` 里的系统集成收成一个接缝：系统代理（含归属核对）、开机启动、根证书信任、抓包会话密钥、TUN。Linux 实现是现有模块；Windows 实现从 `apps/windows-gui` 搬入（DPAPI 密钥、`platform-windows` 的代理快照与恢复、启动任务、证书对话框、Wintun），`apps/windows-gui` 改为调用它们，代码只留一份 | 两个系统的 CI 各跑各的实现的测试 |
| 1b-2 | 数据迁移：`Store` 能读 Windows 旧版的全部已存文件并转成 `client-core` 的格式。单向、可重复执行；新存储成功载入一次之前不删旧文件 | 用旧版真实文件样例做夹具测试，逐项核对订阅、节点、选择、规则、DNS、设置 |
| 1b-3 | 命令对齐：Windows 的每个命令都由 `Client::invoke` 应答，去掉 `WINDOWS_ONLY` 清单（TUN 模式、脚本运行时、证书与驱动命令）。两边行为不同的地方逐条列出，取更严格的那个（代理归属核对、手动信任） | 契约样例测试；`every_interface_command_is_answered` 不再有例外 |
| ~~1b-4~~ | 不做（2026-11-15 定下）。原内容：`tauri_ui.rs` 缩成薄包装并删除旧后端 | 无 |

1b-2 的转换在 `client_core::backend::legacy_windows::migrate`（旧目录 `%ProgramData%\AnixOps\NetworkCore` → 共用核心的
存储目录）：只读旧目录，不改不删；新的 `state.json` 最后写，已经存在就什么都不做。由后台服务第一次启动时调用（PR 2），
调用前新目录必须已经限制为只有客户端可读，因为解密根证书的私钥也在搬运之列。

| 旧版保存的东西 | 搬到新存储后 |
|---|---|
| 订阅列表（地址、名称、流量信息、节点数、更新间隔、上次错误）和正在使用的那一个 | 保留。正在使用的订阅连同它的内容和节点修改一起搬，节点不用重新下载就能看到 |
| 选中的节点、分流模式、DNS 预设、屏蔽 QUIC、直连出口、TUN 模式、登录后启动、启动后自动连接 | 保留 |
| 我的规则、自定义 DNS、域名重定向、Wi-Fi 规则、通知开关、流量统计、插件库、ANX 设备许可 | 原样复制（两边格式相同） |
| HTTPS 解密的根证书和私钥 | 原样复制，系统里已有的信任继续有效 |
| 已下载的公共规则列表（国内域名、国内 IP 等） | 转成新格式；旧版没下载过的不凭空生成 |
| 订阅的上次更新时间 | 不保留：旧版写的是不带时区的本地时间。搬完后每个订阅会尽快更新一次 |
| 没在使用的订阅的内容 | 旧版本来就没存，选中或更新时重新下载 |
| 旧版写入的系统代理备份、它安装的证书指纹和驱动 | 不搬：由旧版程序自己恢复和卸载 |
| 抓包会话 | 不搬：密钥绑在旧版程序的 Windows 账户上，会话本身一周过期 |
| HTTPS 解密开关、主题、调试模式、自动恢复内核 / 自动刷新订阅 / 自动选最快节点 | 开关不搬（根证书在，重新打开不用再信任）；后三项共用核心还没有对应功能，随 1b-3 补齐后再搬 |

- PR 1 到 3 不碰界面，对现在的 Tauri 版也有好处。
- PR 6 到 10 每个平台各一个 PR，Windows 先、Linux 后；如果 Windows 卡在验证上，Linux 先走。
- 骨架 PR 合并时在 `platform-parity.md` 加“Windows 原生”“Linux 原生”两列，每个 PR 合并后打勾。
- 安卓和两个桌面端都切换之后，React 界面和 Tauri 依赖才整体删除。
- 迁移期间新功能先进 `client-core`，Tauri 界面只接入到可用。

## 对 Tauri 界面工作的影响

用户 2026-11-14 决定“只留能带到原生的”：

| 项目 | 处理 |
|---|---|
| 请求检查器的信息结构、结构化正文 | 做完，是原生版照着做的规格 |
| 全平台截图与基线比对 | 做完，原生截图复用比对脚本和目录约定 |
| 节点表格、密度分级 | 做 |
| 快捷键表和命令清单 | 作为规格写进 `docs/ui-ux.md`；Tauri 里不实现命令面板 |
| 动效规范 | 写进 AnixOps Design 并应用到 Apple；Tauri 不铺开 |
| 系统字体栈、渲染出错兜底页、缺陷修复 | 做 |
| 其余手写控件换 react-aria-components | 停 |
| Windows 融合标题栏 | 停，WinUI 3 自带 |
| 真实 WebView 窗口多缩放截图 | 停，验收标准移到原生版 |
| 桌面自有风格重绘 | 停，被“跟随系统设计语言”取代 |

## 风险

| 风险 | 应对 |
|---|---|
| WinUI 3 表格和性能不达标 | 验证 PR 的三项标准；不过关就报告，不硬上 |
| 四套界面功能走样 | 逻辑和文案只有一份；契约样例测试；对照表每个 PR 更新；同一份演示数据的截图并排看 |
| CI 验不了手感 | 真实窗口截图和自动化；其余列入人工验收 |
| GTK 界面在 KDE 上是 GNOME 观感 | 接受；托盘用 StatusNotifierItem |
| 不再支持 Ubuntu 22.04、Debian 12 | 切换前在下载页和发布说明写明 |
| Windows 安装包变大 | 验证 PR 量出实际体积后记录在这里 |
| 未签名的系统服务被安全软件拦截 | 需要代码签名证书 |

## 已定的取舍

1. **范围**：已定（用户 2026-11-14）：Windows 和 Linux 都换原生，Windows 先过验证 PR。
2. **视觉**：已定（用户 2026-11-14）：跟随系统设计语言加品牌色，取代此前“桌面自有风格”的决定。
3. **Windows 权限**：已定（用户 2026-11-14）：界面不提权，内核在系统服务里。
4. **Linux TUN 的进程形态**：已定（用户 2026-11-14）：原生版改用常驻 systemd 守护进程，复用 T14 的网卡和路由代码；
   Tauri Linux 版继续用 pkexec 辅助进程，不改。
5. **Tauri 界面工作**：已定（用户 2026-11-14）：只留能带到原生的。
