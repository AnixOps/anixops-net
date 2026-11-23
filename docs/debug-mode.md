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
- HTTP/2 可以把 Cookie 拆成多个 `cookie` 字段；重放时多条 Cookie（抓到的或在编辑框里写成多行的）合并成一行，用 `; ` 分隔
  （RFC 9113 §8.2.3），编辑框里显示的就是实际发出的内容。其他同名头照常用逗号合并。
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
  - 插件的主机和排除按内核导入的结果计算（iOS / macOS 的 `MitmPlugin.decryptedHostnames`，与引擎读到的一致）：没有段头的
    Quantumult X 片段（`hostname = …`）、被注释的段头（`#[mitm]`）、Surge `[General]` 的 `force-http-engine-hosts` 都算在内，
    引擎因写法读不懂而丢掉的 hostname 行不算。插件列表显示“N 个主机 · M 行生效（K 行不支持）· S 个脚本”；还没被内核检查过的
    旧插件按段名统计，打开插件页、解密域名页或请求页时会补做一次检查。
- 列表和映射存在同一个 `debug-mappings.json`，最多 500 行。列表（2026-11-18 起）是 `domains`：每行 `pattern`、`exclude`（排除）、`enabled`（停用的行保留但不下发）；同时仍写 `hosts`（启用的解密行），旧版本只读它，旧文件只有 `hosts` 时按全部启用读入。
  - 通配符与引擎 `[MITM] hostname` 一致：`*` 匹配任意字符（含点），`?` 匹配一个字符；`*.example.com` 匹配所有子域名，不含 `example.com` 本身。通配行必须含点和至少一个字母或数字，单独的 `*` 不能加入。
  - 排除行写成 `-pattern`，优先于列表、插件和映射里的所有解密规则（引擎合并插件时任一插件的排除都胜出）；输入框里直接写 `-example.com` 也是排除。
  - 生成的“AnixOps 调试映射”插件把解密行、映射的主机和排除行一起写进 `[MITM] hostname = %APPEND% …, -排除…`，所以只有列表（哪怕只有排除）、没有映射时也会生成这个插件。
  - 列表默认为空，不带任何预设（公开版不带预设规则）。失败标记来自本机这次连接的解密结果，不是内置名单。
- 桌面端（Tauri，`crates/desktop-mitm/src/mappings.rs`）读写同一形状，规则相同（`normalize_pattern`、`pattern_matches`）；命令 `list_decrypt_domains`、`add_decrypt_host {host, exclude}`、`edit_decrypt_host`、`set_decrypt_host_enabled`、`remove_decrypt_host`，`list_decrypt_hosts` 只回启用的解密行。“改写 › 解密域名”卡片有解密 / 排除、开关、编辑和解密失败标记（`runtime_control` 的 `mitm_hosts`）。`client-core` 有同样的命令（Android 用），`view.decrypt_hosts` 多了 `excluded` 参数和答案字段；Android 界面另行跟进。
- 解密失败标记（Apple）：隧道运行时解密域名页每 5 秒读内核 `mitm_hosts`，最后一次连接失败的域名列在“解密失败”里，写明原因：App 拒绝证书（证书类告警，多为证书固定或未信任根证书）、握手中断开（证书固定常见）、握手失败、超时、源站 TLS 失败、源站连不上；一键“排除”。“清除这些记录”发 `clear_mitm_hosts`。请求页“没有请求的连接”里已在列表的域名也显示同一原因。内核只记域名、次数、时间和原因码，最多 256 个，断开即清空。
- 域名统一转成小写，去掉协议、路径和端口；IP 地址不能加入（Rust `mappings::normalize_host`、Swift `DebugMappings.normalizeHost` 规则相同）。
- 加密连接的详情里有“解密此域名”，一键把连接的域名加进列表；已在列表里或目标是 IP 时不显示，说明文字会写明原因。
- 调试模式的请求页在还抓不到 HTTPS 请求时显示检查清单：连接 → 信任根证书 → （Tauri）开启 HTTPS 解密 → 添加要解密的域名 → 开始抓包，每步写明去哪里做，全部完成后消失。
  Apple 端的映射插件不受插件总开关影响，所以没有“开启 HTTPS 解密”这一步；Android 上 App 读不到证书是否被信任，不列这一步。
- 首次解密确认（Apple，TERMS.md 第 4 节）：开启 HTTPS 解密的每条路径共用一道确认（`TunnelController.requireDecryptionConsent()`，根视图 `RootView` / `MacRootView` 上的同一个对话框，编辑表单打开时由表单弹出）。插件总开关；解密域名的添加、编辑、启用停用的行、“改为解密”；加密连接详情和请求页的“解密此域名 / 解密”；Map Local / Map Remote 的保存和开关，只要让映射插件新增了要解密的域名（`DecryptionConsent.isRequired`），都会先问；删除、停用、只加排除行不问。取消则不保存。确认写入 `https-decryption-consent-version`，一次安装只问一次。
  - 兜底：未确认时，会解密任何域名的映射插件不写进 VPN 配置（`DecryptionConsent.profilePlugin`）；只有排除行或不带主机的映射照常写入。解密列表和映射本身仍保存在 `debug-mappings.json`。
  - 旧版本保存了解密域名或映射、但从未确认过的用户：启动后问一次（每次启动最多一次）；确认后映射插件写进配置，取消则从配置里拿掉，不解密。
    取消后列表里的域名仍在、但不解密，加密连接详情也不再显示“解密此域名”（域名已在列表里）。要恢复解密：下次启动时确认，或打开插件总开关，或添加一个新的解密域名，任一处确认后全部生效。

## 截图

CI 的 macOS 截图（`mac-screenshots-<sha>`）额外包含调试模式的 `debug-activity-light` / `debug-activity-dark`（请求表格和检查器）、`debug-exit-light`、`debug-settings-light`，以及首次启动的 `choose-home-light`。
演示数据用 `-AnixOpsMode debug|proxy|choose` 选择模式；调试模式的演示请求由 `TunnelController.demoControl` 应答。

## 请求列表（计划 E5）

调试模式的“请求”按 HTTP 请求逐条列出，最新在上。右上角可以切到全部连接。

- **macOS 和宽屏 iPad**（仿 Proxyman）：可排序的表格（时间、方法、状态、主机、路径、类型、大小、耗时）加右侧常驻检查器，选中一行就在检查器里看请求和响应，可以重放、生成映射、导出；工具栏按钮可以收起检查器。方向键切换行，右键可以拷贝网址。状态码按颜色区分：2xx 绿、3xx 黄、4xx/5xx 和没有响应红、等待中灰。macOS 的隧道诊断（复制诊断信息、扩展日志、导出日志）放在工具栏的“诊断”菜单里。
- **iPhone 和窄屏 iPad**：列表，点开一条进入请求详情。
- **请求详情 / 检查器**（`ExchangeInspector.swift`，三个平台同一个视图）：顶行是方法、状态和网址，下面四个标签：概览（基本信息、重放、导出、映射）、请求、响应、时间线，选中的标签会记住。请求和响应各自在“头 / 查询 / 正文”之间切换。正文按内容显示：JSON 是可折叠的树（点一行选中并折叠；接键盘时方向键移动和折叠，Return 或空格折叠，Mac 上 ⌘C 拷贝选中的值；下方显示 JSONPath，点一下拷贝）或格式化文本，图片直接预览，表单是名值对，其余是原文，任何内容都能切到十六进制。JSON 用自己的解析器（`JsonDocument.swift`），数字按服务器发来的原样显示，64 位 id 不会经过浮点数取整，键的顺序和重复的键也保留。正文的解压、解码、JSON 解析和格式化都在后台完成，界面先显示进度；没有完整抓到的正文不按 JSON 解析，直接显示原文。格式化、原文和十六进制分段懒加载显示，最多 20 万个字符。时间线把同一连接上的请求画在一条时间轴上；内核只记录每个请求的开始和结束，没有 DNS、建连、TLS 的分段。
- Mac 的检查器在窗口里不设导航标题（否则窗口标题会从“请求”变成“GET 200”）；有检查器时快捷筛选放在表格这一栏、请求计数的上方。

- 数据来自内核 `capture_requests`（networkcore d1a8cc1 起），每秒按 `since_rev` 增量读取（networkcore d9ff54f 起）：请求头读到就出现一行“进行中”，响应到来、结束时在原位更新；行按首次出现排序，以连接 id 加序号标识。`dropped_before_seq` 以内已结束的行随内核一起删掉；本地最多保留 5000 行（`NetworkCoreBridge/CaptureRequests.swift`）。桌面窗口（Windows、Linux）不再把 `dropped_before_seq` 当作全局截断线：内核按连接从早到晚淘汰，而 seq 按请求结束的先后分配，一条长连接（HTTP/2、keep-alive、WebSocket）先被淘汰时它可能带着最新的 seq，其他仍保留的请求 seq 更小。它变大时，窗口按 `since_rev` 从 0 分页读一遍内核仍保留的全部行（至多每 5 秒一次），只删掉内核不再列出的行，包括被淘汰连接上“进行中”的行（`capture-filter.ts` 的 `readAllKept` / `keepOnly`）。旧内核没有 `last_rev` 时退回按 `since_seq` 只列已结束的请求。
- 行上的标签：`SSE n` / `WS n`（事件流或 WebSocket 及已收到的消息数）、“改写未生效”（改写规则因为流式响应或正文超过 1 MiB 没有执行）、“进行中”。
- 请求详情：进行中的请求每秒重读（`capture_exchange`）；“事件” / “WebSocket 消息”逐条列出方向、事件名或帧类型、大小和内容开头，点开看全文，进行中时每秒追加（`since_message`）。WebSocket 只读，消息原样转发；permessage-deflate 由内核解压，解不开的标出来。每个请求保留最近 512 条、每条前 4 KiB，更早的显示为“未保留”。
- HTTP/2 连接有“HTTP/2 帧”：App ↔ AnixOps 与 AnixOps ↔ 服务器两段的帧头（类型、流、标志、长度、错误码、窗口增量），可按链路筛选，或只看 RST_STREAM、GOAWAY 和错误（`capture_h2_frames`）。
- 正文解压在 App 进程里调内核 `networkcore_ios_decode_body`（gzip、deflate、br、zstd，可叠加，最多 16 MiB），与桌面和导出一致；内核解不开时退回系统 Compression 框架。
- 快捷筛选（出错、JSON、图片、非 GET、HTTP/2、大于 100 KB、本地应答）和搜索框拼成同一个过滤表达式交给内核解析，语法见 project files `debug-client/kernel-capture-api.md` §4。快捷筛选的条件会加到搜索框每个 `OR` 分组里。
- 表达式有误时内核返回出错位置，列表上方标红提示。
- 只列 MITM 解密后的请求和明文 HTTP 请求；抓包未开启时空白页给出“开始抓包”。
- 没有解密出请求的抓包连接不会藏起来（Apple，`CaptureCoverage`）：列表为空时按域名或地址分组列在下面（连接数、流量、TCP/UDP），点进去看原始字节，未在解密列表里的域名旁有“解密”，点后加入调试解密列表并重连；列表有请求时，状态行给出“另有 N 个连接没有解密出请求”的入口。已在解密列表却没有请求的域名会说明可能原因（加入前建立的连接、证书绑定、非 443 端口）。只在没有筛选条件时统计。
- App 侧的检查清单只看本机（根证书已信任、有解密域名）；隧道扩展另外报告引擎是否真的开了解密（`TunnelAppMessage.httpsDecryption`：`on` / `off` + 原因 / `unused`），`off` 时请求页标红说明原因。
- 更换根证书（Apple，iOS 和 macOS）：生成或导入新根证书后（`TunnelController.rootCertificateChanged()`），VPN 配置记下新根的 SHA-256 指纹（`networkcore.mitm-root-fingerprint`，公开值），已连接且在解密的隧道自动重连改用新根；macOS 还先把新值交给运行中的系统扩展保存并等它应答（`TunnelAppMessage.startSecrets`），这样抢先由 On Demand 发起的启动也用新根。
  - macOS 由系统发起、不带 App 值的启动（On Demand、系统设置的 VPN 开关、重启后）用的是上次 App 交给扩展的副本；配置在解密而副本里的根证书指纹与 VPN 配置记下的不一致时（断开期间换了根），和解密包摘要不一致一样，启动以 `start secrets are older than the profile` 失败，App 在运行或下次回到前台时带新值重连一次（同一配置、同一根只重连一次），不会用旧根签发。旧配置没有指纹时照旧使用副本。
  - 隧道 `on` 时同时报告正在使用的根证书指纹；与钥匙串里的不一致时（换根后还没重连），请求页标红提示重新连接。
  - macOS 换根后撤销这台 Mac 对旧根的信任：先删用户域信任设置（`SecTrustSettingsRemoveTrustSettings`，要登录密码），再删登录钥匙串里的证书，最后用旧根签一张探测证书核对系统已不再信任（`MitmRootRevocation`）。取消授权或删不掉时保留钥匙串条目并说明到“钥匙串访问”手动删除；信任设置在系统级（管理员域、“系统”钥匙串）时 App 无权更改，同样提示手动删除。iOS 上旧描述文件只能由用户在设置里删除，证书页写明。
- 抓包跟隧道同生命周期，断开后照样能看（`TunnelController+CaptureCopies.swift`、`CaptureViewer.swift`）：
  - 抓包在隧道扩展的引擎里，会话密钥只有扩展有，App 读不到扩展的会话（没有 App Group，也不打算为此改 Apple 开发者后台）。所以连接时 App 让扩展把抓包用 App 自己的密钥重新加密后交过来（内核 `copy_capture`，App 密钥是钥匙串里的另一项 `app-viewer`），存在 App 自己的 Application Support/AnixOps-Captures，由 App 进程里的内核抓包查看器（`networkcore_ios_capture_viewer`，不需要引擎）打开。
  - 连接期间 App 每 5 秒看一次 `capture`，有变化就拷一份当前抓包，会话 id 是这次运行的 `run_session`；连接时还把扩展保存过、App 没有或只有旧版本的会话拷过来（App 里删掉的会话记在 `networkcore.capture.deleted-sessions`，不会再拷回来）。
  - 用户或系统断开隧道，抓包随之停止；扩展在 `stopTunnel` 里把有内容的抓包用同一个 `run_session` 加密保存（`save_capture` 的 `run: true`，名称“断开时自动保存”），下次连接时它替换 App 里同一 id 的副本，所以不会出现两份。从 App 断开时先等最多 3 秒拷最后一份。
  - 断开后请求页和连接页自动改为显示 App 里这次运行的副本，顶部横幅写明“正在查看已保存的会话”，可以照常筛选、看详情和导出；“回到实时抓包”或“关闭会话”回到实时状态，重新连接时自动回到实时抓包。查看已保存会话期间，读抓包和连接列表的请求都发给查看器，实时抓包在扩展里照常进行。
  - 查看器载入会话时保留原来的连接 id（`load_capture` 在查看器里不重新编号），连接页的“抓包内容”与请求一一对应。
  - App 自己发起的重连（换节点、点“解密此域名”、改插件或映射）开始前抓包是开着的，重连后 App 自动重新开启抓包。

## 导出单条请求与加密会话（计划 F1 / F5 / E11）

- 请求详情的“导出这条请求”：cURL、代码（Python requests / JavaScript fetch / Swift URLSession / Kotlin OkHttp / Go net/http）、Postman、Insomnia、HAR，经内核 `export_capture` 的 `id` + `exchange` 生成。
- “遮盖 Cookie、令牌和密码”默认打开（内核 `redact`）；整份 pcapng / HAR 导出也按内核默认脱敏。
- 打包导出 zip（内核 `export_capture` 的 `format: "zip"`）：一个文件里有 README.txt、connections.json（每个连接的目标、时间、流量和请求清单）、capture.har（有解密的请求时）和 capture.pcapng。请求页 ⋯、抓包内容页和已保存会话的长按菜单里有“遮盖密码和 Cookie”和“完整，不遮盖”两项，经系统分享面板发出。
- 请求列表“更多”里可以“保存当前抓包”和查看“已保存的会话”。
  - 隧道扩展在引擎启动后自己调用 `set_capture_storage`：目录是扩展自己的 Application Support/AnixOps-Captures，密钥是仅本机可用的 32 字节随机数（`CaptureSessionStorage`），不经过 App。
    - iOS：密钥在钥匙串里（`AfterFirstUnlockThisDeviceOnly`）。
    - macOS：Developer ID 版的隧道是以 root 运行的 System Extension，不在用户登录上下文里，用不了数据保护钥匙串（Apple TN3137），所以密钥是扩展容器里的文件 Application Support/`capture-session.key`（不在会话目录里），权限 0600，只有 root 能读，与 `start-secrets.json` 同一保护级别（`CaptureSessionKeyFile`）。新密钥先写进私有临时文件，再用 `link` 放到位，已有文件永不覆盖；文件读不出来或长度不是 32 字节时报错，不换新密钥，免得已保存的会话全部打不开（与 Tauri 端规则相同）。之前能读到的钥匙串密钥（只有非系统扩展的构建才可能有）在第一次创建文件时沿用。
    - 设置失败时扩展事件里记固定原因（钥匙串状态码、密钥文件不可读 / 长度不对 / 写不进去的 errno），不记密钥。macOS 的“已保存的会话”页在这种情况下写“隧道扩展读不到会话密钥”。
  - 会话用 ChaCha20-Poly1305 加密，保留 7 天。“已保存的会话”列的是 App 里的副本，连接与否都能打开；打开后请求列表显示它，实时抓包不受影响。删除会同时删 App 里的副本和（已连接时）扩展里的会话。

## Tauri 客户端（Windows / Linux / Android）

同一套行为，共用 `apps/windows-gui/ui` 的界面和 `crates/desktop-mitm`。

- **模式**：存储键为 `anixops.app-mode`（`proxy` / `debug`）和 `anixops.debug-exit`（`direct` / `node`），保存在 WebView 的 localStorage（`ui/src/mode.ts`）。首次选择、老用户直接用代理模式、导入节点后自动切到“经过节点”，都与 Apple 端一致。
- **侧栏**：调试网络下是“请求 · 改写”，下方是“出口 · 设置”，打开就在请求页，没有概览。“改写”就是扩展页（插件、Map Local / Remote、解密证书）；订阅、节点和分流规则（含 DNS）都在“出口”里。Android 窄屏下侧栏变成底栏。
- **直连出口**：后端命令 `set_direct_egress` 让引擎使用不含节点的 `{"outbound":{"type":"direct"}}` 配置。Linux、Android 和 Windows 都支持。
- **请求页**（`pages/RequestsPage.tsx`，仿 Proxyman）：左边是请求表格，右边常驻检查器（选中即看请求和响应，可重放、映射、导出）。窗口窄于 1100 px 时两者轮流显示，Android 上点一行打开详情。
- **进行中的请求与流**：请求表按 `since_rev` 增量读取（`capture-filter.ts` 的 `mergeRequests` / `nextRev`），请求头读到就出现，响应到来时原位更新；行以 `连接 id:序号`（`requestKey`）标识。类型列附 `SSE n` / `WS n` 和“改写未生效”。检查器在进行中时每两秒重读；选中的请求暂时不在列表中（换了筛选条件正在重读、超出列表上限或被内核淘汰）时，检查器保留选中时的内容并注明“已不在列表中”，打开的重放和映射编辑器不会被关掉，只有选另一条请求、清空抓包或载入会话才换掉它；流结束后，检查器最后一次读到的事件 / 消息（如 SSE 的 `[DONE]`、WebSocket 的 Close）和总数也会并入列表；有事件或 WebSocket 消息时多一个“事件” / “WebSocket”标签（`capture_exchange` + `since_message`，进行中每秒追加，只读）；HTTP/2 连接多一个“HTTP/2 帧”标签（`capture_h2_frames`，可按链路、只看 RST_STREAM / GOAWAY / 错误筛选）。概览里写明改写未生效的原因。
- **正文解压**：后端命令 `decode_body`（`desktop_mitm::capture::decode_body`，调内核 `capture::decode::decode_body`）解 gzip、deflate、br、zstd，可叠加，最多 16 MiB，只抓到开头的正文也解出能解的部分；内核解不开时退回 WebView 的 DecompressionStream（只限完整的 gzip / deflate）。
- **检查器**（`pages/Inspector.tsx`、`pages/BodyViewer.tsx`）：顶行是方法、状态和网址，下面四个标签：概览、请求、响应、时间线。请求和响应各自在“头 / 查询 / 正文”之间切换，选择会记住。正文按内容显示：JSON 是可折叠的树（方向键移动和折叠，Ctrl+C 拷贝选中的值，下方显示并可拷贝 JSONPath）或带颜色的格式化文本，图片直接预览，表单是名值对，其余是原文，任何内容都能切到十六进制。JSON 用自己的解析器（`json-view.ts`），数字按服务器发来的原样显示，不会像 `JSON.parse` 那样把 64 位 id 取整。时间线把同一连接上的请求画在一条时间轴上；内核目前只记录每个请求的开始和结束，没有 DNS、建连、TLS 的分段。
- **停靠与分栏**：检查器可以停靠在表格右侧或下方（检查器“更多”菜单），中间的分隔条可以拖动、用方向键调整、双击恢复默认，位置和比例记在本机（`split-pane.ts`）。右侧停靠需要窗口宽 1100 px 以上，下方停靠需要宽 720 px、高 600 px 以上，否则表格和检查器轮流显示。
  - 页面工具栏只放整份抓包的操作：视图切换（请求 / 连接 / 已保存）、抓包开关、暂停刷新、“更多”里的导出全部 pcapng / HAR 和清空抓包（红色，要确认）。
  - 表格（`pages/RequestList.tsx`）列出时间、方法、状态、主机、路径、类型、大小、耗时，点列头排序（`ui/src/request-table.ts`），默认最新在上。状态码按颜色区分：2xx 绿、3xx 黄、4xx/5xx 和没有响应红、等待中灰。只画可见的行，最多保留 2000 条。
  - 键盘：表格获得焦点后用 ↑ ↓、j / k、PageUp / PageDown、Home / End 切换，Enter 打开；Ctrl+F 跳到筛选框，Ctrl+R 重放选中的请求。
  - 用 `capture_requests` 每秒增量读取，筛选框写的就是内核的过滤表达式。快速筛选（出错、JSON、图片、HTTP/2、大于 100 KB）会拼进表达式（`ui/src/capture-filter.ts`）。
- **导出**：整份抓包可以“打包导出 zip”（遮盖 / 完整两项，内含连接清单、HAR 和 pcapng），连接可以导出 pcapng / HAR，单条请求可以导出 cURL、五种语言的代码、Postman、Insomnia 或 HAR，都保存到“下载”文件夹。“遮盖 Cookie、令牌和密码”默认打开。
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
  - 载入会替换当前抓到的内容并停止实时抓包；当前有内容时“载入”要再点一次确认。
  - 断开时也能列出、打开、导出和删除会话：引擎没在运行时，后端把抓包命令交给内核的抓包查看器（`desktop_mitm::sessions::viewer_control`，内核 `capture_viewer`，同一个目录和密钥）。用户断开（Android 上 VPN 被收回）时自动保存的会话随即在查看器里打开（`open_in_viewer`），请求页继续显示断开前的请求，顶部写明已断开、已加密保存，“更多”里可以打包导出或关闭会话；重新连接时查看器清空（`close_viewer`），列表回到实时抓包。
- **抓包跟引擎同生命周期，重启和断开不丢**（`desktop_mitm::sessions::preserve`）：
  - 内核目前只有 `reload_rules` 能在运行中替换规则，没有替换解密域名、映射和插件的命令，所以改这些（含“解密此域名”、“映射到本地/远程”）、开关 HTTPS 解密或 TUN、改 DNS 重定向、换节点都会重启引擎。
  - 重启前，如果实时抓包开着且有内容，后端先把它加密保存成会话“重连前自动保存”；用户断开、退出程序（Android 上 VPN 被系统收回）时保存成“断开时自动保存”，名称跟随界面语言。正在查看的已保存会话（`load_capture` 会关掉抓包）不会再存一次。保存失败只记日志并在提示里说明，不会挡住重启或断开。
  - 重启后自动重新开启抓包，并带上原来的容量上限；操作提示写明“当前抓包已保存为……”。
  - 新引擎的 `seq` / `rev` 和连接 id 从 1 重新计数：请求表发现内核返回的计数比已读到的小（`engineRestarted`），就清空列表从头读，并取消原来的选中，免得旧行挡住新请求或同 id 指向别的请求。Android 的 `Core.requests` 做同样的判断。
  - Android 每秒读一次请求表，只让整份行列表在有变化时穿过 JNI：下一次读取的位置（`nextSince` / `nextRev`，与 client-view 的
    `next_since` / `next_rev` 逐行对应，`ViewModelsTest` 用 client-view 的样例核对）在 Kotlin 里算；内核没有新请求、也没有挤出
    已列出的行时，不再合并和重排，沿用上次的结果；暂停期间只在换排序时重排一次。
  - Linux 和 Android 由 `client-core` 在重启（`restart_if_running`）、断开和退出时做同样的保存与恢复。会话密钥在用户开启抓包时由客户端读出后交给后端，重启时不会在持有后端锁的情况下去问钥匙环或 Keystore；没拿到密钥时抓包照常恢复，只是不保存。
- **桌面抓包容量**：内核默认只留 2 MiB（按 iOS 网络扩展的内存上限定的），桌面窗口每次开启抓包都带上 `max_total_bytes` = 16 MiB（内核允许的最大值，`DESKTOP_CAPTURE_BUDGET`），否则普通连接几秒就会把解密的请求挤出去。Android 仍用内核默认值。
- **Map Local / Map Remote**（`desktop_mitm::mappings`）：存放在插件目录的 `debug-mappings.json`，格式与 Apple 端相同。`plugins::apply` 把生成的“AnixOps 调试映射”接在导入的插件后面。映射要开启 HTTPS 解密才生效，保存后正在运行的引擎会重新加载。
