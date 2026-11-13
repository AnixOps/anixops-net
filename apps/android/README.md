# AnixOps Net for Android

安卓客户端：Linux 客户端的 Tauri 窗口和后端（`apps/linux-gui`，`gui` feature）跑在安卓上，界面是共用的
`apps/windows-gui/ui`。流量走系统 VPN：

```text
App ──▶ VpnService（vpn-plugin，Kotlin）──TUN 描述符──▶ 内核 EmbeddedRuntime（用户态 TCP/IP，与 iOS Packet Tunnel 同一路径）──▶ 节点
```

- `vpn-plugin/`：Tauri 安卓插件。`start` 第一次会弹出系统的 VPN 授权，然后建立接口（198.18.0.1/24、fd00:4e43::1/64，
  全局路由，DNS 198.18.0.2 由内核应答），把 TUN 描述符交给 Rust；`stop` 结束服务。App 自己的流量用
  `addDisallowedApplication` 排除在 VPN 外，内核连节点不会绕回隧道。
- `src/lib.rs`：移动端入口，告诉窗口数据目录在哪、怎么开关 VPN。
- 后端在 `apps/linux-gui/src/backend/tun.rs`（只在 `target_os = "android"` 编译）：读写 TUN、节点切换时在同一个接口上
  重启内核。

## 构建

只在 GitHub Actions 构建（CI 的 `android` job）：`tauri android init` 生成 Gradle 工程（`gen/`，不提交），
`tauri android build --apk --target aarch64` 出 arm64 APK，再用 `ANDROID_KEYSTORE_*` secrets 签名；没有配置 secrets
时用本次运行临时生成的调试密钥签名，作为 artifact `anixops-net-android-<sha>` 上传（保留 3 天）。

本目录是独立的 Cargo workspace（插件和 Tauri 移动端依赖不进根 workspace 的 `cargo clippy --workspace`）。
`Cargo.lock` 以根目录的锁文件为起点，内核 `rev` 跟根 `Cargo.toml` 一致；升级内核后 CI 会自动跟着解析。

## 现在的限制

- 只有 arm64（绝大多数安卓手机）。
- 不会开机自动连接，也没有常驻通知；被别的 VPN 顶掉或在系统设置里关掉后，界面显示内核错误，重新连接即可。
- 出口 IP 探测走本地代理，安卓没有本地代理，概览页暂不显示出口地址。
- HTTPS 解密：安卓 7 起 App 默认不信任用户安装的证书，只有浏览器等信任用户证书的 App 能被解密；证书导出入口待做。
