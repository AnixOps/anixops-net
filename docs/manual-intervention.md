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
ios-export-compliance-answer=pending-reanswer-standard-algorithms-non-exempt
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
- 后台刷新（进程已被系统回收）：间隔设为 1 小时后打开很多别的 App（或等一夜）让系统回收 AnixOps Net 的进程，不是从多任务里上划关闭；
  之后不打开 App，用 Xcode 的 `e -l objc -- (void)[[BGTaskScheduler sharedScheduler] _simulateLaunchForTaskWithIdentifier:@"net.anixops.subscriptions.refresh"]`
  或等系统调度。再打开 App 时更新时间已变化，后台刷新请求仍在（下一次照常触发）。
- 连接中导入新节点或新订阅后自动重连：导入完成页写“正在重新连接”，首页、小组件和实时活动的节点与实际出口一致。
- 连接中下拉更新、且当前节点被服务商删除后，自动重连到新的默认节点；内容没变的更新不会断开连接。
- 后台刷新换了当前节点时不断开连接；下次打开 App 回到前台时重连一次，之后首页节点与实际出口一致。
- 重启手机后、首次输入锁屏密码前让系统触发后台刷新（上面的 `_simulateLaunchForTaskWithIdentifier`，此时钥匙串还读不到订阅）；
  输入锁屏密码后打开 App：首页和代理页照常列出全部订阅分组，不出现欢迎页；此时导入一个新订阅，原有分组都还在，没有被新分组替换。
  读不到时导入或更新会提示“还没读到已保存的订阅，请先输入锁屏密码，再重试。”，不改动已保存的分组。
- 同一进程内的后台重试：重启手机后、首次输入锁屏密码前触发一次后台刷新（同上），不让系统回收进程（不打开别的大量 App）；输入锁屏密码但不打开
  AnixOps Net，再用同一条 `_simulateLaunchForTaskWithIdentifier` 触发一次（模拟系统安排的重试）。这次后台运行读到已保存的订阅并更新
  到期的分组：之后打开 App，更新时间已变化，分组都在。
- 调试模式用户在 App 未运行时用快捷指令执行 `anixops://connect`（或点网页上的 `anixops://import` 链接）冷启动 App：仍是调试模式的
  “请求 · 改写 · 出口 · 设置”四个标签，没有被改成代理模式。
- 网络切换导致隧道断开、首页出现“连接已断开…”卡片后，等自动重连（按需连接）成功：卡片自动消失；在卡片还在时点“重试”不会断开已连上的隧道。
- 首页出现带“重试”“换个节点”和“技术详情”的卡片后，切到别的 App 再回来，按钮和详情都还在。
- 首次连接前（调试模式连接条、代理首页）状态行显示“轻点连接，首次需要允许添加 VPN 配置”/出口文字，没有英文诊断；
  启用别的 VPN App 让本配置被停用后显示“已被其他 VPN 停用，轻点重新启用”。macOS 上为“打开开关…”。
- 粘贴 `http://` 订阅链接：预览直接提示“只支持 HTTPS 订阅链接…”，不会下载后再报“链接可能已失效”。
- 订阅里节点 B 经节点 A 中转时，在 A 的菜单里点“删除节点”先弹出确认，说明 B 会一并删除；确认后 A、B 都删除，其余节点照常连接和测速。
- 订阅启用 ANX 授权的面板：节点列表测速和节点体检对 ANX 节点给出延迟结果，不再全部显示“失败”。
- 规则模式下在策略组里切换成员（不断开），然后在本地规则里加一条规则：状态显示规则已热重载，隧道不重连、下载不中断。

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
      （客户端用自己的 managed-config 校验，跳过内核里“解密必须由服务持有代理”这条服务时代的规则，见 `apps/windows-gui/src/gui/managed_config.rs`；内核放宽后可去掉。）
- [ ] 便携版留下的服务时代配置（`system_proxy_owner: service` 且解密已开）在导入或更新订阅后能写入，解密保持开启。
- [ ] 已连接时开启/关闭 HTTPS 解密，客户端自动重连，系统代理保持在 127.0.0.1:7890。
- [ ] 抓包中添加解密域名、保存映射或启停插件：引擎重启后抓包自动重新开启，“已保存的会话”里多一条“重连前自动保存”；提示写明已保存。
- [ ] 抓包中断开连接或从托盘退出：“已保存的会话”里多一条“断开时自动保存”。
- [ ] 关闭 HTTPS 解密后，根证书和私钥保留（再次开启不需要重新信任）；根证书只在卸载时，或用户自己在 `certmgr.msc` 里删除时才移除。
- [ ] 断开连接后 7890/7891 端口都被释放。

## Windows 节点热切换、订阅重载与桌面界面真机验收（1.0.0 评审 c3）

CI 只跑纯函数和组件逻辑的单测，以下需要 Windows 真机（系统代理模式和 TUN 模式各一遍）：

- [ ] 已连接时在代理页切换节点、在规则页切换 select 策略组：提示“现有连接不受影响”，正在进行的下载 / 视频通话不中断，系统代理和 TUN 网卡不被重置；概览的出口地址随之改变。
- [ ] 单节点配置（没有策略组）切换节点：提示“内核已重启”，连接恢复正常。
- [ ] 开启“自动选择最快节点”并连接 1 小时以上：日志里多数轮次是“相差不大，保持不变”，只有当前节点不通或明显更慢时才切换，切换时现有连接不中断；FINAL 走 url-test / fallback 组时不会被改掉。
- [ ] 已连接时在代理页对另一个订阅点“使用”：提示“当前连接已改用它”，出口地址变成新订阅的节点；对另一个订阅里的节点直接点选也能切过去。
- [ ] 服务商只改了节点密码（或手动改订阅内容）后“更新当前订阅”：连接自动改用新凭据并能上网；内容没变化的自动更新不会中断连接。
- [ ] TUN 模式下点“连接”，在网卡创建的几秒内窗口不出现“未响应”，托盘菜单可点。
- [ ] 300 个节点“全部测速”进行中，点连接开关或按 Ctrl+Shift+C 能立即断开；连接或断开进行中再按会提示“正在连接或断开，请稍候”。
- [ ] 中文、繁體中文、English 三种界面语言下，连接、断开、切换节点、更新订阅、保存规则的提示和“活动 › 当前会话 › 运行状态”都是对应语言。
- [ ] 调试模式抓包到预算上限（持续浏览 + 一条长时间的 HTTP/2 或 WebSocket 连接）：请求列表不会一次性清空大量仍可导出的请求，被淘汰连接上的“进行中”行会消失。
- [ ] 请求页打开重放编辑器修改请求体后改动筛选条件：编辑器和内容保留，检查器注明“已不在列表中”。
- [ ] 调试一个 SSE 流式接口：流结束后消息面板显示最后的 `data: [DONE]`，“共 N 条”与实际一致。
- [ ] Linux 桌面窗口三种语言下：导入插件、启用 / 停用 / 删除插件、保存 / 删除映射、在解密列表里添加（解密与排除各一次）/ 编辑 / 暂停 / 删除域名，提示先说改了什么（插件名和脚本数、映射规则、域名），再说对连接的作用（已应用到当前连接 / 下次连接时生效 / 开启 HTTPS 解密后生效）；根证书未信任时开启 HTTPS 解密，提示后半句是“根证书还没有信任”。

已知限制（1.0.0 之后跟进）：

- 内核只在被淘汰连接的最大 seq 超过已有值时抬高 `dropped_before_seq`；之后淘汰较年轻（seq 更小）或请求都还在进行中的连接不会抬高它，这些行要等下一次抬高时才从桌面请求列表消失。需要内核提供淘汰计数或最小保留标记。
- 活动页的 `lastError` / `configurationError` 是后端动态生成的英文错误文本，没有结果代码，界面原样显示。

## Windows TUN 模式真机验收

桌面模式新增「TUN 模式」开关：客户端用 Wintun 虚拟网卡接管全部流量，系统代理和 HTTPS 解密照常保留。CI 没有 Wintun 驱动，只能真机验收（Windows 10/11 x64，管理员）：

- [ ] 安装目录（MSI 和便携 ZIP）里有 `wintun.dll`；设置 → 运行方式 → 打开「TUN 模式」，已连接时客户端自动重连。
- [ ] 「网络连接」里出现 `AnixOps Net` 网卡；`route print` 有 `0.0.0.0/1`、`128.0.0.0/1` 走该网卡，原默认路由仍在。
- [ ] 不走系统代理的程序（`curl --noproxy "*" https://www.google.com/generate_204`、游戏）经过代理；活动页可能只显示系统代理那一路的连接。
- [ ] 代理服务器连接不回环：连接稳定、CPU 正常，`netstat -ano` 中客户端到节点的连接本地地址是物理网卡 IP。
- [ ] 关闭 TUN 模式或断开后，`AnixOps Net` 网卡和路由消失，网络恢复正常。
- [ ] 已连接时开启 TUN 但网卡创建失败（例如安全软件拦截 Wintun）：开关回到关闭，连接按原设置恢复，系统代理仍指向客户端，提示写明“已按原设置恢复连接”；原设置也连不上时提示写明“当前已断开”，界面显示未连接。
- [ ] 已知限制：DNS 服务器在本地网段（如路由器 192.168.1.1）时 Windows 可能并发向它查询；切换 Wi-Fi/有线后需要重连一次。

## macOS Developer ID 版真机验收

macOS 客户端改为官网分发（Developer ID 签名 + 公证），隧道从 App 扩展改成 System Extension。签名、公证在私有 anx 仓库的
「macOS with ANX」workflow 里完成；CI 只能做不签名构建，以下只能真机验收（macOS 14+）：

- [ ] 解开 ZIP / 挂载 DMG，把 AnixOps Net 拖进「应用程序」后打开，Gatekeeper 不报「无法验证开发者」；`spctl -a -vv` 显示 `Notarized Developer ID`。
- [ ] 第一次连接时提示允许网络扩展；在「系统设置 › 通用 › 登录项与扩展 › 网络扩展」允许后，再连接成功。
- [ ] 不在「应用程序」里直接打开时，连接给出「请先把 AnixOps Net 拖进应用程序文件夹」的提示。
- [ ] 订阅规则、HTTPS 解密（根证书已信任）、脚本插件在 Mac 上照常生效（隧道读的是 App 启动时交给它、并在配置改动后
  随时更新的密钥副本）。
- [ ] 睡眠唤醒、从系统设置的 VPN 开关直接连接（不经 App）也能连上。
- [ ] 安装新版本后再连接，系统扩展被替换为新版本（`systemextensionsctl list` 版本号更新）。
- [ ] 抓包会话（ios-bridge-features-01，CI 跑不到 root 系统扩展）：连接后开始抓包、解密几个请求，“更多 › 保存当前抓包”成功；
  从 App 断开后请求页写“已加密保存”，重连后“已保存的会话”里有“断开时自动保存”并能载入；App 内换节点触发重连后，之前的抓包也存成了会话；
  重启 Mac、安装新版本替换系统扩展后旧会话仍可打开；开发者诊断的扩展事件里没有 `capture storage unavailable`；
  `sudo ls -l` 扩展容器的 `Application Support/capture-session.key` 为 `-rw-------  root`，32 字节。
- [ ] 换根证书（ios-bridge-features-07、apple-extensions-mac-06）：已连接并在解密时，扩展页“重新生成…”确认后隧道自动重连，
  系统要求登录密码撤销旧证书信任；信任新证书后 Safari 打开解密的站点证书链指向新根。`security dump-trust-settings` 里不再有旧根，
  用导出的旧 `.cer` 签发的证书链不被信任；取消登录密码时页面说明旧证书仍受信任并保留在“钥匙串访问”里。“使用自己的根证书…”同样核对一遍。
- [ ] 换根后从系统设置的 VPN 开关、控制中心，或重启后由 On Demand 连接：Safari 证书链仍指向新根。断开但自动重连仍开着时换根，
  再让系统拉起隧道：第一次启动失败（`root certificate in the kept start secrets is older than the profile`，原因为
  `start secrets are older than the profile`），App 在运行或下次回到前台时带新副本重连一次并连上，证书链指向新根；同一根不会反复重连。
- [ ] 旧版本留下的信任（1.0.0 之前“重新生成”只删了钥匙串条目，没撤销信任，旧根仍被信任且在“钥匙串访问”里看不到）：
  发行说明写明手动清理办法：`security dump-trust-settings` 查看用户域信任；有旧证书 `.cer` 时
  `security remove-trusted-cert <旧证书.cer>` 删除。不要整体导入改写用户域信任设置，会误删其他证书的信任。

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
- [ ] S3 崩溃自愈（Android）：连接后 `adb shell am crash io.anixops.net`（或在开发者选项里结束进程），系统重启 VPN 服务后
  重新连接；手动断开后再结束进程不应自己连上；10 分钟内连续崩溃 3 次后停止重启并出现“不再自动重新连接”通知。
- [ ] Android “始终开启的 VPN”：在系统设置里打开后，连接失败（例如删掉全部节点）时出现“VPN 未能连接”通知，而不是静默失败。
- [ ] S4 唤醒/切网（iOS）：Wi-Fi ↔ 蜂窝、两个 Wi-Fi 之间各切 20 次，每次切换后网页和 Hysteria2/TUIC 节点都能在几秒内恢复；
  开发者模式的事件里每次有一条 `network change: reset … sessions, closed … connections`，扩展内存（诊断里的
  footprint）20 次后不持续增长。锁屏超过 1 分钟再解锁，应有 `wake: … closed …`。
- [ ] S6 SSID 规则（iOS、macOS）：把家里 Wi-Fi 设为“断开”，加入该 Wi-Fi 后隧道在 300 ms 内断开（用系统 VPN 状态栏图标或
  `log stream --predicate 'subsystem == "com.apple.networkextension"'` 的时间戳计时），离开后自动连接。
- [ ] S4 桌面（Windows、Linux，本机代理模式）：拔插网线或换 Wi-Fi、睡眠 2 分钟后唤醒，网页几秒内恢复；Windows 日志
  （`tauri-gui`）里有 `network change: engine reset {...}` / `wake: engine reset {...}`。TUN 模式这一项不覆盖。

## Windows 系统代理残留恢复真机验收

CI 只跑归属记录、启动恢复、卸载恢复和断开步骤的单元测试（`owned_proxy`、`connection` 的测试），以下需要真机：

- [ ] 连接后在任务管理器结束 `networkcore-windows-gui.exe`，再启动客户端（未开自动连接）：系统代理恢复为连接前的设置，
  弹出“网络设置已恢复”通知，`gui.log`（`tauri-gui`）有 `its system proxy was restored`；`netsh winhttp show proxy`
  恢复为连接前的值。
- [ ] 连接后结束进程，先在 Windows 设置里关掉“使用代理服务器”，再启动客户端：用户的新设置不被改动，
  但 `netsh winhttp show proxy` 恢复为连接前的值，日志有 `its system proxy was restored`。
- [ ] 连接后结束进程，先改掉用户代理，再用 `netsh winhttp set proxy` 设成别的值，然后启动客户端：两者都不被改动，
  日志有 `stale proxy ownership dropped`。
- [ ] 开启“自动恢复核心”，连接后让断开时的代理恢复失败（例如用调试器暂停注册表写入或拒绝写 Internet Settings 键）再点“断开”：
  界面报错，核心已停止，之后不会自动重连，也不弹“连接已断开”；设置页“恢复网络设置”可再次恢复。
- [ ] 连接状态下注销、关机和重启：再次登录后代理已是连接前的设置（`session end (query): system proxy restored`）。
  注销时被其他程序取消后，客户端仍连接且代理重新指向本机端口（`session end cancelled: system proxy applied again`）。
- [ ] 连接状态下从“应用和功能”卸载：卸载后浏览器能直接上网，`netsh winhttp show proxy` 为直连，`gui.log` 的 `purged:` 行
  有 `proxy_users=1 winhttp_proxy=true`。
- [ ] 把 `desktop-state.json` 截短后启动：客户端能启动并弹出“设置已重置”，数据文件夹里有 `desktop-state.json.broken-*`。
- [ ] 把 `managed-config.json` 改成无效 JSON 后点“断开”和托盘“退出”：核心停止、代理恢复。

## 苹果平台 VPN 配置写入真机验收

小组件、控制中心、实时活动、快捷指令和系统设置都会改同一份 VPN 配置。App 现在把对 VPN 配置和节点密钥的每次改动排成
一队、一次一个（`SerialAsyncQueue`），每次改动前重新读取系统里保存的配置、只改自己负责的字段（`ConfigurationWrite`），
收到 `NEVPNConfigurationChange` 和回到前台时也会重新读取。CI 只跑单元测试（`SerialAsyncQueueTests`、
`ConfigurationWriteTests`、`NodeSecretStoreTests`），以下需要真机：

- [ ] iOS：从控制中心或小组件连接（自动重连随之打开），再在 App 里断开：隧道保持断开，不会被自动重连拉起。
- [ ] iOS：在实时活动里断开后，在 App 里改一次 DNS、再换一个节点：隧道不会自己连上，“自动重连”仍是关闭状态。
- [ ] iOS、macOS：用快捷指令把模式切到全局，回到 App 首页显示“全局”；随后改一次 DNS、再换一个节点，模式仍是全局。
  切到直连同理。
- [ ] iOS、macOS：连接状态下在代理页快速连点两个不同节点，再连点同一个节点几次：最终用的是最后点的节点，隧道重连成功，
  开发者模式的诊断里没有 `node key is unavailable`。
- [ ] 从 0.3.0-beta.11 升级：在 beta.11 上因快速切换节点出现 `node key is unavailable` 的设备，升级后打开 App 一次即可
  重新连接（App 启动时为当前节点重新保存配置）；单个粘贴的节点需要重新导入。
- [ ] macOS Developer ID 版（System Extension 读不到钥匙串，只用 App 交给它的密钥副本）：连接后，分别做一次
  规则模式下在同一订阅里换节点（命令面板或 `anixops://select`）、连接中导入一个节点、订阅更新换了当前节点、修改插件；
  每次之后 `sudo kill -9` 系统扩展进程、重启 Mac（开机自动连接），以及从系统设置的 VPN 开关直接连接，都能连上，
  开发者模式的诊断里有 `start secrets updated by the app`，没有 `node key is unavailable`，HTTPS 解密状态不变。
- [ ] macOS：隧道断开但自动重连仍开着时（例如 Wi-Fi 规则为“断开”的网络里）换一个节点，再离开该网络：系统拉起隧道
  失败一次（`start secrets are older than the profile`）后，App 在运行或下次回到前台时自动带新密钥重连一次并连上；
  再次失败时提示“网络扩展保存的配置副本已过期……请重新连接”，不再提示节点没有响应。
- [ ] macOS：隧道断开但自动重连仍开着时，改插件（启用一个、在仍有插件启用时停用一个，或改内容）或调试映射，再让系统拉起隧道（离开该网络、
  重启后自动连接或从系统设置的 VPN 开关直接连接）：第一次启动失败（`https decryption payload missing from the kept
  start secrets`，原因为 `start secrets are older than the profile`），App 在运行或下次回到前台时带新副本重连一次并连上，
  HTTPS 解密保持开启、用的是新插件；同一配置不会反复重连（退出并重新打开 App 也不再重连）。
- [ ] macOS 已知限制：隧道断开时只在同一订阅里换策略组成员（不换节点、不改插件）时，系统不经 App 拉起的隧道仍按上次
  交给系统扩展的分组选择运行，不报错；打开 App 连接一次或连接后再选一次即恢复。确认该行为与说明一致。

## iOS 更换根证书真机验收（ios-app-tools-03）

- [ ] 已连接并在解密时，“HTTPS 解密证书 › 重新生成根证书”确认后隧道自动重连；安装并信任新描述文件、删除旧描述文件后，
  解密域名的请求恢复，不需要手动断开再连；请求页不再出现“隧道仍在用旧证书签发”。
- [ ] 未连接时重新生成：连接后直接用新根，不出现上面的提示。

## iOS / macOS HTTPS 解密首次确认真机验收

开启 HTTPS 解密的每条路径共用一道首次确认（`TunnelController.requireDecryptionConsent()`，见 `docs/debug-mode.md`
“首次解密确认”）。CI 只能编译和跑单元测试（`DecryptionConsentTests` 覆盖哪些改动需要确认、未确认时配置里的映射插件），
对话框在根视图还是编辑表单上弹出、取消后表单的状态、启动时的补问只能真机确认。以下每项在 iPhone、iPad 和 macOS 上各做一遍；
“未确认”指全新安装，或删除 App 重装后（macOS 上可用 `defaults delete <bundle id> https-decryption-consent-version`）：

- [ ] 未确认时，以下每条路径都弹出同一个“开启 HTTPS 解密前请确认”对话框，且只弹一次：改写 › 解密域名里添加域名；
  加密连接详情的“解密此域名”；请求页“没有请求的连接”里的“解密”；映射编辑表单里保存一个带主机的 Map Local；
  请求检查器里保存 Map Local；插件总开关。
- [ ] 在编辑表单（映射编辑、解密域名编辑、请求检查器）里触发时，对话框显示在表单上方，不会被表单挡住或不出现。
  点“取消”：表单保持打开、内容不丢，列表和映射都没有保存；点“我确认，开启”：保存并关闭表单。
- [ ] 确认之后，上面任何一条路径都不再询问；重启 App、断开重连后也不再询问。
- [ ] 升级安装：旧版本保存过解密域名或带主机的映射、从未确认过，升级后启动 App 只问一次。确认后映射插件写进 VPN 配置，
  解密生效；取消后映射插件从配置里拿掉、已连接时只重连一次，之后访问这些域名不解密（列表和映射仍保留在页面上）。
  下次启动会再问一次。
- [ ] 删除一行、停用一行、只添加排除行（`-example.com`）、删除或停用映射，在未确认时都不询问。
- [ ] iPad 多窗口、macOS 多窗口：两个窗口都打开时触发一次，记录对话框出现在几个窗口里；在任一窗口回答后其余对话框消失，
  等待中的操作都按同一回答处理，不会再弹第二次。
- [ ] 冷启动同时带 `anixops://import` 链接（导入页弹出）或首次引导页时，记录启动补问是否出现；如果对话框没出现，
  随后的插件总开关、添加域名是否也不再弹框（已知风险：根视图已有弹出内容时对话框可能无法显示）。

## macOS 命令行工具

`anixops` 的解析、退出码和套接字通道有单元测试，CI 还在构建好的 App 里运行工具检查帮助、版本和退出码
（`docs/macos-cli.md`）。签名后的真实行为只能真机验收（macOS 14+，Developer ID 版）：

- [ ] 「macOS with ANX」workflow 给 `Contents/MacOS/anixops` 用 Developer ID 签名并开 Hardened Runtime，公证通过。
- [ ] App 运行时 `anixops status` 显示状态；macOS 15 上不弹「想要访问其他 App 的数据」。
- [ ] App 未运行时 `anixops connect` 在后台打开 App 并连上；`anixops status -q; echo $?` 未连接为 3、已连接为 0。
- [ ] 按 docs/macos-cli.md 用 `sudo ln -sf … /usr/local/bin/anixops` 安装后，在任意目录（不是 App 所在目录）、App 未运行时执行
  `anixops connect`：App 被打开并连上，不报“没有运行”。
- [ ] App 正常退出（⌘Q）后 `ls ~/Library/Group\ Containers/*net.anixops.networkcore/cli.sock` 不存在；`kill -9` App 后留下的旧套接字
  也不影响：再执行 `anixops connect`，工具等到 App 真正开始监听再发送，一次成功。
- [ ] 关掉主窗口只留菜单栏：订阅仍按间隔自动更新（更新时间变化），“自动检查更新”打开时照常检查，脚本插件的通知照常出现；
  重新打开主窗口不会重复读取配置或出现两份脚本通知。
- [ ] `anixops mode rule`、`anixops select <节点>`、`anixops test`、`anixops import <订阅链接>` 与窗口里同样的操作效果一致，窗口实时更新。
- [ ] 另一个用户（或 `sudo -u nobody`）运行 `anixops status` 连不上（退出码 69 或 71）。

## 安卓 APK 签名与真机验收

CI 的 `android-native` job 每次都产出原生版 arm64 APK（artifact `anixops-net-android-<sha>`），一律用每次运行临时生成的
调试密钥签名（文件名 `*-debug.apk`），每次构建的密钥都不同，装新版前要先卸载旧版，也不能覆盖安装正式版。
正式签名密钥只在 `release.yml` 的 tag 发布里使用，见下面「安卓正式签名密钥」。

- [ ] 发版时 `release.yml` 把正式签名的 APK 作为 `anixops-net-<版本>-android-arm64.apk` 放进 GitHub Release。没有正式密钥、
  签名证书与 `ANDROID_RELEASE_CERT_SHA256` 不符时发布直接失败，不再用临时签名发布。
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
- [ ] 用户协议（`TERMS.md`）目前只有中文，而 1.0.0 起系统语言不是中文时界面为英文，首次启动仍要同意中文协议。上架 Play 前由维护者
  决定并提供英文版协议（属于法律文本，产品范围的决定，不由代码自动生成），再在首次启动和设置页按 App 语言显示。
- [ ] 1.0.0 起申请 `ACCESS_BACKGROUND_LOCATION`：在 Play Console → 应用内容 → 位置权限里声明用途（Wi-Fi 规则在后台读取
  Wi-Fi 名称，App 内先说明再跳系统设置），附演示视频；前台服务声明补充“打开 Wi-Fi 规则时 VPN 断开后保持等待网络”的用途。
  数据安全表单里位置一项仍是“不收集”（名称只在本机比对）。
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

有正式版后，「最新版」、固定直链和 `latest.json` 只指向最新的正式版（不是草稿、不是预发布），1.0.0 之后再发的 beta 不会顶替它；
`latest.json` 带 `channel`（`stable`，只有在还没有正式版时才是 `beta`）。比正式版更新的预发布单列在页面的「测试版」小节，
另有 `latest-beta.json` 和 `/beta/windows/` 这类测试版直链。脚本读取全部分页的 Release，测试在 `download_page_test.py`。

- [ ] 维护者在 Settings → Pages 把 Source 设为「GitHub Actions」（只做一次；没设之前 `Deploy to GitHub Pages` job 会失败）。
  设好后手动运行一次 `Download page` workflow。
- [ ] 打开下载页，确认各平台按钮指向最新正式版，`/windows/` 等固定链接能跳转；发了比正式版新的 beta 后，`/windows/` 仍指向正式版，
  `/beta/windows/` 指向 beta。


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
- [ ] MSI 卸载后：卸载时已登录用户自己「受信任的根证书颁发机构」（当前用户）里信任过的 HTTPS 解密根证书被删除，
  `mitm\root-ca-key.pem` 私钥被删除，设置页安装过的驱动被卸载；`%ProgramData%\AnixOps\NetworkCore\logs\gui.log`
  有 `purged:` 记录。卸载过程中不弹窗。未登录的用户仍保留根证书，需要自己在 `certmgr.msc` 里删除。
- [ ] 从 beta.11 用 MSI 升级到 1.0：升级后分组列表和节点仍在、可以直接连接（`gui.log` 有 `--purge skipped`，
  若托管配置已被旧版重置则有 `managed configuration repaired`）；开过「登录后启动」的用户重新登录后客户端自动启动；
  信任过的解密根证书仍受信任；HTTPS 解密如之前开着，升级后需要在扩展页重新开启。
- [ ] 1.0 首次启动后，`icacls C:\ProgramData\AnixOps\NetworkCore` 和其中的 `desktop-state.json`、`embedded\`、
  `anx-license\` 只有 SYSTEM 和 Administrators；用另一个标准用户执行 `type C:\ProgramData\AnixOps\NetworkCore\desktop-state.json`
  报拒绝访问。
- [ ] 数据目录收回不碰目录外的文件：在 beta 上用标准用户在 `C:\ProgramData\AnixOps\NetworkCore\embedded\` 里建一个指向
  目录外文件（如 `C:\Windows\win.ini`）的硬链接 `mklink /H x.json C:\Windows\win.ini`（Windows 拒绝建立时记下即可，说明这条路径本就不通），再升级到 1.0 并启动：`gui.log` 的
  `data folder closed` 记录里 `removed` 含该链接，`icacls C:\Windows\win.ini` 与升级前一致。CI 的单元测试覆盖同样情形
  （`crates/windows-data` 的硬链接测试），真机只复核实际权限。
- [ ] 标准用户抢先建的上级目录被接管：在干净机器上用标准用户执行 `mkdir C:\ProgramData\AnixOps`，再安装并启动 1.0：
  `icacls C:\ProgramData\AnixOps` 的属主为 Administrators，只有 SYSTEM、Administrators 完全控制和 Users 读取/执行，
  客户端正常启动。CI 的单元测试覆盖同样情形（`crates/windows-data` 中上级目录属主的测试），真机只复核实际权限。
- [ ] 数据目录被其他程序占用时仍能启动：开一个命令提示符 `cd /d C:\ProgramData\AnixOps\NetworkCore`（再开一个停在
  `C:\ProgramData\AnixOps`），保持窗口打开，分别在首次升级到 1.0 和之后的普通启动时打开客户端：都能正常启动，
  `icacls` 结果同上。CI 的单元测试覆盖同样情形（`crates/windows-data` 中占用目录和文件的测试）。
- [ ] 数据目录无法收回时的启动失败提示目前只有英文（`apps/windows-gui/src/main.rs` 的 `secure_data_directory`）：这时数据目录
  不可用，界面语言设置读不到，提示在界面目录加载前弹出。1.0 接受这一例外；后续把这条提示放进 `crates/client-strings`，
  按系统界面语言选择简体中文、繁体中文或英文。

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

- [ ] Windows、Linux、Android：连接、断开、更新订阅后，活动页“日志”列出对应记录；网址只显示协议和域名（`https://sub.example.com/…`），路径、查询参数里的 token 和分享链接的密码都不出现；订阅更新失败的系统通知同样不含网址里的 token。
- [ ] 级别切到“错误”只剩出错的行；Windows 有多份日志时能按模块筛选；“更多 → 拷贝筛选出的日志”能粘贴出文本，“清空日志”确认后清空。

## Android 诊断报告与出口地址真机验收（T8）

- [ ] 连接后概览页显示出口地址和位置，与浏览器打开 `https://www.cloudflare.com/cdn-cgi/trace` 看到的一致。
- [ ] 活动页“生成诊断报告”后提示保存到 `Download/anixops-net-diagnostics-<时间>.log`，文件管理器里能打开，是纯文本，末尾有应用日志。
- [ ] Android 8 / 9：首次生成诊断报告或打开 HTTPS 解密时询问存储权限，允许后文件出现在「下载」；拒绝后提示需要存储权限，不报成功。
  再次打开解密（同一根证书）不产生 `(1)` 副本；换一张根证书时文件名带新指纹前 8 位。

## 系统通知真机验收（T11）

- [ ] Windows（MSI 安装后从开始菜单打开）、Linux（GNOME / KDE）、Android 13+：设置页“通知”三个开关默认打开。
- [ ] Android 13+：首次连接成功后系统询问通知权限（之前不问）；拒绝后设置页“通知”出现“系统未允许本应用发送通知”一行和“打开设置”按钮，
  在通知关闭时打开任一通知开关会再次询问或跳到系统设置。
- [ ] Android：点击“连接已断开”“订阅更新失败”通知打开 App；点击带链接的脚本通知打开浏览器（只接受 http / https）；
  长文字可展开；连续两次断开只留一条“连接已断开”；切换 App 语言后通知渠道名和通知文字都跟着变。
- [ ] 装一个会调用 `$notification.post` 的插件脚本，命中后系统通知中心出现脚本标题和内容。
- [ ] 连接后结束内核或在 Android 系统设置里断开 VPN，几秒内出现“连接已断开”；Windows 打开“内核意外退出时自动恢复一次”时文字是“正在重新连接”。
- [ ] 把订阅地址改成打不开的地址并等到自动更新，出现“订阅更新失败”。关掉对应开关后不再出现；切换界面语言后通知文字跟着变。
- [ ] iOS / macOS 全新安装：导入订阅后第一次连接成功（在添加 VPN 配置的系统提示之后）出现通知授权提示，启动 App 时不会弹
  （从旧版本升级、隧道已由按需连接连上且从未回答过的用户，打开 App 时会弹一次，属预期）。
  拒绝后设置 › 通知显示“在系统设置中开启通知”和说明开关不起作用的脚注；从未回答时显示“允许 AnixOps Net 发送通知”。
  允许后，意外断开出现“已断开”，自动更新失败出现“订阅更新失败”。
- [ ] iOS / macOS：脚本通知的 `url` 为 `anixops://mode?set=direct` 或 `anixops://disconnect` 时，点按通知只打开 App，不切换模式、不断开；
  `https://` 链接照常在浏览器打开。
- [ ] iOS：连接后锁屏，在实时活动（锁屏横幅和灵动岛）上点“断开”：隧道断开，实时活动立即消失，不再显示“已连接”和计时；
  从控制中心断开后，下一次后台刷新或打开 App 时实时活动结束。

## `anixops://` 链接真机验收（T13）

- [ ] Windows、Linux（GNOME / KDE）、Android：启动过一次客户端后，在浏览器地址栏或终端（`xdg-open` / `start`）打开 `anixops://connect`、`anixops://disconnect`、`anixops://toggle`，客户端窗口来到前台并执行。
- [ ] `anixops://mode?set=direct`、`anixops://select?node=<节点名>`、`anixops://select?policy=<策略组>&node=<成员>` 生效；`anixops://import?url=<订阅链接>` 打开代理页的订阅、链接已填好，点“导入”才保存。
- [ ] 客户端没运行时打开链接：启动客户端并执行（Windows / Linux 只有一个窗口，不会多开）。
- [ ] 客户端没运行、且是新装（没有订阅）时打开 `anixops://select?node=<节点名>`：不会先弹“找不到节点”，节点加载后再切换；打开 `anixops://toggle` 时等状态读完再执行，不会被静默忽略；新用户打开 `anixops://import?url=<订阅链接>`，同意条款并选好模式后直接进入代理（调试模式为“出口”）页，订阅链接已填好。
- [ ] Windows（MSI 安装后）：客户端在托盘运行时，在浏览器打开 `anixops://toggle`：不弹 UAC，窗口来到前台并执行；注册表 `HKCU\Software\Classes\anixops\shell\open\command` 指向安装目录里的 `anixops-net-link.exe`。
- [ ] Windows：客户端没运行时打开链接：只弹一次 UAC，同意后客户端启动并执行；在 UAC 里点“否”：弹出提示说明客户端没有启动、链接已保留，一小时内从开始菜单打开客户端后链接被执行。
- [ ] Windows：从 0.3.0 测试版升级后首次启动客户端，上述注册自动改为指向 `anixops-net-link.exe`；客户端运行时再从开始菜单点一次快捷方式，窗口来到前台，不会出现第二个托盘图标。
- [ ] Linux：已连接并接管系统代理时，在浏览器打开 `anixops://import?url=…`，或在启动器里再点一次图标：窗口来到前台，`gsettings get org.gnome.system.proxy mode`（KDE 看 `kioslaverc`）仍是 `manual`，浏览器流量仍经过代理；`~/.config/anixops-net/state.json` 的 `proxy_backup` 没有被清空。第二个进程立即退出，不弹 TUN 授权框。

## Wi-Fi 规则真机验收（T12）

- [ ] Windows：设置页打开“Wi-Fi 规则”，点“当前网络”填入 Wi-Fi 名称（Windows 11 先在“隐私和安全性 > 位置”允许桌面应用），设为“断开”；连上后切到这个 Wi-Fi，十几秒内断开，换到别的 Wi-Fi 或有线后按规则连接。
- [ ] Android：打开规则时系统询问位置权限；允许后“当前网络”能填入 Wi-Fi 名称。Wi-Fi 和移动网络来回切换时按规则连接 / 断开（App 在后台也生效），拒绝权限时“其他 Wi-Fi”规则仍生效。
- [ ] Android 10+ 后台：只给“使用时允许”位置时，设置页“Wi-Fi 规则”出现后台位置提示；点“允许”先看到说明对话框，再跳到
  系统设置选“始终允许”。之后锁屏超过 1 分钟（或 `adb shell am make-uid-idle io.anixops.net`），在后台切到指定 Wi-Fi，
  规则仍按名称执行；不给“始终允许”时后台切到指定 Wi-Fi 不应被当成“其他 Wi-Fi”。关掉系统定位时设置页出现“位置信息已关闭”提示。
- [ ] Android 后台重连：规则让 VPN 断开后通知栏保留“Wi-Fi 规则：等待下一个网络”通知；App 在后台（锁屏或划掉最近任务）切到“连接”规则的网络，
  能重新连上。国产系统（MIUI、ColorOS 等）的后台冻结 / 省电策略下也确认一次，如被冻结在说明里写明需要关闭省电限制。
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

## 连接质量报告（iOS、macOS，默认关闭）

iOS、macOS 的“向订阅服务商报告连接质量”改为设置开关、默认关闭（docs/network-behavior.md “连接质量报告”，PRIVACY.md
已同步）。以下无法由 CI 完成：

- [ ] 法务确认这项可选上报是否需要在 App Store Connect 的 App Privacy 里申报（建议：诊断数据 / 其他诊断数据，与身份
  关联，不用于跟踪，用途为 App 功能）；确认后同步两份 `PrivacyInfo.xcprivacy` 和
  `apps/ios/Privacy/PrivacyManifestDataInventory.md`，并更新上方 `app-privacy-determination` 的确认日期。公开版和 ANX 版
  共用同一个 bundle ID，答案对两者都适用。
- [ ] 真机抓包确认：新装或从旧版本升级后默认不发出 `/report` 请求；打开开关并停在概览约一小时后发出一次，内容与
  network-behavior.md 一致；关闭后不再发送。同时确认连接时这条请求实际走的路径（是否进入隧道），与 network-behavior.md
  “走哪条路”一致，不一致时改文档。
- [ ] 通知面板（anix-control）一侧：上报现在只在用户打开开关后才有，大多数客户端不再上报；macOS 客户端的 `platform`
  从原来误报的 `ios` 改为 `macos`，面板的字段校验需要接受 `macos`。

## 内核 1c14184：直连模式 DNS、规则集本机缓存、证书固定、局域网共享防护（1.0.0）

客户端固定的内核从 cce9d63 升到 1c14184（networkcore_anixops #265、#266）。CI 只跑单元测试，以下需要人工或真机：

- [ ] anx 仓库的 Cargo.lock 内核与本仓库 `Cargo.toml` 固定的内核（1c14184）一致后，再触发 anx 的 ANX 构建和
  `publish-client.yml`。本仓库的 `Cargo.lock` 是按内核 1c14184 的 `Cargo.toml` 手工改的（依赖与 443aea4 相同）（只有 `transport-tls` 新增
  `ring`、`tracing` 两条依赖边，两者已在锁文件里）；CI 的 `--locked` 构建如果报锁文件需要更新，运行一次
  `Refresh Cargo Lock` workflow。
- [ ] 直连模式（五个平台）：选一个已失效的节点（例如改错端口），切到“直连”，DNS 保持默认，打开 TUN（Windows / Linux）
  或连接 VPN（iOS / macOS / 安卓）：网页能正常打开，诊断或抓包里 DNS 查询直接发往 DNS 服务器，不经节点；开启 Fake-IP
  （安卓、Windows TUN）后同样能打开。直连模式下“我的规则”（含拒绝规则）不再生效，与规则测试页“当前是直连模式，实际连接
  不按规则分流”的提示一致。Windows 从旧版本升级后不需要重新切换模式：启动时按当前模式重写引擎配置。
- [ ] 规则集本机缓存：添加一份 `RULE-SET,https://…,REJECT` 规则集并连接一次，等规则集页显示已更新；断开，断网或让列表
  地址不可达（例如 hosts 指向 127.0.0.1），重新连接：规则立即生效，规则集页的条目数不是 0（下载失败照样显示）。缓存目录：
  iOS / macOS 为隧道扩展自己的 Application Support 下的 `rule-sets`（macOS 系统扩展以 root 运行，在 root 的目录下），
  安卓为 App 数据目录下的 `rule-sets`，Linux 为 `~/.config/anixops-net/rule-sets`，Windows 为
  `%ProgramData%\AnixOps\NetworkCore\rule-sets`（数据目录的访问限制随之继承，普通用户不能写入）。目录里只有以摘要命名的文件，不含列表地址。
- [ ] 证书固定：订阅里带 `pinSHA256`（Hysteria2）或 `pcs` 等固定的自签节点，`insecure=1`，在新版本重新导入后能连接；把固定
  改成别的值后连接失败（握手被拒，服务端收不到密码）。
- [ ] 证书固定升级迁移：旧版本导入的节点在出站里没有固定。
  - 安卓、Linux、原生桌面：每次启动都由保存的订阅原文重新生成出站，升级后自动带上固定，无需操作。
  - Windows：启动时用保存的订阅原文（`embedded\source.txt`）连同节点编辑重新导入当前订阅一次，之后写入
    `embedded\import-revision`。其他订阅（作为“订阅·名称”策略组链接）的节点缓存没有原文，订阅列表里显示“由旧版本导入：更新后
    证书固定才会生效”，切换到该订阅（会重新下载导入）或更新一次后消失；本地文件导入的显示“重新导入”。
  - iOS / macOS：没有保存订阅原文。会自动更新的订阅组升级后立即更新一次；手动更新和粘贴导入的组在订阅列表里显示同样的提示，
    需要用户更新或重新导入。单个粘贴的节点（不在任何订阅组里）无法识别，需要用户重新导入，发布说明里写明。
  - 真机确认：从 0.3.0-beta.11 升级后，Windows 当前订阅的 `embedded\nodes.json` 里带固定的节点有
    `pinned_certificate_sha256`；iOS 会自动更新的订阅组在打开 App 后更新时间变为当前时间。
- [ ] 内核对“`insecure` 且没有固定”的节点只经 `tracing` 记一条警告，客户端目前没有安装 tracing subscriber，看不到这条警告；
  节点详情里标出“未验证服务器证书”的节点留作后续。

## 内核 6892161：订阅导入容错、HTTP / SOCKS5 节点、MITM 改写与抓包加固（1.0.0）

客户端固定的内核从 1c14184 升到 6892161（networkcore_anixops #267、#268）。两次合并没有新增依赖（只有
`platform-windows` 多开了几个 `windows-sys` feature），`Cargo.lock` 只换了内核提交，`cargo metadata --locked` 通过。

- [ ] anx 仓库的 Cargo.lock 内核换成 6892161 后，再触发 anx 的 ANX 构建和 `publish-client.yml`。
- [ ] Windows 系统代理（内核 1.0 起连接时同时写入每个拨号 / VPN 连接，不再改 WinHTTP）：建一个 VPN 连接并给它单独设代理，
  连接后该 VPN 条目的代理指向本机端口；正常断开、结束进程后重新打开窗口、注销取消这三种情况都恢复为原值，WinHTTP
  （`netsh winhttp show proxy`）始终不变。卸载程序替其他账户恢复时仍只恢复局域网设置；该账户 VPN 条目上的本机端口代理需要手动关掉（后续改进：卸载时读取该账户的 Connections 注册表值一并恢复）。

## iOS / macOS 英文界面截图检查（1.0.0）

CI 的截图任务目前只画中文界面，英文比中文长，是否被截断要人工看（以后可加 `-AppleLanguages (en)` 的截图运行）：

- [ ] iPhone（最小屏）与 macOS 窗口最小宽度，系统语言为 English：首次启动的模式选择页（“Debug network” / “Proxies” 两张卡片和说明）、
  调试模式的请求页（列表行、筛选、检查器标签页）与规则页（规则行、编辑表单）没有被截断而无省略号的文字；繁體中文各看一遍。

## iOS / macOS 引擎配置 64 KiB 上限（1.0.0 后续，需要改内核）

iOS / macOS 的隧道扩展把整份 JSON 配置一次交给内核 `networkcore_ios_runtime_init`，内核 1c14184 在
`crates/engine-embedded/src/ffi.rs` 里限定 `MAX_CONFIG_LEN = 64 * 1024`，超出就拒绝整份配置。插件原文、调试映射生成的插件
（Map Local 响应体按 Base64 内联）和订阅规则都在这份配置里，所以实际可用的远小于 App 允许的 8 MB 解密总量和 4 MB 映射上限：

- 一个常见的去广告插件（约 68–450 KB）单独就超出，HTTPS 解密在连接时被关闭；
- 一条超过约 46 KB 的 Map Local 响应体同样会让解密关闭；
- 订阅里超过约 1,500 条内联规则时规则不生效，隧道只用选中的节点。

1.0.0 的客户端已经做到不静默失败：扩展先测量（`EngineBudget`），按脚本、HTTPS 解密、规则的顺序去掉超出的部分，请求页和规则页
说明原因；插件页和映射页在已启用插件加映射已经放不进 64 KiB 时直接提示“连接时 HTTPS 解密会被关闭”。但功能本身受限，
需要内核改动后才能解决（客户端不能单方面放宽，`EngineBudget.maxConfigBytes` 必须与内核一致）：

- [ ] 内核（networkcore_anixops，engine-embedded）：二选一，由维护者决定——
  - 提高 `MAX_CONFIG_LEN`（`networkcore_ios_runtime_init`、`networkcore_ios_measure_delay`、`networkcore_ios_checkup` 三处检查都用它），
    同时确认 Network Extension 的内存上限（iOS 约 50 MB）下解析数 MB JSON 的峰值内存；或
  - 像脚本（`script_assets`）一样，把 `mitm.plugins` 和 `routing` 改为由扩展写入文件、配置里只传路径，内核读文件时仍按
    `MAX_POLICY_BYTES`（8 MB）等各自上限检查。
- [ ] 内核合并后，本仓库在同一个 commit 里：升级 `Cargo.toml` 固定的内核 rev；按新上限改
  `apps/ios/Sources/NetworkCoreBridge/EngineBudget.swift` 的 `maxConfigBytes`（改为文件方式时，改为只测量仍内联的部分）；
  更新 `EngineBudgetTests`、`apps/ios/README.md` 的 “Engine budgets” 一节、插件页 / 映射页的 64 KB 提示文案
  （`TunnelController.decryptionExceedsConfigWarning`，Localizable.xcstrings 里的英文和繁体同步改）和发布说明。
- [ ] 真机验收（改内核后）：iOS 与 macOS 各导入一个 100 KB 以上的去广告插件并打开 HTTPS 解密，连接后请求页不再显示
  “合计超出了隧道配置的上限”，插件的重写生效；保存一条 200 KB 的 Map Local 响应体，命中时返回该内容；导入含 3,000 条以上内联
  规则的订阅，规则页不再提示“规则没有生效”，开发者诊断的扩展事件里没有 `config too large`。
- [ ] 改内核前的 1.0.0 真机确认：启用一个 100 KB 的插件，插件页和映射页显示 64 KB 提示；连接后请求页显示
  “HTTPS 解密没有生效：……超出了隧道配置的上限”，VPN 正常连接、其余规则生效。
## 引擎 IPC 与核心服务加固（1.0.0，client-core-ipc-01/02/03/04/05/07/13）

CI 只跑单元测试和管理员账号下的端到端测试，以下需要人工或真机：

- [ ] 已保存的抓包会话与解密域名（安卓、Linux Tauri）：抓包页“保存会话”后能在“已保存的会话”里列出、打开、删除；
  开着解密访问几个网站后，解密域名页列出这些域名（失败的带原因），“清除”后列表变空。运行中保存“我的规则”提示
  “applied; open connections stay”，正在进行的下载不中断。
- [ ] 安卓 ANX 设备授权：在要求设备授权的面板上刷新订阅后 ANX 节点能连接，面板能看到这台设备；授权目录在 App 数据
  目录下的 `anx-license`（宿主机上的契约测试有 HOME，测不出这一点）。原生 Windows 服务沿用从旧版搬来的设备密钥，
  面板里不多出一台设备。
- [ ] 原生 Windows / Linux 窗口从文件导入订阅：在导入框里填一个本地订阅文件的路径，窗口以当前用户读文件，导入后订阅
  名是文件名；填另一个用户才能读的文件、文件夹或超过 16 MiB 的文件时，窗口说明原因，核心不读任何路径。
- [ ] Windows 命名管道（标准用户真机，原生版发布后）：标准用户的窗口照常连上服务（访问表 `0x12019b` 不含建实例权限，
  CI 的管理员账号测不到这一点）；另一个标准用户运行一个对 `\\.\pipe\AnixOpsNet.Core` 调 `CreateNamedPipeW` 的小程序，
  服务运行时得到拒绝访问；停掉服务后它抢先建出同名管道，窗口显示“不是 AnixOps Net 核心服务的管道”且不连上。
- [ ] Windows 服务失败重启：`sc qfailure AnixOpsNetCore` 显示三次 RESTART，`sc qfailureflag AnixOpsNetCore` 为 TRUE；
  用 PowerShell 循环打开、关闭 `\\.\pipe\AnixOpsNet.Core` 一分钟，服务一直在运行，窗口仍能连接。
- [ ] Linux 守护进程（deb / rpm 带上 `anixops-netd` 之后）：`cat /proc/$(pidof anixops-netd)/limits` 的 Max open files
  软上限为 524288；经代理开上千条并发连接（BT、压测工具）后守护进程不退出；用没有登录会话的账号开几百条不说话的连接，
  超过 16 条的被立即关闭，5 秒后其余的也被关闭，正常用户的窗口不受影响。
- [ ] Linux 根证书更换：先信任生成的根证书，再在扩展页导入团队 CA：导入的提示说明这台电脑还没信任它，扩展页显示未信任；
  点“信任根证书”后显示已信任，`certutil -d sql:$HOME/.pki/nssdb -L -n "AnixOps Net HTTPS Decryption Root" -a` 只列出一张证书。

## 1.0.0 发布准备（release-prep：ci-release-02/03/07/08/09，release-readiness-05/08/09/10）

### 版本号（单一来源）

版本号的来源是 `Cargo.toml` 的 `[workspace.package] version`。其他构建系统读不了它，各自保留一份副本：`Cargo.lock` 里的
各工作区成员、两份 `tauri.conf.json`、`apps/windows-gui/ui/package.json`、`apps/windows-native/Directory.Build.props`、
`app.manifest`（四段式 `X.Y.Z.0`）、`apps/ios/project.yml` 的 `MARKETING_VERSION`、`build.gradle.kts` 的 versionName 默认值。
`.github/scripts/version.py` 负责改写和检查：CI 每次检查所有副本等于来源；`release.yml`、`release-tag.yml`（读要打 tag 的那个
提交里的文件）、`play-bundle.yml` 和 `ios-testflight.yml`（在 tag 上运行时）还检查来源等于 tag 去掉 `v` 和预发布后缀后的版本。
源码只写 `X.Y.Z`，`-beta.N` / `-rc.N` 由 tag 带上。

- [ ] 打 `v1.0.0`（或 `v1.0.0-rc.1`）之前，先在一个 PR 里运行 `python3 .github/scripts/version.py set 1.0.0`（只改上面这些字段，
  `Cargo.lock` 里恰好也是 0.3.0 的第三方包不会被改），同时把 `build.gradle.kts` 的 versionCode 升到下一个值，写好
  `docs/release-notes/v1.0.0.md`。这个 PR 的 CI 通过后再用 `release-tag.yml` 打 tag。iOS / macOS 的「关于」页会显示新版本，
  截图对比会变，PR 描述里写明「界面变化：版本号」。
- [ ] `play-bundle.yml` 在 tag 上运行时用 tag 作为 AAB 的 versionName；不在 tag 上运行时必须填 `version-name`。已经带着
  错误 versionName 上传过的 versionCode 不能复用。

### 安卓正式签名密钥

正式签名密钥（Google Play 应用签名密钥，别名 `anixops`）以前是仓库级 secret，CI 的每次运行（含同仓库 PR 和任意分支的手动运行）
都会拿到。现在 CI 一律用临时密钥；只有 `release.yml` 的 `sign-android` job 在 tag 发布时进入 Environment `android-release`
使用它。这个 job 不跑 Gradle、Rust 和第三方 action，只下载构建 job 交出的未签名 APK、核对 SHA-256、签名、核对证书、出证明。
以下只能由维护者在 GitHub 设置里完成，**完成前 tag 发布会在 `sign-android` 失败**（不会再发出临时签名的 APK）：

- [ ] Settings → Environments → New environment，名字 `android-release`（必须完全一致；拼错时 GitHub 会新建一个空 Environment，
  `sign-android` 的 “The release signing key is configured” 步骤会因此失败）。
- [ ] 该 Environment → Deployment protection rules：勾选 Required reviewers，填维护者本人。
- [ ] 该 Environment → Deployment branches and tags：选 “Selected branches and tags” → Add deployment branch or tag rule →
  Ref type 选 **Tag**，Name pattern 填 `v*`。不要添加任何分支规则（从 main 手动运行 `release.yml` 只做构建验证，不进入这个
  Environment，用临时密钥签名）。
- [ ] 该 Environment → Environment secrets → Add secret，逐个写入 `ANDROID_KEYSTORE_BASE64`（`base64 -w0 <keystore>.jks`）、
  `ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`、`ANDROID_KEY_PASSWORD`。GitHub 不能导出已有 secret 的值，需要用维护者
  保管的原始 keystore 和密码重新填写。
- [ ] Settings → Secrets and variables → Actions → Repository secrets：**删除**同名的四个仓库级 secret。不删就等于没修：不声明
  Environment 的 workflow（含 PR 上的 CI）仍能读到仓库级 secret。
- [ ] Settings → Secrets and variables → Actions → Variables → New repository variable：`ANDROID_RELEASE_CERT_SHA256`，值为应用签名
  证书的 SHA-256（Play Console → 测试和发布 → 应用完整性 → 应用签名 → “应用签名密钥证书”的 SHA-256 证书指纹；或
  `keytool -list -v -keystore <keystore>.jks -alias anixops`）。带冒号、大小写均可。它是公开信息，用 variable 而不是 secret。
  `release.yml` 的 release policy 和 `release-tag.yml` 在构建前检查它，`sign-android` 签名后比对，防止 secret 被误换成上传密钥等
  其他 keystore。
- [ ] 验证：从 main 手动运行一次 `Release`（只构建），`Sign Android APK` 不请求审批、产出 `*-debug` 签名；第一次 tag 发布时
  `Sign Android APK` 请求审批，批准后 “Signed with the release key” 通过。
- [ ] 评估密钥暴露：2026-11-16 起这把密钥进过大量同仓库 PR（含代理会话分支）和 main 的 CI 运行，并与当时按标签引用的第三方 action
  在同一个 job。目前没有泄露证据。请检查这些运行的日志有无异常步骤或外发，并决定是否准备轮换预案：APK 签名方案 v3.1 的
  `apksigner rotate` 只对 Android 9 及以上生效（minSdk 26 的 8.x 设备仍只认旧证书），Play 侧需要在 Play Console 申请应用签名
  密钥升级。

### 第三方 action 固定到提交 SHA

所有 workflow 和 `.github/actions/*` 里的 action 都固定到完整的 40 位提交 SHA，旁边注释对应的正式 release 标签；
`.github/scripts/workflow_policy.py` 在 CI 第一个 job 里检查（同时检查安卓正式密钥只出现在 `release.yml` 声明了
`android-release` 的 job，Play 密钥只出现在声明了 `google-play` 的 job）。`.github/dependabot.yml` 每周提出 action 升级 PR。
`cargo-deny` 固定为 0.22.0。

- [ ] Settings → Actions → General → Actions permissions：勾选 “Require actions to be pinned to a full-length commit SHA”。
- [ ] Settings → Code security → Dependabot：确认 Dependabot version updates 已启用，升级 PR 照常经过完整 CI。

### 出口合规（App Store Connect 加密问卷，release-readiness-09）

2026-10-13 的答案 `standard-https-only-exempt` 只适用于当时「只调用系统 HTTPS」的内部 TestFlight 构建。现在 App 与隧道扩展（iOS 和
macOS）都链接 `NetworkCoreRuntime.xcframework`，其中自带 TLS（rustls + ring）、Shadowsocks AEAD（AES-GCM、ChaCha20-Poly1305）、
WireGuard、QUIC 类传输等加密实现，用来保护用户流量的机密性，不属于 Category 5 Part 2 的豁免情形。因此 `apps/ios/project.yml`
的四个目标（`NetworkCoreApp`、`NetworkCorePacketTunnel`、`NetworkCoreMac`、`NetworkCoreMacTunnel`）已改为
`ITSAppUsesNonExemptEncryption: true`（Widget 不链接运行时，不声明）。CI 检查这些值不再是 false，且本文件不再保留旧答案。
以下只能由维护者（必要时与法务）完成：

- [ ] App Store Connect → App → 通用 → App 加密文稿（App Encryption Documentation）→ 新建，按实际二进制回答：
  1. 使用加密：是；
  2. 是否符合美国 EAR 第 5 类第 2 部分的豁免：否（加密用于保护用户流量，不是仅用于认证、版权保护等豁免用途）；
  3. 是否实现专有或非标准算法：否（开源版只用公开标准算法）；
  4. 是否在 Apple 系统加密之外或之上实现标准算法：是。
  美国 EAR 侧通常按大众市场（5A992 / 5D992，§740.17(b)(1)）处理。2021 年 BIS 规则之后，大多数大众市场终端产品已不再需要年度
  自分类报告，是否仍需提交请法务确认后再写入本文件，不要照抄旧做法。
- [ ] 法国区：在法国上架需要先向 ANSSI 完成加密产品申报，再把申报文件上传到上面的加密文稿；Apple 审核通过后给出合规码。
  不在法国上架就在「价格与销售范围」里排除法国，并把这个决定记在这里。
- [ ] 拿到合规码后，在 `project.yml` 的 `NetworkCoreApp` 和 `NetworkCorePacketTunnel`（及 macOS 两个目标）加
  `ITSEncryptionExportComplianceCode`，或在 workflow 里从 Variables 注入。没有合规码之前，每个 TestFlight 构建都会显示
  「缺少出口合规信息」，需要在该构建页面回答一次后内部测试者才能安装（`ios-testflight.yml` 的 summary 也会提示）。
  不要用删除 `ITSAppUsesNonExemptEncryption` 来绕过，删掉的效果相同，只是不显眼。
- [ ] 完成后把第 44 行的标记改成实际答案与日期，例如 `ios-export-compliance-answer=standard-algorithms-non-exempt-YYYY-MM-DD`，
  并按 `docs/architecture/ios-app-review-manual-confirmation-source-contract.md` 记录 `ios-app-review-export-compliance`。
- [ ] ANX 版（anx 仓库 `release-ios.yml` 复用本仓库的 `project.yml`）含未公开的专有协议，属于「专有或非标准算法」，
  需要单独回答问卷（通常需要 CCATS 和法国申报），并在该 workflow 里显式写 ANX 版自己的 ITS 键和合规码，不能继承开源版的答案。

### 第三方许可声明（release-readiness-10）

每个发布产物都附带 `THIRD_PARTY_NOTICES.txt`，由 `.github/actions/third-party-notices` 从包管理器的元数据生成
（`.github/scripts/third_party_notices.py`）：cargo-about 0.9.3 扫描客户端和固定内核各自的 `Cargo.lock`（许可证白名单
`about.toml` 与 `deny.toml` 相同），`pnpm licenses list --prod` 扫描桌面界面，Android 的 `releaseRuntimeClasspath` 读取各模块
POM（只接受 Apache-2.0，其他许可证要人工补充后才能通过），再加上 `third_party/notices/` 里人工维护的条目（随 -sys crate 编译进来的
QuickJS-ng、PCRE2，Windows 附带的 WebView2Loader.dll 与 wintun.dll）和 Inter 字体的 OFL。文件开头写明本项目源码地址，并按
MPL-2.0 §3.2 列出每个 MPL-2.0 组件的源码下载地址。无法确定许可证、许可证不在白名单或必有条目缺失时生成失败。

放置位置：Linux tar 包根目录和 deb 的 `/usr/share/doc/anixops-net/`（`copyright` 改为 DEP-5 格式），Windows MSI 安装目录和
便携 ZIP（同时附 `LICENSE.txt`、`wintun-LICENSE.txt`），APK / AAB 的 `assets/`，iOS App 包内。各打包步骤都检查文件确实在包里。
CI 的 `Third-party notices` job 在每次相关改动时跑同一套生成，提前暴露问题。

- [ ] 1.0.0 发布前人工复核一次生成的 `THIRD_PARTY_NOTICES.txt`（CI artifact `third-party-notices-<sha>`），重点看 ring、
  MPL-2.0 组件和人工条目。
- [ ] Windows 版的 `mitm-anixops-sys` 静态链接 MSYS2 的 libsystre（TRE，BSD-2-Clause），它可能连带 libintl / libiconv
  （LGPL-2.1+）。请确认实际链接了哪些库；如有 LGPL 库静态链接，需要改为动态链接或换用不依赖它们的实现，并在
  `third_party/notices/` 补充相应声明。这一项不能由生成器自动判断。
- [ ] macOS Developer ID 版和 ANX 版由 anx 仓库打包，需要在那边接入同一个 composite action（ANX 版还要并入 anx 自有依赖）。
- [ ] 应用内「关于 › 开源许可」页面（Android、iOS / macOS、Tauri）尚未实现：目前声明文件随包附带但不在界面里显示。
- [ ] WiX 安装界面（`WixUI_InstallDir`）的许可协议页仍是 WiX 默认的占位文本，建议设置 `WixUILicenseRtf` 为本项目许可证，
  或换用不带许可协议页的对话框集。
