# 人工介入事项

本文件记录客户端仓库中无法由 GitHub Actions 自动完成的事项。客户端从
[AnixOps/networkcore_anixops](https://github.com/AnixOps/networkcore_anixops) 迁出前的历史人工验收记录仍在内核仓库的
`docs/manual-intervention.md`。

## 仓库迁移后的设置（2026-10-26）

客户端于 2026-10-26 从内核仓库迁入本仓库。迁移后需要维护者在本仓库完成：

- [ ] 创建 GitHub Environment `ios-app-store-connect`（含 required reviewer），写入下方 `ios-upload-internal-workflow-secrets`
  列出的 secrets，以及 `APPLE_WIDGET_PROVISIONING_PROFILE_BASE64`。
- [ ] 在仓库或 environment Variables 中写入 `IOS_APP_BUNDLE_ID`、`IOS_TUNNEL_BUNDLE_ID`、`IOS_WIDGET_BUNDLE_ID`。
- [ ] 手动运行一次 `iOS internal TestFlight`，确认能上传。迁移后 build number = 本仓库 workflow run number + 1000，
  保证比内核仓库上传过的 build 大。
- [x] Windows MSI / 便携 ZIP 与 Linux GUI tarball / .deb 的发布 workflow 已迁入本仓库 `release.yml`（`v*` tag 发布，手动触发只打包和证明）。推送发布 tag 需要维护者明确同意。

## iOS TestFlight/App Store Connect Upload Workflow

以下字段是后续 iOS release readiness 读取的机器状态。当前没有真实 Swift/Xcode source、signing、
archive/export、App Store Connect API、protected environment 或 manual approval，因此 upload/release 保持阻断。

```text
ios-upload-workflow-status=enabled
ios-upload-workflow-source-contract=docs/architecture/ios-testflight-app-store-connect-upload-workflow-source-contract.md
ios-upload-workflow-archive-export=blocked
ios-upload-workflow-app-store-connect-api=blocked
ios-upload-workflow-protected-environment=blocked
ios-upload-workflow-manual-approval=blocked
ios-upload-workflow-testflight-upload=blocked
ios-upload-workflow-app-store-upload=blocked
ios-upload-workflow-app-review-submission=blocked
ios-upload-workflow-release-assets=blocked
ios-upload-workflow-macos-runner=blocked
ios-upload-workflow-build-processing-check=blocked
ios-upload-workflow-confirmed-at=2026-10-22
ios-upload-workflow-confirmed-by=kalijerry
ios-upload-scope=internal-testflight-only
ios-upload-scope-decision=maintainer-2026-10-13
ios-upload-external-testflight=blocked
ios-upload-beta-app-review=blocked-not-required-for-internal
ios-upload-bundle-id-storage=github-variables-only
ios-upload-api-key-status=team-key-added-to-environment
ios-export-compliance-answer=standard-https-only-exempt-2026-10-13
ios-upload-internal-confirmation-status=confirmed
ios-upload-internal-confirmation-items=app-record,export-compliance,internal-testing-group,app-privacy-determination,privacy-policy-determination,environment-secrets-and-variables
ios-upload-internal-workflow=.github/workflows/ios-testflight.yml
ios-upload-internal-workflow-status=enabled-manual-dispatch-internal-testflight-only
ios-upload-internal-workflow-variables=IOS_APP_BUNDLE_ID,IOS_TUNNEL_BUNDLE_ID
ios-upload-internal-workflow-secrets=APP_STORE_CONNECT_API_KEY_ID,APP_STORE_CONNECT_ISSUER_ID,APP_STORE_CONNECT_API_PRIVATE_KEY,TEAM_ID,APPLE_CERTIFICATE_P12_BASE64,APPLE_CERTIFICATE_PASSWORD,APPLE_KEYCHAIN_PASSWORD,APPLE_PROVISIONING_PROFILE_BASE64,APPLE_PACKET_TUNNEL_PROVISIONING_PROFILE_BASE64
```


## Windows 去除第三方内核后真机验收（Phase 4A）

在 Windows 真机上安装包含 Phase 4A 的构建后：

- 用旧版本（兼容内核导入）留下的配置启动，记录连接时的提示；重新导入订阅后能连接，节点切换、延迟测试、策略组切换和五种分流模式都可用。
- 开启 HTTPS 解密：浏览器经解密访问 HTTPS 站点正常；关闭解密后浏览器仍能正常上网。
- 设置页不再出现兼容内核和附加协议内核卡片，规则页不再出现 JSON 编辑和 DNS JSON。
- 诊断报告里只有内置引擎和 MITM 的状态字段。

## Windows 去除 SD-WAN 隧道后真机验收

在 Windows 真机上安装去除前台隧道的构建后：

- 用带 `"tunnel"` 字段的旧托管配置启动服务，服务正常启动，不再尝试拉起外部隧道进程；
  旧版本留下的隧道进程和路由需要先用旧版本停止，或手动结束进程并删除其路由。
- 设置页的“系统与网络”卡片不再出现 TUN 配置输入框；`networkcore-windows tunnel ...` 返回未知命令。

前台隧道原有的人工验收要求随功能一并移除（历史见内核仓库
`docs/archive/2026-10-removed-engines/windows-stage-4-pop-peer-manual-acceptance.md`）。SD-WAN 组网将在
ANX 私有协议中重新提供。

## iOS 订阅组与自动更新真机验收

CI 只能编译和跑单元测试，以下需在 TestFlight 真机上确认：

- 旧版本升级后，原来的订阅变成第一个订阅组且仍是当前节点。
- 导入两个订阅链接后，代理页出现两个订阅组，各自显示节点数、更新时间；服务商返回 `subscription-userinfo` 时显示已用/总流量和到期日。
- 订阅组菜单的“立即更新”、“自动更新”间隔、重命名、删除可用；下拉刷新更新全部订阅。
- 点另一个订阅组的节点会切到该组并重连。
- 带规则的订阅按规则分流时，“规则”页的选择组里出现“订阅·名称”成员，选它后流量走另一个订阅组的节点。
- 后台刷新：设置 > 通用 > 后台 App 刷新打开时，把间隔设为 1 小时，一段时间后不打开 App 也能看到更新时间变化（iOS 调度时间不保证）。

## Windows 启动即管理员运行真机验收

GUI 清单改为 `requireAdministrator`，登录后启动从 HKCU Run 改为按用户的提权计划任务 `\AnixOps\AnixOps Net <SID>`。CI 覆盖 MSI 安装/卸载和任务清理；以下需要在真机确认：

- [ ] 双击 `networkcore-windows-gui.exe` 弹出 UAC，同意后正常进入界面；拒绝则不启动。
- [ ] 设置里打开「登录后启动」，注销再登录，客户端自动以管理员启动且不弹 UAC。
- [ ] 旧版本开过「登录后启动」的机器升级后首次启动，HKCU Run 里的 `AnixOpsNetworkCore` 被删除，计划任务已创建。
- [ ] MSI 卸载后计划任务被删除，卸载不会因为这一步失败。
- [ ] 以管理员身份运行后，系统代理仍写到当前登录用户（同一账户提权）；标准用户输入其他管理员账户凭据的情况不支持，需记录现象。

## Windows 桌面模式 HTTPS 解密真机验收

桌面模式下 HTTPS 解密改由客户端进程自己运行（客户端已固定以管理员运行），不再需要 Windows 服务：

- [ ] 扩展页开启 HTTPS 解密，连接成功；浏览器访问 HTTPS 站点证书颁发者为 NetworkCore 根证书，插件改写生效。
- [ ] 已连接时开启/关闭 HTTPS 解密，客户端自动重连，系统代理保持在 127.0.0.1:7890。
- [ ] 关闭 HTTPS 解密后，「受信任的根证书颁发机构」里的 NetworkCore 根证书被移除。
- [ ] 断开连接后 7890/7891 端口都被释放。

## Windows TUN 模式真机验收

桌面模式新增「TUN 模式」开关：客户端用 Wintun 虚拟网卡接管全部流量，系统代理和 HTTPS 解密照常保留。CI 没有 Wintun 驱动，只能真机验收（Windows 10/11 x64，管理员）：

- [ ] 安装目录（MSI 和便携 ZIP）里有 `wintun.dll`；设置 → 运行方式 → 打开「TUN 模式」，已连接时客户端自动重连。
- [ ] 「网络连接」里出现 `AnixOps Net` 网卡；`route print` 有 `0.0.0.0/1`、`128.0.0.0/1` 走该网卡，原默认路由仍在。
- [ ] 不走系统代理的程序（`curl --noproxy "*" https://www.google.com/generate_204`、游戏）经过代理；活动页可能只显示系统代理那一路的连接。
- [ ] 代理服务器连接不回环：连接稳定、CPU 正常，`netstat -ano` 中客户端到节点的连接本地地址是物理网卡 IP。
- [ ] 关闭 TUN 模式或断开后，`AnixOps Net` 网卡和路由消失，网络恢复正常。
- [ ] 已知限制：DNS 服务器在本地网段（如路由器 192.168.1.1）时 Windows 可能并发向它查询；切换 Wi-Fi/有线后需要重连一次。

## macOS Developer ID 版真机验收

macOS 客户端改为官网分发（Developer ID 签名 + 公证），隧道从 App 扩展改成 System Extension。签名、公证在私有 anx 仓库的
「macOS with ANX」workflow 里完成；CI 只能做不签名构建，以下只能真机验收（macOS 14+）：

- [ ] 解开 ZIP / 挂载 DMG，把 AnixOps Net 拖进「应用程序」后打开，Gatekeeper 不报「无法验证开发者」；`spctl -a -vv` 显示 `Notarized Developer ID`。
- [ ] 第一次连接时提示允许网络扩展；在「系统设置 › 通用 › 登录项与扩展 › 网络扩展」允许后，再连接成功。
- [ ] 不在「应用程序」里直接打开时，连接给出「请先把 AnixOps Net 拖进应用程序文件夹」的提示。
- [ ] 订阅规则、HTTPS 解密（根证书已信任）、脚本插件在 Mac 上照常生效（隧道读的是 App 启动时交给它的密钥副本）。
- [ ] 睡眠唤醒、从系统设置的 VPN 开关直接连接（不经 App）也能连上。
- [ ] 安装新版本后再连接，系统扩展被替换为新版本（`systemextensionsctl list` 版本号更新）。

## macOS 应用内更新

应用内更新的代码、发布工具和 CI 篡改检查已经就绪（`docs/macos-updates.md`），但对外更新通道默认关闭：没有 feed URL
和公钥的构建不会检查更新。启用需要维护者同意并完成：

- [ ] 在可信的机器上生成签名密钥：`anixops-appcast keygen --out update-signing.key`，私钥存进 anx 仓库 Environment
  `apple-anx` 的 secret `MACOS_UPDATE_SIGNING_KEY`，公钥存为 variable `MACOS_UPDATE_PUBLIC_KEYS`；本机副本离线保存后删除。
- [ ] 决定 feed 和安装包放在哪个 HTTPS 地址（官网或对象存储），存为 variable `MACOS_UPDATE_FEED_URL`。
- [ ] 让「macOS with ANX」workflow 出带签名 `.pkg` 的版本（需要 Developer ID Installer 证书），并生成 `appcast.json`。
- [ ] 上传第一份 `appcast.json` 与安装包，真机验收：
  - [ ] 旧版本在「设置 › 软件更新」点「检查更新」，看到新版本和说明；下载后「安装并退出」打开 Installer，装完版本号更新。
  - [ ] 把服务器上的 `appcast.json` 改一个字节，检查更新提示「更新信息未通过签名校验，已忽略」。
  - [ ] 把服务器上的 `.pkg` 换成别的文件，下载后提示「更新安装包未通过校验，已删除」，下载目录里没有留下文件。

## 客户端可观测性真机验收

见 [`docs/architecture/client-health.md`](architecture/client-health.md)。Linux 的冷启动、指标端点与完整性由 CI 验证，其余需真机：

- [ ] Windows：连接后浏览器打开 `http://127.0.0.1:9477/metrics`，有 `networkcore_upload_bytes_total` 与 `anixops_client_integrity{state="verified"} 1`（MSI 安装版）。
- [ ] Windows：设置 › 诊断与隐私 显示「安装完整性：完整」；改动安装目录里的 `wintun.dll` 后重启客户端显示「文件缺失或被改动」。
- [ ] 关闭「崩溃报告」后 `crashes` 目录被删除；重新打开后开关状态在重启后保持。
- [ ] iOS / macOS：设置 › 隐私 有「崩溃报告」开关；开发者模式（macOS 为活动页）复制的诊断信息里有 `install integrity: verified` 与 `metrics:` 段，`metrics:` 段里没有节点名、策略组名。
- [ ] iOS：发生过崩溃的设备次日打开 App 后，诊断信息里出现 `kind: metrickit crash diagnostic` 的报告，且没有网址与 IP。

## 稳定性与自愈真机验收（S3 / S4 / S6）

客户端打开“设置 › 连接 › 自动重连”（默认开）后，VPN 配置带 On Demand 规则；内核 `reset_network` 在切网和唤醒时重建出站连接。
CI 只能跑规则与重建逻辑的单元测试（`OnDemandSettingsTests`、`NetworkRecoveryTests`，内核
`reset_network_twenty_times_leaks_nothing`），以下需要真机：

- [ ] S3 崩溃自愈（iOS、macOS）：连接后强杀扩展（iOS 用 Xcode Debug › Attach 后 kill，或 `kill -9` macOS 的
  `com.example.networkcore.macos.packettunnel` 进程），记录到重新显示“已连接”的时间，目标 ≤ 5 s；手动断开后不应自己连上。
- [ ] S4 唤醒/切网（iOS）：Wi-Fi ↔ 蜂窝、两个 Wi-Fi 之间各切 20 次，每次切换后网页和 Hysteria2/TUIC 节点都能在几秒内恢复；
  开发者模式的事件里每次有一条 `network change: reset … sessions, closed … connections`，扩展内存（诊断里的
  footprint）20 次后不持续增长。锁屏超过 1 分钟再解锁，应有 `wake: … closed …`。
- [ ] S6 SSID 规则（iOS、macOS）：把家里 Wi-Fi 设为“断开”，加入该 Wi-Fi 后隧道在 300 ms 内断开（用系统 VPN 状态栏图标或
  `log stream --predicate 'subsystem == "com.apple.networkextension"'` 的时间戳计时），离开后自动连接。
- [ ] S4 桌面（Windows、Linux，本机代理模式）：拔插网线或换 Wi-Fi、睡眠 2 分钟后唤醒，网页几秒内恢复；Windows 日志
  （`tauri-gui`）里有 `network change: engine reset {...}` / `wake: engine reset {...}`。TUN 模式这一项不覆盖。

## macOS 命令行工具

`anixops` 的解析、退出码和套接字通道有单元测试，CI 还在构建好的 App 里运行工具检查帮助、版本和退出码
（`docs/macos-cli.md`）。签名后的真实行为只能真机验收（macOS 14+，Developer ID 版）：

- [ ] 「macOS with ANX」workflow 给 `Contents/MacOS/anixops` 用 Developer ID 签名并开 Hardened Runtime，公证通过。
- [ ] App 运行时 `anixops status` 显示状态；macOS 15 上不弹「想要访问其他 App 的数据」。
- [ ] App 未运行时 `anixops connect` 在后台打开 App 并连上；`anixops status -q; echo $?` 未连接为 3、已连接为 0。
- [ ] `anixops mode rule`、`anixops select <节点>`、`anixops test`、`anixops import <订阅链接>` 与窗口里同样的操作效果一致，窗口实时更新。
- [ ] 另一个用户（或 `sudo -u nobody`）运行 `anixops status` 连不上（退出码 69 或 71）。

## 安卓 APK 签名与真机验收

CI 的 `android-native` job 每次都产出原生版 arm64 APK（artifact `anixops-net-android-<sha>`）。没有签名密钥时用临时调试密钥签名，
每次构建的密钥都不同，装新版前要先卸载旧版。

- [ ] 生成正式签名密钥（只做一次，丢了就无法给已安装的用户升级）：
  `keytool -genkeypair -v -keystore anixops-net.jks -alias anixops-net -keyalg RSA -keysize 4096 -validity 10000`，
  把 `base64 -w0 anixops-net.jks` 的结果和密码写入仓库 secrets：`ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、
  `ANDROID_KEY_ALIAS`、`ANDROID_KEY_PASSWORD`。配置后 CI 自动改用它签名（文件名里的 `debug` 变成 `release`）。
- [ ] 发版时 `release.yml` 会把 APK 作为 `anixops-net-<版本>-android-arm64.apk` 放进 GitHub Release。配置正式密钥前，
  Release 说明会写明「临时签名，装新版前要先卸载旧版」。
- [ ] 真机验收：安装 APK，首次启动同意用户协议，导入订阅，点连接时允许 VPN，浏览器能打开网页，断开后网络恢复。
- [ ] 原生版另验：快捷设置里加“AnixOps”磁贴，点它连接 / 断开；系统设置 › VPN 里设为“始终开启”，重启手机后自动连上；
  浏览器打开 `anixops://toggle` 能切换连接；另一个 VPN 接管后 App 与磁贴显示已断开。
- [ ] targetSdk 升到 36 后（Google Play 要求）重新做上面两项真机验收，重点看 VPN 连接、前台通知、磁贴和始终开启。

## Google Play

Play 版是开源版（用户 2026-11-15 决定），包名 `io.anixops.net.android`。

- [x] Environment `google-play`（需维护者审批）里配了 `PLAY_SERVICE_ACCOUNT_JSON`、`PLAY_UPLOAD_KEYSTORE_BASE64`、
  `PLAY_UPLOAD_KEYSTORE_PASSWORD`、`PLAY_UPLOAD_KEY_ALIAS`、`PLAY_UPLOAD_KEY_PASSWORD`。
- [ ] 手动运行 `play-bundle.yml`（Actions → Google Play bundle → Run workflow），选轨道（默认 `alpha` 封闭测试，`none`
  只打包不上传）并可填英文更新说明。审批后它用上传密钥签好 AAB，以**草稿**放进所选轨道并以“不送审”提交；送审由维护者在
  Play Console → 发布概览里点。AAB 也保留在 artifact `anixops-net-play-<versionCode>-<sha>` 里。`google-play` 只允许 `v*`
  tag 时，从 main 运行要先在 Environment 的 Deployment branches 里临时加上 `main`。
- [ ] 每次上传的 versionCode 必须比上一次大（`apps/android-native/app/build.gradle.kts`）。
- [ ] Play Console 里的应用签名、商店信息、隐私政策、数据安全、内容分级、VPN 和前台服务声明只能人工填写。
- [ ] VPN 服务和前台服务声明要附 90 秒内的演示视频：手动运行 `play-demo-video.yml`（Actions → Play demo video → Run
  workflow），它在模拟器上跑真实的调试版 APK 并加上英文字幕；下载 artifact 里的 `anixops-net-play-demo.mp4`，由维护者
  上传到 YouTube（不公开列出）或云盘，再把链接填进两项声明。
  字幕时间取自脚本的计时，和录像可能差两三秒；上传前看一遍，必要时用 artifact 里的 `raw.mp4` 和 `captions.tsv` 调整后重新生成。
- [ ] 服务账号 `play-publisher` 要在 Play Console → 用户和权限里受邀，只给测试轨道的发布权限；上传报 401/403 时先查这一项。
  正式版由维护者在 Play Console 推广。


## 开源版下载页（GitHub Pages）

2026-11-14 用户决定客户端分两版发布：开源版（不含 ANX）放本仓库 GitHub Release，ANX 版只在服务商面板里下载。
开源版的固定下载页由 `download-page.yml` 生成并部署到 GitHub Pages：`https://anixops.github.io/anixops-net/`，
另有固定直链 `/windows/`、`/windows-portable/`、`/linux-deb/`、`/linux/`、`/android/`（跳转到最新版文件）和
`latest.json`。`release.yml` 发布 Release 后会自动触发它。

- [ ] 维护者在 Settings → Pages 把 Source 设为「GitHub Actions」（只做一次；没设之前 `Deploy to GitHub Pages` job 会失败）。
  设好后手动运行一次 `Download page` workflow。
- [ ] 打开下载页，确认各平台按钮指向最新版，`/windows/` 等固定链接能跳转。


## 手机连电脑抓包（真机验收）

自动测试只覆盖了回环地址上的部分：设置页、证书下载、转发与连接标签。下面几项需要用真机验收：

- [ ] macOS：连接后打开「扩展 → 手机抓包」，iPhone 扫码，按设置页的步骤设代理并安装、信任描述文件，`http://cert.anixops/` 显示“代理已生效”，请求页能看到 `LAN <iPhone 地址>` 的连接，HTTPS 内容能解密。
- [ ] Windows（桌面模式）与 Linux：同样的步骤，再用一台 Android 手机安装 `.crt` 验收一遍。
- [ ] 第一次开启时，macOS 和 Windows 可能弹出防火墙提示，需要允许传入连接。


## 抓包会话加密保存（Tauri，真机验收）

CI 只覆盖编译和会话请求、清理、密钥文件的单测；系统密钥库要在真机上确认：

- [ ] Windows（桌面模式）：连接、开始抓包、在“请求”页保存会话；`%LOCALAPPDATA%\AnixOps Net\capture\` 下有 `session-key.dpapi` 和 `sessions\*.anxcap`；重启客户端后能载入；换一个 Windows 账户复制这两个文件过去打不开。
- [ ] Linux（GNOME 或 KDE）：保存会话后 `secret-tool lookup application anixops-net purpose capture-session-key` 能查到，`capture/` 下只有 `session-key.secret-service` 标记、没有 `session-key` 文件；锁住钥匙环再列出会话时提示打开钥匙环，而不是换新密钥。
- [ ] Linux（没有钥匙环的桌面）：`capture/session-key` 权限是 `0600`，界面说明写的是“密钥保存在只有你能读取的文件里”。
- [ ] Android：保存、退出 App、重新连接后能载入；清除 App 数据后旧会话随之删除。

## 自定义 DNS 与 Fake-IP（Tauri，真机验收）

CI 覆盖文本解析、`dns` 段生成和引擎校验；解析结果与 Fake-IP 要在真机上看：

- [ ] Windows（TUN 模式）：分流页 DNS 选“自定义 DNS”，填一个 `direct` 服务器和一条 `DOMAIN-SUFFIX`，保存后连接；`nslookup` 该域名走到填的服务器（看节点日志或抓包）。打开 Fake-IP 再连接，`nslookup example.com` 返回 `198.18.x.x`，浏览器正常打开，`nslookup router.lan` 之类局域网名仍是真实地址。
- [ ] Windows（系统代理模式）：Fake-IP 打开时浏览器照常上网（Fake-IP 只在 TUN 模式下起作用）。
- [ ] Android：同上两项，在 VPN 里确认 Fake-IP 返回 `198.18.x.x`，微信、推送等不受影响。
- [ ] Linux：没有 Fake-IP 开关；自定义 DNS 保存后正在运行的引擎重启，Hosts 里写的名字按填的地址连接。

## Windows 删除服务模式后真机验收（W1）

- [ ] 在 beta.6 上选「Windows 服务」运行方式并连接，然后用 MSI 升级：升级后 `sc query AnixOpsNetworkCore` 报不存在，
  客户端以窗口进程连接，系统代理和 HTTPS 解密正常。
- [ ] 便携版：旧便携版注册过服务的机器上启动新版，`gui.log` 有删除旧服务的记录，服务不存在，之前由服务设置的系统代理已恢复。
- [ ] MSI 卸载后根证书（NetworkCore 根证书和 HTTPS 解密根证书）和驱动都被删除；`%ProgramData%\AnixOps\NetworkCore\logs\gui.log`
  有 `purged` 记录。卸载过程中不弹窗。

## 节点编辑与分享真机验收（T4 / T5）

- [ ] Windows、Linux、Android：节点卡片“更多”菜单复制分享链接，粘贴到另一台设备能导入同一节点。Android 上不需要长按或悬停就能看到菜单按钮。
- [ ] 编辑一个节点的名称、服务器、端口后，连接中的内核自动重启，流量走新的服务器；重启客户端后改动仍在。
- [ ] 删除一个没有被策略组用到的节点后列表少了它；更新订阅后编辑和删除都恢复为订阅里的内容。
- [ ] Windows 上从 beta.6 升级后，第一次编辑节点会提示先更新一次订阅（旧版本没有保存订阅内容），更新后可以编辑。

## 插件从文件 / 粘贴导入真机验收（T7）

- [ ] Windows、Linux：扩展页插件卡片“更多导入方式 → 从文件导入”选一个 `.plugin` / `.sgmodule` 文件，插件出现在列表里，脚本已下载，开启 HTTPS 解密后生效。
- [ ] Android：同样从文件导入（系统文件选择器）和粘贴插件内容都能导入；插件行的“更多”按钮不用长按就能看到。
- [ ] 插件行“更多 → 查看内容”显示解密域名、脚本地址和逐行报告；再次导入同名插件替换原来那条，不会出现两条。

## 流量统计真机验收（T9）

- [ ] Windows、Linux、Android：连接后浏览几分钟，活动页“流量统计”今天的柱状图和总量增长，按主机、按策略列出访问过的站点和策略组。
- [ ] 退出并重新打开客户端后统计仍在；“更多 → 清空统计”确认后清空。

## 日志查看真机验收（T10）

- [ ] Windows、Linux、Android：连接、断开、更新订阅后，活动页“日志”列出对应记录；订阅地址只显示到路径，不含 token 等查询参数。
- [ ] 级别切到“错误”只剩出错的行；Windows 有多份日志时能按模块筛选；“更多 → 拷贝筛选出的日志”能粘贴出文本，“清空日志”确认后清空。

## Android 诊断报告与出口地址真机验收（T8）

- [ ] 连接后概览页显示出口地址和位置，与浏览器打开 `https://www.cloudflare.com/cdn-cgi/trace` 看到的一致。
- [ ] 活动页“生成诊断报告”后提示保存到 `Download/anixops-net-diagnostics-<时间>.log`，文件管理器里能打开，是纯文本，末尾有应用日志。

## 系统通知真机验收（T11）

- [ ] Windows（MSI 安装后从开始菜单打开）、Linux（GNOME / KDE）、Android 13+（首次连接时允许通知）：设置页“通知”三个开关默认打开。
- [ ] 装一个会调用 `$notification.post` 的插件脚本，命中后系统通知中心出现脚本标题和内容。
- [ ] 连接后结束内核或在 Android 系统设置里断开 VPN，几秒内出现“连接已断开”；Windows 打开“内核意外退出时自动恢复一次”时文字是“正在重新连接”。
- [ ] 把订阅地址改成打不开的地址并等到自动更新，出现“订阅更新失败”。关掉对应开关后不再出现；切换界面语言后通知文字跟着变。

## `anixops://` 链接真机验收（T13）

- [ ] Windows、Linux（GNOME / KDE）、Android：启动过一次客户端后，在浏览器地址栏或终端（`xdg-open` / `start`）打开 `anixops://connect`、`anixops://disconnect`、`anixops://toggle`，客户端窗口来到前台并执行。
- [ ] `anixops://mode?set=direct`、`anixops://select?node=<节点名>`、`anixops://select?policy=<策略组>&node=<成员>` 生效；`anixops://import?url=<订阅链接>` 打开代理页的订阅、链接已填好，点“导入”才保存。
- [ ] 客户端没运行时打开链接：启动客户端并执行（Windows / Linux 只有一个窗口，不会多开）。

## Wi-Fi 规则真机验收（T12）

- [ ] Windows：设置页打开“Wi-Fi 规则”，点“当前网络”填入 Wi-Fi 名称（Windows 11 先在“隐私和安全性 > 位置”允许桌面应用），设为“断开”；连上后切到这个 Wi-Fi，十几秒内断开，换到别的 Wi-Fi 或有线后按规则连接。
- [ ] Android：打开规则时系统询问位置权限；允许后“当前网络”能填入 Wi-Fi 名称。Wi-Fi 和移动网络来回切换时按规则连接 / 断开（App 在后台也生效），拒绝权限时“其他 Wi-Fi”规则仍生效。
- [ ] 手动连接或断开后，在同一个网络里不会被规则改回，换网络后才按规则执行。

## 按网络选择策略组（subnet 组）真机验收

客户端在网络变化时（以及每次隧道启动后）向内核报告当前网络（`set_network`）：iOS / macOS 报告类型（Wi-Fi / 有线 / 蜂窝）、路由器和接口地址；Windows / Linux / Android 报告本机出口地址，加上“Wi-Fi 规则”能读到的类型与 Wi-Fi 名称（Windows 11 与 Android 需要位置权限）。TUN 模式下出口地址是隧道自己的地址，被忽略。

- [ ] 配一个 `subnet` 组（如 `SUBNET:192.168.1.0/24` 选直连，默认选代理）：在该网段时连接走直连，切到蜂窝或别的网段后几秒内新连接走代理；连接列表里的策略组成员随之变化。
- [ ] Windows / Android 允许位置权限后，`SSID:<名称>` 条件生效。
- [ ] iOS / macOS 的 `SSID:` / `BSSID:` 条件需要 Wi-Fi 名称：要在 Apple 开发者后台给 App ID 开 “Access Wi-Fi Information” 能力并在网络扩展里用 `NEHotspotNetwork.fetchCurrent` 读取。这属于 Apple 账号配置，等维护者决定后再做；在此之前 iOS / macOS 只能用 `SUBNET:`、`ROUTER:`、`TYPE:` 条件。

## Android 扫码导入真机验收（T5）

- [ ] 代理页“导入订阅”点“扫码”：第一次询问相机权限；允许后出现取景框，扫订阅链接或 `vless://` 等分享链接的二维码，链接填进输入框，点“导入”后节点出现。
- [ ] 扫码时点“取消”回到页面；拒绝相机权限时提示去系统设置打开。

## 流量重定向真机验收（T6）

- [ ] Windows 打开 TUN 模式、Android 连接 VPN 后，在“流量重定向”新建一条（例如游戏服务器 IP → 开发服务器 `IP:端口`，协议选 UDP），游戏或 `nc -u` 的流量到达开发服务器，UDP 回包正常。
- [ ] 域名写法（`game.example.com`）匹配子域名；关掉开关或删除后恢复原地址；连接列表里能看到改发后的地址。

## Linux TUN 模式真机验收（T14）

- [ ] 装 `.deb`（Ubuntu 22.04+ 或 Debian 12，内核 5.7+），设置页“运行方式”打开 TUN 模式后连接：弹出 polkit 授权对话框，说明写的是“创建 AnixOps Net 虚拟网卡”；授权后 `ip addr` 能看到 `anxtun0`（198.18.0.1/24），`ps` 里没有以 root 运行的 anixops-net。
- [ ] 不走系统代理的程序（`curl --noproxy '*' https://ip.sb`、游戏、`nc -u`）也经过节点；流量重定向对它们生效；断开或退出后 `anxtun0` 和两条半默认路由消失，网络恢复。
- [ ] 授权对话框点“取消”时提示“TUN mode was not allowed”，不会留下网卡；关闭 TUN 模式后连接回到系统代理方式。
- [ ] tarball 版本（未装 polkit 规则）用通用的“以其他用户运行程序”授权，同样能连上。

## 苹果平台接管方式真机验收（`docs/system-proxy.md`）

- [ ] iOS 默认“VPN + 系统代理”连接后，设置 > 无线局域网 > 当前网络 > 配置代理 不需要改动；Safari 打开网页，连接列表里能看到按域名（`CONNECT` 目标）走规则的连接；`URL-REGEX` 规则对明文 HTTP 生效。
- [ ] iOS 同样模式下，不走系统代理的 App（游戏、`ping`、UDP）仍然经过 VPN；诊断事件里有 “system proxy on port 8890”。
- [ ] iOS 切到“仅 VPN”后自动重连一次，诊断事件里没有 system proxy，行为与之前版本一致。
- [ ] macOS 三种模式都能连接：“仅系统代理”时 系统设置 > 网络 > 当前网络 > 代理 显示网页代理和安全网页代理为 127.0.0.1:8890（或诊断事件里的端口），`netstat -rn` 没有指向 utun 的默认路由；`curl -x '' https://ip.sb`（不走代理）直接出去。断开后系统代理恢复原样。
- [ ] macOS 有 `PROCESS-NAME` 规则时，经系统代理的连接在连接列表里显示进程名，规则生效。
- [ ] 编辑“跳过代理的地址”加入 `*.example.com` 后重连，访问它不经过代理（连接列表里看不到经代理的连接；“VPN + 系统代理”下仍经 VPN）。
- [ ] 开着 HTTPS 解密时，经系统代理的 HTTPS 也被解密，抓包和改写与 VPN 路径一致。
- [ ] macOS 网关模式：打开后重连，诊断事件里是 “gateway: ip forwarding on” 还是 “may not turn it on”（沙盒里的系统扩展能否改 sysctl 待真机确认）；后一种情况按设置页命令手动开启，状态变成“已开启”。
- [ ] 另一台设备（手机或游戏机）把路由器设为 Mac 的局域网地址、DNS 设为 198.18.0.2：能上网，Mac 的连接列表里出现它的连接，规则、MITM 生效；断开 VPN 后 `sysctl net.inet.ip.forwarding` 回到 0（如果是隧道打开的）。

## Windows / Linux 原生界面（`docs/architecture/desktop-native.md`）

- [ ] Windows 代码签名证书：发布物现在没有 Authenticode 签名。原生版把内核放进系统服务，未签名的服务更容易被安全软件
  拦截，安装时也有 SmartScreen 警告。正式切换到原生版之前需要维护者申请证书，并把证书或托管签名服务的凭据配置成仓库
  secrets。不阻塞开发。
- [ ] 原生预览版真机验收（预览版发布后）：Windows 11 上检查贴靠布局、125% 和 150% 缩放、中文输入法、讲述人朗读；
  Linux 上在 GNOME 和 KDE 各检查一次窗口、托盘、深浅色和触控板滚动。
- [ ] Windows 请求列表的滚动手感（原生预览版发布后）：在请求页放入几万条请求，用触控板惯性滚动、鼠标滚轮快速滚动、
  拖动滚动条各试一次，确认没有空白行和明显掉帧。CI 只能量程序自己发起的移动（见架构文档“过关标准”第 1 项的量法），
  量不了合成线程上的惯性滚动。
- [ ] Windows 旧版数据搬运的真机核对（原生预览版发布后、正式切换之前）：在一台用过 Tauri 版的 Windows 机器上装原生预览版，
  核对订阅、选中的节点、分流模式、DNS、我的规则、插件和解密根证书都在，旧版程序仍能照常启动。CI 里的夹具是照着旧版的
  结构体写的，不是真机上取的文件，所以要这一步。
- [ ] Windows 原生版 MSI 的真机核对（预览版发布后）：CI 的 runner 是管理员账号，测不到普通用户。在 Windows 10 和 11 上
  用标准用户账号（非管理员）登录，由管理员装好 MSI 后：窗口启动不弹 UAC，标题栏显示“核心服务 <版本>”；连接后浏览器走
  代理，断开后系统代理恢复；另一个用户同时登录时各自的系统代理互不影响；升级和卸载时窗口被关闭，卸载后“服务”里没有
  AnixOps Net Core，`Program Files\AnixOps\Net` 不存在。未签名的安装包会有 SmartScreen 警告，见上面的证书一项。
- [ ] Windows 原生版 TUN 模式的真机核对（设置页有开关之后）：标准用户账号下打开 TUN 模式并连接，不弹 UAC；“网络连接”里
  出现 AnixOps Net 网卡，不走系统代理的程序（命令行的 `curl`、游戏）也经节点出去；断开后网卡消失、网络正常；睡眠唤醒和
  切换 Wi-Fi 后仍然可用；卸载后网卡和 Wintun 驱动都不在（设备管理器、`pnputil /enum-drivers`）。CI 只测到分流模式为直连、
  管理员账号的情况。Tauri 版和原生版用同一个网卡名，两个版本不要同时开 TUN 模式。
- [ ] Windows 原生版 HTTPS 解密根证书的真机核对（扩展页有“信任根证书”之后）：标准用户账号下点信任，Windows 弹安全警告，
  选“是”后 Edge / Chrome 访问被解密的网站不报证书错误，选“否”时界面说明没有信任；同一台电脑的另一个用户不受影响
  （`certmgr.msc` 里没有这张证书）；卸载后当前用户的“受信任的根证书颁发机构”里没有 AnixOps Net HTTPS Decryption Root。
  CI 测到了证书库和证书链验证，没测浏览器。
- [ ] Linux 守护进程单元的真机核对（deb / rpm 带上 `anixops-netd` 之后）：在 Ubuntu 和 Fedora 上 `systemctl start
  anixops-netd`，确认单元里的收紧项（`ProtectSystem=strict`、`ProtectHome=yes`、`CapabilityBoundingSet`、
  `RestrictAddressFamilies`）下连接、TUN 模式、HTTPS 解密都正常；用普通登录用户能控制，用没有登录会话的账号
  （`sudo -u nobody`）连接时得到拒绝的说明。CI 的 runner 上没有 systemd 会话，测不到这一层。
- [ ] Linux 原生版骨架的真机核对（预览版发布后）：CI 只在 Xvfb（X11、Cairo 渲染）里跑。在 GNOME（Wayland）和 KDE
  上各看一遍：窗口拖窄后折成一栏并能返回侧栏；系统切换深浅色时窗口跟着变；强调色是品牌色；系统开高对比度时回到
  系统配色；Orca 能读出侧栏的六个页面；125% / 200% 缩放下文字清晰。

## macOS 自有根证书与指标端口真机验收（A2、A3）

- [ ] 用 `openssl ecparam -name prime256v1 -genkey -noout -out ca.key && openssl req -x509 -new -key ca.key -subj /CN=Team -days 365 -addext basicConstraints=critical,CA:true -out ca.pem` 生成团队 CA；扩展页“使用自己的根证书…”粘贴两段 PEM，替换后名称变为 Team，信任后重新连接，Safari 打开解密的站点证书链指向 Team。
- [ ] 粘贴 RSA 私钥、别的证书的私钥、非 CA 证书时分别给出对应提示，原证书不变。
- [ ] 设置页打开“Prometheus 指标”，`curl http://127.0.0.1:9477/metrics` 断开时只有 `anixops_client_*`，连接后加上内核指标；`curl -X POST` 返回 405；关掉开关后端口关闭。

## v0.3.0-beta.7 发布前后的人工事项

发布说明在 `docs/release-notes/v0.3.0-beta.7.md`。`release.yml` 在打包前检查：发布说明存在且没有占位符，
安卓 versionCode 高于上一个发布 tag 里的值（本版 3002；3001 已用于 2026-11-16 的 Google Play 内部测试包）。

发布前（维护者）：

- [ ] 仓库级 secrets `ANDROID_KEYSTORE_BASE64` / `ANDROID_KEYSTORE_PASSWORD` / `ANDROID_KEY_ALIAS` / `ANDROID_KEY_PASSWORD`
  （Google Play 应用签名密钥，别名 `anixops`）。没有设置时 APK 用每次构建临时生成的密钥签名，以后的版本无法覆盖安装，
  发布说明会提示先卸载。2026-11-16 main 上的 CI 显示这组 secrets 仍为空。
- [ ] 推送 tag `v0.3.0-beta.7`（云端会话推 tag 会被拒，维护者在本机推）。
- [ ] anx 仓库的 Cargo.lock 内核与本仓库 `Cargo.toml` 固定的内核一致后，再触发 anx 的 iOS / macOS / Windows / Linux / Android
  ANX 构建和 `publish-client.yml`。

发布后真机验收（CI 只有单元测试、模拟器冒烟测试和截图对比）：

- [ ] Linux TUN 模式：Ubuntu 24.04 / Debian 13 上打开 TUN，polkit 询问一次；`ip route` 有接管路由，不走系统代理的程序
  （`curl --noproxy '*'`）经过代理；关闭或断开后网卡和路由消失。
- [ ] 流量重定向：桌面和安卓各建一条重定向，命中的请求发往目标地址，检查器里能看到重定向标记。
- [ ] 安卓快捷设置磁贴：添加磁贴，点按连接 / 断开，状态与 App 内一致。
- [ ] 安卓“始终开启的 VPN”：系统设置里选 AnixOps Net 并开启，重启手机后自动连接；在 App 内断开时系统不会立刻拉起。
- [ ] 安卓 `anixops://` 链接与扫码导入：浏览器点链接能拉起 App 并导入订阅；扫码导入节点和订阅。
- [ ] 从 beta.6 升级的桌面：原来选旧版预设分流模式的变成规则模式；原来选 DNS 国内外分流的变成等价的自定义 DNS；
  Windows 旧服务被卸载。

2026-11-16：beta.7 的第一次 tag 发布在 “Publish GitHub Release” 失败。配好 `ANDROID_KEY_ALIAS`（`anixops`）后，GitHub 把含有
secret 值的 job output（安卓 artifact 名和 APK 名都含 `anixops`）丢弃，发布步骤拿不到安卓包。修复后 release.yml 由版本号直接推出这两个名字。
tag 当时指向的 d225b10 还是旧 workflow，需要把 tag 移到包含修复的 commit 再推（Release 没有发出，移动 tag 不影响用户）。

## v0.3.0-beta.11 发布前后的人工事项

发布说明在 `docs/release-notes/v0.3.0-beta.11.md`，安卓 versionCode 3007（beta.10 是 3005；3006 已于 2026-11-22 上传到 Google Play 测试轨道的草稿）。
tag 由 Actions 的 “Release tag”（`release-tag.yml`）打，流程同 beta.10。内核仍是 networkcore cce9d63，anx 的 Cargo.lock 已一致。
本版不触发 macOS TestFlight（Mac App Store）上传，macOS 仍发 Developer ID 版。

- [ ] anx 的 ANX 构建和 `publish-client.yml` 进 Environment `apple-anx` / `anx-release`，需要维护者在 Actions 页面批准。
- [ ] Linux 的 ANX 构建只能在带 WebKitGTK 的自托管 runner（标签 `9700x`）上跑，发布时需要它在线。

发布后真机验收（CI 只有单元测试、模拟器冒烟测试和截图对比）：

- [ ] 新 App 图标在五个平台的桌面 / 启动器、任务栏 / Dock、设置里都显示正确；从 beta.10 覆盖安装后图标更新。
- [ ] Linux：开启系统代理再断开，GNOME / KDE 的代理设置恢复为接管前的值（含原本手动设过的项）。
- [ ] Windows：刷新订阅时正在编辑的节点不丢；刷新失败后编辑内容仍在。
- [ ] iOS / macOS 深色模式：导入、编辑弹出页与窗口有层次区分；禁用按钮为中性灰。
- [ ] 安卓：连接时快速断网 / 恢复数次，VPN 状态正确，不残留连接。

## v0.3.0-beta.10 发布前后的人工事项

发布说明在 `docs/release-notes/v0.3.0-beta.10.md`，安卓 versionCode 3005（beta.9 是 3004）。tag 由 Actions 的 “Release tag”（`release-tag.yml`）打，流程同 beta.9。

- [ ] anx 仓库的 Cargo.lock 内核与本仓库 `Cargo.toml` 固定的内核一致后，再触发 anx 的 ANX 构建和 `publish-client.yml`；
  这些 run 进 Environment `apple-anx` / `anx-release`，需要维护者在 Actions 页面批准。
- [ ] Linux 的 ANX 构建只能在带 WebKitGTK 的自托管 runner（标签 `9700x`）上跑，发布时需要它在线。

发布后真机验收（CI 只有单元测试、模拟器冒烟测试和截图对比，节点体检没有接真实节点）：

- [ ] 节点体检在五个平台各对一个真实节点跑一次：只测延迟、勾选测下载速度各一次，结果合理，下载不超过 200 MB。
- [ ] iOS / macOS 未连接隧道时节点体检也能完成；连接时不影响正在用的连接。
- [ ] 节点不通时显示失败和丢包，不卡住界面。

## v0.3.0-beta.9 发布前后的人工事项

发布说明在 `docs/release-notes/v0.3.0-beta.9.md`，安卓 versionCode 3004（beta.8 是 3003）。tag 由 Actions 的 “Release tag”（`release-tag.yml`）打，流程同 beta.8。

- [ ] anx 仓库的 Cargo.lock 内核与本仓库 `Cargo.toml` 固定的内核（4870006）一致后，再触发 anx 的 ANX 构建和 `publish-client.yml`；
  这些 run 进 Environment `apple-anx` / `anx-release`，需要维护者在 Actions 页面批准。
- [ ] 原生 Windows / Linux 窗口的连接列表（#212）还没有随安装包发布：桌面安装包仍是 Tauri 版。

发布后真机验收（CI 只有单元测试、模拟器冒烟测试和截图对比）：

- [ ] ANX 版：两个都开启设备授权的面板，各自的 ANX 节点都能连接；从 beta.8 升级后最近添加的面板不需要重新授权。
- [ ] macOS 全新安装后不自动检查更新；手动检查正常。
- [ ] 规则测试与命中次数在五个平台各试一次；按 URL 添加一份 `geoip-<国家>` 后对应规则生效。
- [ ] iOS `CELLULAR-RADIO` 在蜂窝网络下命中；macOS 网关模式下 `DEVICE-NAME` 按设备名命中。
- [ ] 解密域名：通配符、排除、停用在 iOS / macOS / Windows / 安卓各验一次；证书固定的 App 出现在“解密失败”并可一键排除。
- [ ] iOS / macOS 抓包：未解密连接列表显示，断开隧道后抓包停止且会话已保存。

## v0.3.0-beta.8 发布前后的人工事项

发布说明在 `docs/release-notes/v0.3.0-beta.8.md`，安卓 versionCode 3003（beta.7 是 3002）。

从这一版起 tag 由 GitHub Actions 打：在 Actions 里运行 “Release tag”（`release-tag.yml`），输入版本号（可选 commit，默认 main 最新）。
它先检查版本号、commit 在 main 上、tag 不存在、发布说明存在且无占位符、同一 commit 在 main 上的 CI 已成功，再打 tag，
然后在该 tag 上以 workflow_dispatch 启动 `release.yml`（GITHUB_TOKEN 推的 tag 不触发 push 事件）。维护者不再需要在本机推 tag。

- [ ] anx 仓库的 Cargo.lock 内核与本仓库 `Cargo.toml` 固定的内核（becc3f6）一致后，再触发 anx 的 ANX 构建和 `publish-client.yml`；
  这些 run 进 Environment `apple-anx` / `anx-release`，需要维护者在 Actions 页面批准。
- [ ] 原生 Windows / Linux 窗口的连接列表（#212）还没有随安装包发布：桌面安装包仍是 Tauri 版。

发布后真机验收（CI 只有单元测试、模拟器冒烟测试和截图对比）：

- [ ] iOS / macOS “VPN + 系统代理”与 macOS “仅系统代理”、网关模式按 `docs/system-proxy.md` 验收（见上文接管方式一节）。
- [ ] DNS：`quic://` 上游、4 个默认服务器竞速、备用服务器、`system` 在 Windows / Linux / 安卓 / iOS 上各查一次。
- [ ] 策略组：拔掉自动测速组当前节点的网络，下一条连接换到其他节点；切换 Wi-Fi 后测速结果清空重测。
- [ ] REJECT-TINYGIF 对明文 HTTP 回 1x1 GIF；同一主机 30 秒内被拒超过 10 次后改为静默丢弃。
- [ ] iOS ANX 版：开启设备授权的订阅里 ANX 节点能连接（不再出现 code 6）。
