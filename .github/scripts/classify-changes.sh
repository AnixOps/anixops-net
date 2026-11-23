#!/usr/bin/env bash
# Which jobs a pull request needs, from the paths it changes (one per line on stdin). Prints `name=true|false`
# lines for $GITHUB_OUTPUT.
#
# Fails closed: a path no rule below names runs every job. Only paths no build, test or packaging step reads are
# skipped, and they are listed by name. A new input that a job reads (a tool's configuration, a new folder) is
# therefore checked until someone maps it to a narrower set of jobs. Before, unnamed paths were ignored, which let
# changes to deny.toml and .cargo/config.toml skip the jobs that read them (third-party review F15).
# .github/scripts/classify-changes.test.sh pins the mapping for representative paths.
set -euo pipefail

rust=false; ui=false; windows=false; linux=false; apple=false; android_native=false
windows_native=false; linux_native=false
all() { rust=true; ui=true; windows=true; linux=true; apple=true; android_native=true; windows_native=true; linux_native=true; }

while IFS= read -r path; do
  case "${path}" in
    "") ;;
    # Workspace, lockfile, workflows and scripts, and the Rust tool configuration every Rust job reads:
    # cargo-deny's rules (rust-deps) and cargo's own settings (every build).
    Cargo.toml | Cargo.lock | .github/* | deny.toml | .cargo/* | rust-toolchain | rust-toolchain.toml) all ;;
    # AnixOps Design, vendored: every client's colours are generated from it.
    apps/windows-gui/ui/src/design/*)
      ui=true; windows=true; linux=true; android_native=true; apple=true; windows_native=true
      linux_native=true ;;
    apps/windows-gui/ui/*) ui=true; windows=true; linux=true ;;
    # The native client's installer; the native job builds it and installs, upgrades and removes it.
    installer/windows-native/*) windows_native=true ;;
    apps/windows-gui/* | installer/*) rust=true; windows=true ;;
    # The anixops:// handler the Windows client registers; the Windows job builds, tests and packages it.
    apps/windows-link/*) rust=true; windows=true ;;
    apps/linux-gui/*) rust=true; linux=true ;;
    # The Linux daemon; the workspace job builds and tests it, and the native window is run against it.
    apps/linux-daemon/*) rust=true; linux_native=true ;;
    # The Windows service; the Windows job builds and tests it, and the native app is tested against it.
    apps/windows-service/*) rust=true; windows=true; windows_native=true ;;
    # The list of pages: the native Linux window's tests check that it has the same ones.
    apps/windows-native/src/AnixOps.Net.Presentation/Section.cs) rust=true; windows_native=true ;;
    # The native Windows app (C#); its tests read the message samples under crates/client-ipc.
    apps/windows-native/*) windows_native=true ;;
    # The native Linux window (GTK 4 and libadwaita); the workspace job tests its pages and options.
    apps/linux-native/*) rust=true; linux_native=true ;;
    # The native Android app (Compose over crates/client-core through JNI); its colours come from the
    # client tokens.
    apps/android-native/*) rust=true; android_native=true ;;
    apps/ios/Design/*) apple=true; android_native=true; windows_native=true; linux_native=true ;;
    crates/client-ipc/fixtures/* | crates/client-service/fixtures/*)
      rust=true; windows=true; linux=true; android_native=true; windows_native=true ;;
    # The native Windows app is tested against the real service, which is built from these; the
    # native Linux window takes its wording from crates/client-strings.
    crates/*)
      rust=true; windows=true; linux=true; android_native=true; windows_native=true
      linux_native=true ;;
    apps/ios/*) apple=true ;;
    # The desktop UI and the native Android app bundle TERMS.md (first-start consent; the Android
    # screenshots show it) and PRIVACY.md (Settings › About), and the Apple tests check both. The Windows
    # and Linux desktop jobs only package the interface the ui job builds and tests, so the text needs
    # nothing from them.
    TERMS.md | PRIVACY.md) ui=true; apple=true; android_native=true ;;
    # Documentation and repository metadata no job reads. A README.md inside a crate or app is not here: the
    # build can read it (rustdoc include_str!).
    docs/* | LICENSE | .gitignore) ;;
    */*) all ;;
    *.md) ;;
    *) all ;;
  esac
done

echo "rust=${rust}"
echo "ui=${ui}"
echo "windows=${windows}"
echo "linux=${linux}"
echo "apple=${apple}"
echo "android_native=${android_native}"
echo "windows_native=${windows_native}"
echo "linux_native=${linux_native}"
