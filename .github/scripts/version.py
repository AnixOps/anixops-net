#!/usr/bin/env python3
"""The client version: one source, every copy checked, and the release tag checked against it.

The source is `version` under [workspace.package] in Cargo.toml (every crate inherits it, and with it
env!("CARGO_PKG_VERSION") in the subscription User-Agent, crash reports, metrics and diagnostics). The other build
systems cannot read Cargo.toml, so they hold copies, which must equal it:

  Cargo.lock                                         every workspace member's entry (builds use --locked)
  apps/windows-native/Directory.Build.props          <Version>
  apps/windows-native/src/AnixOps.Net.App/app.manifest   assemblyIdentity version (X.Y.Z.0)
  apps/ios/project.yml                               MARKETING_VERSION (iOS and macOS CFBundleShortVersionString)
  apps/android-native/app/build.gradle.kts           versionName default (builds without -Panixops.versionName)

The source holds the plain MAJOR.MINOR.PATCH; a prerelease tag adds its suffix (v1.0.0-rc.1 builds from 1.0.0).

Usage:
  version.py check [--tag vX.Y.Z[-alpha|beta|rc.N]] [--rev REV]
      Every copy equals the source; with --tag the source also equals the tag without "v" and its prerelease
      suffix. --rev reads the files at that commit (git show REV:path) instead of the working tree, as
      release-tag.yml does for the commit it is about to tag.
  version.py set X.Y.Z
      Writes X.Y.Z into the source and every copy (Cargo.lock: only the workspace members' entries). This is the
      whole version bump.
  version.py --self-test
"""

import json
import os
import re
import subprocess
import sys
import tempfile

SOURCE = "Cargo.toml"
TAG = re.compile(r"^v(\d+\.\d+\.\d+)(-(alpha|beta|rc)\.[1-9][0-9]*)?$")
PLAIN = re.compile(r"^\d+\.\d+\.\d+$")


class Tree:
    """Reads repository files from the working tree or from a commit."""

    def __init__(self, root, rev=None):
        self.root = root
        self.rev = rev

    def read(self, path):
        if self.rev is None:
            with open(os.path.join(self.root, path), encoding="utf-8") as f:
                return f.read()
        return subprocess.run(
            ["git", "-C", self.root, "show", f"{self.rev}:{path}"],
            check=True, capture_output=True, text=True,
        ).stdout

    def write(self, path, text):
        if self.rev is not None:
            raise SystemExit("cannot write files at a commit")
        with open(os.path.join(self.root, path), "w", encoding="utf-8", newline="") as f:
            f.write(text)


# Each copy: (path, pattern whose group 2 is the version, how the version is written there).
# Patterns keep groups 1 and 3 (the text around the version) so `set` rewrites only the version.
COPIES = [
    ("apps/windows-native/Directory.Build.props", re.compile(r"(<Version>)([^<]*)(</Version>)"), "{}"),
    ("apps/windows-native/src/AnixOps.Net.App/app.manifest", re.compile(r'(<assemblyIdentity version=")([^"]*)(")'), "{}.0"),
    ("apps/ios/project.yml", re.compile(r'(^    MARKETING_VERSION: ")([^"]*)(")', re.M), "{}"),
    ("apps/android-native/app/build.gradle.kts",
     re.compile(r'(^\s*versionName = findProperty\("anixops\.versionName"\) as String\? \?: ")([^"]*)(")', re.M), "{}"),
]
SOURCE_PATTERN = re.compile(r'(\[workspace\.package\][^\[]*?^version = ")([^"]*)(")', re.M | re.S)


def source_version(tree):
    match = SOURCE_PATTERN.search(tree.read(SOURCE))
    if not match:
        raise SystemExit(f"{SOURCE}: no version under [workspace.package]")
    return match.group(2)


def workspace_members(tree):
    """Package names of the workspace members that inherit the workspace version."""
    text = tree.read(SOURCE)
    block = re.search(r"^members = \[(.*?)\]", text, re.M | re.S)
    if not block:
        raise SystemExit(f"{SOURCE}: no workspace members")
    names = []
    for member in re.findall(r'"([^"]+)"', block.group(1)):
        manifest = tree.read(f"{member}/Cargo.toml")
        package = re.search(r"^\[package\](.*?)(?=^\[|\Z)", manifest, re.M | re.S)
        if not package:
            continue
        name = re.search(r'^name = "([^"]+)"', package.group(1), re.M)
        if name and re.search(r"^version\.workspace = true", package.group(1), re.M):
            names.append(name.group(1))
    return names


LOCK_ENTRY = re.compile(r'(\[\[package\]\]\nname = "([^"]+)"\nversion = ")([^"]*)("\n)((?:(?!\[\[package\]\]).)*)', re.S)


def lock_versions(tree, members):
    """{member: version} from Cargo.lock's path entries (no source line) of the workspace members."""
    found = {}
    for match in LOCK_ENTRY.finditer(tree.read("Cargo.lock")):
        name, version, rest = match.group(2), match.group(3), match.group(5)
        if name in members and not re.search(r"^source = ", rest, re.M):
            found[name] = version
    return found


def copies(tree):
    """[(label, version or None)] of every copy."""
    result = []
    for path, pattern, form in COPIES:
        match = pattern.search(tree.read(path))
        if not match:
            result.append((path, None))
            continue
        value = match.group(2)
        # app.manifest is four-part: X.Y.Z.0 stands for X.Y.Z.
        if form == "{}.0":
            value = value[:-2] if value.endswith(".0") and value.count(".") == 3 else f"(not X.Y.Z.0: {value})"
        result.append((path, value))
    members = workspace_members(tree)
    locked = lock_versions(tree, members)
    for name in members:
        result.append((f"Cargo.lock ({name})", locked.get(name)))
    return result


def check(tree, tag=None):
    errors = []
    version = source_version(tree)
    if not PLAIN.match(version):
        errors.append(f"{SOURCE}: version {version!r} must be MAJOR.MINOR.PATCH (a tag adds the prerelease suffix)")
    for label, value in copies(tree):
        if value is None:
            errors.append(f"{label}: version not found")
        elif value != version:
            errors.append(f"{label}: {value} differs from {SOURCE} {version}; run .github/scripts/version.py set {version}")
    if tag is not None:
        match = TAG.match(tag)
        if not match:
            errors.append(f"tag {tag!r} must look like v1.0.0, v1.0.0-beta.1 or v1.0.0-rc.1")
        elif match.group(1) != version:
            errors.append(
                f"tag {tag} is version {match.group(1)}, but the source is {version}: "
                f"bump it first with .github/scripts/version.py set {match.group(1)}"
            )
    return version, errors


def set_version(tree, version):
    if not PLAIN.match(version):
        raise SystemExit(f"{version!r} is not MAJOR.MINOR.PATCH")
    text = tree.read(SOURCE)
    tree.write(SOURCE, SOURCE_PATTERN.sub(lambda m: m.group(1) + version + m.group(3), text, count=1))
    for path, pattern, form in COPIES:
        text = tree.read(path)
        new, count = pattern.subn(lambda m: m.group(1) + form.format(version) + m.group(3), text, count=1)
        if count != 1:
            raise SystemExit(f"{path}: version not found")
        tree.write(path, new)
    members = set(workspace_members(tree))

    def lock_entry(match):
        if match.group(2) in members and not re.search(r"^source = ", match.group(5), re.M):
            return match.group(1) + version + match.group(4) + match.group(5)
        return match.group(0)

    tree.write("Cargo.lock", LOCK_ENTRY.sub(lock_entry, tree.read("Cargo.lock")))


def run(argv, root):
    if not argv:
        raise SystemExit(__doc__)
    command, rest = argv[0], argv[1:]
    if command == "set":
        if len(rest) != 1:
            raise SystemExit("usage: version.py set X.Y.Z")
        set_version(Tree(root), rest[0])
        print(f"version set to {rest[0]}")
        return 0
    if command != "check":
        raise SystemExit(__doc__)
    tag = rev = None
    while rest:
        if rest[0] == "--tag" and len(rest) > 1:
            tag, rest = rest[1], rest[2:]
        elif rest[0] == "--rev" and len(rest) > 1:
            rev, rest = rest[1], rest[2:]
        else:
            raise SystemExit(__doc__)
    version, errors = check(Tree(root, rev), tag)
    for error in errors:
        print(f"::error::{error}")
    if errors:
        return 1
    print(f"version {version}: every copy matches" + (f", tag {tag} matches" if tag else ""))
    return 0


# --- self-test -------------------------------------------------------------------------------------------------

FIXTURE = {
    "Cargo.toml": '[workspace]\nmembers = [\n    "apps/a",\n    "crates/b",\n]\nresolver = "2"\n\n'
                  '[workspace.package]\nversion = "0.3.0"\nedition = "2021"\n\n[workspace.dependencies]\n'
                  'serde = { version = "1.0.0" }\n',
    "apps/a/Cargo.toml": '[package]\nname = "app-a"\nversion.workspace = true\n\n[dependencies]\n'
                         'gtk = { version = "2" }\n',
    "crates/b/Cargo.toml": '[package]\nname = "crate-b"\nversion.workspace = true\n',
    "Cargo.lock": '# generated\nversion = 4\n\n'
                  '[[package]]\nname = "app-a"\nversion = "0.3.0"\ndependencies = [\n "crate-b",\n]\n\n'
                  '[[package]]\nname = "crate-b"\nversion = "0.3.0"\n\n'
                  '[[package]]\nname = "dtor"\nversion = "0.3.0"\n'
                  'source = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "00"\n',
    "apps/windows-native/Directory.Build.props": "<Project>\n  <PropertyGroup>\n    <Version>0.3.0</Version>\n"
                                                 "  </PropertyGroup>\n</Project>\n",
    "apps/windows-native/src/AnixOps.Net.App/app.manifest": '<?xml version="1.0" encoding="utf-8"?>\n'
        '<assembly manifestVersion="1.0">\n  <assemblyIdentity version="0.3.0.0" name="AnixOps.Net.App" />\n'
        '</assembly>\n',
    "apps/ios/project.yml": 'settings:\n  base:\n    SWIFT_VERSION: "5.10"\n    MARKETING_VERSION: "0.3.0"\n'
                            '    CURRENT_PROJECT_VERSION: "1"\n',
    "apps/android-native/app/build.gradle.kts": '        versionCode = 3007\n'
        '        versionName = findProperty("anixops.versionName") as String? ?: "0.3.0"\n',
}


def self_test():
    failures = []

    def fixture():
        root = tempfile.mkdtemp()
        for path, text in FIXTURE.items():
            os.makedirs(os.path.dirname(os.path.join(root, path)) or root, exist_ok=True)
            with open(os.path.join(root, path), "w", encoding="utf-8") as f:
                f.write(text)
        return root

    def expect(label, root, tag, want_ok):
        _version, errors = check(Tree(root), tag)
        if (not errors) != want_ok:
            failures.append(f"{label}: expected {'pass' if want_ok else 'failure'}, got {errors or 'pass'}")

    root = fixture()
    expect("consistent", root, None, True)
    expect("stable tag", root, "v0.3.0", True)
    expect("prerelease tag", root, "v0.3.0-rc.2", True)
    expect("tag of another version", root, "v1.0.0", False)
    expect("prerelease tag of another version", root, "v1.0.0-beta.1", False)
    expect("malformed tag", root, "1.0.0", False)
    expect("malformed suffix", root, "v0.3.0-preview.1", False)

    for path in FIXTURE:
        if path in ("apps/a/Cargo.toml", "crates/b/Cargo.toml"):
            continue
        drifted = fixture()
        with open(os.path.join(drifted, path), encoding="utf-8") as f:
            text = f.read()
        if path == "Cargo.toml":
            text = text.replace('version = "0.3.0"', 'version = "0.3.1"')
        elif path == "Cargo.lock":
            text = text.replace('name = "crate-b"\nversion = "0.3.0"', 'name = "crate-b"\nversion = "0.2.9"')
        else:
            text = text.replace("0.3.0", "0.3.1", 1)
        with open(os.path.join(drifted, path), "w", encoding="utf-8") as f:
            f.write(text)
        expect(f"drift in {path}", drifted, None, False)

    bumped = fixture()
    set_version(Tree(bumped), "1.0.0")
    expect("after set", bumped, "v1.0.0", True)
    expect("after set, old tag", bumped, "v0.3.0", False)
    with open(os.path.join(bumped, "Cargo.lock"), encoding="utf-8") as f:
        lock = f.read()
    if 'name = "dtor"\nversion = "0.3.0"' not in lock:
        failures.append("set changed a registry package that happens to share the version")
    with open(os.path.join(bumped, "apps/windows-native/src/AnixOps.Net.App/app.manifest"), encoding="utf-8") as f:
        if 'assemblyIdentity version="1.0.0.0"' not in f.read():
            failures.append("set did not write the four-part manifest version")
    with open(os.path.join(bumped, "Cargo.toml"), encoding="utf-8") as f:
        if 'serde = { version = "1.0.0" }' not in f.read():
            failures.append("set changed a dependency version in Cargo.toml")
    try:
        set_version(Tree(fixture()), "1.0.0-rc.1")
        failures.append("set accepted a prerelease version")
    except SystemExit:
        pass

    for failure in failures:
        print(f"::error::self-test: {failure}")
    if failures:
        return 1
    print("version.py self-test: all cases pass")
    return 0


def main():
    if sys.argv[1:] == ["--self-test"]:
        sys.exit(self_test())
    root = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True).stdout.strip()
    sys.exit(run(sys.argv[1:], root or os.getcwd()))


if __name__ == "__main__":
    main()
