#!/usr/bin/env python3
"""Fail when interface code picks its own colours instead of AnixOps Design tokens (docs/ui-ux.md, "自动检查").

    python3 .github/scripts/ui_lint.py            # check every platform's interface sources
    python3 .github/scripts/ui_lint.py --self-test

Colours reach the interface only through the generated token files (.github/scripts/design_tokens.py): the
Swift `brand.*` / `Brand.*` palette, the desktop CSS variables, BrandTokens.kt, BrandTokens.xaml and the GTK
brand-*.css. Every pair of those tokens is checked for contrast in AnixOps Design (guidelines/color.md), so a
colour written anywhere else is a colour nobody checked, in one theme at least. Six rules, the first three about
colour on every platform, the next three about the structure of the SwiftUI screens (Apple has no cheap runtime
harness, so what the other platforms measure by running the page is read from the source here), the last two about
the words in every platform's string tables:

  U1 colour-literal   a colour written in the code: hex, rgb(), Color(red:), Color.orange, Color(0xFF...), "#AARRGGBB"
  U2 derived-colour   a token made lighter or mixed: `.opacity()` on a colour, color-mix(), .copy(alpha = ...).
                      The tints the interface needs exist as tokens: accent-soft, success-soft, warning-soft,
                      danger-soft, fill-1/2/3, label-2/3, separator.
  U3 disabled-opacity a disabled control drawn by fading it. Faded accent reads as a grey-blue block in dark mode
                      and the text drops below 3:1. Disabled is fill-1 behind label-3 text (actions.md H10).
  U4 settings-budget  the first screen of SettingsView (iOS) and MacSettingsPage (macOS), the `Form` only, has more
                      than 8 sections or 16 rows (density.md N25). Advanced and privacy items live on sub-pages.
                      A section view the composite table below does not know fails, so none is left uncounted.
  U5 fold-user-lists  a ForEach over a list the user adds (plugins, scripts, Wi-Fi rules, rules, hosts, mappings,
                      redirects) inside a Form / List / Section shows every row. Fold it with .prefix(n) or a
                      shown / visible slice, or put it on a pushed page that is the list (N26).
  U6 master-switch    a Toggle in the same Section as such a ForEach: the master switch is a row among the items
                      instead of its own section above them (N27). A Toggle inside the ForEach rows is per item.
  U7 formal-copy      a string a user sees, in any of the three languages, uses second person, a colloquial word or
                      particle, a contraction, a filler or an exclamation mark (voice-and-tone.md W1-W3). The word
                      list is formal-copy.json, a byte-identical copy of AnixOps Design guidelines/formal-copy.json.
  U8 no-explanation   such a string explains instead of stating: an explanatory connective (because, so that,
                      这样, 以便 ... W5) or more than two sentences (W6). Cut the reason and the mechanism (W4).

U7 and U8 read the string tables, not the code: Localizable.xcstrings and InfoPlist.xcstrings (Apple), i18n-en.ts and
i18n-zh-hant.ts with their Chinese keys (desktop), values*/strings.xml (Android) and client-strings catalogue.json
(Windows and Linux native). Placeholders (%@, %1$s, {name}) are removed before the words are matched.

There is no allow list and no inline marker: a string that needs an exception is rewritten; a colour the tokens do not have is added to AnixOps Design and
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


# ---------------------------------------------------------------------------------------------------------------
# SwiftUI structure rules (U4-U6). The source is masked first (comments and string contents blanked, same length,
# so offsets and line numbers still match), then read by brace matching; no Swift parser is needed for this.

SETTINGS_SECTION_BUDGET = 8
SETTINGS_ROW_BUDGET = 16

# (file, struct) of every settings first screen. A missing file or struct is a finding, so a rename cannot switch
# the check off.
SETTINGS_PAGES = (
    ("apps/ios/Sources/NetworkCoreAppHost/SettingsView.swift", "SettingsView"),
    ("apps/ios/Sources/NetworkCoreMac/MacPages.swift", "MacSettingsPage"),
)

# Views a settings Form may use as a whole section, as (sections, rows) in their worst case on the first screen,
# read from the views themselves. OnDemandSections says "Sections" but draws a single Section. A view ending in
# Section / Sections that is not here fails U4: read it, add it, so new sections are never left uncounted.
SETTINGS_COMPOSITES = {
    "AppModeSection": (1, 1),     # AppModeViews.swift: the mode picker
    "OnDemandSections": (1, 2),   # OnDemandSettingsView.swift: auto reconnect switch, Wi-Fi rules link
    "AboutSection": (1, 4),       # DeveloperView.swift: version, 用户协议, 隐私政策, 开发者模式 once unlocked
    "MacUpdateSection": (1, 3),   # MacPages.swift: auto-check switch, check row, download / install button
}

# A row is one of these used directly in the Form or in a Section of it. What sits inside one (a Label in a
# NavigationLink, a Link in a LabeledContent) belongs to that row. SettingsSubpageRow is the macOS push row.
SETTINGS_ROW_VIEWS = ("Toggle", "Picker", "NavigationLink", "Button", "Link", "LabeledContent", "Label", "TextField",
                      "SettingsSubpageRow")

# What counts as a list the user adds (U5 / U6): any identifier in the collection expression that is one of these
# words. The one place to extend when a new kind of user list appears.
USER_LIST_WORD = re.compile(r"plugins?|scripts?|scriptReferences|wifi|rules?|hosts?|mappings?|redirects?", re.I)
# A slice that shows only the first rows: .prefix(n), or a name that says so (visible(...), shown, shownNodes(...)).
# The fold row ("显示全部 N 个" / "收起") is the author's job; the lint only knows the list is not shown whole.
FOLDED_NAME = re.compile(r"(?:visible|shown|folded|collapsed|capped|limited)\w*")

PAIRS = {"(": ")", "{": "}", "[": "]"}


def mask_swift(text: str) -> str:
    """Blank comments and string contents (keeping quotes and newlines) so braces in them never count."""
    out = list(text)
    n = len(text)
    i = 0
    stack: list[list] = []  # ["str", triple] or ["interp", paren depth]
    comment = 0

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        top = stack[-1] if stack else None
        if comment:
            if text.startswith("/*", i):
                comment += 1
                blank(i, i + 2)
                i += 2
            elif text.startswith("*/", i):
                comment -= 1
                blank(i, i + 2)
                i += 2
            else:
                blank(i, i + 1)
                i += 1
        elif top and top[0] == "str":
            if text.startswith("\\(", i):
                stack.append(["interp", 1])
                blank(i, i + 2)
                i += 2
            elif text[i] == "\\":
                blank(i, i + 2)
                i += 2
            elif (top[1] and text.startswith('"""', i)) or (not top[1] and text[i] == '"'):
                i += 3 if top[1] else 1
                stack.pop()
            else:
                blank(i, i + 1)
                i += 1
        elif text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end < 0 else end
            blank(i, end)
            i = end
        elif text.startswith("/*", i):
            comment = 1
            blank(i, i + 2)
            i += 2
        elif text.startswith('"""', i):
            stack.append(["str", True])
            i += 3
        elif text[i] == '"':
            stack.append(["str", False])
            i += 1
        elif top and top[0] == "interp" and text[i] in "()":
            top[1] += 1 if text[i] == "(" else -1
            if top[1] == 0:
                stack.pop()
                blank(i, i + 1)
            i += 1
        else:
            i += 1
    return "".join(out)


def close_of(masked: str, i: int) -> int:
    """Index just after the bracket group opening at masked[i]; the end of the text when it never closes."""
    opener = masked[i]
    closer = PAIRS[opener]
    depth = 0
    for k in range(i, len(masked)):
        if masked[k] == opener:
            depth += 1
        elif masked[k] == closer:
            depth -= 1
            if depth == 0:
                return k + 1
    return len(masked)


TRAILING_CLOSURE = re.compile(r"\s*(?:\w+\s*:\s*)?(?=\{)")


def call_end(masked: str, i: int) -> int:
    """End of a call whose arguments or first closure open at masked[i]: the (...) group, then every trailing
    closure, `label: { ... }` ones included (Section { } header: { } footer: { }, NavigationLink { } label: { })."""
    k = close_of(masked, i) if masked[i] == "(" else i
    while True:
        if masked[k:k + 1] == "{" and k == i:
            k = close_of(masked, k)
            continue
        m = TRAILING_CLOSURE.match(masked, k)
        if not m:
            return k
        k = close_of(masked, m.end())


def bracket_after(masked: str, pos: int) -> int | None:
    """Index of the ( or { that follows masked[pos:] after blanks, if any."""
    m = re.compile(r"[ \t]*(?=[({])").match(masked, pos)
    return m.end() if m else None


def line_of(text: str, pos: int) -> int:
    return text.count("\n", 0, pos) + 1


def struct_body(masked: str, name: str) -> tuple[int, int] | None:
    """Span of `var body: some View { ... }` inside `struct name`."""
    m = re.search(rf"\bstruct\s+{re.escape(name)}\b[^{{]*\{{", masked)
    if not m:
        return None
    end = close_of(masked, m.end() - 1)
    b = re.compile(r"\bvar\s+body\s*:\s*some\s+View\s*\{").search(masked, m.end(), end)
    if not b:
        return None
    return b.end() - 1, close_of(masked, b.end() - 1)


def settings_budget(masked: str, name: str) -> tuple[int, int, list[str]] | None:
    """(sections, rows, section views missing from SETTINGS_COMPOSITES) of the first `Form` in `struct name`."""
    body = struct_body(masked, name)
    if not body:
        return None
    form = re.compile(r"(?<![\w.])Form\s*\{").search(masked, body[0], body[1])
    if not form:
        return None
    start = form.end() - 1
    end = close_of(masked, start)
    token = re.compile(
        r"(?<![\w.])(?:(Section)(?=\s*[({])|(\w*Sections?)\b|(" + "|".join(SETTINGS_ROW_VIEWS) + r")(?=\s*[({]))"
    )
    sections = rows = 0
    unknown: list[str] = []
    k = start + 1
    while True:
        m = token.search(masked, k, end)
        if not m:
            break
        k = m.end()
        if m.group(1):
            sections += 1  # its rows are counted as the scan goes on inside it
            continue
        if m.group(2):
            known = SETTINGS_COMPOSITES.get(m.group(2))
            if known:
                sections += known[0]
                rows += known[1]
            else:
                unknown.append(m.group(2))
        else:
            rows += 1
        opening = bracket_after(masked, k)
        if opening is not None:
            k = call_end(masked, opening)
    return sections, rows, unknown


def check_settings(path: str, text: str, name: str) -> list[str]:
    result = settings_budget(mask_swift(text), name)
    if result is None:
        return [f"{path}:1: U4 settings-budget: no `Form` found in the body of {name}; update SETTINGS_PAGES in ui_lint.py"]
    sections, rows, unknown = result
    found = []
    for view in dict.fromkeys(unknown):
        found.append(f"{path}:1: U4 settings-budget: {name} uses {view}, which is not in SETTINGS_COMPOSITES; read it, "
                     "count its sections and rows, and add it to the table in ui_lint.py")
    if sections > SETTINGS_SECTION_BUDGET:
        found.append(f"{path}:1: U4 settings-budget: {name} first screen has {sections} sections, over "
                     f"{SETTINGS_SECTION_BUDGET}; move the rarely used ones to a sub-page (N25)")
    if rows > SETTINGS_ROW_BUDGET:
        found.append(f"{path}:1: U4 settings-budget: {name} first screen has {rows} rows, over "
                     f"{SETTINGS_ROW_BUDGET}; move the rarely used ones to a sub-page (N25)")
    return found


def pushed_pages(masked: str) -> set[str]:
    """Names of views built inside the destination of a NavigationLink or a navigationDestination."""
    names: set[str] = set()
    for m in re.finditer(r"(?<![\w.])NavigationLink\b|\.navigationDestination\b", masked):
        opening = bracket_after(masked, m.end())
        if opening is None:
            continue
        dest = None
        if masked[opening] == "(":
            args_end = close_of(masked, opening)
            args = masked[opening:args_end]
            following = bracket_after(masked, args_end)
            trailing = following if following is not None and masked[following] == "{" else None
            if "destination:" in args:
                dest = (opening, args_end)
            elif trailing is not None and (m.group(0).startswith(".") or not re.search(r"\bvalue\s*:", args)):
                dest = (trailing, close_of(masked, trailing))
        else:
            dest = (opening, close_of(masked, opening))
        if dest:
            names.update(re.findall(r"(?<![\w.])([A-Z]\w*)\s*\(", masked[dest[0]:dest[1]]))
    return names


def is_list_page(masked: str, name: str) -> bool:
    """A page that is the list: the root of its body is a List or a Form."""
    body = struct_body(masked, name)
    return bool(body and re.compile(r"\s*(?:List|Form)\s*[({]").match(masked, body[0] + 1))


def user_list_foreach(masked: str) -> list[tuple[int, int, str]]:
    """(start, end, collection expression) of every ForEach over a list the user adds."""
    found = []
    for m in re.finditer(r"(?<![\w.])ForEach\s*\(", masked):
        opening = m.end() - 1
        close = close_of(masked, opening)
        depth = 0
        arg_end = close - 1
        for k in range(opening + 1, close - 1):
            c = masked[k]
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
            elif c == "," and depth == 0:
                arg_end = k
                break
        expr = masked[opening + 1:arg_end].strip()
        if any(USER_LIST_WORD.fullmatch(w) for w in re.findall(r"[A-Za-z_]\w*", expr)):
            found.append((m.start(), call_end(masked, opening), expr))
    return found


def containers(masked: str, names: str) -> list[tuple[int, int]]:
    spans = []
    for m in re.finditer(rf"(?<![\w.])(?:{names})(?=\s*[({{])", masked):
        opening = bracket_after(masked, m.end())
        if opening is not None:
            spans.append((m.start(), call_end(masked, opening)))
    return spans


def check_lists(path: str, text: str, pages: set[str]) -> list[str]:
    """U5 and U6 for one Swift file; `pages` is every view in the tree that a NavigationLink pushes."""
    masked = mask_swift(text)
    lists = user_list_foreach(masked)
    if not lists:
        return []
    structs = [(m.start(), close_of(masked, m.end() - 1), m.group(1))
               for m in re.finditer(r"\bstruct\s+(\w+)\b[^{]*\{", masked)]
    holders = containers(masked, "Form|List|Section")
    sections = containers(masked, "Section")
    every_foreach = [(m.start(), call_end(masked, m.end() - 1)) for m in re.finditer(r"(?<![\w.])ForEach\s*\(", masked)]
    found = []
    for start, _, expr in lists:
        words = re.findall(r"[A-Za-z_]\w*", expr)
        folded = ".prefix(" in expr or any(FOLDED_NAME.fullmatch(w) for w in words)
        owner = min((s for s in structs if s[0] < start < s[1]), key=lambda s: s[1] - s[0], default=None)
        page = owner is not None and owner[2] in pages and is_list_page(masked, owner[2])
        if any(s[0] < start < s[1] for s in holders) and not folded and not page:
            found.append(f"{path}:{line_of(text, start)}: U5 fold-user-lists: ForEach over `{expr}` shows every row; "
                         "fold it with .prefix(n) or a shown / visible slice plus 显示全部, or move it to a pushed "
                         "page that is the list (N26)")
        mine = [s for s in sections if s[0] < start < s[1]]
        if mine:
            section = min(mine, key=lambda s: s[1] - s[0])
            for t in re.finditer(r"(?<![\w.])Toggle\s*[({]", masked[section[0]:section[1]]):
                at = section[0] + t.start()
                if not any(a < at < b for a, b in every_foreach):  # a switch inside the rows is per item
                    found.append(f"{path}:{line_of(text, at)}: U6 master-switch-section: a Toggle in the same Section "
                                 f"as the list `{expr}`; give the switch its own section above the list (N27)")
    return found


# --- U7 / U8: the words in the string tables (AnixOps Design voice-and-tone.md, W1-W7) ---------------------------

COPY_RULES_FILE = Path(__file__).resolve().parent / "formal-copy.json"
PLACEHOLDER = re.compile(r"%(?:\d+\$)?(?:ll|l|h)?[@dusfxXc%]|\{[A-Za-z_][\w.]*\}|\\[nt'\"@?]")
SENTENCE_END = re.compile(r"[。！？]+|(?<=[\w)\]'\"’”%])[.!?]+(?=\s|$)")


@dataclass(frozen=True)
class CopyEntry:
    path: str
    key: str
    lang: str  # "zh" (Simplified or Traditional) or "en"
    text: str


def copy_rules() -> list[tuple[str, str, re.Pattern[str], str]]:
    import json

    data = json.loads(COPY_RULES_FILE.read_text(encoding="utf-8"))
    out = []
    for rule in data["rules"]:
        lint = "U8" if rule["id"] in ("W4", "W5", "W6") else "U7"
        out.append((lint, rule["lang"], re.compile(rule["pattern"]), f'{rule["id"]} {rule["use"]}'))
    return out


def max_sentences() -> int:
    import json

    return int(json.loads(COPY_RULES_FILE.read_text(encoding="utf-8"))["max_sentences"])


def sentences(text: str) -> int:
    parts = [p for p in SENTENCE_END.split(text) if p.strip(" \n\t\"'”’）)】」…")]
    return len(parts)


def xcstrings_entries(path: str, text: str) -> list[CopyEntry]:
    import json

    out = []
    for key, entry in json.loads(text).get("strings", {}).items():
        if entry.get("shouldTranslate") is False:
            continue
        locs = entry.get("localizations", {})
        if "zh-Hans" not in locs and re.search(r"[一-鿿]", key):
            out.append(CopyEntry(path, key, "zh", key))
        for lang, loc in locs.items():

            def values(node: object) -> list[str]:
                if isinstance(node, dict):
                    if "stringUnit" in node:
                        return [node["stringUnit"].get("value", "")]
                    return [v for child in node.values() for v in values(child)]
                return []

            for value in values(loc):
                out.append(CopyEntry(path, key, "en" if lang == "en" else "zh", value))
    return out


TS_PAIR = re.compile(r'^\s*("(?:[^"\\]|\\.)*")\s*:\s*("(?:[^"\\]|\\.)*")\s*,?\s*$', re.M)


def ts_entries(path: str, text: str, lang: str, keys: bool) -> list[CopyEntry]:
    """Desktop tables map the Chinese source text (the key) to English or Traditional Chinese."""
    import json

    out = []
    for m in TS_PAIR.finditer(text):
        key, value = json.loads(m.group(1)), json.loads(m.group(2))
        if keys:
            out.append(CopyEntry(path, key, "zh", key.split("|")[0]))
        out.append(CopyEntry(path, key, lang, value))
    return out


def android_entries(path: str, text: str) -> list[CopyEntry]:
    import xml.etree.ElementTree as ET

    lang = "zh" if "/values-zh" in path else "en"
    out = []
    for node in ET.fromstring(text):
        if node.get("translatable") == "false":
            continue
        if node.tag == "string":
            out.append(CopyEntry(path, node.get("name", ""), lang, "".join(node.itertext())))
        elif node.tag in ("plurals", "string-array"):
            for item in node:
                out.append(CopyEntry(path, node.get("name", ""), lang, "".join(item.itertext())))
    return out


def catalogue_entries(path: str, text: str) -> list[CopyEntry]:
    import json

    out = []
    for key, langs in json.loads(text).items():
        for lang, value in langs.items():
            out.append(CopyEntry(path, key, "en" if lang == "en" else "zh", value))
    return out


def copy_entries(root: Path, files: list[str]) -> list[CopyEntry]:
    out: list[CopyEntry] = []
    for path in files:
        if path.startswith("apps/ios/Sources/") and path.endswith(".xcstrings"):
            out.extend(xcstrings_entries(path, (root / path).read_text(encoding="utf-8")))
        elif path == "apps/windows-gui/ui/src/i18n-en.ts":
            out.extend(ts_entries(path, (root / path).read_text(encoding="utf-8"), "en", keys=True))
        elif path == "apps/windows-gui/ui/src/i18n-zh-hant.ts":
            out.extend(ts_entries(path, (root / path).read_text(encoding="utf-8"), "zh", keys=False))
        elif re.fullmatch(r"apps/android-native/app/src/main/res/values(?:-zh(?:-r\w+)?)?/strings\.xml", path):
            out.extend(android_entries(path, (root / path).read_text(encoding="utf-8")))
        elif path == "crates/client-strings/catalogue.json":
            out.extend(catalogue_entries(path, (root / path).read_text(encoding="utf-8")))
    return out


def check_copy(entries: list[CopyEntry]) -> list[str]:
    rules = copy_rules()
    limit = max_sentences()
    findings = []
    seen: set[tuple[str, str, str]] = set()
    for entry in entries:
        text = PLACEHOLDER.sub(" ", entry.text)
        if (entry.path, entry.lang, text) in seen:
            continue
        seen.add((entry.path, entry.lang, text))
        problems = [message for lint, lang, pattern, message in rules
                    if lang in (entry.lang, "any") and pattern.search(text)]
        lints = ["U8" if message.startswith(("W4", "W5", "W6")) else "U7" for message in problems]
        if sentences(text) > limit:
            problems.append(f"W6 more than {limit} sentences; keep what the control does and cut the explanation (W4)")
            lints.append("U8")
        if problems:
            findings.append(f"{entry.path}: {lints[0]} [{entry.key[:60]}] {'; '.join(problems)}\n    {entry.text[:160]}")
    return findings


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


def check_structure(root: Path, files: list[str]) -> list[str]:
    """U4 on the settings pages, U5 and U6 on every Apple Swift file."""
    swift = [p for p in files if p.startswith("apps/ios/Sources/") and p.endswith(".swift")]
    texts = {p: (root / p).read_text(encoding="utf-8") for p in swift}
    pages: set[str] = set()
    for text in texts.values():
        pages |= pushed_pages(mask_swift(text))
    findings = []
    for path, name in SETTINGS_PAGES:
        if path not in texts:
            findings.append(f"{path}:1: U4 settings-budget: file not found; update SETTINGS_PAGES in ui_lint.py")
            continue
        findings.extend(check_settings(path, texts[path], name))
    for path, text in texts.items():
        findings.extend(check_lists(path, text, pages))
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
    findings.extend(check_structure(root, files))
    findings.extend(check_copy(copy_entries(root, files)))
    for finding in findings:
        print(finding)
    if findings:
        print(f"\n{len(findings)} finding(s). Rules and fixes: docs/ui-ux.md, section 自动检查.", file=sys.stderr)
        return 1
    print("ui_lint: no findings")
    return 0


def settings_form(sections: int = 1, rows: int = 1, extra: str = "") -> str:
    """A SettingsView whose first Form has `sections` empty sections and `rows` Toggle rows in the first one."""
    body = "Section {\n" + "Toggle(\"a\", isOn: $a)\n" * rows + "}\n" + "Section { }\n" * (sections - 1)
    return f"struct SettingsView: View {{\n var body: some View {{\n NavigationStack {{\n Form {{\n{body}{extra}\n}}\n}}\n}}\n}}\n"


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
    structure = [
        ("U4", settings_form(sections=9), True),
        ("U4", settings_form(sections=8), False),
        ("U4", settings_form(rows=17), True),
        ("U4", settings_form(rows=16), False),
        ("U4", settings_form(extra="NewThingSection(tunnel: tunnel)"), True),
        ("U4", settings_form(extra="AppModeSection(tunnel: tunnel)"), False),
        # Braces and brackets in strings and comments are not structure.
        ("U4", settings_form(extra='Text("}} { \\(a ? "}" : "{") ")\n// Section {\nSection { Toggle("a", isOn: $a) }'), False),
        # Rows inside a row belong to it; rows in the sub-page function are not the first screen.
        ("U4", settings_form(rows=16, extra='NavigationLink { Toggle("x", isOn: $x) } label: { Label("y", systemImage: "z") }'),
         True),
    ]
    for rule, source, expect in structure:
        got = [f for f in check_settings("S.swift", source, "SettingsView") if " U4 " in f]
        if bool(got) != expect:
            failed += 1
            print(f"self-test: U4 settings page gave {got or 'no finding'}, expected {'a finding' if expect else 'none'}")
    lists = [
        ("U5", "struct P: View { var body: some View { Form { Section { ForEach(model.library.plugins) { p in Text(p) } } } } }", set(), True),
        ("U5", "struct P: View { var body: some View { Form { Section { ForEach(model.scriptReferences, id: \\.path) { r in Text(r) } } } } }", set(), True),
        ("U5", "struct P: View { var body: some View { Form { Section { ForEach($settings.wifi) { $r in Text(r) } } } } }", set(), True),
        ("U5", "struct P: View { var body: some View { Form { Section { ForEach(model.plugins.prefix(5)) { p in Text(p) } } } } }", set(), False),
        ("U5", "struct P: View { var body: some View { Form { Section { ForEach(visible(model.plugins, showsAll: a)) { p in Text(p) } } } } }", set(), False),
        ("U5", "struct P: View { var body: some View { Form { Section { ForEach(shown) { p in Text(p) } } } } }", set(), False),
        # The page that is the list, pushed by a NavigationLink: every row shows. Not pushed, or not a List root: fold.
        ("U5", "struct RulesPage: View { var body: some View { List { Section { ForEach(rules.rules) { r in Text(r) } } } } }", {"RulesPage"}, False),
        ("U5", "struct RulesPage: View { var body: some View { List { Section { ForEach(rules.rules) { r in Text(r) } } } } }", set(), True),
        ("U5", "struct RulesPage: View { var body: some View { VStack { Section { ForEach(rules.rules) { r in Text(r) } } } } }", {"RulesPage"}, True),
        # Not a user list, or not in a Form / List / Section.
        ("U5", "struct P: View { var body: some View { Form { Section { ForEach(Appearance.choices) { p in Text(p) } } } } }", set(), False),
        ("U5", "struct P: View { var body: some View { VStack { ForEach(hosts) { h in Text(h) } } } }", set(), False),
        ("U5", "struct P: View { var body: some View { Form { Section { ForEach(LocalRules.Policy.allCases) { p in Text(p) } } } } }", set(), False),
        ("U6", "struct P: View { var body: some View { List { Section { Toggle(\"all\", isOn: $on)\nForEach(rules.prefix(5)) { r in Text(r) } } } } }", set(), True),
        ("U6", "struct P: View { var body: some View { List { Section { Toggle(\"all\", isOn: $on) }\nSection { ForEach(rules.prefix(5)) { r in Text(r) } } } } }", set(), False),
        ("U6", "struct P: View { var body: some View { List { Section { ForEach(rules.prefix(5)) { r in Toggle(r.name, isOn: $r.on) } } } } }", set(), False),
        ("U6", "struct P: View { var body: some View { List { Section { Toggle(\"x\", isOn: $on)\nForEach(Mode.allCases) { r in Text(r) } } } } }", set(), False),
    ]
    for rule, source, pages, expect in lists:
        got = [f for f in check_lists("P.swift", source, pages) if f" {rule} " in f]
        if bool(got) != expect:
            failed += 1
            print(f"self-test: {rule} {source[:90]!r} gave {got or 'no finding'}, expected {'a finding' if expect else 'none'}")
    pushes = [
        ("NavigationLink { RulesPage(a: 1) } label: { Text(\"r\") }", {"RulesPage"}),
        ("NavigationLink(destination: RulesPage()) { Text(\"r\") }", {"RulesPage"}),
        ("NavigationLink(\"r\") { RulesPage() }", {"RulesPage"}),
        (".navigationDestination(item: $x) { page in RulesPage(page) }", {"RulesPage"}),
        ("NavigationLink(value: Page.rules) { Label(\"r\", systemImage: \"x\") }", set()),
        ("NavigationLink { Text(\"d\") } label: { RulesRow() }", set()),
    ]
    for source, expect in pushes:
        if pushed_pages(mask_swift(source)) - {"Text", "Label"} != expect:
            failed += 1
            print(f"self-test: pushed_pages({source!r}) gave {pushed_pages(mask_swift(source))}, expected {expect}")
    copy = [
        ("zh", "只打开你信任的来源。", "U7"),
        ("zh", "暂无插件", None),
        ("zh", "还没有插件", "U7"),
        ("zh", "粘贴网址导入试试吧", "U7"),
        ("zh", "连接断开了", "U7"),
        ("zh", "别关闭窗口", "U7"),
        ("zh", "按类别分组，识别失败时分别显示别名", None),
        ("zh", "统一下载", None),
        ("zh", "开启后生成根证书，这样才能解密 HTTPS。", "U8"),
        ("zh", "第一句。第二句。第三句。", "U8"),
        ("zh", "第一句。第二句。", None),
        ("zh", "已导入 %lld 个节点", None),
        ("en", "Turn on only plugins you trust.", "U7"),
        ("en", "Couldn't import the plugin.", "U7"),
        ("en", "Import failed: the certificate has expired.", None),
        ("en", "Done!", "U7"),
        ("en", "Matches 1.1.1.1 and example.com; host != null is not a sentence end.", None),
        ("en", "Opens YouTube", None),
        ("en", "The proxy stays on so that requests keep working.", "U8"),
        ("en", "One. Two. Three.", "U8"),
        ("en", "Wildcards * and ? are supported. The prefix - excludes a domain.", None),
        ("en", "Show all {count} plugins", None),
    ]
    for lang, text, expected in copy:
        got = check_copy([CopyEntry("t.json", "k", lang, text)])
        rule = got[0].split(": ", 2)[1].split(" ")[0] if got else None
        if rule != expected:
            failed += 1
            print(f"self-test: copy {text!r} gave {rule}, expected {expected}")
    tables = [
        ('{"strings": {"只打开你信任的来源": {"localizations": {"en": {"stringUnit": {"state": "translated", "value": "Trusted only"}}}}}}',
         xcstrings_entries, 2),
        ('<resources><string name="a">A</string><plurals name="b"><item quantity="one">B</item></plurals>'
         '<string name="c" translatable="false">C</string></resources>', android_entries, 2),
        ('{"a": {"zh": "甲", "zh-Hant": "甲", "en": "A"}}', catalogue_entries, 3),
    ]
    for source, reader, count in tables:
        entries = reader("apps/android-native/app/src/main/res/values/strings.xml" if reader is android_entries else "t", source)
        if len(entries) != count:
            failed += 1
            print(f"self-test: {reader.__name__} read {len(entries)} entries, expected {count}")
    ts = ts_entries("i18n-en.ts", '  "关闭|verb": "Close",\n  "你好": "Hello",\n', "en", keys=True)
    if [(e.lang, e.text) for e in ts] != [("zh", "关闭"), ("en", "Close"), ("zh", "你好"), ("en", "Hello")]:
        failed += 1
        print(f"self-test: ts_entries gave {ts}")
    total = len(cases) + len(structure) + len(lists) + len(pushes) + len(copy) + len(tables) + 1
    print(f"ui_lint self-test: {total - failed}/{total} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    sys.exit(run(ROOT))
