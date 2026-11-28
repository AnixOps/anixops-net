# Agent 规范

AI 编码助手与贡献者在本仓库工作时必须遵守：

- 只在本机编辑代码、文档和 workflow；不在本机执行测试、构建、编译、打包或发布。
- 所有验证通过 GitHub Actions 完成。推送后只对当前 commit 的 run 做一次、最多两次非阻塞查询；不得使用
  `gh run watch`、无限循环或长时间轮询。失败时只读取失败 job/step 日志；只有 run `completed` 且 `success` 才能声称 CI 通过。
- 内核代码（协议、引擎、MITM、脚本运行时、平台适配 crate）只在
  [AnixOps/networkcore_anixops](https://github.com/AnixOps/networkcore_anixops) 修改；本仓库只通过 `Cargo.toml` 中固定的
  `rev` 引用内核。需要内核改动时，先在内核仓库合并，再在本仓库升级 `rev`。
- 不得在本仓库依赖私有仓库（例如 anx）：本仓库公开，cargo 会解析所有依赖，包括可选依赖。
- 无法自动完成的事项记录到 `docs/manual-intervention.md`。
- 客户端 UI 只维护各平台的原生实现。停止推进非原生 UI 变更，清理现有非原生 UI 时一并移除专用依赖、资源、测试及构建和发布入口。
- Windows 和 Linux 原生界面以 macOS 客户端为视觉参考，对齐布局、信息层级和品牌风格，交互使用各平台原生控件。
- 清理冗余代码前核对原生客户端、共享后端、数据迁移和发布流程的调用关系，删除已无调用方及兼容用途的实现。
- 所有平台的界面改动都必须遵守 AnixOps Design 的操作与层级规范（`guidelines/actions.md`），客户端的组件对照和自查清单见
  [`docs/ui-ux.md`](docs/ui-ux.md)。界面 PR 的描述里写明自查结果；要破例先改规范。
