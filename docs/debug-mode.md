# 调试模式（Apple 客户端）

AnixOps Net 是一个 App 两种模式（计划 A3/A4）：**代理**按订阅规则分流；**调试网络**把抓包、HTTPS 解密、改写和重放放在首位，代理只是底座。
本页记录 iOS / macOS（SwiftUI）的实现；Windows、Linux、Android（Tauri）的实现见文末「Tauri 客户端」。

## 两种模式

| | 代理 | 调试 |
|---|---|---|
| iOS 标签 | 首页 · 代理 · 规则 · 扩展 · 设置 | 首页 · 请求 · 扩展 · 设置 |
| macOS 侧栏 | 概览 · 代理 · 规则 · 扩展 · 活动 · 设置 | 概览 · 活动 · 扩展 · 设置 |
| 订阅与节点 | 自己的标签 / 侧栏项 | 设置 › 出口 |
| 首页导入按钮 | 有 | 没有（在 设置 › 出口 › 订阅与节点 里导入） |

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

## 截图

CI 的 macOS 截图（`mac-screenshots-<sha>`）额外包含 `debug-home-light`、`debug-settings-light` 和首次启动的 `choose-home-light`。
演示数据用 `-AnixOpsMode debug|proxy|choose` 选择模式。

## 请求列表（计划 E5）

调试模式的“请求”（iOS 标签 / macOS 活动页）按 HTTP 请求逐条列出，最新在上，点开是请求详情（可重放、生成映射）。右上角可以切到全部连接。

- 数据来自内核 `capture_requests`（networkcore d1a8cc1 起），每秒按 `since_seq` 增量读取，`dropped_before_seq` 以内的行随内核一起删掉；本地最多保留 5000 行（`NetworkCoreBridge/CaptureRequests.swift`）。
- 快捷筛选（出错、JSON、图片、非 GET、HTTP/2、大于 100 KB、本地应答）和搜索框拼成同一个过滤表达式交给内核解析，语法见 project files `debug-client/kernel-capture-api.md` §4。快捷筛选的条件会加到搜索框每个 `OR` 分组里。
- 表达式有误时内核返回出错位置，列表上方标红提示。
- 只列 MITM 解密后的请求和明文 HTTP 请求；抓包未开启时空白页给出“开始抓包”。

## 后续

- 导出 cURL / 代码 / Postman / Insomnia（默认脱敏）和加密保存抓包会话（内核已支持：`export_capture` 新格式、`set_capture_storage` 等）。

## Tauri 客户端（Windows / Linux / Android）

同一套行为，共用 `apps/windows-gui/ui` 的界面和 `crates/desktop-mitm`。

- **模式**：存储键为 `anixops.app-mode`（`proxy` / `debug`）和 `anixops.debug-exit`（`direct` / `node`），保存在 WebView 的 localStorage（`ui/src/mode.ts`）。首次选择、老用户直接用代理模式、导入节点后自动切到“经过节点”，都与 Apple 端一致。
- **侧栏**：调试网络下是“请求 · 规则 · 扩展”，“出口”放在“设置”上方，订阅与节点都在出口里。Android 窄屏下侧栏变成底栏。
- **直连出口**：后端命令 `set_direct_egress` 让引擎使用不含节点的 `{"outbound":{"type":"direct"}}` 配置。Linux、Android 和 Windows 都支持。
- **请求列表**（`pages/RequestList.tsx`）：用 `capture_requests` 每秒增量读取，筛选框写的就是内核的过滤表达式。快速筛选（出错、JSON、图片、HTTP/2、大于 100 KB）会拼进表达式（`ui/src/capture-filter.ts`）。
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
