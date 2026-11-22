#!/usr/bin/env python3
"""Fail when interface code picks its own colours instead of AnixOps Design tokens (docs/ui-ux.md, "自动检查").

    python3 .github/scripts/ui_lint.py            # check every platform's interface sources
    python3 .github/scripts/ui_lint.py --self-test

Colours reach the interface only through the generated token files (.github/scripts/design_tokens.py): the
Swift `brand.*` / `Brand.*` palette, the desktop CSS variables, BrandTokens.kt, BrandTokens.xaml and the GTK
brand-*.css. Every pair of those tokens is checked for contrast in AnixOps Design (guidelines/color.md), so a
colour written anywhere else is a colour nobody checked, in one theme at least. Three rules:

  U1 colour-literal   a colour written in the code: hex, rgb(), Color(red:), Color.orange, Color(0xFF...), "#AARRGGBB"
  U2 derived-colour   a token made lighter or mixed: `.opacity()` on a colour, color-mix(), .copy(alpha = ...).
                      The tints the interface needs exist as tokens: accent-soft, success-soft, warning-soft,
                      danger-soft, fill-1/2/3, label-2/3, separator.
  U3 disabled-opacity a disabled control drawn by fading it. Faded accent reads as a grey-blue block in dark mode
                      and the text drops below 3:1. Disabled is fill-1 behind label-3 text (actions.md H10).

There is no allow list and no inline marker: a colour the tokens do not have is added to AnixOps Design and
apps/ios/Design/tokens.json first, then used by name.

Standard library only.
"""

from __future__ import annotations

import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

HUES = r"(?:red|green|blue|orange|yellow|purple|pink|teal|mint|cyan|indigo|brown|gray|grey|white|black)"
KOTLIN_HUES = r"(?:Red|Green|Blue|Yellow|Cyan|Magenta|Gray|LightGray|DarkGray|White|Black)"
DISABLED_WORDS = r"(?:isEnabled|enabled|disabled|isDisabled|canSubmit|canImport|canSave)"


@dataclass(frozen=True)
class Rule:
    rule: str
    pattern: re.Pattern[str]
    message: str


@dataclass(frozen=True)
class Scope:
    """One platform's interface sources: which files, which generated token files to skip, which rules."""

    name: str
    roots: tuple[str, ...]
    suffixes: tuple[str, ...]
    skip: tuple[str, ...]
    rules: tuple[Rule, ...]


def r(rule: str, pattern: str, message: str) -> Rule:
    return Rule(rule, re.compile(pattern), message)


SWIFT = (
    r("U1", r"\b(?:Color|UIColor|NSColor)\(\s*(?:red|white|hue|\.sRGB|\.displayP3|calibratedRed|srgbRed|displayP3Red|calibratedWhite|hex)\b|#colorLiteral",
      "colour literal; use a brand.* token"),
    r("U1", rf"\b(?:Color|UIColor|NSColor)\.(?:{HUES}|system\w+|light\w+|dark\w+)\b",
      "system colour; use a brand.* token (Color.clear and .primary/.secondary styles are fine)"),
    r("U1", rf"(?:foregroundStyle|foregroundColor|tint|fill|stroke|strokeBorder|background|color:|shadow\(color:)\s*\(?\s*\.{HUES}\b",
      "system colour; use a brand.* token"),
    r("U2", rf"(?:\b(?:[bB]rand|Color)\.\w+|\.{HUES}|\b\w*(?:[cC]olor|[tT]int)\w*\)?)\.opacity\(",
      "colour made lighter with .opacity(); use a tint token (soft, fill-1/2/3, text2/text3, border)"),
    r("U2", r"\([^()]*\b(?:brand|Brand|tint|color|Color)\b[^()]*\)\.opacity\(",
      "colour made lighter with .opacity(); use a tint token (soft, fill-1/2/3, text2/text3, border)"),
    r("U3", rf"\.opacity\(\s*!?\s*(?:[\w.]+\.)?{DISABLED_WORDS}\s*\?",
      "disabled state drawn by fading; use raised (fill-1) behind text3 (label-3)"),
)

CSS_COLOUR = r"#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\(|(?<![-\w])(?:white|black)\b(?![-\w])"
CSS = (
    r("U1", CSS_COLOUR, "colour literal; use a token variable from design/tokens.css or the vendored tokens.css"),
    r("U2", r"\bcolor-mix\(|\balpha\(|\bmix\(", "derived colour; use a tint token (fill-1/2/3, *-soft, label-2/3, separator)"),
)
TSX = (
    r("U1", r"""["'`]\s*(?:#[0-9a-fA-F]{3,8}|(?:rgba?|hsla?)\()""", "colour literal in a component; use a CSS token variable"),
)
KOTLIN = (
    r("U1", rf"\bColor\(\s*0x|\bColor\.(?:{KOTLIN_HUES})\b|\bColor\(\s*(?:red|0\.\d|\d+\s*,)", "colour literal; use a BrandTokens / MaterialTheme colour"),
    r("U2", r"\.copy\(\s*alpha\s*=", "colour made lighter with copy(alpha); use a tint token"),
    r("U3", rf"\balpha\([^)]*\b{DISABLED_WORDS}\b|\balpha\s*=\s*if\s*\(\s*!?\s*{DISABLED_WORDS}",
      "disabled state drawn by fading; use the theme's disabled colours"),
)
XAML = (
    r("U1", r"""=\s*"#[0-9A-Fa-f]{3,8}"|>\s*#[0-9A-Fa-f]{6,8}\s*<|\bColor\s*=\s*"(?:White|Black|Red|Gray|Transparent)\"""",
      "colour literal; use a BrandTokens.xaml resource"),
    r("U3", r"""Property="Opacity"[^>]*Value="0?\.\d""", "disabled state drawn by fading; use the disabled brushes"),
)
CSHARP = (
    r("U1", r"\bColor\.FromArgb\(|\bColors\.\w+|\bColorHelper\.FromArgb\(", "colour literal; use a BrandTokens.xaml resource"),
)
RUST_GTK = (
    r("U1", r"\bgdk::RGBA::(?:new|parse)\(|\bRGBA::(?:new|parse)\(", "colour literal; use a style class from brand-*.css"),
    r("U1", r"""["']\s*#[0-9a-fA-F]{3,8}\b""", "colour literal; use a style class from brand-*.css"),
)

SCOPES = (
    Scope("Apple", ("apps/ios/Sources",), (".swift",),
          ("apps/ios/Sources/NetworkCoreAppHost/BrandTokens.swift",), SWIFT),
    Scope("Desktop (Windows / Linux, web view)", ("apps/windows-gui/ui/src",), (".css",),
          ("apps/windows-gui/ui/src/design/",), CSS),
    Scope("Desktop components", ("apps/windows-gui/ui/src",), (".tsx",),
          ("apps/windows-gui/ui/src/design/",), TSX),
    Scope("Android", ("apps/android-native/app/src/main",), (".kt",),
          ("apps/android-native/app/src/main/java/io/anixops/net/ui/theme/BrandTokens.kt",), KOTLIN),
    Scope("Windows native", ("apps/windows-native/src",), (".xaml",),
          ("apps/windows-native/src/AnixOps.Net.App/Theme/BrandTokens.xaml",), XAML),
    Scope("Windows native code", ("apps/windows-native/src",), (".cs",), (), CSHARP),
    Scope("Linux native", ("apps/linux-native/data",), (".css",), ("apps/linux-native/data/brand-",), CSS),
    Scope("Linux native code", ("apps/linux-native/src",), (".rs",), (), RUST_GTK),
)

DISABLED_SELECTOR = re.compile(r":disabled|\[disabled\]|\[data-disabled\]|\[aria-disabled")
FADE = re.compile(r"(?<![-\w])opacity\s*:\s*(0?\.\d+|0\.\d+)\s*[;}]")


def strip_comments(line: str, suffix: str, in_block: bool) -> tuple[str, bool]:
    """Drop comments so prose about colours (`/* #fff is not used */`) is not a finding."""
    out = []
    i = 0
    while i < len(line):
        if in_block:
            end = line.find("*/", i)
            if end < 0:
                return "".join(out), True
            i = end + 2
            in_block = False
            continue
        if line.startswith("/*", i):
            in_block = True
            i += 2
            continue
        if suffix != ".css" and line.startswith("//", i):
            break
        if suffix == ".xaml" and line.startswith("<!--", i):
            break
        out.append(line[i])
        i += 1
    return "".join(out), in_block


def css_disabled_fades(text: str) -> list[tuple[int, str]]:
    """U3 for stylesheets: a rule whose selector is a disabled state and whose body fades the element."""
    found = []
    for match in re.finditer(r"([^{}]+)\{([^{}]*)\}", text):
        selector, body = match.group(1), match.group(2)
        if not DISABLED_SELECTOR.search(selector):
            continue
        fade = FADE.search(body + ";")
        if fade and float(fade.group(1)) < 1:
            line = text.count("\n", 0, match.start(2) + fade.start()) + 1
            found.append((line, selector.strip().splitlines()[-1].strip()))
    return found


def tracked(root: Path) -> list[str]:
    out = subprocess.run(["git", "-C", str(root), "ls-files", "-z"], check=True, capture_output=True).stdout
    return [p for p in out.decode("utf-8").split("\0") if p]


def check_file(path: str, text: str, scope: Scope) -> list[str]:
    suffix = Path(path).suffix
    findings = []
    in_block = False
    for number, raw in enumerate(text.splitlines(), 1):
        code, in_block = strip_comments(raw, suffix, in_block)
        for rule in scope.rules:
            if rule.pattern.search(code):
                findings.append(f"{path}:{number}: {rule.rule} {rule.message}\n    {raw.strip()[:160]}")
                break
    if suffix == ".css":
        cleaned = re.sub(r"/\*.*?\*/", lambda m: "\n" * m.group(0).count("\n"), text, flags=re.S)
        for number, selector in css_disabled_fades(cleaned):
            findings.append(f"{path}:{number}: U3 disabled state drawn by fading ({selector}); "
                            "use fill-1 behind label-3 text")
    return findings


def run(root: Path) -> int:
    findings: list[str] = []
    files = tracked(root)
    for scope in SCOPES:
        for path in files:
            if not path.startswith(scope.roots) or not path.endswith(scope.suffixes):
                continue
            if any(path.startswith(s) for s in scope.skip):
                continue
            if "/test/" in path or "/androidTest/" in path or path.endswith((".test.ts", ".test.tsx")):
                continue
            text = (root / path).read_text(encoding="utf-8")
            findings.extend(check_file(path, text, scope))
    for finding in findings:
        print(finding)
    if findings:
        print(f"\n{len(findings)} finding(s). Rules and fixes: docs/ui-ux.md, section 自动检查.", file=sys.stderr)
        return 1
    print("ui_lint: no findings")
    return 0


def self_test() -> int:
    swift = next(s for s in SCOPES if s.name == "Apple")
    css = next(s for s in SCOPES if s.name.startswith("Desktop (W"))
    kotlin = next(s for s in SCOPES if s.name == "Android")
    cases = [
        (swift, "a.swift", "static let red = Color(red: 0.9, green: 0.2, blue: 0.3)", "U1"),
        (swift, "a.swift", ".foregroundStyle(Color.orange)", "U1"),
        (swift, "a.swift", ".foregroundStyle(.orange)", "U1"),
        (swift, "a.swift", ".background(brand.primary.opacity(0.14))", "U2"),
        (swift, "a.swift", ".background(Color.white.opacity(0.08), in: Capsule())", "U1"),
        (swift, "a.swift", ".opacity(isEnabled ? 1 : 0.4)", "U3"),
        (swift, "a.swift", ".opacity(rule.enabled ? 1 : 0.45)", "U3"),
        (swift, "a.swift", ".opacity(isEnabled && configuration.isPressed ? 0.82 : 1)", None),
        (swift, "a.swift", ".background((tint ?? brand.primary).opacity(0.12), in: Circle())", "U2"),
        (swift, "a.swift", ".foregroundStyle(.secondary)", None),
        (swift, "a.swift", ".background(Color.clear)", None),
        (swift, "a.swift", ".background(brand.soft, in: Capsule())", None),
        (swift, "a.swift", "// Color(red: 1, green: 0, blue: 0) was too loud", None),
        (swift, "a.swift", ".opacity(0.6) // fading a whole view while it moves is not a colour", None),
        (css, "a.css", ".x { color: #fff; }", "U1"),
        (css, "a.css", ".x { background: rgb(0 0 0 / 5%); }", "U1"),
        (css, "a.css", ".x { background: color-mix(in srgb, var(--a) 5%, var(--b)); }", "U2"),
        (css, "a.css", "button:disabled { cursor: default; opacity: .45; }", "U3"),
        (css, "a.css", ".row.selected .row-label { opacity: 1; }", None),
        (css, "a.css", "label.toggle-row input { position: absolute; opacity: 0; }", None),
        (css, "a.css", ".x { color: var(--label-3); } /* not #fff */", None),
        (css, "a.css", ".x { white-space: nowrap; }", None),
        (kotlin, "a.kt", "val c = Color(0xFF5B63E6)", "U1"),
        (kotlin, "a.kt", "tint = MaterialTheme.colorScheme.primary.copy(alpha = 0.4f)", "U2"),
        (kotlin, "a.kt", "color = MaterialTheme.colorScheme.onSurfaceVariant", None),
    ]
    failed = 0
    for scope, path, line, expected in cases:
        got = check_file(path, line, scope)
        rule = got[0].split(": ", 2)[1].split(" ")[0] if got else None
        if rule != expected:
            failed += 1
            print(f"self-test: {line!r} gave {rule}, expected {expected}")
    print(f"ui_lint self-test: {len(cases) - failed}/{len(cases)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    sys.exit(run(ROOT))
