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
