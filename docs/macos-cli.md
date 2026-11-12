# macOS 命令行工具 anixops

`anixops` 随 App 一起发布，在 `AnixOps Net.app/Contents/MacOS/anixops`。放进 PATH（设置 › 命令行 里有可复制的命令）：

```bash
sudo mkdir -p /usr/local/bin
sudo ln -sf "/Applications/AnixOps Net.app/Contents/MacOS/anixops" /usr/local/bin/anixops
```

## 命令

| 命令 | 作用 |
| --- | --- |
| `anixops status` | 连接状态、节点、订阅、代理模式 |
| `anixops connect` / `disconnect` | 连接或断开，等到结果才返回（别名 `up` / `down`） |
| `anixops mode [rule\|global\|direct]` | 不带参数查看，带参数切换 |
| `anixops nodes [--group 订阅]` | 列出订阅组和节点，`*` 为当前节点，带上次测速结果（别名 `ls`） |
| `anixops select <节点> [--group 订阅]` | 切换节点，名称或 ID 都可以；多个订阅同名时优先当前订阅，否则要 `--group`（别名 `use`） |
| `anixops test [--group 订阅]` | 对所有节点做 HTTP 延迟测试并列出结果 |
| `anixops import <链接\|文件\|->` | 导入订阅链接、节点链接或节点列表文件；`-` 从标准输入读 |
| `anixops version` | 工具版本 |

选项：`--json` 输出 JSON（字段见 `ControlResponse`）；`-q` / `--quiet` 不输出；`--timeout <秒>` 改等待时间（默认
status / nodes 10 秒，连接、切换 45 秒，导入 90 秒，测速 180 秒）。节点名以 `-` 开头时写在 `--` 之后。

除 `status` 外，App 没有运行时工具会先在后台打开它；`status` 只报告「没有运行」，方便脚本判断。

## 退出码

按 sysexits(3)：

| 退出码 | 含义 |
| --- | --- |
| 0 | 成功 |
| 1 | App 尝试了但失败（隧道没连上、订阅下载失败、切换没生效） |
| 3 | `status -q`：App 在运行但没有连接（同 `systemctl is-active`） |
| 64 | 用法错误，或当前状态下不能这样做（例如单个节点用规则模式） |
| 65 | 找不到订阅或节点，或导入内容里没有可用节点 |
| 66 | 读不到导入文件 |
| 69 | App 没有运行（或打不开），或现在做不了（例如还没导入节点） |
| 70 | 工具内部错误 |
| 71 | 系统调用失败 |
| 75 | 等待 App 超时 |
| 76 | 工具与 App 的协议版本不一致，或回答无法识别 |

```bash
anixops status -q && echo 已连接
anixops select "香港 01" --group 机场 || echo "失败: $?"
anixops test --json | jq '.groups[].nodes[] | {name, latencyMs}'
```

## 实现

- 工具不直接动 VPN：它没有 Network Extension 权限，也读不到钥匙串。它把请求发给正在运行的 App，由 App 用窗口里同一个
  `TunnelController` 去做，所以命令和点按钮的效果一样。
- 通道是 App 组容器里的 Unix 套接字 `~/Library/Group Containers/<TeamID>.net.anixops.networkcore/cli.sock`，权限 0600，
  App 再用 `LOCAL_PEERCRED` 核对对方是同一个用户。一次连接一个请求一个回答，各一行 JSON，带协议版本号
  （`NetworkCoreBridge/ControlChannel.swift`）。
- 解析、退出码、文本输出在 `NetworkCoreBridge/CommandLine.swift`，有单元测试；`Sources/NetworkCoreCLI/main.swift` 只做 I/O。
  CI 在构建好的 App 里直接运行工具，检查帮助、版本和几个退出码。
- 工具不进沙盒，只带 App 组权限，不需要描述文件；随 App 一起用 Developer ID 签名、开 Hardened Runtime。

## 安全边界

能连上套接字的只有同一用户的进程，和能直接点 App 窗口的进程是同一个范围；工具不能读出节点密码、私钥或订阅令牌，
`nodes` 只给名称、协议和测速结果。

## 待真机验收

见 `docs/manual-intervention.md`「macOS 命令行工具」。
