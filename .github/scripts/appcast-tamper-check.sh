#!/usr/bin/env bash
# End-to-end check of the macOS update feed, run only in GitHub Actions on macos-26 (docs/macos-updates.md):
# builds apps/ios/Tools/appcast with the app's own verifier, signs a feed and an archive, and requires every
# tampered copy to be refused with exit code 1.
set -euo pipefail

work="${RUNNER_TEMP:-$(mktemp -d)}/appcast-check"
rm -rf "${work}"
mkdir -p "${work}"
tool="${work}/anixops-appcast"

swiftc -O -o "${tool}" \
  apps/ios/Sources/NetworkCoreBridge/UpdateFeed.swift \
  apps/ios/Tools/appcast/main.swift

product=anixops-net-macos
"${tool}" keygen --out "${work}/key" 2> "${work}/keygen.txt"
test "$(stat -f '%Lp' "${work}/key")" = 600
public_key="$(sed -n 's/^public key: //p' "${work}/keygen.txt")"
test -n "${public_key}"
other_key="$("${tool}" keygen 2>&1 >/dev/null | sed -n 's/^public key: //p')"

head -c 3145745 /dev/urandom > "${work}/AnixOps-Net.pkg"
"${tool}" item --key-file "${work}/key" --product "${product}" --version 0.3.9 --build 42 --min-system 14.0 \
  --url https://updates.example.com/AnixOps-Net-0.3.9.pkg --file "${work}/AnixOps-Net.pkg" --out "${work}/item.json"
python3 -c 'import json,sys; print(json.dumps([json.load(open(sys.argv[1]))]))' "${work}/item.json" > "${work}/items.json"
"${tool}" feed --key-file "${work}/key" --product "${product}" --channel stable --sequence 7 \
  --items "${work}/items.json" --out "${work}/appcast.json"

# The signing key also works from the environment, as in the release workflow.
ANIXOPS_UPDATE_SIGNING_KEY="$(cat "${work}/key")" "${tool}" feed --product "${product}" --channel stable \
  --sequence 8 --items "${work}/items.json" --out "${work}/appcast-env.json"

verify() {
  "${tool}" verify --public-keys "${public_key}" --product "${product}" "$@"
}

verify --feed "${work}/appcast.json" --last-sequence 7 --archive "${work}/AnixOps-Net.pkg" --build 42
verify --feed "${work}/appcast-env.json"
# Key rotation: the feed verifies while the new key is listed next to an unrelated one.
"${tool}" verify --public-keys "${other_key},${public_key}" --product "${product}" --feed "${work}/appcast.json"

expect_refused() {
  local name="$1"
  shift
  set +e
  "$@" > "${work}/out.txt" 2>&1
  local code=$?
  set -e
  if [ "${code}" -ne 1 ]; then
    echo "::error::${name}: expected exit code 1, got ${code}"
    cat "${work}/out.txt"
    exit 1
  fi
  echo "refused as expected (${name}): $(tail -n 1 "${work}/out.txt")"
}

# One flipped byte in the archive.
python3 - "${work}/AnixOps-Net.pkg" "${work}/changed.pkg" <<'PY'
import sys
data = bytearray(open(sys.argv[1], "rb").read())
data[1000] ^= 1
open(sys.argv[2], "wb").write(bytes(data))
PY
expect_refused "changed archive" verify --feed "${work}/appcast.json" --archive "${work}/changed.pkg" --build 42

# A truncated archive.
head -c 1000 "${work}/AnixOps-Net.pkg" > "${work}/short.pkg"
expect_refused "truncated archive" verify --feed "${work}/appcast.json" --archive "${work}/short.pkg" --build 42

# The feed with the download pointed elsewhere, the old signature kept.
python3 - "${work}/appcast.json" "${work}/redirected.json" <<'PY'
import base64, json, sys
envelope = json.load(open(sys.argv[1]))
payload = base64.b64decode(envelope["payload"]).replace(b"updates.example.com", b"updates.example.org")
envelope["payload"] = base64.b64encode(payload).decode()
json.dump(envelope, open(sys.argv[2], "w"))
PY
expect_refused "redirected feed" verify --feed "${work}/redirected.json"

# A changed signature.
python3 - "${work}/appcast.json" "${work}/badsig.json" <<'PY'
import base64, json, sys
envelope = json.load(open(sys.argv[1]))
signature = bytearray(base64.b64decode(envelope["signature"]))
signature[0] ^= 1
envelope["signature"] = base64.b64encode(bytes(signature)).decode()
json.dump(envelope, open(sys.argv[2], "w"))
PY
expect_refused "changed signature" verify --feed "${work}/badsig.json"

# Replay of an older feed, and a key the app does not trust.
expect_refused "replayed feed" verify --feed "${work}/appcast.json" --last-sequence 8
expect_refused "untrusted key" "${tool}" verify --public-keys "${other_key}" --product "${product}" \
  --feed "${work}/appcast.json"

# A plain-HTTP download, even when signed.
"${tool}" item --key-file "${work}/key" --product "${product}" --version 0.3.9 --build 42 --min-system 14.0 \
  --url http://updates.example.com/AnixOps-Net-0.3.9.pkg --file "${work}/AnixOps-Net.pkg" --out "${work}/http-item.json"
python3 -c 'import json,sys; print(json.dumps([json.load(open(sys.argv[1]))]))' "${work}/http-item.json" > "${work}/http-items.json"
"${tool}" feed --key-file "${work}/key" --product "${product}" --channel stable --sequence 9 \
  --items "${work}/http-items.json" --out "${work}/http.json"
expect_refused "plain HTTP download" verify --feed "${work}/http.json"

echo "appcast tamper check passed"
