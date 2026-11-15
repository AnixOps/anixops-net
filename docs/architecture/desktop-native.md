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
   （名字和样例在 `crates/client-service/fixtures/`，C# 界面按同一批样例实现）。规则：
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
5. **文案单一来源。** 简体、繁体、英文只保留一份目录，CI 生成 `strings.xml`、`.resw`、gettext 和 TypeScript，并检查
   生成物与源一致。现在 TypeScript 的 key 是简体原文，迁移时换成稳定 id。

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
（`backend/system_proxy_windows.rs`）只写当前用户的设置；恢复前读一次当前值，已经不是客户端写入的那一份就不动它。
WinHTTP 的写入随后台服务一起做（PR 2）。

Windows 的登录启动项还没有接：内核仓库现有的函数建的是提权的计划任务（给需要管理员权限的 Tauri 版用），原生版的
托盘不提权，要用当前用户的 Run 项，随 Windows 的其余系统集成一起补。

| 事情 | 后台进程（核心） | 用户会话（界面进程） |
|---|---|---|
| 内核、TUN、路由、本机代理监听 | 做 | — |
| 订阅、节点、规则、设置的存储 | 做。Windows 在 `%ProgramData%\AnixOps Net`，Linux 在 `/var/lib/anixops-net`，界面不直接读写 | — |
| 抓包会话和它的密钥、MITM 根证书私钥 | 做，界面进程读不到 | — |
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

骨架（4b）已有的做法，后面的页面照此继续：

- 界面不带窗口的部分（页面清单、文案、启动参数）在 `AnixOps.Net.Presentation`，普通单元测试覆盖；`AnixOps.Net.App`
  只放 XAML 和接线。
- 控件样式和字号用系统自带的，品牌色和间距来自生成的 `Theme/BrandTokens.xaml`（`Brand…Brush`、`Brand…Radius`、
  `BrandSpacing…`）。高对比度主题不套品牌色。系统强调色（按钮、开关的填充）目前仍是 Windows 的设置，换成品牌色需要
  一组深浅色阶，AnixOps Design 里还没有，补上之后再接。
- 每个可操作控件有 `AutomationId`（侧栏是 `Nav.<页面 id>`，页面标题是 `Page.Title`）。`AnixOps.Net.App.UiTests` 启动
  构建出的程序，经 UI Automation 逐页切换、核对名称，并检查能接收键盘焦点的控件都有名称。
- 截图由同一批测试按 页面 × 语言 × 深浅色 生成，作为 CI 产物上传。入库做基线比对要等页面有了真实内容。
- 启动参数 `--language zh|en`、`--theme light|dark`、`--page <id>` 只为测试和截图固定环境，不认识的参数忽略。

## 迁移清单与 PR 顺序

原生版在 `apps/windows-native` 和 `apps/linux-native` 平行开发，以“预览”名称并存安装。`platform-parity.md` 里该平台
每一格都对齐、有截图基线和测试之后，一个 PR 切换并删除该平台的 Tauri 版。哪个平台先对齐就先切哪个。

| PR | 内容 | 前置 |
|---|---|---|
| 0 | 本方案 | — |
| 1a | `crates/client-core`：`backend` 和 `client` 从 `apps/linux-gui` 挪成独立 crate，行为不变 | 安卓线的 `Client` 模块合并 |
| 1b | Windows 后端并入 `client-core`，Tauri Windows 行为不变 | 1a |
| 2 | `client-ipc`、Windows 服务、Linux 守护进程，MSI / deb / rpm 安装与卸载 | 1 |
| 3 | `client-view`、文案单一来源 | 1 |
| 4a | `apps/windows-native/src/AnixOps.Net.Ipc`：界面一侧的通道（C#），帧、四种消息、握手、双向请求；测试读 `crates/client-ipc/fixtures/` 的同一批样例，CI 任务“Windows native app” | 2 |
| 4b | Windows 验证：WinUI 3 工程骨架（`AnixOps.Net.App`）、自绘标题栏、侧栏导航、Mica、设计 tokens 生成 XAML 资源字典、CI 里启动真实窗口做 UIA 检查并截图 | 4a |
| 4c | Windows 验证：请求列表原型（虚拟化表格、筛选），量“过关标准”第 1 项 | 4b |
| 4d | Windows 验证：界面接上服务（`session.*` 处理方、和 `anixops-net-service --console` 的端到端测试、MSI），量“过关标准”第 2、3 项 | 4c |
| 5 | Linux 骨架：导航、主题、CI 截图与 AT-SPI 检查 | 2 |
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
| 已下载的公共规则列表（国内域名、国内 IP、GFW 列表） | 转成新格式；旧版没下载过的不凭空生成 |
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
