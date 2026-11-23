# AnixOps Net Privacy Policy / 隐私政策

Effective date / 生效日期：2026-11-23

[English](#english) · [中文](#中文)

## English

This policy covers the AnixOps Net client on Android, iOS, macOS, Windows and Linux, including the Google Play,
App Store and TestFlight builds, published by AnixOps Studio ("we"). The client is open source; the full list of
connections it makes is in [docs/network-behavior.md](docs/network-behavior.md), and that document is checked against
the source code in CI.

### What we collect

**Nothing by default.** The client contains no analytics, telemetry, advertising, crash-reporting or
remote-configuration SDK, and it never sends us your device identifiers, advertising ID, location, contacts,
installed-app list, browsing history or network traffic. The only report the client can send on its own is the
optional connection-quality report on iOS and macOS (see "Connections the client makes"): it is off until you turn it
on, and it goes to the provider of your subscription, not to a separate server of ours.

### Data that stays on your device

The client stores the following only on your device, in app-private storage. It is not uploaded to us and is not
included in system or cloud backups:

- Subscriptions, nodes, rules, plugins and scripts that you add, and your settings.
- Connection logs, traffic statistics and, when you turn it on, packet captures. You decide whether to export them.
- Crash reports, with addresses, domains and keys removed. You decide whether to export and send them.
- A device key per subscription server, used only as described below.

Uninstalling the app deletes this data, except on Windows. There the client keeps it in a folder for the whole computer
(`%ProgramData%\AnixOps\NetworkCore`) that only the system and administrators can open, not other users of the
computer. Uninstalling there removes the HTTPS decryption key and the decryption root from the certificate stores of
the users signed in at the time; your subscriptions, settings and logs stay in that folder until you delete it.

### Connections the client makes

- **Subscription updates** go only to the subscription address you enter. They carry the app version in the
  User-Agent. A subscription server that declares support for licensed protocols also receives a public key that is
  unique to that server, so two servers cannot link the same device.
- **Traffic you route through the VPN** goes to the servers in your own configuration. We do not provide servers,
  and we cannot see this traffic. The operator of each server you choose handles your data under its own policy.
- **Latency and exit-address checks** run when you tap them or right after you connect, and send empty requests that
  carry no identifiers (details in docs/network-behavior.md).
- **Connection-quality report (iOS and macOS, off by default).** Only if you turn on "Report connection quality to
  subscription provider" in Settings › Privacy, then while the app is open and connected, about once an hour, the
  client sends the connection counts of nodes using the ANX protocol to the subscription provider that supplied
  them. The address is your subscription link without its query and file extension, plus `/report`; the token in
  the link goes with it, so the provider can match the report to your account. The report contains the app version,
  platform, network type (Wi-Fi, cellular, wired or other), the period covered, and for each node its name, server
  address and port, protocol, success and failure counts, whether it was paused after repeated rejections, and its
  clock offset. It never contains the sites or addresses you visit. The provider handles it under its own policy.
  Turning the setting off stops the report and deletes the counts kept for it; turning it on again does not send
  anything from the time it was off. If none of your nodes use the ANX protocol, nothing is sent.

### Android permissions

- **VPN service** (`BIND_VPN_SERVICE`): routes device traffic through the client according to your rules, only after
  you start the connection and accept the system prompt. Traffic is processed on the device; none of it is sent to us.
- **Camera**: scanning a subscription or node QR code, asked for when you tap Scan. Images are not stored.
- **Location**: reading the current Wi-Fi name for Wi-Fi rules, asked for only when you turn those rules on. On
  Android 10 and later, background location ("Allow all the time") is optional and only asked for after an in-app
  explanation, so the rules can read the Wi-Fi name while the app is in the background. The location itself is not
  read, stored or sent.
- **Notifications and foreground service**: the "connected" notification that keeps the VPN running, and, while Wi-Fi
  rules are on, a "waiting for network" notification that lets the rules reconnect from the background.

### Children

The client is not directed at children under 13 and is intended for adults.

### Changes and contact

Changes to this policy are published in this file, with the history in the public repository. Questions:
**contact@anixops.com**.

## 中文

本政策适用于 AnixOps Studio（下称“我们”）发布的 AnixOps Net 客户端（Android、iOS、macOS、Windows、Linux），
包括 Google Play、App Store 与 TestFlight 版本。客户端开源，它会发出的全部连接写在
[docs/network-behavior.md](docs/network-behavior.md)，CI 会按源码核对这份文档。

### 我们收集什么

**默认不收集任何数据。** 客户端不含任何统计、遥测、广告、崩溃上报或远程配置 SDK，不会向我们发送设备标识、广告标识、
位置、通讯录、已安装应用列表、浏览记录或网络流量。客户端唯一可能自己发出的上报是 iOS、macOS 上可选的连接质量报告
（见“客户端会发出的连接”）：在你打开之前不会发送，发往你的订阅服务商，而不是我们另设的服务器。

### 只保存在你设备上的数据

以下内容只保存在本机的应用私有存储里，不上传给我们，也不进入系统或云备份：

- 你添加的订阅、节点、规则、插件、脚本，以及各项设置。
- 连接日志、流量统计，以及你开启后才有的抓包记录，是否导出由你决定。
- 崩溃报告（已去掉地址、域名、密钥），是否导出发送由你决定。
- 每个订阅服务器各一把设备密钥，用途见下文。

除 Windows 外，卸载应用即删除以上数据。Windows 版把这些数据放在整台电脑共用的目录（`%ProgramData%\AnixOps\NetworkCore`）里，
只有系统和管理员能打开，本机其他用户不能读取。在 Windows 上卸载会删除 HTTPS 解密私钥，并从卸载时已登录用户的证书存储里
删除解密根证书；订阅、设置和日志留在该目录里，需要你自己删除。

### 客户端会发出的连接

- **订阅更新**只发往你填写的订阅地址，User-Agent 里带应用版本。声明支持授权协议的订阅服务器还会收到一把只属于
  该服务器的公钥，两个服务器无法据此认出同一台设备。
- **经 VPN 转发的流量**发往你自己配置的服务器。我们不提供服务器，也看不到这些流量；你选用的服务器由其运营方按
  其自己的政策处理你的数据。
- **延迟和出口地址检测**在你点按或刚连接后进行，发出的是不带任何标识的空请求（详见 docs/network-behavior.md）。
- **连接质量报告（iOS、macOS，默认关闭）**：只有你在“设置 › 隐私”打开“向订阅服务商报告连接质量”后，App 打开且已连接时，
  客户端才约每小时把使用 ANX 协议的节点的连接计数发给提供这些节点的订阅服务商。发往的地址是订阅链接去掉参数和扩展名后
  加 `/report`，链接里的令牌随之发出，服务商能据此对应到你的账号。内容为 App 版本、平台、网络类型（Wi-Fi、蜂窝、有线或
  其他）、统计时长，以及每个节点的名称、服务器地址和端口、协议、成功和各类失败次数、是否因连续被拒而暂停、时间偏差；
  不含你访问的网站或地址。服务商按其自己的政策处理。关闭开关即停止上报并删除为此保存的计数；重新打开不会补发关闭期间的
  数据。没有使用 ANX 协议的节点时不会发送。

### Android 权限

- **VPN 服务**（`BIND_VPN_SERVICE`）：在你开始连接并同意系统提示后，按你的规则转发本机流量。流量在设备上处理，
  不发送给我们。
- **相机**：扫描订阅或节点二维码，点“扫码”时才申请，不保存图像。
- **位置**：Wi-Fi 规则需要读取当前 Wi-Fi 名称，只在你打开 Wi-Fi 规则时申请。Android 10 及以上的后台位置（“始终允许”）
  是可选的，App 内先说明用途再申请，用于在后台读取 Wi-Fi 名称；不读取、不保存、不发送位置本身。
- **通知与前台服务**：显示“已连接”通知，保持 VPN 运行；打开 Wi-Fi 规则时显示“等待网络”通知，以便规则在后台重新连接。

### 儿童

客户端不面向 13 岁以下儿童，适用对象为成年人。

### 变更与联系

本政策的修改发布在本文件中，历史记录见公开仓库。如有问题请联系 **contact@anixops.com**。
