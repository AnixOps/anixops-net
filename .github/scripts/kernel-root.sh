#!/usr/bin/env bash
# Prints the directory of the pinned NetworkCore kernel checkout (the git dependency in Cargo.toml).
# CI builds kernel-owned artifacts from it with the kernel's own Cargo.lock: the iOS runtime
# XCFrameworks (engine-embedded, node-import) and the Windows CLI bundled into the MSI.
set -euo pipefail

manifest="$(cargo metadata --format-version 1 --locked |
  jq -r '.packages[] | select(.name == "engine-embedded") | .manifest_path')"
if [[ -z "${manifest}" || ! -f "${manifest}" ]]; then
  echo "::error::engine-embedded is not in the dependency graph; is the kernel pinned in Cargo.toml?" >&2
  exit 1
fi
# <kernel>/crates/engine-embedded/Cargo.toml
dirname "$(dirname "$(dirname "${manifest}")")"
