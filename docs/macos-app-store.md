# macOS 的 Mac App Store / TestFlight 构建

macOS 客户端有两种构建，同一份代码，并行发布：

| | Developer ID 版（`NetworkCoreMac`） | Mac App Store 版（`NetworkCoreMacAppStore`） |
| --- | --- | --- |
| 分发 | 官网、面板下载（ZIP / DMG / PKG），公证 | TestFlight（以后可上 Mac App Store） |
| 签名 | Developer ID Application + 公证 | Apple Distribution，安装包用 Mac Installer Distribution |
| 隧道 | System Extension（`Contents/Library/SystemExtensions`），用户在系统设置里允许 | App 扩展（`Contents/PlugIns/NetworkCoreMacAppStoreTunnel.appex`），第一次连接时系统只问是否添加 VPN 配置 |
| 网络扩展权限 | `packet-tunnel-provider-systemextension` + `system-extension.install` | `packet-tunnel-provider` |
| 应用内更新 | 有（`docs/macos-updates.md`，默认不自动检查） | 没有，由 App Store / TestFlight 更新 |
| `anixops` 命令行工具 | 有（`docs/macos-cli.md`） | 没有 |
| 网关模式开 IP 转发 | 系统扩展以 root 运行，可能自己打开 | 扩展以用户身份运行，打不开，设置页给出 `sudo sysctl` 命令 |

## 为什么要两种

- Mac App Store（TestFlight 也一样）不收 System Extension，网络扩展必须是 App 扩展；Developer ID 版反过来，Apple 要求
  直装版的网络扩展用 System Extension。
- Mac App Store 要求每个可执行文件都在沙盒里。`anixops` 命令行工具在终端里启动，沙盒里的工具连不到 App 的
  App Group 容器，所以 Mac App Store 版不带它，也不启动它要连的本地 socket（`MacControlService`）。
- App Store 审核指南 2.4.5 不允许 App 自己下载安装更新，所以 Mac App Store 版没有 `AppUpdater` 和「软件更新」设置。

代码里用编译条件 `ANIXOPS_APP_STORE`（只在 `NetworkCoreMacAppStore` 目标上打开）去掉这三部分。两个 App 目标共用
`apps/ios/project.yml` 的 `targetTemplates: NetworkCoreMacApp`（源文件与 Info.plist），各自写设置和依赖。隧道两种
构建都用 `Sources/NetworkCorePacketTunnel`；App 在连接时照样把钥匙串里的密钥交给扩展（`TunnelStartSecrets`），
两种构建的启动流程一样。

## 构建与上传

- 公开仓库 CI（`ios-xcode-build` 任务）每次都不签名构建 `NetworkCoreMacAppStore`（Release），检查：扩展是
  `com.apple.networkextension.packet-tunnel` 的 App 扩展、没有 System Extension、没有 `anixops`、`Contents/MacOS` 只有
  主程序、两个权限文件都开沙盒且只要 `packet-tunnel-provider`、运行时库已链接。
- 签名和上传在私有 anx 仓库的「macOS TestFlight with ANX」workflow（`release-macos-appstore.yml`）里做，和 iOS 一样
  带 ANX、只上传内部 TestFlight。`upload=no` 只构建、签名、导出 .pkg 并检查签名；`upload=yes` 才上传，属于发布，
  要用户明确同意。构建号是 run number + 7000，与 iOS（+5000）和开源上传（+1000）分开。

## Apple 账号侧（维护者做一次）

建议 macOS 版用 iOS App 的同一个 bundle ID（通用购买）：App Store Connect 里同一条 App 记录同时有 iOS 和 macOS，
内部测试员在两边的 TestFlight 都能装。anx 的 Environment `apple-anx` 已经有 `MACOS_APP_BUNDLE_ID` /
`MACOS_TUNNEL_BUNDLE_ID`（Developer ID 版在用），Mac App Store 版沿用。

1. App Store Connect › App › AnixOps Net：左栏平台旁的「+」加 **macOS** 平台（不用填审核信息，内部测试不审核）。
2. 开发者后台 › Identifiers：确认 App 和隧道两个 App ID 都勾了 **Network Extensions**（Developer ID 版已经勾过）。
3. 开发者后台 › Certificates：新建 **Mac Installer Distribution** 证书（用钥匙串助理生成 CSR），装进钥匙串后连同私钥
   导出为 .p12。App 签名沿用 iOS 上传的 Apple Distribution 证书，不用新建。
4. 开发者后台 › Profiles：新建两个 **Distribution › Mac App Store Connect** 描述文件，平台 macOS，分别选 App 和隧道的
   App ID、选那张 Apple Distribution 证书，下载两个 `.provisionprofile`。
5. anx 仓库 › Settings › Environments › `apple-anx` 加 secret：
   - `MACOS_APPSTORE_INSTALLER_P12_BASE64`：第 3 步 .p12 的 base64（`base64 -i installer.p12 | pbcopy`）；
     OpenSSL 3 导出的要加 `-legacy`，否则 macOS 读不了。
   - `MACOS_APPSTORE_INSTALLER_PASSWORD`：.p12 的密码。
   - `MACOS_APPSTORE_APP_PROFILE_BASE64`、`MACOS_APPSTORE_TUNNEL_PROFILE_BASE64`：两个描述文件的 base64。
6. 先跑一次 `upload=no` 确认签名；同意上传后跑 `upload=yes`。处理完成后（通常十几分钟到一小时）内部测试员在 Mac 上的
   TestFlight App 里就能装（TestFlight 需要 macOS 12+，本 App 需要 macOS 14+）。

workflow 会检查：描述文件的 App ID、平台是 macOS、是 Mac App Store Connect 类型（不列设备、不是 Developer ID）、带
`packet-tunnel-provider`；签好的 App 与扩展都是 Apple Distribution、开沙盒、没有 System Extension 权限、隧道里有 ANX
出站；.pkg 是 Mac Installer Distribution 签名。哪一项不对都会给出具体原因。

## 同一台 Mac 上两种版本

两种版本 bundle ID 相同，同时装会互相覆盖。从 Developer ID 版换到 TestFlight 版时，先断开并退出旧版、把它从「应用程序」
删掉（macOS 会一并移除它的系统扩展），再从 TestFlight 安装；第一次连接时系统会请求添加 VPN 配置。
