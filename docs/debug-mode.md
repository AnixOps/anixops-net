# 调试模式（Apple 客户端）

AnixOps Net 是一个 App 两种模式（计划 A3/A4）：**代理**按订阅规则分流；**调试网络**把抓包、HTTPS 解密、改写和重放放在首位，代理只是底座。
本页记录 iOS / macOS（SwiftUI）的实现；Windows、Linux、Android（Tauri）的实现见文末「Tauri 客户端」。

## 两种模式

| | 代理 | 调试 |
|---|---|---|
| iOS 标签 | 首页 · 代理 · 规则 · 扩展 · 设置 | 请求 · 改写 · 出口 · 设置 |
| macOS 侧栏 | 概览 · 代理 · 规则 · 扩展 · 活动 · 设置 | 请求 · 改写 · 出口 · 设置 |
| 打开时 | 概览 | 请求（调试模式没有概览） |
| 订阅、节点与规则 | 自己的标签 / 侧栏项 | 出口 |
| 导入节点 | 首页右上角 / 侧栏工具栏 | 出口 › 订阅与节点 |
| 连接开关 | 首页主卡 / 侧栏底部 | iOS：每个标签底部的连接条（标签栏上方，iPad 在详情栏底部）；macOS：侧栏底部 |

调试模式在五个平台上一样（2026-11-14 用户决定）：请求 · 改写 · 出口 · 设置，打开就是请求。“改写”就是代理模式的“扩展”（插件、Map Local / Remote、解密证书），只是名字不同；“出口”放直连 / 经过节点的选择，以及订阅、节点、规则和 DNS。

- 模式存在 App 的 defaults，键 `app-mode`（`proxy` / `debug`，`NetworkCoreBridge/AppMode.swift`）。
- 首次启动（同意用户协议之后）让用户选，默认选中“调试网络”。已经导入过节点或关过旧欢迎页的老用户直接按代理模式，不再问。
- 从 `anixops://` 链接（通常是订阅链接）进来的新用户跳过选择，直接用代理模式。
- 设置最上面的“使用方式”可以随时切换；切换不删除订阅、节点和插件，连接中会自动重连。

## 出口（计划 G2）

调试模式下抓到的流量从“出口”发出，键 `debug-exit`：

- **直连**（默认）：隧道用不含节点的直连配置（内核 `"type": "direct"` 出站 + `direct` 模式，`TunnelProfile.direct`）。不需要任何节点就能抓包、解密和改写。
- **经过节点**：按代理模式的方式用选中的节点和订阅规则，可以切换节点对比不同地区的出口（G4，只用用户自己的节点）。
  还没有节点时暂时直连。

在调试模式下导入订阅或节点会把出口切到“经过节点”。直连时选节点、更新订阅都只改订阅组里的选择，隧道配置保持直连。

## 单条重放与编辑（计划 E1，免费）

请求详情里的“重放”按抓到的样子立即重发，“编辑后重放”先改方法、URL、请求头和请求体再发。

- App 用 URLSession 发出（`NetworkCoreBridge/RequestReplay.swift`），连接中会经过隧道，自然出现在抓包里。
- 不用 Cookie 存储和缓存，不跟随重定向，显示服务器自己的响应（状态、耗时、响应头、响应体）。
- 去掉 HTTP/2 伪头（`:authority` 等）和 Host、Content-Length 等由系统填写的头；请求体最大 1 MB。
- 抓到的请求体被截断时不能直接重放，只能编辑后重放。

## Map Local / Map Remote（计划 E3）

扩展 › Map Local / Map Remote 管理映射；请求详情里的“映射到本地 / 映射到远程”用抓到的请求预填一条。

- 存在 App Support 的 `debug-mappings.json`（`NetworkCoreBridge/DebugMappings.swift`），最多 200 条，本地响应体最大 256 KiB。
- 不改内核：启用的映射生成一个名为“AnixOps 调试映射”的插件，排在导入的插件后面随隧道配置下发，走现有的 MITM 改写引擎。
  - Map Local → `[Map Local]` 行：`<正则> data-type=base64 data="…" status-code=N header="名称:值|…"`。
  - Map Remote → `[Rewrite]` 行：`<正则> 307 <目标>$1`，方法和请求体由 307 保留。
  - 匹配到的主机加进 `[MITM] hostname = %APPEND% …`，需要信任根证书。
- 匹配方式：此 URL（查询参数任意）、以此开头（剩余部分接到目标后）、正则（目标原样使用，可写 `$1`）。
- 从请求生成 Map Local 时去掉 Content-Length、Content-Encoding 等逐跳 / 长度头；响应体被截断或仍是压缩内容时留空待填写。
- 保存后连接中的隧道自动重连。

## 解密域名与首次使用清单（2026-11-14 用户决定）

HTTPS 解密按域名逐个开启，不默认解密全部流量。

- 解密的域名来自三处：改写 › 解密域名 里的列表、启用的插件的 `[MITM]`、启用的映射匹配的主机。
- 列表和映射存在同一个 `debug-mappings.json` 的 `hosts` 字段（为空时不写），最多 500 个；写 `*.example.com` 包含所有子域名。
  生成的“AnixOps 调试映射”插件把它们和映射的主机一起写进 `[MITM] hostname = %APPEND% …`，所以只有列表、没有映射时也会生成这个插件。
- 域名统一转成小写，去掉协议、路径和端口；IP 地址不能加入（Rust `mappings::normalize_host`、Swift `DebugMappings.normalizeHost` 规则相同）。
- 加密连接的详情里有“解密此域名”，一键把连接的域名加进列表；已在列表里或目标是 IP 时不显示，说明文字会写明原因。
- 调试模式的请求页在还抓不到 HTTPS 请求时显示检查清单：连接 → 信任根证书 → （Tauri）开启 HTTPS 解密 → 添加要解密的域名 → 开始抓包，每步写明去哪里做，全部完成后消失。
  Apple 端的映射插件不受插件总开关影响，所以没有“开启 HTTPS 解密”这一步；Android 上 App 读不到证书是否被信任，不列这一步。

## 截图

CI 的 macOS 截图（`mac-screenshots-<sha>`）额外包含调试模式的 `debug-activity-light` / `debug-activity-dark`（请求表格和检查器）、`debug-exit-light`、`debug-settings-light`，以及首次启动的 `choose-home-light`。
演示数据用 `-AnixOpsMode debug|proxy|choose` 选择模式；调试模式的演示请求由 `TunnelController.demoControl` 应答。

## 请求列表（计划 E5）

调试模式的“请求”按 HTTP 请求逐条列出，最新在上。右上角可以切到全部连接。

- **macOS 和宽屏 iPad**（仿 Proxyman）：可排序的表格（时间、方法、状态、主机、路径、类型、大小、耗时）加右侧常驻检查器，选中一行就在检查器里看请求和响应，可以重放、生成映射、导出；工具栏按钮可以收起检查器。方向键切换行，右键可以拷贝网址。状态码按颜色区分：2xx 绿、3xx 黄、4xx/5xx 和没有响应红、等待中灰。macOS 的隧道诊断（复制诊断信息、扩展日志、导出日志）放在工具栏的“诊断”菜单里。
- **iPhone 和窄屏 iPad**：列表，点开一条进入请求详情。
- **请求详情 / 检查器**（`ExchangeInspector.swift`，三个平台同一个视图）：顶行是方法、状态和网址，下面四个标签：概览（基本信息、重放、导出、映射）、请求、响应、时间线，选中的标签会记住。请求和响应各自在“头 / 查询 / 正文”之间切换。正文按内容显示：JSON 是可折叠的树（点一行选中并折叠；接键盘时方向键移动和折叠，Return 或空格折叠，Mac 上 ⌘C 拷贝选中的值；下方显示 JSONPath，点一下拷贝）或格式化文本，图片直接预览，表单是名值对，其余是原文，任何内容都能切到十六进制。JSON 用自己的解析器（`JsonDocument.swift`），数字按服务器发来的原样显示，64 位 id 不会经过浮点数取整，键的顺序和重复的键也保留。时间线把同一连接上的请求画在一条时间轴上；内核只记录每个请求的开始和结束，没有 DNS、建连、TLS 的分段。
- Mac 的检查器在窗口里不设导航标题（否则窗口标题会从“请求”变成“GET 200”）；有检查器时快捷筛选放在表格这一栏、请求计数的上方。

- 数据来自内核 `capture_requests`（networkcore d1a8cc1 起），每秒按 `since_rev` 增量读取（networkcore d9ff54f 起）：请求头读到就出现一行“进行中”，响应到来、结束时在原位更新；行按首次出现排序，以连接 id 加序号标识。`dropped_before_seq` 以内已结束的行随内核一起删掉；本地最多保留 5000 行（`NetworkCoreBridge/CaptureRequests.swift`）。旧内核没有 `last_rev` 时退回按 `since_seq` 只列已结束的请求。
- 行上的标签：`SSE n` / `WS n`（事件流或 WebSocket 及已收到的消息数）、“改写未生效”（改写规则因为流式响应或正文超过 1 MiB 没有执行）、“进行中”。
- 请求详情：进行中的请求每秒重读（`capture_exchange`）；“事件” / “WebSocket 消息”逐条列出方向、事件名或帧类型、大小和内容开头，点开看全文，进行中时每秒追加（`since_message`）。WebSocket 只读，消息原样转发；permessage-deflate 由内核解压，解不开的标出来。每个请求保留最近 512 条、每条前 4 KiB，更早的显示为“未保留”。
- HTTP/2 连接有“HTTP/2 帧”：App ↔ AnixOps 与 AnixOps ↔ 服务器两段的帧头（类型、流、标志、长度、错误码、窗口增量），可按链路筛选，或只看 RST_STREAM、GOAWAY 和错误（`capture_h2_frames`）。
- 正文解压在 App 进程里调内核 `networkcore_ios_decode_body`（gzip、deflate、br、zstd，可叠加，最多 16 MiB），与桌面和导出一致；内核解不开时退回系统 Compression 框架。
- 快捷筛选（出错、JSON、图片、非 GET、HTTP/2、大于 100 KB、本地应答）和搜索框拼成同一个过滤表达式交给内核解析，语法见 project files `debug-client/kernel-capture-api.md` §4。快捷筛选的条件会加到搜索框每个 `OR` 分组里。
- 表达式有误时内核返回出错位置，列表上方标红提示。
- 只列 MITM 解密后的请求和明文 HTTP 请求；抓包未开启时空白页给出“开始抓包”。

## 导出单条请求与加密会话（计划 F1 / F5 / E11）

- 请求详情的“导出这条请求”：cURL、代码（Python requests / JavaScript fetch / Swift URLSession / Kotlin OkHttp / Go net/http）、Postman、Insomnia、HAR，经内核 `export_capture` 的 `id` + `exchange` 生成。
- “遮盖 Cookie、令牌和密码”默认打开（内核 `redact`）；整份 pcapng / HAR 导出也按内核默认脱敏。
- 请求列表“更多”里可以“保存当前抓包”和查看“已保存的会话”。
  - 隧道扩展在引擎启动后自己调用 `set_capture_storage`：目录是扩展自己的 Application Support/AnixOps-Captures，密钥是钥匙串里仅本机可用的 32 字节随机数（`CaptureSessionStorage`），不经过 App。
  - 会话用 ChaCha20-Poly1305 加密，保留 7 天。打开会话会替换当前抓包并停止实时抓包，之后照常筛选、查看和导出。

## Tauri 客户端（Windows / Linux / Android）

同一套行为，共用 `apps/windows-gui/ui` 的界面和 `crates/desktop-mitm`。

- **模式**：存储键为 `anixops.app-mode`（`proxy` / `debug`）和 `anixops.debug-exit`（`direct` / `node`），保存在 WebView 的 localStorage（`ui/src/mode.ts`）。首次选择、老用户直接用代理模式、导入节点后自动切到“经过节点”，都与 Apple 端一致。
- **侧栏**：调试网络下是“请求 · 改写”，下方是“出口 · 设置”，打开就在请求页，没有概览。“改写”就是扩展页（插件、Map Local / Remote、解密证书）；订阅、节点和分流规则（含 DNS）都在“出口”里。Android 窄屏下侧栏变成底栏。
- **直连出口**：后端命令 `set_direct_egress` 让引擎使用不含节点的 `{"outbound":{"type":"direct"}}` 配置。Linux、Android 和 Windows 都支持。
- **请求页**（`pages/RequestsPage.tsx`，仿 Proxyman）：左边是请求表格，右边常驻检查器（选中即看请求和响应，可重放、映射、导出）。窗口窄于 1100 px 时两者轮流显示，Android 上点一行打开详情。
- **进行中的请求与流**：请求表按 `since_rev` 增量读取（`capture-filter.ts` 的 `mergeRequests` / `nextRev`），请求头读到就出现，响应到来时原位更新；行以 `连接 id:序号`（`requestKey`）标识。类型列附 `SSE n` / `WS n` 和“改写未生效”。检查器在进行中时每两秒重读；有事件或 WebSocket 消息时多一个“事件” / “WebSocket”标签（`capture_exchange` + `since_message`，进行中每秒追加，只读）；HTTP/2 连接多一个“HTTP/2 帧”标签（`capture_h2_frames`，可按链路、只看 RST_STREAM / GOAWAY / 错误筛选）。概览里写明改写未生效的原因。
- **正文解压**：后端命令 `decode_body`（`desktop_mitm::capture::decode_body`，调内核 `capture::decode::decode_body`）解 gzip、deflate、br、zstd，可叠加，最多 16 MiB，只抓到开头的正文也解出能解的部分；内核解不开时退回 WebView 的 DecompressionStream（只限完整的 gzip / deflate）。
- **检查器**（`pages/Inspector.tsx`、`pages/BodyViewer.tsx`）：顶行是方法、状态和网址，下面四个标签：概览、请求、响应、时间线。请求和响应各自在“头 / 查询 / 正文”之间切换，选择会记住。正文按内容显示：JSON 是可折叠的树（方向键移动和折叠，Ctrl+C 拷贝选中的值，下方显示并可拷贝 JSONPath）或带颜色的格式化文本，图片直接预览，表单是名值对，其余是原文，任何内容都能切到十六进制。JSON 用自己的解析器（`json-view.ts`），数字按服务器发来的原样显示，不会像 `JSON.parse` 那样把 64 位 id 取整。时间线把同一连接上的请求画在一条时间轴上；内核目前只记录每个请求的开始和结束，没有 DNS、建连、TLS 的分段。
- **停靠与分栏**：检查器可以停靠在表格右侧或下方（检查器“更多”菜单），中间的分隔条可以拖动、用方向键调整、双击恢复默认，位置和比例记在本机（`split-pane.ts`）。右侧停靠需要窗口宽 1100 px 以上，下方停靠需要宽 720 px、高 600 px 以上，否则表格和检查器轮流显示。
  - 页面工具栏只放整份抓包的操作：视图切换（请求 / 连接 / 已保存）、抓包开关、暂停刷新、“更多”里的导出全部 pcapng / HAR 和清空抓包（红色，要确认）。
  - 表格（`pages/RequestList.tsx`）列出时间、方法、状态、主机、路径、类型、大小、耗时，点列头排序（`ui/src/request-table.ts`），默认最新在上。状态码按颜色区分：2xx 绿、3xx 黄、4xx/5xx 和没有响应红、等待中灰。只画可见的行，最多保留 2000 条。
  - 键盘：表格获得焦点后用 ↑ ↓、j / k、PageUp / PageDown、Home / End 切换，Enter 打开；Ctrl+F 跳到筛选框，Ctrl+R 重放选中的请求。
  - 用 `capture_requests` 每秒增量读取，筛选框写的就是内核的过滤表达式。快速筛选（出错、JSON、图片、HTTP/2、大于 100 KB）会拼进表达式（`ui/src/capture-filter.ts`）。
- **导出**：连接可以导出 pcapng / HAR，单条请求可以导出 cURL、五种语言的代码、Postman、Insomnia 或 HAR，都保存到“下载”文件夹。“遮盖 Cookie、令牌和密码”默认打开。
- **重放**（`desktop_mitm::replay`）：
  - 桌面经引擎的本机代理发出，Android 经内核的本机重放代理发出（`start_replay_proxy`，只监听 127.0.0.1，一次性 Basic 凭据，凭据只留在后端）。
  - 不跟随重定向，不保存 Cookie，响应最多保留 1 MiB；开着 HTTPS 解密时会信任解密根证书。
- **保存的会话**（`desktop_mitm::sessions`，调试模式的“请求”页）：把当前抓包整体加密保存成会话，之后可以载入回看请求、导出，或删除。
  - 加密由内核完成（`set_capture_storage` / `save_capture` / `capture_sessions` / `load_capture` / `delete_capture`，ChaCha20-Poly1305）。
    后端在每个会话操作前把目录和密钥告诉内核；窗口拿不到密钥，`runtime_control` 也拒绝窗口发 `set_capture_storage`。
  - 密钥：Windows 用 DPAPI 封装后存在 `%LOCALAPPDATA%\AnixOps Net\capture`；Android 用 Keystore 里的 AES-GCM 密钥封装，存在 no-backup 目录；
    Linux 存进 Secret Service（`secret-tool`），没有钥匙环时存成只有本人可读（0600）的文件，界面会写明。
    已有的密钥读不出来（例如钥匙环没解锁）时报错，不会换新密钥，免得已保存的会话全部打不开。
  - 7 天后删除：内核在设置存储、保存和列出时清理；客户端启动时也按文件名里的时间清理一次，不连接也会过期。
  - 载入会替换当前抓到的内容并停止实时抓包；当前有内容时“载入”要再点一次确认。会话要在连接时才能列出和载入（内核在运行时才应答）。
- **Map Local / Map Remote**（`desktop_mitm::mappings`）：存放在插件目录的 `debug-mappings.json`，格式与 Apple 端相同。`plugins::apply` 把生成的“AnixOps 调试映射”接在导入的插件后面。映射要开启 HTTPS 解密才生效，保存后正在运行的引擎会重新加载。
