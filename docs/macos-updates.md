# macOS 应用内更新

Developer ID 版（官网分发）的 AnixOps Net 自己检查、下载并安装新版本。Mac App Store 版不用这套机制。

代码：`apps/ios/Sources/NetworkCoreBridge/UpdateFeed.swift`（格式与校验）、`apps/ios/Sources/NetworkCoreMac/AppUpdater.swift`
（检查、下载、安装）、`apps/ios/Tools/appcast/main.swift`（发布工具）。CI 的「Signed update feed refuses tampering」步骤
（`.github/scripts/appcast-tamper-check.sh`）每次都用发布工具签一份 feed 和安装包，再要求每一种篡改都被拒绝。

## 流程

1. App 启动后、之后每 15 分钟看一次是否到期，每 24 小时最多检查一次（设置 › 软件更新可关掉自动检查，或手动「检查更新」）。
2. 下载 feed（HTTPS，最大 1 MiB），用内置公钥验签；验不过、产品名不对、`sequence` 比上次接受的小（回放旧 feed）、
   任何条目不是 HTTPS 或字段不合法，整份 feed 都丢弃，界面只说「更新信息未通过签名校验，已忽略」。
3. 从 feed 里挑 `build` 比当前大、`minimumSystemVersion` 不高于本机的最新条目，只接受 `.pkg`。
4. 用户点「下载更新」：下载到 `~/Downloads/AnixOps Net <版本> (<build>).pkg`，核对长度、SHA-256 和安装包签名，
   任何一项不对就删除文件，不会打开。
5. 用户点「安装并退出」：打开 Installer 之前再校验一次（文件在下载目录里放过），然后交给 Installer，App 退出。
   Installer 打开时 Gatekeeper 还会检查 Developer ID Installer 签名和公证，作为第二道关。
6. 新版本第一次连接时照常替换系统扩展（`SystemExtensionActivation`）。

没有配置 feed URL 或公钥的构建（CI、开发构建、开源仓库直接构建）更新功能关闭，设置页只提示去发布页下载。

## 格式

feed URL 上放的是一个 JSON 信封：

```json
{
  "format": "anixops-appcast-v1",
  "payload": "<UpdateFeed 的 JSON，base64>",
  "signature": "<Ed25519 签名，base64>"
}
```

- feed 签名覆盖的字节：`"anixops-appcast-v1\n"` + payload 解码后的原始字节。直接签字节，不需要 JSON 规范化。
- `UpdateFeed`：`schema`（1）、`product`（`anixops-net-macos`）、`channel`、`sequence`（每发一次 +1，单调递增）、
  `generatedAt`（ISO 8601）、`items`。
- `UpdateItem`：`version`、`build`、`minimumSystemVersion`、`url`（必须 HTTPS）、`length`、`sha256`（64 位小写十六进制）、
  `signature`、`notes`、`publishedAt`。
- 安装包签名覆盖的字节：`"anixops-update-archive-v1\n<product>\n<build>\n<length>\n<sha256>"`。把产品和 build 一起签进去，
  别的 build 的签名不能挪用；前缀不同，feed 签名和安装包签名也不能互换。
- 密钥是标准 Ed25519，编码与 AnixOps/anix-kit 的 `anixsign` 一致：私钥是 PKCS#8 PEM（也接受 32 字节种子的 base64），
  公钥是 32 字节的 base64（也接受 PEM SPKI），key id 是原始公钥 SHA-256 的前 16 个十六进制字符（`keygen` 和 `verify`
  会打印）。任何标准实现（CryptoKit、OpenSSL、Python `cryptography`、`anixsign`）都能签，单元测试里有一份 Python 签的
  feed 和一对 PKCS#8 / SPKI 的 PEM。

## 防住什么

| 攻击 | 结果 |
| --- | --- |
| 改 feed（换下载地址、改版本号、改 SHA-256） | feed 验签失败，整份丢弃 |
| 用别的密钥签 feed | 验签失败 |
| 回放旧的已签 feed，藏起新版本 | `sequence` 小于已接受的，拒绝 |
| 替换安装包、截断下载 | 长度或 SHA-256 不符，删除 |
| 把别的 build 的已签安装包换进来 | 安装包签名绑定了 build，验签失败 |
| HTTP 下载被中间人改 | 条目必须 HTTPS；即使被改，SHA-256 和签名也不过 |
| 降级到旧版本 | 只接受 `build` 更大的条目 |

不防：拿到签名私钥的人（所以私钥只放在 anx 仓库 Environment 的 secret 里，不进任何仓库和日志）；同一用户下已经能改
`~/Downloads` 的恶意程序在两次校验之间换文件（安装前会再校验一次，Installer 还会查 Gatekeeper）。

换钥：`ANIXOPS_UPDATE_PUBLIC_KEYS` 可以写多个公钥（逗号分隔）。先发一版同时带新旧公钥的 App，之后的 feed 用新私钥签，
再发一版去掉旧公钥。

## 发布

发布工具和 App 用同一份校验代码编译：

```bash
swiftc -O -o anixops-appcast \
  apps/ios/Sources/NetworkCoreBridge/UpdateFeed.swift apps/ios/Tools/appcast/main.swift

./anixops-appcast keygen --out update-signing.key        # 只做一次；stderr 打印公钥
./anixops-appcast item --key-file update-signing.key --product anixops-net-macos \
  --version 0.3.4 --build 120 --min-system 14.0 \
  --url https://<下载域名>/AnixOps-Net-0.3.4.pkg --file AnixOps-Net-0.3.4.pkg > item.json
# items.json 是一个数组：新条目放进去，旧条目可以保留几个
./anixops-appcast feed --key-file update-signing.key --product anixops-net-macos --channel stable \
  --sequence <上一份 +1> --items items.json --out appcast.json
./anixops-appcast verify --public-keys <公钥> --product anixops-net-macos --feed appcast.json \
  --archive AnixOps-Net-0.3.4.pkg --build 120
```

`item` 和 `feed` 也可以从环境变量 `ANIXOPS_UPDATE_SIGNING_KEY` 读私钥。退出码：0 成功，1 校验失败，64 用法错误，
65 输入数据不对，66 读不到输入文件。

Release 构建在 xcodebuild 的命令行传 `ANIXOPS_UPDATE_FEED_URL=https://…` 和 `ANIXOPS_UPDATE_PUBLIC_KEYS=<公钥>`
（写进 xcconfig 时 `//` 会被当成注释，要写成 `https:/$()/…`）。启用对外更新通道需要维护者同意，步骤见
`docs/manual-intervention.md`「macOS 应用内更新」。
