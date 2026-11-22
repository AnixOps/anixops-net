#!/usr/bin/env bash
# Runs the Mac app's UI tests (apps/ios/UITests/NetworkCoreMac: the entry checks of docs/ui-ux.md, 入口不变量)
# against the demo data, unsigned like the screenshots.
# Usage: mac-ui-tests.sh <path to AnixOps Net.app> <products directory> <result bundle path> [test ...]
#   The products directory is the one `xcodebuild build-for-testing` filled (it holds the .xctestrun file); the tests
#   run from it without building. Each [test] is an -only-testing filter, e.g.
#   NetworkCoreMacUITests/MacEntryInvariantTests.
set -euo pipefail

app="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
products="$2"
result="$3"
shift 3
only=()
for test in "$@"; do
    only+=("-only-testing:${test}")
done
xctestrun="$(find "${products}" -maxdepth 1 -name 'NetworkCoreMac_macosx*.xctestrun' | head -n 1)"
if [ -z "${xctestrun}" ]; then
    echo "::error::no NetworkCoreMac .xctestrun in ${products}; build-for-testing first"
    exit 1
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
terms="$(grep -o '基准版本 [0-9.]*' "${root}/TERMS.md" | awk '{print $2}')"
if [ -z "${terms}" ]; then
    echo "::error::no 基准版本 in TERMS.md; the tests could not get past the consent screen"
    exit 1
fi

# A copy left running by the screenshots would answer instead of the one the tests launch.
pkill -x "AnixOps Net" || true

# The test target has no target application (apps/ios/project.yml): the tests launch the app built for this
# commit by its path. xcodebuild hands TEST_RUNNER_-prefixed variables to the test process without the prefix.
TEST_RUNNER_ANIXOPS_TERMS="${terms}" TEST_RUNNER_ANIXOPS_APP="${app}" xcodebuild test-without-building \
    -xctestrun "${xctestrun}" \
    -destination 'platform=macOS' \
    -resultBundlePath "${result}" \
    ${only[@]+"${only[@]}"}
