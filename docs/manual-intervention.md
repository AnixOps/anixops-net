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
- [ ] Windows MSI 与 Linux GUI 的正式发布 workflow 尚未迁入，目前仍由内核仓库 `release.yml` 打包；迁入后再从内核仓库移除。

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

