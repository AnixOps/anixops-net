#!/usr/bin/env bash
# Interop check between the app's update verifier and AnixOps/anix-kit (docs/macos-updates.md), run only in
# GitHub Actions on macos-26: a feed and archive signed by anix-kit's Go tools must pass the Swift verifier
# (through apps/ios/Tools/appcast, built from the app's own UpdateFeed.swift), and the reverse; a changed
# archive must be refused by both.
set -euo pipefail

kit="${ANIX_KIT_VERSION:-v0.1.0}"
anixsign=(go run "github.com/AnixOps/anix-kit/cmd/anixsign@${kit}")
anixupdate=(go run "github.com/AnixOps/anix-kit/cmd/anixupdate@${kit}")

work="${RUNNER_TEMP:-$(mktemp -d)}/appcast-interop"
rm -rf "${work}"
mkdir -p "${work}"
tool="${work}/anixops-appcast"
swiftc -O -o "${tool}" \
  apps/ios/Sources/NetworkCoreBridge/UpdateFeed.swift \
  apps/ios/Tools/appcast/main.swift

product=anixops-net-macos
head -c 2097169 /dev/urandom > "${work}/AnixOps-Net.pkg"
as_array() {
  python3 -c 'import json,sys; print(json.dumps([json.load(open(sys.argv[1]))]))' "$1"
}

# Go signs, Swift verifies.
read -r go_public go_key_id < <("${anixsign[@]}" keygen "${work}/go.pem")
test -n "${go_public}"
"${anixupdate[@]}" item "${work}/go.pem" "${product}" 0.4.0 140 14.0 \
  https://updates.example.com/AnixOps-Net-0.4.0.pkg "${work}/AnixOps-Net.pkg" > "${work}/go-item.json"
as_array "${work}/go-item.json" > "${work}/go-items.json"
"${anixupdate[@]}" feed "${work}/go.pem" "${product}" stable 5 "${work}/go-items.json" > "${work}/go-feed.json"
"${tool}" verify --public-keys "${go_public}" --product "${product}" --feed "${work}/go-feed.json" \
  --last-sequence 4 --archive "${work}/AnixOps-Net.pkg" --build 140 | tee "${work}/swift-verify.txt"
grep -q "trusted key ids ${go_key_id}" "${work}/swift-verify.txt"
# The Swift tool also signs with the Go-made PKCS#8 key.
"${tool}" item --key-file "${work}/go.pem" --product "${product}" --version 0.4.0 --build 140 --min-system 14.0 \
  --url https://updates.example.com/AnixOps-Net-0.4.0.pkg --file "${work}/AnixOps-Net.pkg" --out "${work}/mixed-item.json"
as_array "${work}/mixed-item.json" > "${work}/mixed-items.json"
"${tool}" feed --key-file "${work}/go.pem" --product "${product}" --channel stable --sequence 6 \
  --items "${work}/mixed-items.json" --out "${work}/mixed-feed.json"
"${anixupdate[@]}" verify "${go_public}" "${product}" "${work}/mixed-feed.json" "${work}/AnixOps-Net.pkg" 140

# Swift signs, Go verifies.
"${tool}" keygen --out "${work}/swift.pem" 2> "${work}/keygen.txt"
swift_public="$(sed -n 's/^public key: //p' "${work}/keygen.txt")"
swift_key_id="$(sed -n 's/^key id: //p' "${work}/keygen.txt")"
test "$("${anixsign[@]}" pub "${work}/swift.pem" | awk '{print $1}')" = "${swift_public}"
test "$("${anixsign[@]}" id "${swift_public}" | awk '{print $1}')" = "${swift_key_id}"
"${tool}" item --key-file "${work}/swift.pem" --product "${product}" --version 0.4.0 --build 140 --min-system 14.0 \
  --url https://updates.example.com/AnixOps-Net-0.4.0.pkg --file "${work}/AnixOps-Net.pkg" --out "${work}/swift-item.json"
as_array "${work}/swift-item.json" > "${work}/swift-items.json"
"${tool}" feed --key-file "${work}/swift.pem" --product "${product}" --channel stable --sequence 9 \
  --items "${work}/swift-items.json" --out "${work}/swift-feed.json"
"${anixupdate[@]}" verify "${swift_public}" "${product}" "${work}/swift-feed.json" "${work}/AnixOps-Net.pkg" 140

# A changed archive is refused by both.
cp "${work}/AnixOps-Net.pkg" "${work}/changed.pkg"
python3 - "${work}/changed.pkg" <<'PY'
import sys
with open(sys.argv[1], "r+b") as f:
    f.seek(4096)
    b = f.read(1)
    f.seek(4096)
    f.write(bytes([b[0] ^ 1]))
PY
if "${anixupdate[@]}" verify "${swift_public}" "${product}" "${work}/swift-feed.json" "${work}/changed.pkg" 140; then
  echo "anixupdate accepted a changed archive" >&2
  exit 1
fi
set +e
"${tool}" verify --public-keys "${go_public}" --product "${product}" --feed "${work}/go-feed.json" \
  --archive "${work}/changed.pkg" --build 140
status=$?
set -e
test "${status}" -eq 1
echo "anix-kit ${kit} interop ok"
