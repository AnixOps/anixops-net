#!/usr/bin/env bash
# Compares this pull request's screenshots of one platform (macOS, iOS) with the ones the main branch took, shot
# by shot, and fails when a shot changed that the pull request description does not acknowledge on a
# "界面变化：" line (docs/ui-ux.md).
# Usage: screenshot-compare.sh <directory of this run's screenshots> <working directory> <platform> <label>
#   <platform> names the artifact main uploads (`<platform>-screenshots-<commit>`) and prefixes every shot's name
#   (`<platform>-home-light`), so one acknowledgement never covers the same page on another platform.
# Needs GH_TOKEN, REPO (owner/name), PR (number) and BASE_SHA (the pull request's base commit).
#
# The base pictures are not taken again: every push to main that touches the Apple apps uploads its screenshots,
# so the newest such artifact at or before the base commit shows main as it is. When none can be found (expired,
# not taken yet, or the screenshots could not be taken on this machine) the comparison is skipped with a warning
# instead of failing a pull request for something it did not do.
set -euo pipefail

after="$1"
work="$2"
platform="$3"
label="$4"
mkdir -p "${work}"

if [ -e "${after}/.incomplete" ] || ! ls "${after}"/*.png > /dev/null 2>&1; then
    echo "::warning::the ${platform} screenshots of this run are incomplete; nothing to compare"
    exit 0
fi

base_id=""
base_commit=""
for commit in $(gh api "repos/${REPO}/commits?sha=${BASE_SHA}&per_page=30" --jq '.[].sha'); do
    base_id="$(gh api "repos/${REPO}/actions/artifacts?name=${platform}-screenshots-${commit}&per_page=1" \
        --jq '[.artifacts[] | select(.expired == false)][0].id // empty')"
    if [ -n "${base_id}" ]; then
        base_commit="${commit}"
        break
    fi
done
if [ -z "${base_id}" ]; then
    echo "::warning::no ${platform} screenshots from main within 30 commits of ${BASE_SHA}; nothing to compare with"
    exit 0
fi
echo "Comparing with the screenshots of main at ${base_commit}"

gh api "repos/${REPO}/actions/artifacts/${base_id}/zip" > "${work}/base.zip"
rm -rf "${work}/base" "${work}/before" "${work}/after"
mkdir -p "${work}/base" "${work}/before" "${work}/after"
unzip -q -o "${work}/base.zip" -d "${work}/base"
if [ -e "${work}/base/.incomplete" ]; then
    echo "::warning::the screenshots of main at ${base_commit} are incomplete; nothing to compare with"
    exit 0
fi

for file in "${work}/base"/*.png; do
    [ -e "${file}" ] && cp "${file}" "${work}/before/${platform}-$(basename "${file}")"
done
for file in "${after}"/*.png; do
    cp "${file}" "${work}/after/${platform}-$(basename "${file}")"
done

npm install --prefix "${work}/tools" --no-audit --no-fund pixelmatch@5.3.0 pngjs@7.0.0 > /dev/null
# Read now, not from the event: re-running the job after editing the description must see the new text.
PR_BODY="$(gh api "repos/${REPO}/pulls/${PR}" --jq '.body // ""')"
export PR_BODY
NODE_PATH="${work}/tools/node_modules" node "$(dirname "$0")/screenshot-diff/diff.cjs" \
    --before "${work}/before" --after "${work}/after" --report "${work}/report" --label "${label}"
