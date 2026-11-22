# 请求详情里的正文预览

请求详情（抓包、调试回放）把正文按内容显示：JSON、表单、图片、Protobuf / gRPC，认不出的显示十六进制。十六进制永远是可选视图之一，好看的视图不会挡住原始字节。
解析器处理的是网络上来的不可信数据，三套实现遵守同一份规则和上限，打印出的文字逐字一致，测试用同一组样例：

| 平台 | 实现 | 测试 |
|---|---|---|
| 桌面（Windows、Linux 共用界面） | `apps/windows-gui/ui/src/binary-view.ts`、`body-view.ts`、`pages/BodyViewer.tsx` | `binary-view.test.ts` |
| iOS、macOS | `apps/ios/Sources/NetworkCoreBridge/CaptureBinary.swift`、`CaptureBody.swift`，界面在 `NetworkCoreAppHost/ExchangeInspector.swift` | `CaptureBinaryTests.swift` |
| Android | `apps/android-native/app/src/main/java/io/anixops/net/ui/CaptureBinary.kt`、`BodyText.kt`、`RequestDetailScreen.kt` | `CaptureBinaryTest.kt` |

Windows、Linux 原生端（WinUI 3、GTK4）还没有请求详情页，做的时候按本页的规则接入，并把同一组测试样例搬过去。

## 先解压，再识别

`Content-Encoding`（gzip、deflate、br、zstd，可叠加）由内核的解码器先解开（解开后最多 16 MiB），再按解开后的字节识别。解不开时按抓到的原始字节显示并说明。

## 识别顺序

1. `Content-Type` 是图片（`image/*`；桌面端含 SVG，Apple 和 Android 把 SVG 当文本）→ 图片。
2. gRPC（`application/grpc`、`application/grpc+proto`、`application/grpc-web`、`application/grpc-web+proto`）且能切出帧 → Protobuf，默认显示解析结果。
3. Protobuf（`application/x-protobuf`、`application/protobuf`、`application/x-google-protobuf`、`application/vnd.google.protobuf`、`application/x-protobuffer`、`*+proto`）且能解析 → Protobuf，默认显示解析结果。
4. 字节不是 UTF-8 文本时：类型不是 `text/*` 且开头是 PNG、JPEG、GIF、WebP、AVIF、BMP 的签名 → 图片；能完整解析成至少一个字段的 Protobuf → 默认十六进制，Protobuf 作为第二个视图（随机字节也可能凑巧像一条消息）；否则十六进制。
5. 文本：表单、JSON、其他文本，各平台原有规则不变。

## Protobuf（无 schema）

输出仿 `protoc --decode_raw`：

```
1: 150
2: "testing"
3 {
  1: 150
}
4: 0x400921fb54442d18 (fixed64)
5: 0x3f800000 (fixed32)
6: <00 ff 10> (3 B)
7: 18446744073709551615 (-1)
```

- varint 按无符号十进制；最高位为 1 时在括号里补上 int64 的值。
- 长度分隔字段：是文本（UTF-8，除制表、换行、回车外没有控制字符）就按带转义的字符串显示，超过 1000 个字符截断；否则能完整解析成消息就展开；再否则显示前 32 字节的十六进制和总长度。
- groups（线型 3、4）、线型 6、7、字段号 0 或超过 2^29−1、varint 超过 10 字节或超过 64 位，都视为“不是 Protobuf”。
- gRPC 每帧一节：`# 消息 N · 大小`；压缩帧（标志位 1）不解，提示导出后用其他工具解开；gRPC-Web 的尾部帧（标志位 0x80）按文本显示。

## 上限

| 上限 | 值 | 超过时 |
|---|---|---|
| 解析的字节 | 2 MiB | 只解析前 2 MiB，末尾标“只解析了开头部分” |
| 嵌套深度 | 16 | 更深的长度分隔字段显示为字节 |
| 字段总数 | 20,000 | 停止解析并标出 |
| 为区分文本和消息而重复扫描的字节 | 16 MiB | 之后的长度分隔字段直接显示为字节，保证深层嵌套也只扫有限次 |
| gRPC 帧数 | 1,000 | 停止并标出 |
| 图片像素 | 67,108,864（8192 × 8192） | 不解码，只显示尺寸提示和十六进制 |
| 图片显示尺寸 | 长边 2048 像素（Apple、Android 按缩略图解码） | 缩小显示 |
| 十六进制 | 桌面、Android 前 64 KiB，Apple 前 16 KiB | 末尾标出 |

抓包只保存了开头的正文按“截断”解析：保留截断前能完整解析的字段并标出。任何输入都不能让解析器抛异常或崩溃；三端测试都有固定种子的随机字节和变异样例模糊测试。

## 导出与用其他应用打开

导出的是解压后的字节，文件名为 `request-body` / `response-body` 加按类型推断的扩展名，未知类型用 `.bin`。

- 桌面：“更多”菜单里的“保存到‘下载’”（不覆盖已有文件）和“用其他应用打开”（写到系统临时目录的 `anixops-net-bodies`，超过一天的文件下次清理）。
- iOS：“更多”菜单里的“导出正文”，走系统分享面板（存到“文件”、发给其他 App）。macOS 另有“用其他应用打开”。
- Android：正文下方“用其他应用打开”和分享按钮，经 `FileProvider` 只暴露缓存里的 `bodies/` 目录。

“用其他应用打开”只对打开时不执行代码的类型提供：常见图片、PDF、纯文本、JSON、常见音视频。HTML、SVG、脚本、XML、CSV（表格软件会执行公式）和未知类型只能导出，不提供打开。名单在三端和 `crates/desktop-mitm/src/capture.rs` 的 `body_type` 里保持一致。

不做独立的解析器产品：更深的解析（按 schema 解码 Protobuf、pcap 分析）先通过导出交给专门工具。
