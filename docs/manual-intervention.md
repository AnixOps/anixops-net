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
- 服务模式下开启 HTTPS 解密：系统代理指向 `127.0.0.1:7890`，内置引擎监听 `127.0.0.1:7891`，浏览器经解密访问 HTTPS 站点正常；关闭解密后系统代理回到 `127.0.0.1:7890` 的内置引擎，且 7891 不再监听。
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
- 导入两个订阅链接后，代理页出现两个订阅组，各自显示节点数、更新时间；机场返回 `subscription-userinfo` 时显示已用/总流量和到期日。
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

- [ ] 设置选「桌面模式」，扩展页开启 HTTPS 解密，连接成功；浏览器访问 HTTPS 站点证书颁发者为 NetworkCore 根证书，插件改写生效。
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
