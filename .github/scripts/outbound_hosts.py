#!/usr/bin/env python3
"""Every address the client sources name must be one docs/network-behavior.md accounts for.

The document lists each connection the apps can open on their own or on request, and says what goes out. This check
keeps it honest: a host written into a source file that is packaged into an app (an `http://`, `https://`, `tls://`
or `quic://` address) must be in ALLOWED below, and each entry there must say where the document describes it. A new
update server, statistics endpoint or lookup service therefore fails CI until it is documented.

It is a tripwire, not a proof: an address put together at run time, or one that comes from the user's own
subscription, rules or plugins, is not in the sources. Those are data the user brought (docs/network-behavior.md
"用户自己的配置"), except when the client sends something of its own to an address it derives from them, such as the
connection-quality report to `<subscription link>/report`. Such an address is listed in RUNTIME below, and the code
that builds it carries a `network-behavior: <section>` marker naming the section that describes it; a marker that is
not listed, a listed source without its marker, or a section the document lacks fails the check.

`--self-test` checks the marker rules on samples first.

Not checked, because no app reads them at run time: tests (and the `#[cfg(test)]` part of a Rust file),
documentation, examples/, build manifests and lockfiles, licences, and the vendored AnixOps Design.
"""

from __future__ import annotations

import ipaddress
import re
import subprocess
import sys
from pathlib import PurePosixPath

DOC = "docs/network-behavior.md"

# Host -> the section of docs/network-behavior.md that covers it. The section title must appear in the document.
ALLOWED = {
    # Delay tests through a node, and the default health check of url-test groups in a subscription.
    "www.gstatic.com": "节点延迟测试",
    "www.google.com": "节点延迟测试",
    "www.youtube.com": "节点延迟测试",
    "cp.cloudflare.com": "节点延迟测试",
    # The exit address, asked through the proxy.
    "www.cloudflare.com": "出口地址",
    # The direct (mainland) delay on the desktop and Android overview, only while connected.
    "www.baidu.com": "国内直连延迟",
    # The public lists behind GEOSITE,cn and GEOIP,CN (docs/no-presets.md), only when the user's rules name them.
    "raw.githubusercontent.com": "GEOSITE,cn 与 GEOIP,CN 列表",
    # DNS servers the settings offer in one tap or show as an example; used only while connected.
    "1.1.1.1": "连接后的 DNS",
    "8.8.8.8": "连接后的 DNS",
    "9.9.9.9": "连接后的 DNS",
    "223.5.5.5": "连接后的 DNS",
    "1.12.12.12": "连接后的 DNS",
    "dns.google": "连接后的 DNS",
    "dns.adguard-dns.com": "连接后的 DNS",
    # Links the user opens in the browser (terms, project pages); the app itself never requests them.
    "github.com": "只在用户点开时打开的链接",
}

# Addresses the client derives at run time from the user's data and sends something of its own to.
# Source file -> the section of docs/network-behavior.md that covers it; the source carries the matching marker.
RUNTIME = {
    # The opt-in connection-quality report, POSTed to the subscription link's path plus /report.
    "apps/ios/Sources/NetworkCoreBridge/ClientReport.swift": "连接质量报告",
}

MARKER = re.compile(r"network-behavior:\s*(\S[^*]*?)\s*(?:\*/)?$")

# Names that are identifiers, not places the app connects to: XML namespaces, DTDs and schemas.
NOT_NETWORK = {
    "schemas.microsoft.com",
    "schemas.android.com",
    "www.apple.com",
    "www.w3.org",
    "www.freedesktop.org",
    "wixtoolset.org",
    "ipc.localhost",
    "localhost",
}

# Reserved for examples and tests (RFC 2606, RFC 6761) and placeholders in help text.
RESERVED_SUFFIXES = (".example", ".test", ".invalid", ".localhost", ".local", ".lan")
RESERVED_NAMES = {"example.com", "example.org", "example.net"}

ADDRESS = re.compile(r"\b(?:https?|tls|quic)://([A-Za-z0-9.-]+)")

SKIPPED_NAMES = {
    "Cargo.toml", "Cargo.lock", "deny.toml", "LICENSE", "TRADEMARKS.md",
    "gradle.properties", "libs.versions.toml", "settings.gradle.kts",
}


def not_packaged(path: str) -> bool:
    p = PurePosixPath(path)
    # third_party/notices/ holds license texts: their addresses are citations, never contacted.
    if path.startswith(("docs/", "examples/", ".github/", "third_party/anixops-design/", "third_party/notices/")) \
            or p.suffix == ".md":
        return True
    if set(p.parts) & {"Tests", "tests", "test", "androidTest", "screenshots", "fixtures", "Tools"}:
        return True
    if p.name in SKIPPED_NAMES or p.suffix in {".lock", ".svg", ".png", ".csproj", ".props", ".slnx", ".wxs"}:
        return True
    if p.name.endswith(".gradle.kts"):
        return True
    return p.name.endswith((".test.ts", ".test.tsx", "_test.rs", "Tests.swift", "Test.kt", "Tests.cs"))


def ignorable(host: str) -> bool:
    host = host.strip(".").lower()
    if host in NOT_NETWORK or host.endswith(RESERVED_SUFFIXES):
        return True
    if any(host == name or host.endswith("." + name) for name in RESERVED_NAMES):
        return True
    if "." not in host:
        # A placeholder such as https://host or https://api in help text or a format string.
        return True
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        return False
    # Private, loopback and documentation ranges (RFC 5737) are never a service on the internet.
    documentation = any(
        address in ipaddress.ip_network(net) for net in ("192.0.2.0/24", "198.51.100.0/24", "203.0.113.0/24")
    )
    return address.is_private or address.is_loopback or address.is_link_local or documentation


def markers(text: str) -> list[tuple[int, str]]:
    """The `network-behavior: <section>` markers in a source, with their line numbers."""
    found = []
    for number, line in enumerate(text.splitlines(), 1):
        match = MARKER.search(line)
        if match:
            found.append((number, match.group(1)))
    return found


def runtime_problems(doc: str, runtime: dict[str, str], sources: dict[str, str]) -> list[str]:
    """Checks RUNTIME against the markers in the packaged `sources` (path -> text) and the document."""
    problems = []
    for path, section in runtime.items():
        if section not in doc:
            problems.append(f"{DOC} has no section '{section}' for the address built in {path}")
        if path not in sources:
            problems.append(f"{path} (RUNTIME) is not a packaged source")
        elif section not in (found for _, found in markers(sources[path])):
            problems.append(f"{path} has no 'network-behavior: {section}' marker")
    for path, text in sources.items():
        for number, section in markers(text):
            # A listed marker's section is checked against the document above.
            if runtime.get(path) != section:
                problems.append(f"{path}:{number}: marker '{section}' is not listed in RUNTIME")
    return problems


def self_test() -> int:
    doc = "## 连接质量报告\n"
    runtime = {"a.swift": "连接质量报告"}
    cases = [
        ("listed and documented", {"a.swift": "/// network-behavior: 连接质量报告\nlet x = 1"}, runtime, doc, 0),
        ("marker missing", {"a.swift": "let x = 1"}, runtime, doc, 1),
        ("source missing", {}, runtime, doc, 1),
        ("section missing", {"a.swift": "// network-behavior: 连接质量报告"}, runtime, "## 别的\n", 1),
        ("unlisted marker", {"a.swift": "// network-behavior: 连接质量报告", "b.rs": "// network-behavior: 新地址"},
         runtime, doc, 1),
        ("block comment", {"a.swift": "/* network-behavior: 连接质量报告 */"}, runtime, doc, 0),
    ]
    failed = 0
    for name, sources, listed, text, expected in cases:
        got = runtime_problems(text, listed, sources)
        if len(got) != expected:
            failed += 1
            print(f"self-test '{name}': expected {expected} problem(s), got {got}")
    if failed:
        return 1
    print(f"self-test: {len(cases)} cases pass")
    return 0


def main() -> int:
    try:
        doc = open(DOC, encoding="utf-8").read()
    except FileNotFoundError:
        print(f"{DOC} is missing")
        return 1
    problems: list[str] = []
    packaged: dict[str, str] = {}
    for host, section in ALLOWED.items():
        if section not in doc:
            problems.append(f"{DOC} has no section '{section}' for {host}")
        if host not in doc:
            problems.append(f"{DOC} does not name {host}")

    files = subprocess.run(["git", "ls-files", "-z"], check=True, capture_output=True).stdout.decode().split("\0")
    for path in filter(None, files):
        if not_packaged(path):
            continue
        try:
            text = open(path, encoding="utf-8").read()
        except (UnicodeDecodeError, IsADirectoryError, FileNotFoundError):
            continue
        packaged[path] = text
        for number, line in enumerate(text.splitlines(), 1):
            # Rust unit tests sit at the end of the file, after #[cfg(test)].
            if path.endswith(".rs") and line.strip() == "#[cfg(test)]":
                break
            for host in ADDRESS.findall(line):
                host = host.strip(".").lower()
                if ignorable(host) or host in ALLOWED:
                    continue
                problems.append(f"{path}:{number}: {host}: {line.strip()[:160]}")
    problems.extend(runtime_problems(doc, RUNTIME, packaged))
    if problems:
        print(f"Addresses the client sources name must be described in {DOC} and listed in")
        print(".github/scripts/outbound_hosts.py (ALLOWED, or RUNTIME for addresses built at run time):")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print(f"Every address in the client sources is described in {DOC}.")
    return 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv[1:] else main())
