#!/usr/bin/env python3
"""Every Windows entry point a person sees carries the AnixOps Net icon.

An advertised MSI shortcut does not take the icon of the program it opens: without an Icon it shows the blank
file icon in the Start menu and in search (seen in 1.0.4). This checks the sources:

- installer/windows-native/Package.wxs: every <Shortcut> names an <Icon>
  that the package defines, ARPPRODUCTICON (the Installed apps entry) names one too, and each <Icon> file
  exists and is the AnixOps Design icon;
- the native window's project sets ApplicationIcon to that file.

.github/scripts/check-windows-icons.ps1 checks the built MSI tables and executables on Windows.

--self-test runs the rules on broken samples first.
"""

from __future__ import annotations

import argparse
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WIX = "{http://wixtoolset.org/schemas/v4/wxs}"
DESIGN_ICON = ROOT / "third_party/anixops-design/brand/products/net/icon.ico"
PACKAGE = "installer/windows-native/Package.wxs"
NATIVE_PROJECT = "apps/windows-native/src/AnixOps.Net.App/AnixOps.Net.App.csproj"


def icon_file(wxs: Path, source: str) -> Path:
    path = source.replace("$(sys.SOURCEFILEDIR)", str(wxs.parent) + "/").replace("\\", "/")
    if "$(" in path:
        raise ValueError(f"Icon SourceFile {source!r} uses a variable other than sys.SOURCEFILEDIR")
    return Path(path).resolve()


def package_problems(wxs: Path, text: str, design: bytes) -> list[str]:
    name = wxs.relative_to(ROOT) if wxs.is_relative_to(ROOT) else wxs
    problems: list[str] = []
    root = ET.fromstring(text)
    icons: dict[str, str] = {}
    for icon in root.iter(f"{WIX}Icon"):
        icons[icon.get("Id", "")] = icon.get("SourceFile", "")
    for icon_id, source in icons.items():
        if not icon_id.lower().endswith(".ico"):
            problems.append(f"{name}: Icon {icon_id!r} must end in .ico (advertised shortcuts need the extension)")
        try:
            path = icon_file(wxs, source)
        except ValueError as error:
            problems.append(f"{name}: {error}")
            continue
        if not path.is_file():
            problems.append(f"{name}: Icon {icon_id!r} file {source} does not exist")
        elif path.read_bytes() != design:
            problems.append(f"{name}: Icon {icon_id!r} file {source} is not the AnixOps Design icon")
    shortcuts = list(root.iter(f"{WIX}Shortcut"))
    if not shortcuts:
        problems.append(f"{name}: no Start menu shortcut")
    for shortcut in shortcuts:
        icon = shortcut.get("Icon")
        if not icon:
            problems.append(f"{name}: shortcut {shortcut.get('Id')!r} has no Icon")
        elif icon not in icons:
            problems.append(f"{name}: shortcut {shortcut.get('Id')!r} names Icon {icon!r}, which is not defined")
    arp = [p.get("Value") for p in root.iter(f"{WIX}Property") if p.get("Id") == "ARPPRODUCTICON"]
    if not arp:
        problems.append(f"{name}: no ARPPRODUCTICON (the Installed apps entry has no icon)")
    elif arp[0] not in icons:
        problems.append(f"{name}: ARPPRODUCTICON names Icon {arp[0]!r}, which is not defined")
    return problems


def project_problems(project: Path, text: str, design: bytes) -> list[str]:
    match = re.search(r"<ApplicationIcon>([^<]+)</ApplicationIcon>", text)
    if not match:
        return [f"{NATIVE_PROJECT}: no ApplicationIcon (AnixOpsNet.exe has no icon)"]
    path = project.parent / match.group(1).replace("\\", "/")
    if not path.is_file():
        return [f"{NATIVE_PROJECT}: ApplicationIcon {match.group(1)} does not exist"]
    if path.read_bytes() != design:
        return [f"{NATIVE_PROJECT}: ApplicationIcon {match.group(1)} is not the AnixOps Design icon"]
    return []


def check() -> list[str]:
    design = DESIGN_ICON.read_bytes()
    path = ROOT / PACKAGE
    problems = package_problems(path, path.read_text(encoding="utf-8"), design)
    project = ROOT / NATIVE_PROJECT
    problems += project_problems(project, project.read_text(encoding="utf-8"), design)
    return problems


def self_test() -> None:
    design = DESIGN_ICON.read_bytes()
    wxs = ROOT / PACKAGE
    good = (
        '<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs"><Package>'
        '<Icon Id="A.ico" SourceFile="$(sys.SOURCEFILEDIR)..\\..\\apps\\windows-native\\src\\AnixOps.Net.App\\Assets\\AnixOpsNet.ico" />'
        '<Property Id="ARPPRODUCTICON" Value="A.ico" />'
        '<File><Shortcut Id="S" Icon="A.ico" Advertise="yes" /></File></Package></Wix>'
    )
    cases = {
        "good": (good, 0),
        "shortcut without icon": (good.replace(' Icon="A.ico"', ""), 1),
        "no ARPPRODUCTICON": (good.replace('<Property Id="ARPPRODUCTICON" Value="A.ico" />', ""), 1),
        "undefined icon": (good.replace('Icon="A.ico" Advertise', 'Icon="B.ico" Advertise'), 1),
        "missing file": (good.replace("icon.ico\"", "nothing.ico\""), 1),
        "wrong file": (good.replace("windows-native\\src\\AnixOps.Net.App\\Assets\\AnixOpsNet.ico", "linux-native\\data\\anixops-net.png"), 1),
        "no shortcut": (good.replace('<File><Shortcut Id="S" Icon="A.ico" Advertise="yes" /></File>', ""), 1),
    }
    for label, (text, expected) in cases.items():
        found = len(package_problems(wxs, text, design))
        if found != expected:
            sys.exit(f"windows_icons self-test: {label}: {found} problems, expected {expected}")
    project = ROOT / NATIVE_PROJECT
    if not project_problems(project, "<Project></Project>", design):
        sys.exit("windows_icons self-test: a project without ApplicationIcon passed")
    print("windows_icons self-test: ok")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    problems = check()
    if problems:
        sys.exit("windows_icons:\n  " + "\n  ".join(problems))
    print("windows_icons: shortcuts, Installed apps entries and executables carry the AnixOps Net icon")


if __name__ == "__main__":
    main()
