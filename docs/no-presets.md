# 公开版不带预设脚本和预设分流规则

用户 2026-11-15 定下的全平台规则：所有公开发布的版本（iOS / macOS TestFlight 与 App Store、Windows、Linux、
Android APK 与 Google Play、原生桌面预览版，以及 anx 私有构建出的 ANX 版）都**不带任何预设脚本和预设分流规则**。
用户自己导入的配置、订阅、插件、规则和规则集不受影响。

## 不带的东西

- 预设分流模式：绕过大陆、GFW 列表、回国。
- 推荐规则列表 / 开源规则库（广告拦截、流媒体、AI 服务等一键添加的列表）。
- 内置的 MITM 插件、去广告规则和脚本（包括内核里的 `networkcore.adblock` 和哔哩哔哩网页版试用）。
- DNS 国内分流预设（大陆域名走国内 DNS 的内置规则）。

## 保留的东西（用户 2026-11-15 确认不算预设分流）

- **局域网和本机地址直连**开关（默认开）：私有地址、`*.local` 不进代理，和 Surge / Loon 的 skip-proxy 一样。
- **GEOSITE,cn / GEOIP,CN 对应的公开列表**：只有在用户自己导入或写的规则引用这两个分类时才下载，不会自己生效
  （iOS 的 `RuleSetCatalog`）。只内置这两份：`geoip-cn`（gaoyifan/china-operator-ip，MIT）和 `geosite-cn`
  （Loyalsoldier/v2ray-rules-dat，GPL-3.0）。App 里只有地址，列表由设备自己下载，来源写在规则页的说明里。
  用户 2026-11-15 确认不内置 GFW 列表；订阅自带的同名规则集优先于内置地址。
- 全局、直连、规则三种代理模式本身；规则模式下订阅没有规则时，只按用户自己的规则分流，其余走代理。
- DNS 服务器的一键选择（阿里 DNS、Cloudflare 等）：只是服务器地址，不带分流规则。

## 空配置的首次启动

没有订阅和规则时，客户端照常启动，提示导入订阅或节点；规则页只显示“添加第一条规则”和“通过链接添加规则集”。

## 从旧版本升级

- 桌面 / Android 保存过绕过大陆、GFW 列表或回国模式的，读入时变成规则模式；以前下载到 `rules/` 的列表不再读取。
- 打开过哔哩哔哩网页去广告的，开关读入时忽略，MITM 不再加载任何内置插件。
- 选过 DNS 国内外分流的，第一次启动时转成内容相同的自定义 DNS（服务器和分域规则写进“自定义”，用户可以自己改或删）。iOS 上原来的一键分流按钮去掉了，已经写进自定义 DNS 的内容不变。

## 防止再带进来

CI 的 “No preset scripts or routing rules” 步骤（`.github/scripts/no_presets.py`）检查会打进安装包的源码：

- 不得出现 `raw.githubusercontent.com/<owner>/…` 形式的规则列表、插件或脚本地址（GEOSITE/GEOIP 对应表除外）；
- 不得出现已去掉的预设的名字（包括 `geosite-gfw` 和 GFW 列表地址）；
- 打进 App 的资源目录（Apple `Resources/`、Android `assets/`、`res/raw/`）不得有插件、脚本或规则列表文件。

测试、文档和 `examples/` 不打进安装包，不检查。要加例外须经用户同意，并在脚本里写明原因。
