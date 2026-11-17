#!/usr/bin/env bash
# Runs the unsigned simulator build of the iPhone app with demo data in a simulator made for this run and saves
# one picture per tab.
# Usage: ios-screenshots.sh <path to NetworkCoreApp.app> <output directory> [language, default zh-Hans]
#
# The pictures are compared pixel by pixel (screenshot-compare.sh), so nothing in them may depend on when or
# where they were taken: the status bar is overridden (9:41, full battery), the demo data has a fixed clock, and
# the device type is named here rather than left to whatever the machine has booted.
set -euo pipefail

app="$1"
out="$2"
language="${3:-zh-Hans}"
mkdir -p "${out}"
terms="$(grep -o '基准版本 [0-9.]*' "$(dirname "$0")/../../TERMS.md" | awk '{print $2}')"
bundle="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${app}/Info.plist")"

# The newest plain iPhone this Xcode knows, by name; a change of model changes every picture's size, which the
# comparison reports as "resized" and asks to be acknowledged once.
type=""
for name in "iPhone 17" "iPhone 16" "iPhone 15"; do
    if xcrun simctl list devicetypes | grep -q "^${name} ("; then
        type="${name}"
        break
    fi
done
if [ -z "${type}" ]; then
    echo "no known iPhone simulator on this machine" >&2
    touch "${out}/.incomplete"
    exit 1
fi

device="$(xcrun simctl create anixops-screenshots "${type}")"
cleanup() {
    xcrun simctl shutdown "${device}" > /dev/null 2>&1 || true
    xcrun simctl delete "${device}" > /dev/null 2>&1 || true
}
trap cleanup EXIT
# Anything that goes wrong from here leaves pictures that cannot be compared.
trap 'touch "${out}/.incomplete"' ERR

xcrun simctl bootstatus "${device}" -b
xcrun simctl status_bar "${device}" override --time "9:41" --dataNetwork wifi --wifiMode active --wifiBars 3 \
    --cellularMode active --cellularBars 4 --operatorName "" --batteryState charged --batteryLevel 100
xcrun simctl install "${device}" "${app}"

# Puts the simulator into the light or dark appearance and waits until it says that it is there: now and then
# the simulator does not take the change, and every picture after it would be of the other appearance.
look() {
    local wanted="$1"
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        if [ "$(xcrun simctl ui "${device}" appearance 2> /dev/null)" = "${wanted}" ]; then
            return 0
        fi
        xcrun simctl ui "${device}" appearance "${wanted}" || true
        sleep 1
    done
    echo "the simulator did not change to the ${wanted} appearance" >&2
    return 1
}

shoot() {
    local section="$1" appearance="$2" mode="${3:-proxy}"
    local file="${out}/${section}-${appearance}.png"
    if [ "${mode}" != proxy ]; then
        file="${out}/${mode}-${section}-${appearance}.png"
    fi
    look "${appearance}"
    xcrun simctl launch "${device}" "${bundle}" -AnixOpsDemo YES -AnixOpsMode "${mode}" -AnixOpsSection "${section}" \
        -AppleLanguages "(${language})" -terms-accepted-version "${terms}" > /dev/null
    # The app is up; give the page time to lay out, load its demo data and finish its first transitions. A fixed
    # wait alone sometimes caught the white launch screen or a passing system banner, so the picture is taken again
    # every 2 seconds until two in a row are the same and it is not the blank launch screen (which compresses to
    # under 100 KB; every page with demo data is well above 150 KB).
    #
    # A system banner (for example "Ready for Apple Intelligence" on a fresh simulator) can stay up for several
    # pictures in a row, so a settled picture must also still be the same 8 seconds later, by which time a banner
    # has gone; if it is not, settling starts again from that later picture.
    sleep 6
    local previous="" current="" size=0 tries=0 later=""
    while :; do
        xcrun simctl io "${device}" screenshot --type=png "${file}" > /dev/null 2>&1
        current="$(shasum "${file}" | cut -d' ' -f1)"
        size="$(stat -f%z "${file}")"
        if [ "${current}" = "${previous}" ] && [ "${size}" -gt 120000 ]; then
            sleep 8
            xcrun simctl io "${device}" screenshot --type=png "${file}" > /dev/null 2>&1
            later="$(shasum "${file}" | cut -d' ' -f1)"
            if [ "${later}" = "${current}" ]; then
                break
            fi
            current="${later}"
        fi
        tries=$((tries + 1))
        if [ "${tries}" -ge 10 ]; then
            echo "::warning::${file##*/} did not settle after ${tries} pictures; keeping the last one"
            break
        fi
        previous="${current}"
        sleep 2
    done
    xcrun simctl terminate "${device}" "${bundle}" > /dev/null 2>&1 || true
    sleep 1
}

# Every light picture first, then the dark ones: the appearance changes once, and never back.
shoot home light
shoot proxies light
shoot rules light
shoot extensions light
shoot settings light
# Debug mode: requests · rewrite · exit · settings.
shoot requests light debug
shoot exit light debug
shoot home dark
shoot requests dark debug
