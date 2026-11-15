#!/usr/bin/env bash
# Draws the native Linux window into PNG files, one per page, language and theme, and checks what came out.
# Run inside a display and a session bus:
#
#   xvfb-run -a --server-args="-screen 0 2400x1600x24" dbus-run-session -- \
#     linux-native-screenshots.sh <anixops-net-preview> <output directory>
#
# The window draws itself with its own renderer (`--screenshot`), so the files show what GTK lays out, not
# what a screen grabber happens to catch.
set -euo pipefail

binary=$1
out=$2
mkdir -p "${out}"

# No GPU on the runner: the Cairo renderer draws the same on every machine.
export GSK_RENDERER=cairo
export GTK_A11Y=none

pages=(overview proxies rules extensions activity settings)
languages=(zh zh-Hant en)
themes=(light dark)

shoot() {
  local file=$1
  shift
  if ! timeout 60 "${binary}" --screenshot "${file}" "$@"; then
    echo "::error::no screenshot for: $*"
    exit 1
  fi
  [[ -s "${file}" ]] || { echo "::error::${file} is missing or empty"; exit 1; }
}

for language in "${languages[@]}"; do
  for theme in "${themes[@]}"; do
    for page in "${pages[@]}"; do
      shoot "${out}/${page}-${language}-${theme}.png" --language "${language}" --theme "${theme}" --page "${page}"
    done
  done
done

# A narrow window shows one column: the page, with a way back to the sidebar.
shoot "${out}/narrow-rules-en-light.png" --language en --theme light --page rules --size 420x720
# Twice the pixels for the same layout.
GDK_SCALE=2 shoot "${out}/scale2-overview-en-light.png" --language en --theme light --page overview

python3 - "${out}" <<'PY'
import hashlib
import struct
import sys
from pathlib import Path

out = Path(sys.argv[1])


def size(path: Path) -> tuple[int, int]:
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        sys.exit(f"{path.name} is not a PNG")
    return struct.unpack(">II", data[16:24])


files = sorted(out.glob("*.png"))
if len(files) != 6 * 3 * 2 + 2:
    sys.exit(f"expected 38 screenshots, found {len(files)}")
digests = {}
for path in files:
    width, height = size(path)
    if path.name.startswith("narrow-"):
        expected = (420, 720)
    elif path.name.startswith("scale2-"):
        expected = (2160, 1440)
    else:
        expected = (1080, 720)
    if (width, height) != expected:
        sys.exit(f"{path.name} is {width}x{height}, expected {expected[0]}x{expected[1]}")
    # Every page, language and theme draws something of its own: two equal files mean an option was not
    # applied (or nothing was drawn at all).
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest in digests:
        sys.exit(f"{path.name} is the same picture as {digests[digest]}")
    digests[digest] = path.name
    print(f"{path.name}: {width}x{height}, {path.stat().st_size} bytes")
print(f"{len(files)} screenshots, all different")
PY
