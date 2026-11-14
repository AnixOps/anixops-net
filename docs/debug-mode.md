# 调试模式（Apple 客户端）

AnixOps Net 是一个 App 两种模式（计划 A3/A4）：**代理**按订阅规则分流；**调试网络**把抓包、HTTPS 解密、改写和重放放在首位，代理只是底座。
本页记录 iOS / macOS（SwiftUI）的实现；Windows、Linux、Android（Tauri）按同一行为另做。

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

## 后续

- 请求列表按 HTTP 请求逐条展示，快捷筛选和表达式搜索共用一个搜索框（内核 `capture_requests` 和过滤表达式，见 project files `debug-client/kernel-capture-api.md`）。
