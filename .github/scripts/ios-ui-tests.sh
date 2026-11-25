#!/usr/bin/env bash
# Runs the iPhone app's UI tests (apps/ios/UITests: the entry checks of docs/ui-ux.md, 入口不变量) in a simulator
# made for this run, against the demo data, unsigned like the screenshots.
# Usage: ios-ui-tests.sh <path to NetworkCoreApp.app> <products directory> <result bundle path> [test ...]
#   The products directory is the one `xcodebuild build-for-testing` filled (it holds the .xctestrun file); the tests
#   run from it without building. Each [test] is an -only-testing filter, e.g.
#   NetworkCoreAppUITests/AccessibilityAuditTests/testDarkTheme, so CI can run parts side by side. A [test] that
#   starts with `!` is a -skip-testing filter instead: a part can take a whole class except the tests another part
#   runs, so a test added to the class later still runs in one of the parts.
set -euo pipefail

app="$1"
products="$2"
result="$3"
shift 3
only=()
for test in "$@"; do
    case "${test}" in
        '!'*) only+=("-skip-testing:${test#!}") ;;
        *) only+=("-only-testing:${test}") ;;
    esac
done
xctestrun="$(find "${products}" -maxdepth 1 -name 'NetworkCoreApp_iphonesimulator*.xctestrun' | head -n 1)"
if [ -z "${xctestrun}" ]; then
    echo "::error::no NetworkCoreApp .xctestrun in ${products}; build-for-testing first"
    exit 1
fi
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
# The test target has no target application (apps/ios/project.yml): the app built for this commit is installed
# here and the tests launch it by bundle identifier.
xcrun simctl install "${device}" "${app}"
bundle="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${app}/Info.plist")"
# A fresh simulator finishes its own first-boot work (data migration, the accessibility services) for a while
# after bootstatus returns, and the first launch of an app pays for its install: launch the app once and let it
# settle, so the first test does not run into XCTest's launch or audit time limit.
xcrun simctl launch "${device}" "${bundle}" -AnixOpsDemo YES -terms-accepted-version "${terms}" > /dev/null
sleep 20
xcrun simctl terminate "${device}" "${bundle}" || true

# xcodebuild hands TEST_RUNNER_-prefixed variables to the test process without the prefix.
TEST_RUNNER_ANIXOPS_TERMS="${terms}" TEST_RUNNER_ANIXOPS_BUNDLE="${bundle}" xcodebuild test-without-building \
    -xctestrun "${xctestrun}" \
    -destination "id=${device}" \
    -resultBundlePath "${result}" \
    ${only[@]+"${only[@]}"}
