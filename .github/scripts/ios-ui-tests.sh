#!/usr/bin/env bash
# Runs the iPhone app's UI tests (apps/ios/UITests: the entry checks of docs/ui-ux.md, 入口不变量) in a simulator
# made for this run, against the demo data, unsigned like the screenshots.
# Usage: ios-ui-tests.sh <derived data path> <result bundle path>
set -euo pipefail

derived="$1"
result="$2"
root="$(cd "$(dirname "$0")/../.." && pwd)"
terms="$(grep -o '基准版本 [0-9.]*' "${root}/TERMS.md" | awk '{print $2}')"
if [ -z "${terms}" ]; then
    echo "::error::no 基准版本 in TERMS.md; the tests could not get past the consent screen"
    exit 1
fi

# The same iPhone as the screenshots, so a layout that hides an entry on that size is caught.
type=""
for name in "iPhone 17" "iPhone 16" "iPhone 15"; do
    if xcrun simctl list devicetypes | grep -q "^${name} ("; then
        type="${name}"
        break
    fi
done
if [ -z "${type}" ]; then
    echo "::error::no known iPhone simulator on this machine"
    exit 1
fi

device="$(xcrun simctl create anixops-ui-tests "${type}")"
cleanup() {
    xcrun simctl shutdown "${device}" > /dev/null 2>&1 || true
    xcrun simctl delete "${device}" > /dev/null 2>&1 || true
}
trap cleanup EXIT
xcrun simctl bootstatus "${device}" -b

# xcodebuild hands TEST_RUNNER_-prefixed variables to the test process without the prefix.
TEST_RUNNER_ANIXOPS_TERMS="${terms}" xcodebuild \
    -project "${root}/apps/ios/NetworkCore.xcodeproj" \
    -scheme NetworkCoreApp \
    -destination "id=${device}" \
    -derivedDataPath "${derived}" \
    -resultBundlePath "${result}" \
    CODE_SIGNING_ALLOWED=NO \
    test
