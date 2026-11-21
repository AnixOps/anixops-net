#!/usr/bin/env bash
# Records the Google Play VPN service / foreground service demo on an emulator (play-demo-video.yml).
#
# It drives the real debug APK the way a person would: agree to the user agreement, pick network debugging, turn the
# connection on, allow the system VPN prompt, show the "Connected" notification of the foreground service, look at the
# captured connections and turn the connection off. Each step adds a caption; ffmpeg burns the captions into the
# recording afterwards, because Play asks for captions when it is not obvious how the VPN is used.
#
# Usage: play-demo-video.sh APK OUT_DIR
set -euo pipefail

apk="$1"
out="$2"
mkdir -p "$out/debug"
pkg="io.anixops.net.android"
captions="$out/captions.tsv"
: >"$captions"

dump() {
  adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1 || true
  adb shell cat /sdcard/ui.xml 2>/dev/null || true
}

# Prints "x y" for the centre of the first node whose text or content-desc equals $1, or nothing.
find_node() {
  dump | python3 -c '
import re, sys, xml.etree.ElementTree as ET
want = sys.argv[1]
try:
    root = ET.fromstring(sys.stdin.read())
except ET.ParseError:
    sys.exit(0)
for node in root.iter("node"):
    if want in (node.get("text"), node.get("content-desc")):
        x1, y1, x2, y2 = map(int, re.findall(r"\d+", node.get("bounds")))
        print((x1 + x2) // 2, (y1 + y2) // 2)
        break
' "$1"
}

fail() {
  echo "::error::$1"
  dump >"$out/debug/ui.xml"
  adb exec-out screencap -p >"$out/debug/screen.png" || true
  adb shell pkill -INT screenrecord || true
  exit 1
}

wait_for() {
  local deadline=$((SECONDS + ${2:-60})) at
  while ((SECONDS < deadline)); do
    at="$(find_node "$1")"
    if [[ -n "$at" ]]; then
      echo "$at"
      return 0
    fi
    sleep 1
  done
  return 1
}

tap() {
  local at
  at="$(wait_for "$1" "${2:-60}")" || fail "\"$1\" did not appear"
  # shellcheck disable=SC2086
  adb shell input tap $at
}

started=0
now() { echo $((SECONDS - started)); }

# caption TEXT: shows TEXT from now until the next caption (or the end).
caption() {
  printf '%s\t%s\n' "$(now)" "$1" >>"$captions"
}

adb install -r -g "$apk"
adb shell settings put global window_animation_scale 0
adb shell settings put global transition_animation_scale 0
adb shell settings put global animator_duration_scale 0

adb shell screenrecord --time-limit 170 --bit-rate 8000000 /sdcard/demo.mp4 &
recorder=$!
started=$SECONDS
sleep 2

caption "AnixOps Net: a network debugging tool"
adb shell am start -W -n "$pkg/io.anixops.net.MainActivity" >/dev/null
sleep 3

caption "First start: read and accept the user agreement"
sleep 2
tap "Agree and continue"

caption "Choose network debugging (no server needed)"
sleep 2
tap "Network debugging"
sleep 3

caption "Turn on the connection to start the VPN service"
sleep 2
tap "Connection"

caption "Android asks for permission. The VPN starts only after the user taps OK"
sleep 3
tap "OK" 30

wait_for "Connected" 60 >/dev/null || fail "the connection did not come up"
caption "Connected: traffic is routed on this device by the VPN service"
sleep 4

caption "Capture lists this device's connections, kept only on the device"
at="$(find_node "Requests")"
# shellcheck disable=SC2086
[[ -n "$at" ]] && adb shell input tap $at && sleep 2
tap "Capture"
sleep 8

caption "The VPN runs as a foreground service with this notification"
adb shell cmd statusbar expand-notifications
sleep 7
adb shell cmd statusbar collapse
sleep 2

caption "Turn the connection off: the VPN service and its notification stop"
sleep 2
tap "Connection"
wait_for "Not connected" 60 >/dev/null || fail "the connection did not stop"
sleep 3

caption "Nothing is sent to the developer. No servers are provided by the app"
sleep 5

adb shell pkill -INT screenrecord || true
wait "$recorder" || true
sleep 2
adb pull /sdcard/demo.mp4 "$out/raw.mp4"
echo "recorded $(now) s"
cat "$captions"
