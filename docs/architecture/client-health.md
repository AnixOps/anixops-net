# 客户端可观测性与发布卫生

功能表 O4（指标）、L6（崩溃上报）、F4（冷启动）、C6（安装完整性）。全部自研：没有第三方遥测 SDK、指标库或崩溃上报服务。
桌面（Windows、Linux）的实现在 `crates/client-health`，iOS / macOS 在 `NetworkCoreBridge/ClientHealth.swift` 与
`NetworkCoreAppHost/ClientHealthMonitor.swift`，引擎指标在内核 `engine_embedded::metrics`（控制命令 `metrics`）。

## 指标（Prometheus 文本格式 0.0.4）

| 平台 | 在哪里 | 内容 |
| --- | --- | --- |
| Windows、Linux | `http://127.0.0.1:9477/metrics`（只监听回环地址） | 引擎指标（连接时）+ `anixops_client_*` |
| iOS、macOS | 开发者模式 / 活动页「复制诊断信息」的 `metrics:` 段 | `anixops_extension_*` + 匿名标签的引擎指标 |

- 引擎指标：`networkcore_build_info`、`networkcore_uptime_seconds`、`networkcore_mode`、
  `networkcore_{upload,download}_bytes_total`、`networkcore_{upload,download}_rate_bytes`、
  `networkcore_connections_{active,opened_total,closed_total}`（按 `network`，已关按 `result` = `ok` / `rejected` /
  `connect_failed`）、`networkcore_policy_group_selected`、`networkcore_policy_member_latency_ms`、
  `networkcore_rule_set_entries`、`networkcore_rule_set_failed`。不含主机、地址、进程、出站设置。
- 桌面客户端指标：`anixops_client_info`、`anixops_client_connected`、`anixops_client_crash_reports_{enabled,kept}`、
  `anixops_client_integrity{state}`、`anixops_client_cold_start_seconds`。
- 端口与开关在状态目录的 `health.json`：`{"crash_reports": true, "metrics_listen": "127.0.0.1:9477"}`，
  `metrics_listen` 为 `null` 关闭端点；非回环地址被忽略。端口被占用时客户端照常运行，设置页显示「端口被占用」。
- 诊断导出里的引擎指标用 `{"cmd":"metrics","anonymous":true}`：策略组、成员、规则集名换成序号。

## 崩溃报告

- 桌面：panic 钩子写 `<状态目录>/crashes/crash-<毫秒>.txt`，最多保留 10 份；内容是版本、系统、线程、panic 信息与位置、
  回溯，整体经 `client_health::redact` 脱敏（URL、主机名、IP、邮箱、UUID、长密钥样字符串、家目录与用户名）。
  只捕获 Rust panic，系统库里的原生崩溃没有报告。
- iOS / macOS：MetricKit 的崩溃诊断（通常在下次启动时送达）与未捕获的 Objective-C 异常，经 `Redaction` 脱敏后存在
  App 的 Application Support/crashes。
- 设置页「崩溃报告」开关默认开启；关闭即停止记录并删除已保存的报告。报告只在本机，生成 / 复制诊断报告时附上。
- 上传：桌面构建时设置 `ANIXOPS_CRASH_REPORT_URL` 才会在启动后发送（`CrashReports::upload`）；目前官方构建不设置，
  没有收集端。iOS / macOS 不上传。

## 冷启动（CI）

`ci.yml` 的 Linux GUI job 在 Xvfb 下运行 `.github/scripts/linux-client-health.sh`：用发布版二进制、空配置目录启动 7 次，
`ANIXOPS_STARTUP_PROBE` 让窗口页面加载完成后立刻写结果并退出；脚本量从 exec 到退出的整段时间，中位数超过 1500 ms
即失败，每次结果写入 Step Summary。同一脚本再启动一次，检查回环指标端点和完整性结果。

## 安装完整性

- 发布流程写 `integrity.sha256`（`sha256sum` 格式，路径相对客户端可执行文件所在目录）：
  - Windows MSI 与便携 ZIP：安装目录里，列出 GUI、服务、CLI、`WebView2Loader.dll`、`wintun.dll`
    （`.github/scripts/write-integrity.ps1`）；CI 的 MSI 安装测试核对安装后的文件。
  - Linux tarball 与 .deb：`share/anixops-net/integrity.sha256`，列出 `anixops-net`。
- 客户端启动后在后台线程计算并比对，结果是 `verified`、`unverified`（开发版本，没有清单）、`damaged`（缺失或改动的文件）
  或 `invalid`，显示在设置页「安装完整性」、指标与诊断报告里。只发现损坏或半更新的安装，不防能同时改写清单的人；
  防篡改靠系统的代码签名。
- iOS / macOS：启动时检查 App 内的扩展（`PlugIns/*.appex`、`Contents/Library/SystemExtensions/*.systemextension`）
  与 App 同版本同 build，且有 packet tunnel 扩展；结果在诊断信息的 `install integrity:` 行。
