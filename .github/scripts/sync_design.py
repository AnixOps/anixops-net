#!/usr/bin/env python3
"""Vendor AnixOps Design (github.com/AnixOps/anixops-design) into the client, and check the vendored copy.

    python3 .github/scripts/sync_design.py --source ../anixops-design --ref <tag-or-commit>
    python3 .github/scripts/sync_design.py --check      # offline, for CI

--source is a git checkout of the design repository; files are read from --ref with `git show`, so the working
tree state does not matter. --check hashes the vendored files and compares them with manifest.json, which the
sync wrote. Never edit a vendored file by hand: change the design repository and sync again.

The vendored directory is apps/windows-gui/ui/src/design/anixops/. The desktop UI imports tokens.css and Inter
from there, .github/scripts/design_tokens.py reads tokens.json from there to generate the Apple palette, and the
app icons are copied from brand/ (see `--icons`).

Standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DESIGN_DIR = ROOT / "apps/windows-gui/ui/src/design/anixops"
REPOSITORY = "AnixOps/anixops-design"

# Vendored path (relative to DESIGN_DIR) -> path in the design repository.
FILES = {
    "LICENSE": "LICENSE",
    "LICENSE-BRAND.md": "LICENSE-BRAND.md",
    "tokens.json": "tokens/tokens.json",
    "tokens.css": "tokens/css/tokens.css",
    "fonts/inter/InterVariable-latin.woff2": "fonts/inter/InterVariable-latin.woff2",
    "fonts/inter/inter.css": "fonts/inter/inter.css",
    "fonts/inter/OFL.txt": "fonts/inter/OFL.txt",
    "brand/mark.svg": "brand/assets/mark.svg",
    "brand/mark-glyph.svg": "brand/assets/mark-glyph.svg",
    "brand/favicon.svg": "brand/assets/favicon.svg",
    "brand/app-icon.svg": "brand/assets/app-icon.svg",
    "brand/app-icon-1024.png": "brand/assets/app-icon/app-icon-1024.png",
    "brand/tauri/32x32.png": "brand/assets/tauri/32x32.png",
    "brand/tauri/128x128.png": "brand/assets/tauri/128x128.png",
    "brand/tauri/128x128@2x.png": "brand/assets/tauri/128x128@2x.png",
    "brand/tauri/icon.png": "brand/assets/tauri/icon.png",
    "brand/tauri/icon.ico": "brand/assets/tauri/icon.ico",
    "brand/tauri/icon.icns": "brand/assets/tauri/icon.icns",
}
GENERATED = ("VERSION", "NOTICE.md", "manifest.json")

# Product icon targets (relative to ROOT) -> vendored file, copied byte for byte (256 px PNGs for the desktop
# windows and the Linux hicolor/256x256 icon). The macOS AppIcon set is resized from the vendored icon.icns by
# `--icons` and is not byte-checked.
ICONS = {
    "apps/ios/Sources/NetworkCoreApp/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png": "brand/app-icon-1024.png",
    "apps/ios/Design/AppIcon.svg": "brand/app-icon.svg",
    "apps/windows-gui/icons/networkcore.png": "brand/tauri/128x128@2x.png",
    "apps/windows-gui/icons/icon.ico": "brand/tauri/icon.ico",
    "apps/linux-gui/icons/networkcore.png": "brand/tauri/128x128@2x.png",
    # The in-app mark (BrandMark in SwiftUI): the rounded slate tile with the gradient orbit A.
    "apps/ios/Sources/NetworkCoreApp/Resources/Assets.xcassets/BrandMark.imageset/BrandMark.png": "brand/tauri/icon.png",
    "apps/ios/Sources/NetworkCoreMac/Resources/Assets.xcassets/BrandMark.imageset/BrandMark.png": "brand/tauri/icon.png",
}


def fail(message: str) -> None:
    sys.exit(f"sync_design: {message}")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def version_of(tokens_css: bytes) -> str:
    head = tokens_css.split(b"\n", 1)[0].decode("utf-8")
    marker = "AnixOps Design "
    if marker not in head:
        fail("tokens.css header names no AnixOps Design version")
    return head.split(marker, 1)[1].rstrip(" .*/")


def notice(version: str, ref: str) -> str:
    return f"""# AnixOps Design {version} (vendored)

The files in this directory are copied unmodified from
https://github.com/{REPOSITORY} at `{ref}` by `.github/scripts/sync_design.py`.
Do not edit them here: change the design repository, then sync again.
CI (`sync_design.py --check`) verifies them against `manifest.json`.

Licences:

- `tokens.json`, `tokens.css`: MIT, see `LICENSE`.
- `fonts/inter/`: Inter, SIL Open Font License 1.1, see `fonts/inter/OFL.txt`.
- `brand/` (the AnixOps mark, glyph, favicon and app icons) and the name
  "AnixOps": **not** MIT. All rights reserved; use is limited to official
  AnixOps products, see `LICENSE-BRAND.md` and `/TRADEMARKS.md`. Forks and
  redistributions that are not official AnixOps releases must replace these
  assets with their own.
"""


def manifest(version: str, ref: str, files: dict[str, bytes]) -> str:
    entries = [
        {"path": path, "source": FILES[path], "bytes": len(data), "sha256": sha256(data)}
        for path, data in sorted(files.items())
    ]
    body = {"name": "AnixOps Design", "repository": f"https://github.com/{REPOSITORY}", "version": version,
            "ref": ref, "files": entries}
    return json.dumps(body, indent=2) + "\n"


def sync(source: Path, ref: str) -> None:
    files: dict[str, bytes] = {}
    for vendored, upstream in FILES.items():
        try:
            files[vendored] = subprocess.run(
                ["git", "-C", str(source), "show", f"{ref}:{upstream}"], check=True, capture_output=True
            ).stdout
        except subprocess.CalledProcessError as error:
            fail(f"cannot read {upstream} at {ref}: {error.stderr.decode().strip()}")
    commit = subprocess.run(["git", "-C", str(source), "rev-parse", f"{ref}^{{commit}}"], check=True,
                            capture_output=True, text=True).stdout.strip()
    pinned = ref if ref.startswith("v") else commit
    version = version_of(files["tokens.css"])
    if DESIGN_DIR.exists():
        shutil.rmtree(DESIGN_DIR)
    for vendored, data in files.items():
        target = DESIGN_DIR / vendored
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    (DESIGN_DIR / "VERSION").write_text(version + "\n", encoding="utf-8")
    (DESIGN_DIR / "NOTICE.md").write_text(notice(version, pinned), encoding="utf-8")
    (DESIGN_DIR / "manifest.json").write_text(manifest(version, pinned, files), encoding="utf-8")
    print(f"vendored AnixOps Design {version} ({pinned}) into {DESIGN_DIR.relative_to(ROOT)}")


MAC_ICONSET = ROOT / "apps/ios/Sources/NetworkCoreMac/Resources/Assets.xcassets/AppIcon.appiconset"


def copy_icons() -> None:
    for target, vendored in ICONS.items():
        destination = ROOT / target
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(DESIGN_DIR / vendored, destination)
        print(f"wrote {target}")
    try:
        from PIL import Image  # only needed for the macOS set; CI's --check does not import it
    except ImportError:
        fail("Pillow is needed to resize the macOS AppIcon set")
    source = Image.open(DESIGN_DIR / "brand/tauri/icon.icns")
    width, _, scale = max(source.info["sizes"], key=lambda entry: entry[0] * entry[2])
    source.size = (width, width)
    source.load(scale=scale)
    source = source.convert("RGBA")
    for size in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            name = f"AppIcon-{size}@{scale}x.png"
            source.resize((size * scale, size * scale), Image.LANCZOS).save(MAC_ICONSET / name, optimize=True)
            print(f"wrote {(MAC_ICONSET / name).relative_to(ROOT)}")


def check() -> None:
    manifest_path = DESIGN_DIR / "manifest.json"
    if not manifest_path.exists():
        fail(f"{manifest_path.relative_to(ROOT)} is missing")
    recorded = json.loads(manifest_path.read_text(encoding="utf-8"))
    problems = []
    listed = {entry["path"] for entry in recorded["files"]}
    if listed != set(FILES):
        problems.append("manifest file list differs from FILES in sync_design.py")
    files = {}
    for entry in recorded["files"]:
        path = DESIGN_DIR / entry["path"]
        if not path.exists():
            problems.append(f"missing {entry['path']}")
            continue
        data = path.read_bytes()
        files[entry["path"]] = data
        if sha256(data) != entry["sha256"]:
            problems.append(f"{entry['path']} differs from the vendored copy")
    actual = {str(p.relative_to(DESIGN_DIR)) for p in DESIGN_DIR.rglob("*") if p.is_file()}
    for extra in sorted(actual - set(FILES) - set(GENERATED)):
        problems.append(f"unexpected file {extra}")
    if not problems:
        version = recorded["version"]
        if version_of(files["tokens.css"]) != version:
            problems.append("tokens.css version differs from manifest.json")
        if (DESIGN_DIR / "VERSION").read_text(encoding="utf-8") != version + "\n":
            problems.append("VERSION differs from manifest.json")
        if (DESIGN_DIR / "NOTICE.md").read_text(encoding="utf-8") != notice(version, recorded["ref"]):
            problems.append("NOTICE.md differs from what the sync writes")
        if manifest_path.read_text(encoding="utf-8") != manifest(version, recorded["ref"], files):
            problems.append("manifest.json is not in the sync's format")
        for target, vendored in ICONS.items():
            if (ROOT / target).read_bytes() != files[vendored]:
                problems.append(f"{target} is not the AnixOps Design icon (run with --icons)")
    if problems:
        fail("; ".join(problems))
    print(f"AnixOps Design {recorded['version']} ({recorded['ref']}): vendored files match manifest.json")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--source", type=Path)
    parser.add_argument("--ref")
    parser.add_argument("--icons", action="store_true", help="also copy the product icons from the vendored set")
    args = parser.parse_args()
    if args.check:
        check()
        return
    if args.source and args.ref:
        sync(args.source, args.ref)
    elif not args.icons:
        parser.error("--source and --ref, --icons or --check is required")
    if args.icons:
        copy_icons()


if __name__ == "__main__":
    main()
