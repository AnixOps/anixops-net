#!/usr/bin/env bash
# Builds anixops-mcp (crates/client-mcp, docs/mcp-desktop.md) for both Mac architectures and joins them into one
# universal binary at apps/ios/Generated/anixops-mcp. The macOS app copies it into Contents/MacOS and signs it
# (apps/ios/Tools/embed-mcp.sh). Runs only in GitHub Actions on macOS; the output is never committed.
# build-ios-runtime-xcframework.sh calls it for every build that has a macOS slice.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
targets=(aarch64-apple-darwin x86_64-apple-darwin)
out="${root}/apps/ios/Generated/anixops-mcp"

rustup target add "${targets[@]}"
export MACOSX_DEPLOYMENT_TARGET=14.0

binaries=()
for target in "${targets[@]}"; do
  cargo build --locked --release --manifest-path "${root}/Cargo.toml" -p client-mcp --bin anixops-mcp \
    --target "${target}"
  binaries+=("${root}/target/${target}/release/anixops-mcp")
done

mkdir -p "$(dirname "${out}")"
lipo -create "${binaries[@]}" -output "${out}"
chmod 755 "${out}"
lipo -archs "${out}"
echo "Built ${out}"
