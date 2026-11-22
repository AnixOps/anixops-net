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
    guard owner == CommandLine.arguments[1], layer == 0, let id = window[kCGWindowNumber as String] as? Int else {
        continue
    }
    // `bounds`: the main window's rectangle as x,y,w,h, for a picture that also shows the sheet lying over it (a
    // sheet is a window of its own, so a picture of the main window by its number leaves the sheet out).
    if CommandLine.arguments.count > 2, CommandLine.arguments[2] == "bounds" {
        let width = bounds?["Width"] as? Double ?? 0
        if height > 200, width > 600 {
            let x = bounds?["X"] as? Double ?? 0
            let y = bounds?["Y"] as? Double ?? 0
            print("\(Int(x)),\(Int(y)),\(Int(width)),\(Int(height))")
            break
        }
    } else if height > 200 {
        print(id)
        break
    }
}
SWIFT

shoot() {
    # A fourth argument opens the command palette (`palette`) or the import sheet (`import`) over the page and
    # names the picture after it.
    local section="$1" appearance="$2" mode="${3:-proxy}" overlay="${4:-}"
    local file="${out}/${section}-${appearance}.png"
    if [ "${mode}" != proxy ]; then
        file="${out}/${mode}-${section}-${appearance}.png"
    fi
    local show_palette=NO show_import=NO
    if [ -n "${overlay}" ]; then
        file="${out}/${overlay}-${appearance}.png"
    fi
    case "${overlay}" in
        palette) show_palette=YES ;;
        import) show_import=YES ;;
    esac
    local accepted="${terms}"
    if [ "${section}" = terms ]; then
        accepted="none"
    fi
    open -n "${app}" --args -AnixOpsDemo YES -AnixOpsMode "${mode}" -AnixOpsSection "${section}" -appearance "${appearance}" \
        -AnixOpsPalette "${show_palette}" -AnixOpsImport "${show_import}" -AppleLanguages "(${language})" -terms-accepted-version "${accepted}"
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
    local rect=""
    if [ "${show_import}" = YES ] && [ -n "${id}" ]; then
        rect="$(swift "${finder}" "${name}" bounds 2>/dev/null || true)"
    fi
    if [ -n "${rect}" ]; then
        screencapture -x -R "${rect}" "${file}"
    elif [ -n "${id}" ]; then
        screencapture -x -o -l "${id}" "${file}" || screencapture -x "${file}"
    else
        echo "no window found for ${section}; saving the whole screen" >&2
        screencapture -x "${file}"
        # A picture of the whole screen cannot be compared with a window (screenshot-compare.sh).
        touch "${out}/.incomplete"
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
# The import sheet over the overview: in dark mode a sheet must stand apart from the black window behind it.
shoot home light proxy import
shoot home dark proxy import

rm -f "${finder}"
ls -l "${out}"
