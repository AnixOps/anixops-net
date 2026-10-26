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
