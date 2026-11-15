# 命令应答样例

客户端服务层（`client_core::client::Client::invoke`）的请求与应答样例，各端共用：

- Rust：`apps/android-native/rust/tests/contract.rs` 检查真实应答的字段与样例一致（字段不多不少）。
- Kotlin：`apps/android-native/core` 的单元测试用同一批样例检查数据类能解析。

命令新增或改字段时，先改这里的样例，两边的测试会一起指出要改的地方。应答都包在 `{"ok": …}` / `{"error": "…"}` 里。
