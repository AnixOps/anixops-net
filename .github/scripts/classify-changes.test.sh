#!/usr/bin/env bash
# Pins what classify-changes.sh selects for representative paths, so a rule that stops covering an input fails here
# instead of silently skipping the jobs that read it.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
failures=0
all_jobs="android_native apple linux linux_native rust ui windows windows_native"

selected() {
  printf '%s\n' "$@" | bash "${here}/classify-changes.sh" | awk -F= '$2 == "true" { print $1 }' | sort | xargs
}

# expect <path> <jobs that must be selected, any order; "none" or "all">
expect() {
  local path="$1" want="$2" got
  got="$(selected "${path}")"
  case "${want}" in
    all) want="${all_jobs}" ;;
    none) want="" ;;
    *) want="$(printf '%s\n' ${want} | sort | xargs)" ;;
  esac
  if [[ "${got}" != "${want}" ]]; then
    echo "::error::${path}: expected [${want}], classified [${got}]"
    failures=$((failures + 1))
  fi
}

shared_rust="rust windows linux android_native windows_native linux_native"

# Rust tool configuration: every Rust job reads it (F15).
expect deny.toml all
expect .cargo/config.toml all
expect rust-toolchain.toml all
expect Cargo.lock all
expect .github/workflows/release.yml all
# Unknown inputs fail closed.
expect examples/mitm/demo.sgmodule all
expect some-new-config.toml all
expect new-folder/file.txt all
# Narrow mappings.
expect crates/client-core/src/client.rs "${shared_rust}"
expect crates/client-core/README.md "${shared_rust}"
expect apps/ios/Sources/NetworkCoreAppHost/TunnelController.swift apple
expect apps/windows-gui/ui/src/pages/RequestList.tsx "ui windows linux"
expect apps/android-native/app/build.gradle.kts "rust android_native"
expect apps/windows-link/src/lib.rs "rust windows"
expect apps/windows-native/src/App.cs windows_native
expect TERMS.md "ui apple android_native"
expect PRIVACY.md "ui apple android_native"
# Documentation only.
expect docs/ui-ux.md none
expect README.md none
expect LICENSE none

# Several paths select the union.
got="$(selected docs/a.md apps/ios/x.swift)"
if [[ "${got}" != "apple" ]]; then
  echo "::error::docs plus an iOS file should select only apple, selected [${got}]"
  failures=$((failures + 1))
fi

if (( failures > 0 )); then
  echo "${failures} classification check(s) failed"
  exit 1
fi
echo "Path classification checks passed"
