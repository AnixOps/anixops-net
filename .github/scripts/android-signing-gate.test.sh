#!/usr/bin/env bash
# Tests for android-signing-gate.sh with a stand-in apksigner: release kind and the pinned certificate pass;
# a throwaway key, a missing or malformed pin, another certificate or several signers fail.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
failures=0

pinned="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
other="fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
apk="${work}/app.apk"
: > "${apk}"

# fake_apksigner <digest>...: prints one signer block per digest, like apksigner verify --print-certs.
fake_apksigner() {
  local script="${work}/apksigner" i=1 digest
  {
    echo '#!/usr/bin/env bash'
    for digest in "$@"; do
      echo "echo 'Signer #${i} certificate DN: CN=AnixOps'"
      echo "echo 'Signer #${i} certificate SHA-256 digest: ${digest}'"
      echo "echo 'Signer #${i} certificate SHA-1 digest: 0000'"
      i=$((i + 1))
    done
  } > "${script}"
  chmod +x "${script}"
  echo "${script}"
}

# expect pass|fail <kind> <expected pin> <signer digests...>
expect() {
  local want="$1" kind="$2" pin="$3" result
  shift 3
  if APKSIGNER="$(fake_apksigner "$@")" bash "${here}/android-signing-gate.sh" "${kind}" "${pin}" "${apk}" > /dev/null 2>&1; then
    result=pass
  else
    result=fail
  fi
  if [ "${result}" != "${want}" ]; then
    echo "::error::android-signing-gate kind=${kind} pin=${pin} signers=[$*]: expected ${want}, got ${result}"
    failures=$((failures + 1))
  fi
}

expect pass release "${pinned}" "${pinned}"
# Fingerprints as Play Console shows them: upper case with colons.
expect pass release "$(echo "${pinned}" | tr 'a-f' 'A-F' | sed 's/../&:/g; s/:$//')" "${pinned}"
expect fail debug "${pinned}" "${pinned}"
expect fail "" "${pinned}" "${pinned}"
expect fail release "" "${pinned}"
expect fail release "not-a-fingerprint" "${pinned}"
expect fail release "${pinned}" "${other}"
expect fail release "${pinned}" "${pinned}" "${other}"
expect fail release "${pinned}"

if [ "${failures}" -ne 0 ]; then
  exit 1
fi
echo "android-signing-gate: all cases pass"
