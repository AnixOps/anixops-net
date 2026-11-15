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
3. **契约有类型。** 每个命令的请求和应答样例放在 `client-core/tests/fixtures/*.json`。C# 和 Kotlin 的数据类型必须
   无损解析同一批样例；命令改字段时 Rust、Kotlin、C# 三边测试一起失败。GTK 界面直接用 Rust 类型。
4. **`crates/client-view`。** 现在 TypeScript 里有行为的纯逻辑（抓包筛选表达式、请求表排序、节点筛选、`anixops://`
   解析）在 Rust 里写一次，作为命令暴露，各端调用，样例测试只保留一份。字节、时长、日期这类显示格式由各端用平台的
   区域设置格式化。
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

后台进程以系统身份运行，碰不到“当前用户”的设置；有些事必须留在界面所在的用户会话里做。界面通过 `Platform` 回调承担
这些事，核心通过事件告诉它什么时候做。在同进程形态（Tauri Linux、安卓、便携版）里，同一组回调的默认实现就是现在
`backend` 里的模块，行为不变。

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
| 4 | Windows 验证：工程骨架、导航、主题 tokens、请求列表原型、CI 截图与 UIA 检查 | 2 |
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
