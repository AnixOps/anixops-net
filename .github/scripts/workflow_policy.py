#!/usr/bin/env python3
"""Supply-chain and signing-key rules for the workflows and composite actions (CI, first job).

1. Every action a workflow or composite action uses is either local (./...) or pinned to a full 40-digit commit SHA
   with its release tag in a comment ("uses: owner/repo@<sha> # v1.2.3"). A moved tag of a third-party action then
   cannot run new code in a job that holds a key; Dependabot (.github/dependabot.yml) proposes the updates.
2. Signing secrets stay in the job that needs them and in its protected environment:
   - ANDROID_KEYSTORE_* / ANDROID_KEY_* (the Android release signing key) only in release.yml, only in jobs that
     declare the android-release environment, so pull requests, branch runs and CI never receive it;
   - PLAY_UPLOAD_* / PLAY_SERVICE_ACCOUNT_JSON only in jobs that declare the google-play environment.

Usage: workflow_policy.py [--self-test] [ROOT]
"""

import os
import re
import sys

USES = re.compile(r"^\s*(?:-\s+)?uses:\s*(\S+)\s*(#.*)?$")
PINNED = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.\-/]+@[0-9a-f]{40}$")
TAG_COMMENT = re.compile(r"^#\s*v\d+(\.\d+)*\S*$")
# secret prefix -> (environment the job must declare, workflow files allowed to use it or None for any)
SECRET_RULES = [
    (re.compile(r"secrets\.(ANDROID_KEYSTORE_|ANDROID_KEY_)\w*"), "android-release", {"release.yml"}),
    (re.compile(r"secrets\.(PLAY_UPLOAD_\w*|PLAY_SERVICE_ACCOUNT_JSON)"), "google-play", None),
]


def check_uses(path, text):
    errors = []
    for number, line in enumerate(text.splitlines(), 1):
        match = USES.match(line)
        if not match:
            continue
        target, comment = match.group(1), (match.group(2) or "").strip()
        if target.startswith("./"):
            continue
        if target.startswith("docker://"):
            errors.append(f"{path}:{number}: docker actions are not allowed: {target}")
        elif not PINNED.match(target):
            errors.append(f"{path}:{number}: {target} is not pinned to a full commit SHA")
        elif not TAG_COMMENT.match(comment):
            errors.append(f"{path}:{number}: {target} needs its release tag in a comment, e.g. '# v4.2.0'")
    return errors


def jobs_of(text):
    """Yields (job id, first line number, job text) of a workflow, by indentation under the top-level jobs: key."""
    lines = text.splitlines()
    in_jobs = False
    current = None
    start = 0
    body = []
    for number, line in enumerate(lines, 1):
        if re.match(r"^\S", line):
            if current is not None:
                yield current, start, "\n".join(body)
                current = None
            in_jobs = line.rstrip() == "jobs:"
            continue
        if not in_jobs:
            continue
        match = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if match:
            if current is not None:
                yield current, start, "\n".join(body)
            current, start, body = match.group(1), number, []
            continue
        if current is not None:
            body.append(line)
    if current is not None:
        yield current, start, "\n".join(body)


def job_environment(job_text):
    """The job's environment: value as written (name or expression), or "" when it declares none."""
    lines = job_text.splitlines()
    for index, line in enumerate(lines):
        match = re.match(r"^    environment:\s*(.*)$", line)
        if not match:
            continue
        value = match.group(1).strip()
        if value:
            return value
        # Mapping form: environment:\n      name: x
        for following in lines[index + 1:]:
            name = re.match(r"^      name:\s*(.*)$", following)
            if name:
                return name.group(1).strip()
            if not following.startswith("      "):
                break
        return ""
    return ""


def check_secrets(path, text):
    errors = []
    name = os.path.basename(path)
    for pattern, environment, allowed_files in SECRET_RULES:
        if not pattern.search(text):
            continue
        if allowed_files is not None and name not in allowed_files:
            errors.append(f"{path}: uses {pattern.search(text).group(0)}, allowed only in {sorted(allowed_files)}")
            continue
        found_in_job = False
        for job, line, body in jobs_of(text):
            hit = pattern.search(body)
            if not hit:
                continue
            found_in_job = True
            declared = job_environment(body)
            if f"'{environment}'" not in declared and declared.strip("'\"") != environment:
                errors.append(f"{path}:{line}: job {job} uses {hit.group(0)} without the {environment} environment")
        if not found_in_job:
            errors.append(f"{path}: {pattern.search(text).group(0)} used outside a job")
    return errors


def check_composite_secrets(path, text):
    # A composite action receives secrets only as inputs; it must never read them itself.
    errors = []
    for pattern, _environment, _files in SECRET_RULES:
        hit = pattern.search(text)
        if hit:
            errors.append(f"{path}: composite action reads {hit.group(0)} directly")
    return errors


def check(root):
    errors = []
    workflows = os.path.join(root, ".github", "workflows")
    actions = os.path.join(root, ".github", "actions")
    for name in sorted(os.listdir(workflows)):
        if not name.endswith((".yml", ".yaml")):
            continue
        path = os.path.join(workflows, name)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        errors += check_uses(path, text)
        errors += check_secrets(path, text)
    if os.path.isdir(actions):
        for name in sorted(os.listdir(actions)):
            for file_name in ("action.yml", "action.yaml"):
                path = os.path.join(actions, name, file_name)
                if os.path.exists(path):
                    with open(path, encoding="utf-8") as f:
                        text = f.read()
                    errors += check_uses(path, text)
                    errors += check_composite_secrets(path, text)
    return errors


SHA = "0123456789abcdef0123456789abcdef01234567"


def self_test():
    failures = []

    def expect(label, errors, want_errors):
        if bool(errors) != want_errors:
            failures.append(f"{label}: expected {'errors' if want_errors else 'no errors'}, got {errors}")

    expect("local action", check_uses("w.yml", "      - uses: ./.github/actions/sign-apk\n"), False)
    expect("pinned with tag", check_uses("w.yml", f"        uses: actions/checkout@{SHA} # v7.0.1\n"), False)
    expect("pinned subpath", check_uses("w.yml", f"      uses: gradle/actions/setup-gradle@{SHA} # v4.4.3\n"), False)
    expect("major tag", check_uses("w.yml", "        uses: Swatinem/rust-cache@v2\n"), True)
    expect("short sha", check_uses("w.yml", "        uses: Swatinem/rust-cache@0123456 # v2\n"), True)
    expect("pinned without tag comment", check_uses("w.yml", f"        uses: actions/cache@{SHA}\n"), True)
    expect("branch", check_uses("w.yml", "        uses: actions/cache@main\n"), True)
    expect("docker", check_uses("w.yml", "        uses: docker://alpine:3\n"), True)

    release_ok = """name: Release
jobs:
  package-android:
    runs-on: ubuntu-latest
    steps:
      - run: echo build
  sign-android:
    environment: ${{ needs.release-policy.outputs.is_tag_release == 'true' && 'android-release' || '' }}
    steps:
      - env:
          KEY: ${{ secrets.ANDROID_KEYSTORE_BASE64 }}
        run: echo sign
"""
    expect("release key in its environment", check_secrets("release.yml", release_ok), False)
    release_bad = release_ok.replace(
        "      - run: echo build", "      - env:\n          K: ${{ secrets.ANDROID_KEY_PASSWORD }}\n        run: x"
    )
    expect("release key in a job without the environment", check_secrets("release.yml", release_bad), True)
    expect("release key in ci.yml", check_secrets("ci.yml", release_ok), True)
    play_ok = """jobs:
  bundle:
    environment: google-play
    steps:
      - env:
          K: ${{ secrets.PLAY_UPLOAD_KEYSTORE_BASE64 }}
"""
    expect("upload key in google-play", check_secrets("play-bundle.yml", play_ok), False)
    play_mapping = play_ok.replace("    environment: google-play", "    environment:\n      name: google-play")
    expect("upload key in google-play (mapping)", check_secrets("play-bundle.yml", play_mapping), False)
    expect("upload key without environment",
           check_secrets("play-bundle.yml", play_ok.replace("    environment: google-play\n", "")), True)
    expect("composite reads a secret", check_composite_secrets("a.yml", "K: ${{ secrets.ANDROID_KEY_ALIAS }}"), True)
    expect("composite takes inputs", check_composite_secrets("a.yml", "K: ${{ inputs.key-alias }}"), False)

    for failure in failures:
        print(f"::error::self-test: {failure}")
    if failures:
        sys.exit(1)
    print("workflow_policy self-test: all cases pass")


def main():
    args = sys.argv[1:]
    if args and args[0] == "--self-test":
        self_test()
        return
    root = args[0] if args else os.getcwd()
    errors = check(root)
    for error in errors:
        print(f"::error::{error}")
    if errors:
        sys.exit(1)
    print("workflow policy: every action pinned, signing secrets only in their environments")


if __name__ == "__main__":
    main()
