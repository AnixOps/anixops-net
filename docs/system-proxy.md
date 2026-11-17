# 苹果平台的接管方式

iOS 和 macOS 的隧道可以用三种方式把流量交给内核，设置页“接管方式”里选，连接中切换会自动重连一次。

| 方式 | iOS | macOS | 做什么 |
|---|---|---|---|
| VPN + 系统代理（默认） | ✅ | ✅ | VPN 接管全部流量（默认路由、DNS 指向 198.18.0.2），同时在隧道设置里注册系统 HTTP / HTTPS 代理（`NEProxySettings`）指向本机 `127.0.0.1` |
| 仅 VPN | ✅ | ✅ | 只有 VPN，和加这个设置之前完全一样 |
| 仅系统代理 | — | ✅ | 只注册系统代理，隧道不带路由和 DNS；不遵守代理设置的程序和 UDP 直接出去 |

iOS 不提供“仅系统代理”：VPN 不带路由时，不遵守代理设置的 App 会悄悄绕开规则，用户看不出原因。

## 为什么要加系统代理

只走 VPN 时，每条 TCP 连接都要经过隧道里的用户态 TCP/IP 协议栈（smoltcp），域名靠 Fake-IP 或首包嗅探才能拿到。
遵守系统代理的 App（URLSession / CFNetwork、浏览器、系统服务，占日常流量的大头）改走代理后：

- 不经过用户态协议栈，省电、省 CPU。
- 规则直接拿到 `CONNECT host:443` 里的域名，不依赖 DNS 映射。
- 明文 HTTP 的请求头（完整网址、User-Agent）在建连前就交给 `URL-REGEX` / `USER-AGENT` 规则（内核 `connect_client_with_first`）。
- macOS 上有 `PROCESS-NAME` 规则时按本机进程名分流。

经代理的连接和 VPN 里的连接用同一个分发器：分流、连接列表、抓包、HTTPS 解密（MITM）、脚本都一样。

## 实现

- 内核：`networkcore_anixops` 的 `engine-embedded::system_proxy`，控制命令 `start_system_proxy`（首选端口 8890，被占用时自动换空闲端口并在回答里报告）、`system_proxy`、`stop_system_proxy`。监听只绑 `127.0.0.1`、不要凭据（能连上它的程序本来也能把同一条连接送进 VPN）。
- 隧道（`PacketTunnelProvider`）：建好内核会话后、应用接口设置前先开监听，拿到端口写进 `NEProxySettings`（HTTP 和 HTTPS 都指向它，`matchDomains = [""]` 对所有域名生效，`excludeSimpleHostnames = true`），再启动隧道。内核拒绝（整机 WireGuard 隧道没有分发器）时不设代理；macOS 的“仅系统代理”此时退回 VPN 接管，诊断事件会记一条。
- 配置：App 的 UserDefaults 保存选择（`networkcore.takeover`、`networkcore.proxy-exceptions`），隧道配置里带一份（`TunnelProfileKeys.takeover`、`TunnelProfileKeys.proxyExceptions`）。

## 跳过代理的地址

`NEProxySettings.exceptionList`，每行一项：域名、`*.` 通配符、IP 或网段，最多 256 项，`0.0.0.0/0` 这类全网段不接受。默认：

```
localhost
*.local
captive.apple.com
127.0.0.0/8
10.0.0.0/8
172.16.0.0/12
192.168.0.0/16
169.254.0.0/16
100.64.0.0/10
::1
fc00::/7
fe80::/10
```

“VPN + 系统代理”下，跳过代理的地址仍按规则走 VPN；“仅系统代理”下它们直接出去。

## macOS 网关模式

设置页“网关模式”打开后（需要“VPN + 系统代理”或“仅 VPN”），同一局域网的设备把**路由器**改成这台 Mac 的局域网地址、**DNS** 改成 `198.18.0.2`，
它们的流量就经过 Mac 进入隧道：Mac 打开 IPv4 转发后，转发的包沿默认路由进 utun，被内核的用户态协议栈接住，
规则、MITM、抓包、连接列表都和本机流量一样；发往任何地址 53 端口的 UDP 查询都由内核回答，`198.18.0.2` 在隧道网段内，
设备的 DNS 查询经 Mac 转发进隧道。

- IPv4 转发（`net.inet.ip.forwarding`）：隧道连接后尝试打开、断开时关掉自己打开的；系统扩展在沙盒里，系统不允许时，
  设置页显示状态和命令 `sudo sysctl -w net.inet.ip.forwarding=1`（可复制，重启 Mac 后失效）。
- 只支持 IPv4。单个 WireGuard 节点的整机隧道把包原样封给对端，对端不会回送局域网设备的地址，不支持。
- 设置页会列出本机的私有 IPv4 地址（`en*` 优先）作为设备要填的路由器地址。

## 已知限制

- 经代理的明文 HTTP 每个请求单独建一条上游连接（代理在转发时加 `Connection: close`，防止一条上游连接被复用到另一个主机），与 VPN 路径的长连接相比多一次握手。
- iOS 上经代理的连接没有进程名（系统不提供）。
- 真机验收清单见 [`manual-intervention.md`](manual-intervention.md)。
