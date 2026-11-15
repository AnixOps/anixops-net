# 安卓原生界面（Jetpack Compose）方案

状态：方案已确认（2026-11-14，取舍见文末）。核对基准：main `439270c`。

2026-11-14 用户决定：安卓界面改用原生 Kotlin + Jetpack Compose + Material 3，套 AnixOps 品牌色；内核与后端沿用现有 Rust
和 VpnService；排在桌面端界面做精之后大规模动手。本页是设计与架构，页面实现按文末的 PR 顺序推进。

## 现状

```text
Tauri WebView（apps/windows-gui/ui，React，与 Windows / Linux 共用）
  └─ Tauri 命令（apps/linux-gui/src/app.rs，76 个，命令名与 bridge.ts 一致）
       └─ Backend（crates/client-core/src/backend，不依赖 Tauri）── engine_embedded::EmbeddedRuntime
  └─ vpn-plugin（Kotlin VpnService，Tauri 插件）── TUN 描述符 ──▶ engine::attach_tunnel
```

- `Backend` 已经不依赖 Tauri（不开 `gui` feature 也能编译和测试）。依赖 Tauri 的是 `app.rs`：命令包装、开关 VPN
  （`TunnelHost`）、后台任务（状态监视、订阅自动更新、Wi-Fi 规则、`anixops://` 链接、通知）。
- 界面里约 1000 行纯逻辑在 TypeScript：抓包筛选表达式、请求表格、节点筛选、标签与格式化、模式、`anixops://` 解析、
  节点表单校验、概览数据、流量统计格式化。
- 安卓特有能力都在 `vpn-plugin`：VPN 授权与建立接口、前台通知、保存到“下载”、Keystore 里的抓包会话密钥、当前网络与 Wi-Fi 名。

## 目标结构

```text
apps/android-native/                 Gradle 工程（提交进仓库，不再 `tauri android init` 生成）
  app/                               Compose 界面、导航、ViewModel；platform/ 下是 VpnService、前台通知、
                                     Rust 回调的安卓能力（PlatformBridge）、Keystore 会话密钥，以后加快捷设置磁贴、
                                     始终开启的 VPN
  core/                              Kotlin 侧的内核桥：NativeCore（JNI）、JSON 信封、命令与数据类型（无界面，JVM 测试）
  rust/                              crate `anixops-net-android`（库名 `anixops_net`，cdylib + rlib）：JNI 入口，调 `Client`
crates/client-core/src/client.rs         从 app.rs 抽出的、不依赖 Tauri 的客户端服务层（`client_core::client`）
```

### 1. 抽出客户端服务层 `client`（Rust，PR 1）

把 `app.rs` 里不属于 Tauri 的部分搬进 `crates/client-core/src/client.rs`（不开 `gui` feature 也编译，根 workspace 的
测试覆盖它）。PR 1 先放在 `apps/linux-gui` 里；之后为了让 Windows 后端和原生桌面界面共用，`backend` 和 `client`
原样挪成了独立的 `crates/client-core`（见 [`desktop-native.md`](desktop-native.md)），`apps/linux-gui` 重新导出它们。

- `Client`：持有 `Backend`、健康状态、抓包会话密钥缓存和待读的 `anixops://` 链接；
  `invoke(command, args: serde_json::Value) -> Result<Value, String>` 按命令名分发，命令名、参数和返回 JSON 与
  `bridge.ts` 完全一致。分发表是一个 `fn handler(name) -> Option<Handler>`；测试读 `bridge.ts`，界面调用的每个命令
  都必须有处理函数或在 Windows 专属清单里，漏掉就失败。
- `Platform` trait：`has_system_vpn`、`open_tunnel`、`close_tunnel`、`save_to_downloads`、`save_base64_to_downloads`、
  `capture_session_key`、`current_network`、`post_notice`，不带 `AppHandle`。
- 后台任务（网络变化、订阅自动更新、Wi-Fi 规则、流量统计与通知）由 `Client::open` / `Client::start_background` 启动；
  通知走 `Platform::post_notice`。
- `apps/linux-gui/src/app.rs` 只剩 Tauri 的部分：一个 `invoke_handler` 把每个命令转给 `Client::invoke`（在阻塞线程上），
  `TauriPlatform` 把 `Platform` 接到 `TunnelHost` 和通知插件，`anixops://` 链接和窗口。行为不变；现有 Tauri 安卓
  （`apps/android`）不用改。
- 原生安卓的事件（连接状态变化、链接、通知）在 PR 2 加：JNI 层实现 `Platform::post_notice`，状态变化由前台服务通知。

好处：命令契约只有一份；Tauri 桌面端、现有 Tauri 安卓和新的 Compose 安卓在迁移期间共用同一套后端和测试。

### 2. JNI 桥（已定，见文末第 1 项）

Rust 侧 `apps/android-native/rust`（`jni` crate，MIT/Apache）只导出四个函数：

| JNI 函数 | 作用 |
|---|---|
| `nativeInit(dataDir, platform)` | 建 `Client`，`platform` 是 Kotlin 对象，Rust 通过它回调 VPN、下载、Keystore、网络、通知 |
| `nativeInvoke(command, argsJson): String` | 同步调用，返回 `{"ok": ...}` 或 `{"error": "..."}`；Kotlin 在 `Dispatchers.IO` 上调用 |
| `nativeSetEventSink(sink)` | Rust 后台任务的事件回调 |
| `nativeAttachTunnel(fd)` / `nativeDetachTunnel()` | VpnService 建好接口后交出描述符 |

Kotlin 侧 `core` 模块：`Core.invoke<T>(Command)` 用 kotlinx.serialization 把数据类编码成与 `bridge.ts` 相同的 JSON；
事件转成 `SharedFlow<CoreEvent>`。每个命令一个类型化的 suspend 函数，页面不直接拼 JSON。

契约测试：`crates/client-core/tests/fixtures/*.json` 存每个命令的请求和应答样例；Rust 测试检查后端产出与样例一致，
Kotlin 单元测试检查数据类能无损解析同一批样例，命令新增或改字段时两边一起失败。

### 3. 进程与生命周期

- 一个进程。内核随 `AnxVpnService`（前台服务）存活，界面（单 Activity）关掉或被系统回收不影响连接；界面回来时
  `Core` 已在运行，直接读快照。
- 支持系统设置里的“始终开启的 VPN”：系统直接启动 `AnxVpnService`，它自己初始化 `Core` 并按上次的节点连接。
  这能部分解除“开机自动连接 ⛔”的限制（用户在系统设置里打开一次即可），实现后更新 `platform-parity.md`。
- 快照：界面在前台时每秒 `runtime_snapshot`（与桌面 `live.ts` 相同节奏），后台停止轮询；连接状态变化另由事件推送，
  前台通知和快捷设置磁贴用事件更新，不轮询。

### 4. 界面逻辑从 TypeScript 移到哪里

原生界面会有 Kotlin（安卓）、C#（Windows）、Rust（Linux）几套（见线程「规划 Windows 和 Linux 原生客户端」），所以有行为的
界面逻辑在 Rust 里写一次，放进共用的 `client-view`，作为 `Client::invoke` 的命令暴露，各端调用，共享样例只测一份：

| 逻辑 | 去处 |
|---|---|
| 抓包筛选表达式、请求表格排序与筛选、节点筛选 | `client-view`（Rust 命令） |
| `anixops://` 链接解析 | `client-view`（Rust 命令） |
| 节点表单校验 | 已在 Rust（`node-edits`） |
| 字节、速率、时长、日期的显示格式 | 各端用平台的区域设置格式化（安卓 `Formatter` / `DateFormat`） |

JNI 一次调用是微秒级，筛选在 Rust 里做不比 Kotlin 慢。`client-view` 由先需要它的线程建，建好后安卓直接调用。

### 5. 导航与页面

与其他平台一致（`docs/debug-mode.md`），手机用底部导航栏，平板和折叠屏展开时用导航栏侧轨（Navigation Rail）：

| 模式 | 手机（底栏） | 打开时 |
|---|---|---|
| 代理 | 首页 · 代理 · 规则 · 扩展 · 设置（与 iOS 相同；“活动”放在首页的连接与流量卡片里） | 首页 |
| 调试网络 | 请求 · 改写 · 出口 · 设置 | 请求 |

- 请求列表与详情、订阅与节点：手机上点开进入详情页；宽度 ≥ 840 dp 时用 Material 3 adaptive 的列表 + 详情双栏，
  对应桌面的常驻检查器。
- 首次启动：用户协议 → 选择使用方式（代理 / 调试网络），与 Tauri 和 Apple 相同。
- 按钮层级按 `docs/ui-ux.md`：E1 = `Button`（filled，每屏最多一个，首页是连接开关），E2 = `OutlinedButton` /
  `FilledTonalButton`，E3 = `TextButton` / `IconButton`，E4 = 溢出菜单里的红色项 + 写出对象名的确认对话框。
  即时开关用 `Switch`，二到四个互斥模式用 `SingleChoiceSegmentedButtonRow`。页面级操作放顶栏，行级操作放行尾。

### 6. 主题与品牌色

- 颜色、字号、圆角、动效时长都从同一份设计 tokens 生成：扩展 `.github/scripts/design_tokens.py`，在生成 `tokens.css`
  的同时生成 `apps/android-native/app/src/main/java/.../theme/Tokens.kt`，CI 检查生成物与源一致（和 CSS 相同）。
- Material 3 的 `ColorScheme` 由品牌 tokens 映射：`primary` = 品牌主色，`surface*` 用 tokens 的背景层级，危险色用
  tokens 的 `danger`；浅色 / 深色跟随系统，不用 Material You 动态取色（已定，见文末第 3 项）。
- 字体用系统字体（与桌面的决定一致），等宽字体（请求详情、日志）用系统 `monospace`。

### 7. 文案与语言

简体（默认）、繁体、英文三种语言，文案用单一来源生成各端格式（安卓 `strings.xml`，Windows `.resw`，Linux gettext，
Tauri 的 TS 表），与原生 Windows / Linux 方案共用。现在 TS 表以简体原文为 key，迁移时换成稳定 id。
安卓侧 Android Lint 的 `MissingTranslation` 设为错误。

### 8. 构建、测试与截图基线

CI 新 job `android-native`（GitHub 托管 runner，变更范围检测同现有 `android`）：

1. `cargo ndk -t arm64-v8a` 构建 `libanixops_net.so`（模拟器冒烟测试加 x86_64，只用于测试，不进发布包）；沿用
   rquickjs 的 bindgen 设置。
2. `gradle lint testDebugUnitTest`（不提交 Gradle wrapper，CI 用 setup-gradle 固定版本）：Kotlin 单元测试、契约样例
   测试（Rust 与 Kotlin 读同一份 `crates/client-core/tests/fixtures`）、Android Lint（警告即失败）。
3. 截图基线：Roborazzi（JVM 上用 Robolectric 原生绘制，不需要模拟器）对每个页面拍 浅色 / 深色 × 简体 / 英文 ×
   手机（411×891 dp）/ 平板（1280×800 dp），对比提交在仓库里的基线图；有差异则失败并上传差异图。基线只能在 PR 里
   随界面改动一起更新，PR 描述附前后对照。
4. 模拟器冒烟测试（x86_64，API 34，PR 2 之后单独一个 PR，随首页迁移一起）：启动 → 同意协议 → 调试模式直连出口 → 连接（VPN 授权用测试专用的
   `prepare` 预授权）→ 本机 HTTP 请求出现在请求列表 → 断开。
5. 组装 arm64 release APK，用与 Tauri 版共用的 `.github/actions/sign-apk` 签名（见下节）。

### 9. 包名与签名延续

- applicationId 保持 `io.anixops.net.android`，签名沿用 `ANDROID_KEYSTORE_BASE64` / `ANDROID_KEYSTORE_PASSWORD` /
  `ANDROID_KEY_ALIAS` / `ANDROID_KEY_PASSWORD` 四个 secrets 和同一段 zipalign + apksigner。这样切换后从 Tauri 版直接
  覆盖升级，数据目录（`files/anixops-net`）不变，订阅、节点、插件、设置原样保留。
- 现在还没有发布密钥，CI 每次用临时调试密钥签名，任意两个 APK 之间本来就不能覆盖安装；用户给出密钥后才有升级链路。
- 迁移期间（已定，见文末第 2 项）原生版用 `applicationIdSuffix = ".next"` 和名称“AnixOps Net（预览）”，可以与现有版本
  并存安装测试；切换的那个 PR 去掉后缀。

## 功能迁移清单

逐项对应 `docs/platform-parity.md` 的 Android 列。每个 PR 合并后在本表打勾；全部打勾才做切换。

| 组 | 内容 | PR |
|---|---|---|
| 基础 | 抽出 `client` 服务层；JNI 桥；VpnService 移植；主题 tokens；导航骨架；CI、截图基线、冒烟测试 | 1、2 ✓（#152、#160） |
| 代理 › 首页 | 连接开关、状态、实时流量、连接列表与断开、出口地址、代理模式切换 | 3 ✓（#190） |
| 代理 › 代理 | 订阅导入 / 更新 / 自动更新 / 重命名 / 删除，扫码导入（CameraX + ZXing core，Apache-2.0；不用 ML Kit，它依赖 Google 服务条款），节点测速、最快、策略组、分享链接、添加 / 编辑 / 删除节点 | 3 ✓（#190） |
| 规则 | 分流模式、我的规则文本编辑、规则集查看与更新、DNS 预设、自定义 DNS / 分域 / Hosts、Fake-IP、屏蔽 QUIC | 4a ✓ |
| 设置 | 使用方式、通知开关、Wi-Fi 规则（位置权限）、诊断报告、崩溃报告、用户协议与首次同意、语言（系统的应用语言页） | 4b ✓ |
| 设置 › 活动 | 日志查看、流量统计（`view.logs`、`view.traffic_stats`） | 4c ✓ |
| 代理 › 订阅行 | 订阅行改读核心的 `subscriptions`（`view.subscriptions`）：上次更新失败的原因、已到期；粘贴的链接用应用自己的语言命名（节点分组按 `source` / `pasted`） | 3b ✓ |
| 扩展 / 改写 | 插件从网址 / 文件 / 剪贴板导入、开关、删除、查看内容；HTTPS 解密开关与首次确认；根证书保存到“下载”与安装指引；解密域名 | 5a ✓ |
| 扩展 / 改写 › 映射 | Map Local / Remote（新建、编辑、开关、删除）、流量重定向 | 5b ✓ |
| 调试 › 请求 | 请求列表、快捷筛选、表达式搜索、详情（请求 / 响应 / 正文）、单条导出（cURL、代码、Postman、Insomnia、HAR，默认脱敏）、重放与编辑后重放、连接视图与 pcapng / HAR 导出、会话加密保存 / 载入 / 删除、检查清单 | 6b ✓ 列表、筛选、详情；6c ✓ 单条导出与重放；6d 其余 |
| 调试 › 出口 | 直连 / 经过节点，订阅与节点、规则与 DNS 入口 | 6a ✓ |
| 系统集成 | `anixops://` 链接、系统通知、快捷设置磁贴、始终开启的 VPN、本机指标端口 | 7 |
| 切换 | 发布流程改出原生 APK；删除 `apps/android`（Tauri）与 `vpn-plugin`；更新 README、`platform-parity.md`、下载页 | 8 |

切换前原生版新增的能力（磁贴、始终开启的 VPN）不阻塞切换，但必须在切换 PR 之前全部有截图基线和测试。

## 排期

- 现在：本方案；确认后可以先做 PR 1（抽出 `client` 服务层，不改界面，Linux 与现有安卓行为不变）和 PR 2（工程骨架与 CI），
  它们不碰桌面界面，与桌面做精的线程不冲突。
- 桌面端做精完成后：PR 3–8 页面迁移和切换。页面的视觉细节（间距、动效、空状态）参照桌面做精线程定下的规则。

## 已定的取舍

1. **界面与 Rust 后端的接口**：已定（用户 2026-11-14）：JSON 命令桥，沿用 `bridge.ts` 的命令名，一份契约三端共用，
   加共享样例测试。
2. **迁移方式**：已定（用户 2026-11-14）：原生版在 `apps/android-native` 平行开发、以“预览”包名并存安装，全部打勾后一个
   PR 切换并删除 Tauri 安卓。
3. **颜色**：已定（用户 2026-11-14）：固定品牌色，深浅色跟随系统，不用 Material You 动态取色。
