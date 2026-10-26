# AnixOps Net

AnixOps Net 的全平台客户端：iOS / macOS App、Windows 与 Linux 桌面 GUI、Windows 托管服务和 MSI 安装器。

网络内核（协议栈、嵌入式引擎、MITM、脚本运行时、CLI 和服务端节点内核）在
[AnixOps/networkcore_anixops](https://github.com/AnixOps/networkcore_anixops)。本仓库只放客户端，通过 Cargo git
依赖引用固定版本的内核。

## 目录

| 路径 | 内容 |
| --- | --- |
| `apps/ios` | iOS + macOS App、Packet Tunnel、Widget（Swift，XcodeGen 生成工程） |
| `apps/windows-gui` | Windows Tauri GUI，`ui/` 是 Windows 与 Linux 共用的 React 界面 |
| `apps/linux-gui` | Linux Tauri GUI |
| `apps/windows-service` | Windows 托管服务 |
| `installer/windows` | WiX MSI 安装器（同时打包内核仓库的 Windows CLI） |
| `docs/` | 客户端相关的设计与合同文档 |

## 内核版本

内核固定在根目录 `Cargo.toml` 的 `[workspace.dependencies]` 里（同一个 `rev`）。升级内核：把所有 `rev` 改成新的内核
commit 或 tag，再运行 `Refresh Cargo Lock` workflow 更新 `Cargo.lock`。CI 用同一个内核 checkout 构建 iOS 运行时
XCFramework 和 MSI 里的 Windows CLI（见 `.github/scripts/kernel-root.sh`）。

本地联调内核时，取消根 `Cargo.toml` 末尾 `[patch]` 段的注释，指向本地内核目录。

## 工作方式

与内核仓库相同：本机只写代码和文档，所有测试、构建、打包、发布都在 GitHub Actions 中完成，详见 [AGENT.md](AGENT.md)。
无法自动完成的事项记录在 [docs/manual-intervention.md](docs/manual-intervention.md)。

## 许可

Apache-2.0，见 [LICENSE](LICENSE)。"AnixOps Net" 名称和标识见 [TRADEMARKS.md](TRADEMARKS.md)。
