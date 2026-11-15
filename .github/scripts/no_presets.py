#!/usr/bin/env python3
"""Public builds ship no preset scripts and no preset routing rules (docs/no-presets.md).

Fails when the client sources that go into a release bring back:

- a rule list, plugin or script URL on raw.githubusercontent.com (a "recommended" catalog or a built-in list),
- one of the names the removed presets went by (routing modes, the rule-list catalog, built-in MITM plugins,
  the DNS split preset),
- a plugin, script or rule-list file inside a directory that is packaged into an app (Apple resources, Android
  assets and raw resources).

Tests, documentation and examples/ are not packaged and are not checked. The user's own imports are data at run
time and never in the sources.

Each exception is listed below with why it is allowed; add one only with the user's agreement.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import PurePosixPath

# Public list URLs that back the GEOSITE,cn and GEOIP,CN categories. They are used only when a rule the user imported
# or wrote names that category (decided 2026-11-15: not a preset), never on their own. No GFW list (user, 2026-11-15).
ALLOWED_LIST_URL_FILES = {
    "apps/ios/Sources/NetworkCoreBridge/RuleSetCatalog.swift",
}

LIST_URL = re.compile(r"raw\.githubusercontent\.com/[A-Za-z0-9_.-]+/")

# Names of presets that were removed. A match outside tests and docs means one came back.
BANNED = [
    (re.compile(r"\bOpenRuleSets\b"), "the recommended rule-list catalog"),
    (re.compile(r"blackmatrix7|ios_rule_script|Loyalsoldier/surge-rules"), "a recommended rule-list source"),
    (re.compile(r"\b(BypassChina|GfwList|ReturnChina)\b|refresh_routing_rules|refreshRoutingRules"),
     "a preset routing mode"),
    (re.compile(r"builtin_(ad_block|bilibili)|BUILTIN_(AD_BLOCK|BILIBILI)|bilibiliWebAdBlock|native_mitm_policy_source"),
     "a built-in MITM ad-block plugin"),
    (re.compile(r"DnsSettings\.split\b|dns_split|value: \"split\""), "the DNS split preset"),
    (re.compile(r"geosite-gfw|release/gfw\.txt"), "a built-in GFW list (only the cn lists are kept)"),
]

PACKAGED_DIRS = [
    re.compile(r"^apps/ios/Sources/[^/]+/Resources/"),
    re.compile(r"^apps/android-native/[^/]+/src/main/(assets|res/raw)/"),
]
PACKAGED_BANNED_SUFFIXES = {
    ".plugin", ".sgmodule", ".lpx", ".js", ".list", ".conf", ".snippet", ".yaml", ".yml", ".txt",
}


def not_packaged(path: str) -> bool:
    p = PurePosixPath(path)
    parts = set(p.parts)
    if path.startswith(("docs/", "examples/", ".github/")) or p.suffix == ".md":
        return True
    if parts & {"Tests", "tests", "test", "androidTest", "screenshots", "fixtures"}:
        return True
    return p.name.endswith((".test.ts", ".test.tsx", "_test.rs"))


def main() -> int:
    files = subprocess.run(
        ["git", "ls-files", "-z"], check=True, capture_output=True
    ).stdout.decode().split("\0")
    problems: list[str] = []
    for path in filter(None, files):
        if any(pattern.match(path) for pattern in PACKAGED_DIRS):
            if PurePosixPath(path).suffix.lower() in PACKAGED_BANNED_SUFFIXES:
                problems.append(f"{path}: plugin, script or rule-list file packaged into an app")
        if not_packaged(path):
            continue
        try:
            text = open(path, encoding="utf-8").read()
        except (UnicodeDecodeError, IsADirectoryError, FileNotFoundError):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if LIST_URL.search(line) and path not in ALLOWED_LIST_URL_FILES:
                problems.append(f"{path}:{number}: rule list / plugin / script URL: {line.strip()[:160]}")
            for pattern, what in BANNED:
                if pattern.search(line):
                    problems.append(f"{path}:{number}: {what}: {line.strip()[:160]}")
    if problems:
        print("Public builds must not ship preset scripts or preset routing rules (docs/no-presets.md):")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print("No preset scripts or preset routing rules in packaged sources.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
