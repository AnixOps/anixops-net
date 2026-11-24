#!/usr/bin/env bash
# Fails when the embedded runtime the Apple apps link would refuse its own handshake.
#
# The packet tunnel compares the runtime's reported (abi, schema, feature set) with the constants in
# apps/ios/Sources/NetworkCoreBridge/NetworkCoreRuntimeFFI.swift and stops the tunnel with "iOS embedded runtime
# ABI is incompatible" when any of them differs. The kernel raises the feature set whenever it adds a C entry
# point (networkcore #269 raised it to 6), so bumping the kernel pin without the Swift constant ships an app that
# cannot connect, which is how TestFlight build 1.0.0 (5036) broke. build-ios-runtime-xcframework.sh runs this
# before every Apple build: CI, TestFlight and the downstream (prebuilt) builds.
#
# Usage: ios-runtime-handshake.sh <kernel checkout> [NetworkCoreRuntimeFFI.swift]
set -euo pipefail

kernel="${1:?usage: ios-runtime-handshake.sh <kernel checkout> [NetworkCoreRuntimeFFI.swift]}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
swift="${2:-${root}/apps/ios/Sources/NetworkCoreBridge/NetworkCoreRuntimeFFI.swift}"
ffi="${kernel}/crates/engine-embedded/src/ffi.rs"
config="${kernel}/crates/engine-embedded/src/config.rs"
header="${kernel}/crates/engine-embedded/include/networkcore_ios_runtime.h"

for f in "${swift}" "${ffi}" "${config}" "${header}"; do
  if [[ ! -f "${f}" ]]; then
    echo "::error::runtime handshake check: ${f} is missing"
    exit 1
  fi
done

# rust_const <file> <name>: the value of `pub const <name>: u32 = <value>;`, which may be a literal or
# crate::config::CONFIG_SCHEMA_VERSION.
rust_const() {
  sed -n "s/^pub const $2: u32 = \(.*\);\$/\1/p" "$1" | head -n 1
}

resolve() {
  local value="$1"
  case "${value}" in
    crate::config::CONFIG_SCHEMA_VERSION) value="$(rust_const "${config}" CONFIG_SCHEMA_VERSION)" ;;
  esac
  if [[ ! "${value}" =~ ^[0-9]+$ ]]; then
    echo "::error::runtime handshake check: cannot read a number from '$1' in the kernel; update ios-runtime-handshake.sh" >&2
    return 1
  fi
  echo "${value}"
}

swift_const() {
  local value
  value="$(sed -n "s/^ *public static let $1: UInt32 = \([0-9][0-9]*\)\$/\1/p" "${swift}" | head -n 1)"
  if [[ -z "${value}" ]]; then
    echo "::error::runtime handshake check: NetworkCoreRuntimeFFI.$1 not found in ${swift}" >&2
    return 1
  fi
  echo "${value}"
}

kernel_abi="$(resolve "$(rust_const "${ffi}" NETWORKCORE_IOS_ABI_VERSION)")"
kernel_schema="$(resolve "$(rust_const "${ffi}" NETWORKCORE_IOS_SCHEMA_VERSION)")"
kernel_features="$(resolve "$(rust_const "${ffi}" NETWORKCORE_IOS_FEATURE_SET_VERSION)")"
header_abi="$(sed -n 's/^#define NETWORKCORE_IOS_ABI_VERSION \([0-9][0-9]*\)u$/\1/p' "${header}" | head -n 1)"
app_abi="$(swift_const abiVersion)"
app_schema="$(swift_const dtoSchemaVersion)"
app_features="$(swift_const featureSetVersion)"

echo "runtime handshake: kernel abi=${kernel_abi} schema=${kernel_schema} feature_set=${kernel_features} (header abi=${header_abi})"
echo "runtime handshake: app    abi=${app_abi} schema=${app_schema} feature_set=${app_features}"

status=0
check() {
  if [[ "$2" != "$3" ]]; then
    echo "::error file=apps/ios/Sources/NetworkCoreBridge/NetworkCoreRuntimeFFI.swift::$1: the pinned kernel reports $2 but NetworkCoreRuntimeFFI expects $3; the tunnel would stop with \"iOS embedded runtime ABI is incompatible\". Update the Swift constant (and adopt the new C entry points) in the same change as the kernel pin."
    status=1
  fi
}
check "ABI version (header)" "${header_abi}" "${kernel_abi}"
check "ABI version" "${kernel_abi}" "${app_abi}"
check "DTO schema version" "${kernel_schema}" "${app_schema}"
check "feature set version" "${kernel_features}" "${app_features}"
exit "${status}"
