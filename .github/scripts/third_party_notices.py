#!/usr/bin/env python3
"""Builds THIRD_PARTY_NOTICES.txt for one release artifact from the package managers' own metadata.

Inputs (each optional, as the artifact needs):
  --cargo LABEL=PATH     `cargo about generate --format json` output for one Cargo workspace (the client's
                         Cargo.lock, or the kernel's for the binaries built from it). Every crate's license text
                         comes from the crate itself (cargo-about), grouped by text. AnixOps's own crates are left out.
  --gradle-deps PATH     `gradle :app:dependencies --configuration releaseRuntimeClasspath` of the Android app;
                         each module's license comes from its POM (following parent POMs) in --gradle-cache, or from
                         the Maven repository when the cache has no POM. Only Apache-2.0 modules are accepted (its
                         text is --apache-text); any other or missing license fails, to be reviewed by hand.
  --extra PATH           a hand-kept notice (third_party/notices/*.txt) for code that no package metadata
                         describes: C libraries compiled in by -sys crates, DLLs shipped beside the client, fonts.
  --require NAME         a package that must appear (guards against a generator silently producing an empty list).

The output starts with the source offer: where AnixOps Net's own source is, and for every MPL-2.0 component
(MPL-2.0 §3.2) where its source code can be obtained.

Usage:
  third_party_notices.py build --title TITLE --out FILE [inputs...]
  third_party_notices.py --self-test
"""

import glob
import json
import os
import re
import sys
import tempfile
import urllib.request
import xml.etree.ElementTree as ElementTree

PROJECT_SOURCE = "https://github.com/AnixOps/anixops-net"
OWN_REPOSITORIES = ("https://github.com/AnixOps/",)
APACHE_NAMES = re.compile(r"apache.*(2\.0|version 2)|^apache-2\.0$|apache license, version 2\.0", re.I)
APACHE_URLS = ("apache.org/licenses/license-2.0",)
MAVEN_REPOSITORIES = ("https://dl.google.com/dl/android/maven2", "https://repo.maven.apache.org/maven2")
GOOGLE_GROUPS = ("androidx.", "com.android.", "com.google.android.")


class NoticeError(Exception):
    pass


# --- Cargo ---------------------------------------------------------------------------------------------------------

def is_own_crate(package):
    """AnixOps's own crates: path or git crates of an AnixOps repository under its Apache-2.0 license."""
    repository = (package.get("repository") or "").rstrip("/")
    own_repository = any(repository.startswith(prefix) for prefix in OWN_REPOSITORIES)
    from_registry = (package.get("source") or "").startswith("registry+")
    return own_repository and not from_registry and (package.get("license") or "Apache-2.0") == "Apache-2.0"


def crate_source_url(package):
    source = package.get("source") or ""
    if source.startswith("registry+https://github.com/rust-lang/crates.io-index") or source == "sparse+https://index.crates.io/":
        return f"https://crates.io/api/v1/crates/{package['name']}/{package['version']}/download"
    return package.get("repository") or ""


def read_cargo(label, path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    errors = []
    components = []
    for entry in data.get("crates", []):
        package = entry["package"]
        if is_own_crate(package):
            continue
        license_info = str(entry.get("license", ""))
        if license_info in ("Unknown", ""):
            errors.append(f"{label}: no license could be determined for crate {package['name']} {package['version']}")
            continue
        if license_info == "Ignore":
            continue
        components.append({
            "ecosystem": f"Rust crate ({label})",
            "name": package["name"],
            "version": package["version"],
            "license": package.get("license") or license_info,
            "source": crate_source_url(package),
        })
    texts = []
    mpl = []
    for license in data.get("licenses", []):
        users = [u["crate"] for u in license.get("used_by", []) if not is_own_crate(u["crate"])]
        if not users:
            continue
        texts.append({
            "title": f"{license.get('name') or license['id']} ({license['id']})",
            "users": sorted({f"{u['name']} {u['version']}" for u in users}),
            "text": license.get("text", ""),
        })
        if license["id"] == "MPL-2.0":
            for user in users:
                mpl.append({"name": user["name"], "version": user["version"], "source": crate_source_url(user),
                            "repository": user.get("repository") or ""})
    return components, texts, mpl, errors


# --- Gradle --------------------------------------------------------------------------------------------------------

DEPENDENCY_LINE = re.compile(
    r"^[|+\\ -]*--- (?P<group>[\w.\-]+):(?P<name>[\w.\-]+)(?::(?P<version>[\w.\-+]+))?"
    r"(?: -> (?P<resolved>[\w.\-+]+))?(?P<rest>.*)$"
)


def gradle_modules(text):
    """{(group, name, version)} of a `gradle dependencies` tree, without constraints and unresolved entries."""
    modules = set()
    for line in text.splitlines():
        match = DEPENDENCY_LINE.match(line)
        if not match:
            continue
        rest = match.group("rest")
        if "(c)" in rest or "(n)" in rest or "FAILED" in rest:
            continue
        version = match.group("resolved") or match.group("version")
        if not version:
            continue
        modules.add((match.group("group"), match.group("name"), version))
    return modules


def strip_namespaces(element):
    for node in element.iter():
        if isinstance(node.tag, str) and "}" in node.tag:
            node.tag = node.tag.split("}", 1)[1]
    return element


def fetch_pom(group, name, version, cache):
    pattern = os.path.join(cache, group, name, version, "*", f"{name}-{version}.pom")
    for path in sorted(glob.glob(pattern)):
        with open(path, encoding="utf-8", errors="replace") as f:
            return f.read()
    repositories = MAVEN_REPOSITORIES if group.startswith(GOOGLE_GROUPS) else MAVEN_REPOSITORIES[::-1]
    relative = f"{group.replace('.', '/')}/{name}/{version}/{name}-{version}.pom"
    for repository in repositories:
        try:
            with urllib.request.urlopen(f"{repository}/{relative}", timeout=30) as response:
                return response.read().decode("utf-8", errors="replace")
        except Exception:  # noqa: BLE001 - try the next repository
            continue
    return None


def pom_licenses(group, name, version, cache, depth=0):
    """[(name, url)] from the POM, or from its parents when it declares none."""
    if depth > 8:
        return []
    text = fetch_pom(group, name, version, cache)
    if text is None:
        return None
    root = strip_namespaces(ElementTree.fromstring(text.encode("utf-8")))
    licenses = [((node.findtext("name") or "").strip(), (node.findtext("url") or "").strip())
                for node in root.findall("./licenses/license")]
    if licenses:
        return licenses
    parent = root.find("./parent")
    if parent is None:
        return []
    return pom_licenses(parent.findtext("groupId", "").strip(), parent.findtext("artifactId", "").strip(),
                        parent.findtext("version", "").strip(), cache, depth + 1)


def is_apache(license_name, license_url):
    return bool(APACHE_NAMES.search(license_name)) or any(u in license_url.lower() for u in APACHE_URLS)


def read_gradle(path, cache, apache_text):
    with open(path, encoding="utf-8") as f:
        modules = gradle_modules(f.read())
    components, errors, users = [], [], []
    if not modules:
        errors.append(f"{path}: no Gradle modules found")
    for group, name, version in sorted(modules):
        coordinates = f"{group}:{name} {version}"
        licenses = pom_licenses(group, name, version, cache)
        if licenses is None:
            errors.append(f"Gradle module {coordinates}: POM not found in the Gradle cache or the Maven repositories")
            continue
        if not licenses:
            errors.append(f"Gradle module {coordinates}: its POM declares no license")
            continue
        if not any(is_apache(n, u) for n, u in licenses):
            errors.append(f"Gradle module {coordinates}: license {licenses} is not Apache-2.0; add its notice by hand")
            continue
        components.append({"ecosystem": "Android library (Gradle)", "name": f"{group}:{name}", "version": version,
                           "license": "Apache-2.0", "source": ""})
        users.append(coordinates)
    texts = []
    if users:
        texts.append({"title": "Apache License 2.0 (Apache-2.0)", "users": users, "text": read_text(apache_text)})
    return components, texts, [], errors


# --- output --------------------------------------------------------------------------------------------------------

def read_text(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read().replace("\r\n", "\n").strip()


def render(title, components, texts, mpl, extras):
    rule = "=" * 78
    out = [
        f"{title}: third-party software notices",
        rule,
        "",
        "AnixOps Net is open source under the Apache License 2.0. Its complete source code, including the",
        f"build scripts used to make this artifact, is available at {PROJECT_SOURCE}.",
        "",
        "This artifact contains the third-party software listed below, each under its own license. The full",
        "license texts and copyright notices follow the list.",
        "",
        "SOURCE CODE OF MPL-2.0 COMPONENTS",
        "-" * 78,
    ]
    if mpl:
        out += [
            "The following components are distributed under the Mozilla Public License 2.0 and are used",
            "unmodified. As MPL-2.0 section 3.2 requires, their source code is available from these addresses:",
            "",
        ]
        for entry in sorted(mpl, key=lambda e: (e["name"], e["version"])):
            line = f"  - {entry['name']} {entry['version']}: {entry['source']}"
            if entry.get("repository") and entry["repository"] != entry["source"]:
                line += f" (repository: {entry['repository']})"
            out.append(line)
        out += ["", "The MPL-2.0 text is included below. On request, contact@anixops.com also sends these sources."]
    else:
        out.append("None of the components below is under the Mozilla Public License.")
    out += ["", "COMPONENTS", "-" * 78]
    seen = set()
    for component in sorted(components, key=lambda c: (c["ecosystem"], c["name"].lower(), c["version"])):
        key = (component["ecosystem"], component["name"], component["version"])
        if key in seen:
            continue
        seen.add(key)
        out.append(f"  {component['name']} {component['version']} [{component['license']}] ({component['ecosystem']})")
    for extra in extras:
        out.append(f"  {read_text(extra).splitlines()[0]}")
    out += ["", "LICENSE TEXTS AND NOTICES", "-" * 78]
    for extra in extras:
        out += ["", rule, read_text(extra)]
    for text in texts:
        out += ["", rule, text["title"], "Used by: " + ", ".join(text["users"]), "", text["text"].strip()]
    return "\n".join(out).rstrip() + "\n"


def build(args):
    title = out_path = None
    cargo, extras, required = [], [], []
    gradle_deps = gradle_cache = apache_text = None
    rest = list(args)
    while rest:
        option = rest.pop(0)
        if not rest:
            raise NoticeError(f"{option} needs a value")
        value = rest.pop(0)
        if option == "--title":
            title = value
        elif option == "--out":
            out_path = value
        elif option == "--cargo":
            label, _, path = value.partition("=")
            if not path:
                raise NoticeError("--cargo takes LABEL=PATH")
            cargo.append((label, path))
        elif option == "--gradle-deps":
            gradle_deps = value
        elif option == "--gradle-cache":
            gradle_cache = value
        elif option == "--apache-text":
            apache_text = value
        elif option == "--extra":
            extras.append(value)
        elif option == "--require":
            required.append(value)
        else:
            raise NoticeError(f"unknown option {option}")
    if not title or not out_path:
        raise NoticeError("--title and --out are required")
    if gradle_deps and not (gradle_cache and apache_text):
        raise NoticeError("--gradle-deps needs --gradle-cache and --apache-text")

    components, texts, mpl, errors = [], [], [], []
    for label, path in cargo:
        parts = read_cargo(label, path)
        components += parts[0]; texts += parts[1]; mpl += parts[2]; errors += parts[3]
    if gradle_deps:
        parts = read_gradle(gradle_deps, gradle_cache, apache_text)
        components += parts[0]; texts += parts[1]; mpl += parts[2]; errors += parts[3]
    for extra in extras:
        if not os.path.isfile(extra) or not read_text(extra):
            errors.append(f"{extra}: missing or empty")
    names = {c["name"] for c in components}
    extra_text = "\n".join(read_text(e) for e in extras if os.path.isfile(e))
    for name in required:
        if name not in names and name not in extra_text:
            errors.append(f"required component {name} is not in the notices")
    if mpl and not any(t["title"].endswith("(MPL-2.0)") for t in texts):
        errors.append("MPL-2.0 components are listed but the MPL-2.0 text is missing")
    if errors:
        raise NoticeError("\n".join(errors))
    text = render(title, components, texts, mpl, extras)
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"{out_path}: {len({(c['ecosystem'], c['name'], c['version']) for c in components})} components, "
          f"{len(mpl)} MPL-2.0, {len(extras)} hand-kept notices")


# --- self-test -----------------------------------------------------------------------------------------------------

def self_test():
    failures = []
    work = tempfile.mkdtemp()

    def write(name, content):
        path = os.path.join(work, name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(content if isinstance(content, str) else json.dumps(content))
        return path

    def crate(name, version, license, repository="", source="registry+https://github.com/rust-lang/crates.io-index"):
        return {"name": name, "version": version, "license": license, "repository": repository, "source": source}

    ring = crate("ring", "0.17.14", "Apache-2.0 AND ISC")
    cssparser = crate("cssparser", "0.36.0", "MPL-2.0", "https://github.com/servo/rust-cssparser")
    own = crate("client-core", "0.3.0", "Apache-2.0", "https://github.com/AnixOps/anixops-net", None)
    vendored = crate("netstack-smoltcp", "0.2.4", "MIT OR Apache-2.0", "", None)
    cargo_ok = write("cargo.json", {
        "overview": [],
        "licenses": [
            {"name": "ISC License", "id": "ISC", "text": "ISC text ring", "used_by": [{"crate": ring, "path": None}]},
            {"name": "Mozilla Public License 2.0", "id": "MPL-2.0", "text": "MPL text",
             "used_by": [{"crate": cssparser, "path": None}]},
            {"name": "Apache License 2.0", "id": "Apache-2.0", "text": "Apache text",
             "used_by": [{"crate": own, "path": None}, {"crate": vendored, "path": None}]},
        ],
        "crates": [{"package": ring, "license": "Apache-2.0 AND ISC"},
                   {"package": cssparser, "license": "MPL-2.0"},
                   {"package": own, "license": "Apache-2.0"},
                   {"package": vendored, "license": "MIT OR Apache-2.0"}],
    })
    cargo_unknown = write("cargo-unknown.json", {
        "overview": [], "licenses": [],
        "crates": [{"package": crate("mystery", "1.0.0", None), "license": "Unknown"}],
    })
    apache = write("LICENSE", "Apache License\nVersion 2.0, January 2004")
    cache = os.path.join(work, "gradle")
    write("gradle/androidx.core/core/1.16.0/abc/core-1.16.0.pom",
          '<project xmlns="http://maven.apache.org/POM/4.0.0"><licenses><license>'
          "<name>The Apache Software License, Version 2.0</name>"
          "<url>http://www.apache.org/licenses/LICENSE-2.0.txt</url></license></licenses></project>")
    write("gradle/com.google.guava/listenablefuture/1.0/abc/listenablefuture-1.0.pom",
          "<project><parent><groupId>com.google.guava</groupId><artifactId>guava-parent</artifactId>"
          "<version>26.0-android</version></parent></project>")
    write("gradle/com.google.guava/guava-parent/26.0-android/abc/guava-parent-26.0-android.pom",
          "<project><licenses><license><name>The Apache Software License, Version 2.0</name></license>"
          "</licenses></project>")
    write("gradle/org.example/gpl/1.0/abc/gpl-1.0.pom",
          "<project><licenses><license><name>GNU General Public License</name></license></licenses></project>")
    gradle_ok = write("deps.txt",
                      "releaseRuntimeClasspath - Runtime classpath of /release.\n"
                      "+--- androidx.core:core:1.16.0\n"
                      "|    \\--- com.google.guava:listenablefuture:1.0 (*)\n"
                      "+--- androidx.core:core -> 1.16.0 (c)\n"
                      "\\--- project :core\n")
    gradle_gpl = write("deps-gpl.txt", "+--- org.example:gpl:1.0\n")
    extra = write("third_party/quickjs-ng.txt", "QuickJS-ng (bundled)\nhttps://github.com/quickjs-ng/quickjs\n\nMIT")
    out = os.path.join(work, "NOTICES.txt")

    def run(label, args, want_ok):
        try:
            build(args + ["--title", "AnixOps Net test", "--out", out])
            ok = True
            detail = ""
        except NoticeError as error:
            ok = False
            detail = str(error)
        if ok != want_ok:
            failures.append(f"{label}: expected {'success' if want_ok else 'failure'} {detail}")

    run("cargo, gradle and extra", ["--cargo", f"client={cargo_ok}",
                                         "--gradle-deps", gradle_ok, "--gradle-cache", cache, "--apache-text", apache,
                                         "--extra", extra, "--require", "ring",
                                         "--require", "QuickJS-ng"], True)
    text = read_text(out) if os.path.exists(out) else ""
    for expected in ["cssparser 0.36.0: https://crates.io/api/v1/crates/cssparser/0.36.0/download",
                     "ring 0.17.14 [Apache-2.0 AND ISC]", "netstack-smoltcp 0.2.4",
                     "androidx.core:core 1.16.0 [Apache-2.0]",
                     "com.google.guava:listenablefuture 1.0", "QuickJS-ng (bundled)", "MPL text", PROJECT_SOURCE]:
        if expected not in text:
            failures.append(f"output lacks {expected!r}")
    if "client-core" in text:
        failures.append("output lists AnixOps's own crate")
    # The component list names each module once; the "Used by" line of its license text names it again.
    if text.count("  androidx.core:core 1.16.0 [") != 1:
        failures.append("a Gradle constraint line was listed as a module")
    run("unknown crate license", ["--cargo", f"client={cargo_unknown}"], False)
    run("non-Apache Gradle module", ["--gradle-deps", gradle_gpl, "--gradle-cache", cache, "--apache-text", apache],
        False)
    run("missing required component", ["--cargo", f"client={cargo_ok}", "--require", "rquickjs-sys"], False)
    run("missing extra file", ["--extra", os.path.join(work, "nope.txt")], False)
    if gradle_modules("+--- a:b:1.0 (n)\n+--- c:d -> 2.0\n") != {("c", "d", "2.0")}:
        failures.append("gradle_modules parsing")

    for failure in failures:
        print(f"::error::self-test: {failure}")
    if failures:
        return 1
    print("third_party_notices self-test: all cases pass")
    return 0


def main():
    args = sys.argv[1:]
    if args == ["--self-test"]:
        sys.exit(self_test())
    if not args or args[0] != "build":
        sys.exit(__doc__)
    try:
        build(args[1:])
    except NoticeError as error:
        for line in str(error).splitlines():
            print(f"::error::{line}")
        sys.exit(1)


if __name__ == "__main__":
    main()
