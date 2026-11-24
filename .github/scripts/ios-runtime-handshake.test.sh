#!/usr/bin/env bash
# Pins ios-runtime-handshake.sh: it passes when the app's constants match the kernel and fails on each kind of
# drift, including the feature set 5/6 mismatch that broke TestFlight build 1.0.0 (5036).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
failures=0

# fixture <dir> <kernel abi> <header abi> <kernel schema> <kernel features> <app abi> <app schema> <app features>
fixture() {
  local dir="$1"
  mkdir -p "${dir}/kernel/crates/engine-embedded/src" "${dir}/kernel/crates/engine-embedded/include"
  cat > "${dir}/kernel/crates/engine-embedded/src/ffi.rs" <<EOF
pub const NETWORKCORE_IOS_ABI_VERSION: u32 = $2;
pub const NETWORKCORE_IOS_SCHEMA_VERSION: u32 = crate::config::CONFIG_SCHEMA_VERSION;
/// feature sets
pub const NETWORKCORE_IOS_FEATURE_SET_VERSION: u32 = $5;
EOF
  echo "pub const CONFIG_SCHEMA_VERSION: u32 = $4;" > "${dir}/kernel/crates/engine-embedded/src/config.rs"
  echo "#define NETWORKCORE_IOS_ABI_VERSION $3u" > "${dir}/kernel/crates/engine-embedded/include/networkcore_ios_runtime.h"
  cat > "${dir}/ffi.swift" <<EOF
public enum NetworkCoreRuntimeFFI {
    public static let abiVersion: UInt32 = $6
    public static let dtoSchemaVersion: UInt32 = $7
    public static let featureSetVersion: UInt32 = $8
}
EOF
}

# expect <pass|fail> <name> <fixture args...>
expect() {
  local want="$1" name="$2" dir="${work}/$2" got
  shift 2
  fixture "${dir}" "$@"
  if bash "${here}/ios-runtime-handshake.sh" "${dir}/kernel" "${dir}/ffi.swift" > "${dir}/out" 2>&1; then
    got=pass
  else
    got=fail
  fi
  if [[ "${got}" != "${want}" ]]; then
    echo "::error::ios-runtime-handshake ${name}: expected ${want}, got ${got}"
    cat "${dir}/out"
    failures=$((failures + 1))
  fi
}

expect pass matching 1 1 1 6 1 1 6
expect fail feature-set-5-vs-6 1 1 1 6 1 1 5
expect fail abi 2 2 1 6 1 1 6
expect fail header-abi 1 2 1 6 1 1 6
expect fail schema 1 1 2 6 1 1 6

# A missing constant fails closed rather than comparing empty strings.
dir="${work}/missing"
fixture "${dir}" 1 1 1 6 1 1 6
sed -i.bak '/featureSetVersion/d' "${dir}/ffi.swift"
if bash "${here}/ios-runtime-handshake.sh" "${dir}/kernel" "${dir}/ffi.swift" > /dev/null 2>&1; then
  echo "::error::ios-runtime-handshake missing-constant: expected fail, got pass"
  failures=$((failures + 1))
fi

if [[ "${failures}" -gt 0 ]]; then
  exit 1
fi
echo "ios-runtime-handshake: all cases behave"
