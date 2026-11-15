#!/usr/bin/env bash
# Opens the unsigned debug build of the macOS app with demo data and saves one picture per page.
# Usage: mac-screenshots.sh <path to AnixOps Net.app> <output directory> [language, default zh-Hans]
set -euo pipefail

app="$1"
out="$2"
name="AnixOps Net"
language="${3:-zh-Hans}"
mkdir -p "${out}"
# Pages are shot with the user agreement already accepted; the "terms" shot shows the first-start consent screen.
terms="$(grep -o '基准版本 [0-9.]*' "$(dirname "$0")/../../TERMS.md" | awk '{print $2}')"

finder="${out}/window-id.swift"
cat > "${finder}" <<'SWIFT'
import CoreGraphics
import Foundation

let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for window in windows {
    let owner = window[kCGWindowOwnerName as String] as? String
    let layer = window[kCGWindowLayer as String] as? Int
    let bounds = window[kCGWindowBounds as String] as? [String: Any]
    let height = bounds?["Height"] as? Double ?? 0
    if owner == CommandLine.arguments[1], layer == 0, height > 200, let id = window[kCGWindowNumber as String] as? Int {
        print(id)
        break
    }
}
SWIFT

shoot() {
    # A fourth argument opens the command palette over the page and names the picture after it.
    local section="$1" appearance="$2" mode="${3:-proxy}" palette="${4:-}"
    local file="${out}/${section}-${appearance}.png"
    if [ "${mode}" != proxy ]; then
        file="${out}/${mode}-${section}-${appearance}.png"
    fi
    local show_palette=NO
    if [ -n "${palette}" ]; then
        file="${out}/${palette}-${appearance}.png"
        show_palette=YES
    fi
    local accepted="${terms}"
    if [ "${section}" = terms ]; then
        accepted="none"
    fi
    open -n "${app}" --args -AnixOpsDemo YES -AnixOpsMode "${mode}" -AnixOpsSection "${section}" -appearance "${appearance}" \
        -AnixOpsPalette "${show_palette}" -AppleLanguages "(${language})" -terms-accepted-version "${accepted}"
    local id=""
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        sleep 2
        id="$(swift "${finder}" "${name}" 2>/dev/null || true)"
        if [ -n "${id}" ]; then
            break
        fi
    done
    # The window is up; give the page a moment to lay out and draw its charts.
    sleep 3
    if [ -n "${id}" ]; then
        screencapture -x -o -l "${id}" "${file}" || screencapture -x "${file}"
    else
        echo "no window found for ${section}; saving the whole screen" >&2
        screencapture -x "${file}"
    fi
    pkill -x "${name}" || true
    sleep 1
}

shoot home light
shoot home dark
shoot proxies light
shoot proxies dark
shoot rules light
shoot extensions light
shoot activity light
shoot settings light
shoot terms light
# Debug mode (requests · rewrite · exit · settings, opening on the requests table) and the first-launch mode choice.
shoot activity light debug
shoot activity dark debug
shoot exit light debug
shoot settings light debug
shoot home light choose
# The command palette (⌘K) over the overview.
shoot home light proxy palette

rm -f "${finder}"
ls -l "${out}"
