#!/usr/bin/env bash
# Builds engine-embedded (which includes node-import) as one static XCFramework. The iOS and
# macOS packet tunnels run the tunnel with it; the containing apps use it for subscription import
# and the HTTP delay test. Runs only in GitHub Actions on macos-26; the output is never committed.
# The crate comes from the kernel commit pinned in Cargo.toml and is built inside that kernel
# checkout with the kernel's own Cargo.lock (see kernel-root.sh).
#
# Usage: build-ios-runtime-xcframework.sh [device|all]
#   device: iPhone slice only (archive and upload)
#   all:    iPhone, simulator and universal macOS slices (default, CI Xcode build)
#
# Output: apps/ios/Generated/NetworkCoreRuntime.xcframework, whose headers expose two Clang
# modules: NetworkCoreRuntimeC (tunnel runtime and delay test) and NetworkCoreImportC (the
# node-import C ABI, whose symbols the engine-embedded library carries).
set -euo pipefail

slices="${1:-all}"
case "${slices}" in
  device | all) ;;
  *)
    echo "::error::usage: build-ios-runtime-xcframework.sh [device|all]"
    exit 2
    ;;
esac

root="$(git rev-parse --show-toplevel)"
kernel="$(bash "${root}/.github/scripts/kernel-root.sh")"
target_dir="${root}/target/kernel"
echo "Kernel checkout: ${kernel}"

device_target=aarch64-apple-ios
sim_targets=(aarch64-apple-ios-sim x86_64-apple-ios)
mac_targets=(aarch64-apple-darwin x86_64-apple-darwin)
targets=("${device_target}")
if [[ "${slices}" == all ]]; then
  targets+=("${sim_targets[@]}" "${mac_targets[@]}")
fi

rustup target add "${targets[@]}"

# Matches the apps' deployment targets; release settings keep the extension binary small.
export IPHONEOS_DEPLOYMENT_TARGET=17.0
export MACOSX_DEPLOYMENT_TARGET=14.0
export CARGO_PROFILE_RELEASE_LTO=thin
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
export CARGO_PROFILE_RELEASE_DEBUG=0
# rquickjs (MITM scripts) ships no iOS bindings; bindgen generates them against the matching SDK.
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios="-isysroot $(xcrun --sdk iphoneos --show-sdk-path)"
simulator_sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios_sim="-isysroot ${simulator_sdk}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_apple_ios="-isysroot ${simulator_sdk}"

for target in "${targets[@]}"; do
  cargo build --locked --release --manifest-path "${kernel}/Cargo.toml" --target-dir "${target_dir}" \
    -p engine-embedded --lib --target "${target}"
done

# package <library file> <xcframework name> <crate dir>:<header>:<module>...
package() {
  local lib="$1" name="$2"
  shift 2
  local out="${root}/apps/ios/Generated/${name}.xcframework"
  local work="${RUNNER_TEMP:-${root}/target}/networkcore-ios-${name}"
  rm -rf "${work}" "${out}"
  mkdir -p "${work}/device" "$(dirname "${out}")"
  # Each module map sits in a directory named after its module, so the module maps of one
  # XCFramework (and of any other linked one, which Xcode copies into the same include/) never collide.
  local spec crate header module
  for spec in "$@"; do
    IFS=: read -r crate header module <<< "${spec}"
    mkdir -p "${work}/headers/${module}"
    cp "${kernel}/crates/${crate}/include/${header}" "${work}/headers/${module}/"
    printf 'module %s {\n    header "%s"\n    export *\n}\n' "${module}" "${header}" > "${work}/headers/${module}/module.modulemap"
  done

  cp "${target_dir}/${device_target}/release/${lib}" "${work}/device/${lib}"
  local args=(-library "${work}/device/${lib}" -headers "${work}/headers")

  if [[ "${slices}" == all ]]; then
    mkdir -p "${work}/simulator"
    local sim_libs=()
    for target in "${sim_targets[@]}"; do
      sim_libs+=("${target_dir}/${target}/release/${lib}")
    done
    lipo -create "${sim_libs[@]}" -output "${work}/simulator/${lib}"
    args+=(-library "${work}/simulator/${lib}" -headers "${work}/headers")

    mkdir -p "${work}/macos"
    local mac_libs=()
    for target in "${mac_targets[@]}"; do
      mac_libs+=("${target_dir}/${target}/release/${lib}")
    done
    lipo -create "${mac_libs[@]}" -output "${work}/macos/${lib}"
    args+=(-library "${work}/macos/${lib}" -headers "${work}/headers")
  fi

  xcodebuild -create-xcframework "${args[@]}" -output "${out}"

  # Linkage of the C ABI is checked on the built binaries by the CI Xcode job; Xcode's nm cannot
  # always read archives produced by Rust's newer LLVM.
  du -sh "${work}/device/${lib}"
  echo "Built ${out} (${slices})"
}

package libengine_embedded.a NetworkCoreRuntime \
  engine-embedded:networkcore_ios_runtime.h:NetworkCoreRuntimeC \
  node-import:networkcore_import.h:NetworkCoreImportC
