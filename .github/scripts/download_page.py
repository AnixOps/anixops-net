#!/usr/bin/env python3
"""Builds the static download page of the open-source client (GitHub Pages, workflow download-page.yml).

Input: the JSON of `GET /repos/{owner}/{repo}/releases` (newest first). The newest published release, beta or
not, is "latest": GitHub's own /releases/latest skips prereleases, and every release so far is a beta.

Output directory:
  index.html          one page with every platform
  latest.json         {version, url, published_at, assets: {key: url}} for scripts and the panel
  <key>/index.html    fixed links that redirect to the newest file, e.g. /windows/ -> the MSI

Only the open-source edition is listed. The ANX edition (closed source) is never published here; the page
points its users to the panel.

Usage: download_page.py RELEASES_JSON OUT_DIR REPO
"""

import html
import json
import os
import sys

# key, asset name suffix, platform, label, note
ASSETS = [
    ("windows", "-windows-x64.msi", "Windows 10/11", "安装版（推荐）", "MSI 安装包，未做代码签名，SmartScreen 提示时选「仍要运行」。"),
    ("windows-portable", "-windows-x64-portable.zip", "Windows 10/11", "便携版", "解压即用。"),
    ("linux-deb", "-linux-amd64.deb", "Debian / Ubuntu", ".deb 安装包", "sudo apt install ./<文件名>，需要 WebKitGTK 4.1。"),
    ("linux", "-linux-x64.tar.gz", "其他 Linux", "tar.gz", "解压后运行 bin/anixops-net，需要 WebKitGTK 4.1。"),
    ("android", "-android-arm64.apk", "安卓 8.0+", "APK（arm64）", "下载后直接安装。"),
]


def latest_release(releases):
    for release in releases:
        if not release.get("draft"):
            return release
    return None


def find_assets(release):
    found = {}
    for key, suffix, *_ in ASSETS:
        for asset in release.get("assets", []):
            if asset["name"].endswith(suffix):
                found[key] = asset
                break
    sums = next((a for a in release.get("assets", []) if a["name"] == "SHA256SUMS"), None)
    return found, sums


def size_text(size):
    return f"{size / (1024 * 1024):.1f} MB"


def redirect_page(url):
    u = html.escape(url, quote=True)
    return (
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">"
        f"<meta http-equiv=\"refresh\" content=\"0; url={u}\"><link rel=\"canonical\" href=\"{u}\">"
        "<title>AnixOps Net 下载</title></head>"
        f"<body><p>正在跳转到 <a href=\"{u}\">{u}</a></p></body></html>\n"
    )


STYLE = """
:root{--bg:#f6f7f9;--card:#fff;--text:#16181d;--muted:#5b6270;--line:#e3e6eb;--accent:#2563eb;--accent-text:#fff}
@media (prefers-color-scheme:dark){:root{--bg:#111318;--card:#1a1d24;--text:#e8eaee;--muted:#9aa2b1;--line:#2a2f39;--accent:#4f8cff;--accent-text:#0b0d11}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:15px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}
main{max-width:860px;margin:0 auto;padding:40px 16px 64px}
h1{font-size:28px;margin:0 0 4px}
h2{font-size:18px;margin:36px 0 12px}
.sub{color:var(--muted);margin:0 0 8px}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(250px,1fr));gap:12px}
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px;display:flex;flex-direction:column;gap:6px}
.card .p{font-weight:600}
.card .n{color:var(--muted);font-size:13px;flex:1}
.btn{display:inline-block;text-align:center;background:var(--accent);color:var(--accent-text);text-decoration:none;border-radius:8px;padding:8px 12px;font-weight:600;margin-top:6px}
.btn.off{background:var(--line);color:var(--muted)}
code{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:13px;word-break:break-all}
a{color:var(--accent)}
.foot{color:var(--muted);font-size:13px;margin-top:36px}
"""


def page(release, found, sums, repo):
    version = html.escape(release["tag_name"])
    release_url = html.escape(release["html_url"], quote=True)
    published = html.escape(release.get("published_at", "")[:10])
    cards = []
    for key, _suffix, platform, label, note in ASSETS:
        asset = found.get(key)
        if asset:
            cards.append(
                f"<div class=\"card\"><div class=\"p\">{html.escape(platform)} · {html.escape(label)}</div>"
                f"<div class=\"n\">{html.escape(note)}<br><code>{html.escape(asset['name'])}</code> · {size_text(asset['size'])}</div>"
                f"<a class=\"btn\" href=\"{html.escape(asset['browser_download_url'], quote=True)}\">下载</a>"
                f"<div class=\"n\">固定链接：<a href=\"{key}/\">{key}/</a></div></div>"
            )
        else:
            cards.append(
                f"<div class=\"card\"><div class=\"p\">{html.escape(platform)} · {html.escape(label)}</div>"
                f"<div class=\"n\">{html.escape(note)}</div><span class=\"btn off\">本版没有这个包</span></div>"
            )
    sums_line = ""
    if sums:
        sums_line = (
            f"校验：<a href=\"{html.escape(sums['browser_download_url'], quote=True)}\">SHA256SUMS</a>，"
            "<code>sha256sum -c SHA256SUMS</code>；来源证明：<code>gh attestation verify &lt;文件&gt; --repo "
            f"{html.escape(repo)}</code>。"
        )
    return f"""<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AnixOps Net 下载</title>
<style>{STYLE}</style>
</head>
<body>
<main>
<h1>AnixOps Net 下载</h1>
<p class="sub">最新版 <a href="{release_url}">{version}</a> · {published}</p>
<p class="sub">这是开源版，支持 SS、SS-2022、VLESS、Trojan 等公开协议。</p>

<h2>开源版</h2>
<div class="grid">
{''.join(cards)}
</div>

<h2>ANX 版（机场用户）</h2>
<div class="grid">
<div class="card"><div class="p">Windows / Linux / macOS / 安卓</div>
<div class="n">包含 ANX 私有协议，只在机场面板里提供。登录面板后在「下载客户端」页面下载。</div></div>
<div class="card"><div class="p">iOS / iPadOS</div>
<div class="n">只有 ANX 版，通过 TestFlight 分发。请在面板里获取 TestFlight 邀请。</div></div>
</div>

<p class="foot">{sums_line}<br>
所有历史版本见 <a href="https://github.com/{html.escape(repo)}/releases">GitHub Releases</a>。
机器可读的最新版信息：<a href="latest.json">latest.json</a>。</p>
</main>
</body>
</html>
"""


def main():
    releases_path, out_dir, repo = sys.argv[1:4]
    with open(releases_path, encoding="utf-8") as f:
        releases = json.load(f)
    release = latest_release(releases)
    if release is None:
        sys.exit("no published release")
    found, sums = find_assets(release)
    if not found:
        sys.exit(f"release {release['tag_name']} has none of the expected assets")

    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "index.html"), "w", encoding="utf-8") as f:
        f.write(page(release, found, sums, repo))
    for key, asset in found.items():
        os.makedirs(os.path.join(out_dir, key), exist_ok=True)
        with open(os.path.join(out_dir, key, "index.html"), "w", encoding="utf-8") as f:
            f.write(redirect_page(asset["browser_download_url"]))
    latest = {
        "version": release["tag_name"],
        "url": release["html_url"],
        "published_at": release.get("published_at"),
        "prerelease": release.get("prerelease", False),
        "assets": {key: asset["browser_download_url"] for key, asset in found.items()},
    }
    if sums:
        latest["assets"]["sha256sums"] = sums["browser_download_url"]
    with open(os.path.join(out_dir, "latest.json"), "w", encoding="utf-8") as f:
        json.dump(latest, f, ensure_ascii=False, indent=2)
        f.write("\n")
    print(f"download page for {release['tag_name']}: {sorted(found)}")


if __name__ == "__main__":
    main()
