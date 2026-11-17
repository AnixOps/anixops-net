#!/usr/bin/env bash
# Runs the Mac app's UI tests (apps/ios/UITests/NetworkCoreMac: the entry checks of docs/ui-ux.md, 入口不变量)
# against the demo data, unsigned like the screenshots.
# Usage: mac-ui-tests.sh <derived data path> <result bundle path>
set -euo pipefail

derived="$1"
result="$2"
root="$(cd "$(dirname "$0")/../.." && pwd)"
terms="$(grep -o '基准版本 [0-9.]*' "${root}/TERMS.md" | awk '{print $2}')"
if [ -z "${terms}" ]; then
    echo "::error::no 基准版本 in TERMS.md; the tests could not get past the consent screen"
    exit 1
fi

# A copy left running by the screenshots would answer instead of the one the tests launch.
pkill -x "AnixOps Net" || true

# xcodebuild hands TEST_RUNNER_-prefixed variables to the test process without the prefix.
TEST_RUNNER_ANIXOPS_TERMS="${terms}" xcodebuild \
    -project "${root}/apps/ios/NetworkCore.xcodeproj" \
    -scheme NetworkCoreMac \
    -destination 'platform=macOS' \
    -derivedDataPath "${derived}" \
    -resultBundlePath "${result}" \
    CODE_SIGNING_ALLOWED=NO \
    test
