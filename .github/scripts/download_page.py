#!/usr/bin/env python3
"""Builds the static download page of the open-source client (GitHub Pages, workflow download-page.yml).

Input: the JSON of `GET /repos/{owner}/{repo}/releases` (newest first, every page). Once a stable release exists,
"latest" is the newest published (not draft) release that is not a prerelease, like GitHub's own /releases/latest:
a beta published after 1.0.0 never replaces it. Before the first stable release, the newest published prerelease is
"latest" and the page labels it as a test version.

A prerelease newer than the stable "latest" is the beta channel: it gets its own section on the page, its own
latest-beta.json and its own fixed links under beta/. The fixed links without beta/ always point to "latest".

Output directory:
  index.html               one page with every platform
  latest.json              {version, url, published_at, prerelease, channel, assets: {key: url}} for scripts and
                           the panel; channel is "stable", or "beta" only while no stable release exists
  <key>/index.html         fixed links that redirect to the newest file, e.g. /windows/ -> the MSI
  latest-beta.json         same fields for a prerelease newer than the stable release (only when there is one)
  beta/<key>/index.html    fixed links to that prerelease's files (only when there is one)

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
    ("windows", "-windows-x64.msi", "Windows 10/11", "安装版（推荐）", "MSI 安装包，未做代码签名。安装前先核对 SHA256SUMS 与来源证明，通过后再在 SmartScreen 提示中选择「仍要运行」。"),
    ("windows-portable", "-windows-x64-portable.zip", "Windows 10/11", "便携版", "解压即用。"),
    ("linux-deb", "-linux-amd64.deb", "Debian / Ubuntu", ".deb 安装包", "sudo apt install ./<文件名>，需要 WebKitGTK 4.1。"),
    ("linux", "-linux-x64.tar.gz", "其他 Linux", "tar.gz", "解压后运行 bin/anixops-net，需要 WebKitGTK 4.1。"),
    ("android", "-android-arm64.apk", "安卓 8.0+", "APK（arm64）", "下载后直接安装。"),
]


def published_releases(releases):
    """Releases that are not drafts, in the order the API lists them (newest first)."""
    return [release for release in releases if not release.get("draft")]


def latest_release(releases):
    """The newest published stable release, or the newest published prerelease while there is no stable one."""
    published = published_releases(releases)
    stable = next((release for release in published if not release.get("prerelease")), None)
    return stable or (published[0] if published else None)


def beta_release(releases):
    """The newest published prerelease listed before (newer than) the stable latest release, or None.

    Without a stable release the newest prerelease already is "latest", so there is no separate beta channel.
    Order comes from the API (newest first), not from comparing tag strings, where beta.10 sorts before beta.9.
    """
    published = published_releases(releases)
    latest = latest_release(releases)
    if latest is None or latest.get("prerelease"):
        return None
    for release in published:
        if release is latest:
            return None
        if release.get("prerelease"):
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


def cards_html(found, prefix=""):
    cards = []
    for key, _suffix, platform, label, note in ASSETS:
        asset = found.get(key)
        if asset:
            link = f"{prefix}{key}/"
            cards.append(
                f"<div class=\"card\"><div class=\"p\">{html.escape(platform)} · {html.escape(label)}</div>"
                f"<div class=\"n\">{html.escape(note)}<br><code>{html.escape(asset['name'])}</code> · {size_text(asset['size'])}</div>"
                f"<a class=\"btn\" href=\"{html.escape(asset['browser_download_url'], quote=True)}\">下载</a>"
                f"<div class=\"n\">固定链接：<a href=\"{link}\">{link}</a></div></div>"
            )
        else:
            cards.append(
                f"<div class=\"card\"><div class=\"p\">{html.escape(platform)} · {html.escape(label)}</div>"
                f"<div class=\"n\">{html.escape(note)}</div><span class=\"btn off\">本版没有这个包</span></div>"
            )
    return "".join(cards)


def page(release, found, sums, repo, beta=None, beta_found=None):
    version = html.escape(release["tag_name"])
    release_url = html.escape(release["html_url"], quote=True)
    published = html.escape(release.get("published_at", "")[:10])
    # Before the first stable release the newest prerelease is "latest"; say so instead of calling it the
    # newest (stable) version.
    latest_label = "最新测试版" if release.get("prerelease") else "最新正式版"
    cards = cards_html(found)
    beta_section = ""
    if beta is not None and beta_found:
        beta_section = (
            "<h2>测试版</h2>\n"
            f"<p class=\"sub\">比正式版更新的测试版 <a href=\"{html.escape(beta['html_url'], quote=True)}\">"
            f"{html.escape(beta['tag_name'])}</a> · {html.escape(beta.get('published_at', '')[:10])}。"
            "测试版可能不稳定，一般用户请用上面的正式版。</p>\n"
            f"<div class=\"grid\">\n{cards_html(beta_found, 'beta/')}\n</div>\n"
        )
    sums_line = ""
    if sums:
        sums_line = (
            f"校验：<a href=\"{html.escape(sums['browser_download_url'], quote=True)}\">SHA256SUMS</a>，"
            "<code>sha256sum -c SHA256SUMS</code>（Windows：<code>Get-FileHash &lt;文件&gt; -Algorithm SHA256</code>）；"
            "来源证明：<code>gh attestation verify &lt;文件&gt; --repo "
            f"{html.escape(repo)}</code>。"
        )
    beta_json = "，测试版：<a href=\"latest-beta.json\">latest-beta.json</a>" if beta_section else ""
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
<p class="sub">{latest_label} <a href="{release_url}">{version}</a> · {published}</p>
<p class="sub">这是开源版，支持 SS、SS-2022、VLESS、Trojan 等公开协议。</p>

<h2>开源版</h2>
<div class="grid">
{cards}
</div>

{beta_section}
<h2>ANX 版（服务商提供）</h2>
<div class="grid">
<div class="card"><div class="p">Windows / Linux / macOS / 安卓</div>
<div class="n">包含 ANX 私有协议，只在服务商面板里提供。登录面板后在「下载客户端」页面下载。</div></div>
<div class="card"><div class="p">iOS / iPadOS</div>
<div class="n">只有 ANX 版，通过 TestFlight 分发。请在面板里获取 TestFlight 邀请。</div></div>
</div>

<p class="foot">{sums_line}<br>
所有历史版本见 <a href="https://github.com/{html.escape(repo)}/releases">GitHub Releases</a>。
机器可读的最新版信息：<a href="latest.json">latest.json</a>{beta_json}。</p>
</main>
</body>
</html>
"""


def write_redirects(out_dir, found):
    for key, asset in found.items():
        os.makedirs(os.path.join(out_dir, key), exist_ok=True)
        with open(os.path.join(out_dir, key, "index.html"), "w", encoding="utf-8") as f:
            f.write(redirect_page(asset["browser_download_url"]))


def release_json(release, found, sums, channel):
    data = {
        "version": release["tag_name"],
        "url": release["html_url"],
        "published_at": release.get("published_at"),
        "prerelease": release.get("prerelease", False),
        "channel": channel,
        "assets": {key: asset["browser_download_url"] for key, asset in found.items()},
    }
    if sums:
        data["assets"]["sha256sums"] = sums["browser_download_url"]
    return data


def write_json(path, data):
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
        f.write("\n")


def build(releases, out_dir, repo):
    release = latest_release(releases)
    if release is None:
        sys.exit("no published release")
    found, sums = find_assets(release)
    if not found:
        sys.exit(f"release {release['tag_name']} has none of the expected assets")
    beta = beta_release(releases)
    beta_found, beta_sums = find_assets(beta) if beta is not None else ({}, None)

    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "index.html"), "w", encoding="utf-8") as f:
        f.write(page(release, found, sums, repo, beta, beta_found))
    write_redirects(out_dir, found)
    channel = "beta" if release.get("prerelease") else "stable"
    write_json(os.path.join(out_dir, "latest.json"), release_json(release, found, sums, channel))
    if beta is not None and beta_found:
        write_redirects(os.path.join(out_dir, "beta"), beta_found)
        write_json(os.path.join(out_dir, "latest-beta.json"), release_json(beta, beta_found, beta_sums, "beta"))
        print(f"beta channel {beta['tag_name']}: {sorted(beta_found)}")
    print(f"download page for {release['tag_name']} ({channel}): {sorted(found)}")


def main():
    releases_path, out_dir, repo = sys.argv[1:4]
    with open(releases_path, encoding="utf-8") as f:
        releases = json.load(f)
    build(releases, out_dir, repo)


if __name__ == "__main__":
    main()
