# MITM 测试插件

两个插件都把 [ipleak.net](https://ipleak.net) 页面上显示的 IPv4 地址改成 `233.333.333.333`（这不是合法地址，看到它就说明 HTTPS 解密和改写生效了）。

| 文件 | 做法 | 下载地址 |
| --- | --- | --- |
| `ipleak-233.plugin` + `ipleak-233.js` | `[Script]` http-response 脚本 | https://raw.githubusercontent.com/AnixOps/anixops-net/main/examples/mitm/ipleak-233.plugin |
| `ipleak-233-rewrite.plugin` | `[Body Rewrite]` 正则替换，不需要脚本 | https://raw.githubusercontent.com/AnixOps/anixops-net/main/examples/mitm/ipleak-233-rewrite.plugin |

使用前先在 App 的「扩展 → HTTPS 解密证书」里完成证书安装和完全信任，然后导入插件、打开「HTTPS 解密」开关并重新连接，再用 Safari 打开 https://ipleak.net 。

只改 IPv4；IPv6 地址保持原样。iOS 解密目前只支持 HTTP/1.1，响应体上限 1 MiB。
