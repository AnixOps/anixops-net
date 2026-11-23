#!/usr/bin/env bash
# Release APK signing gate (release.yml, sign-android): a tag release ships only an APK signed with the release key,
# and the signing certificate must be the pinned one (ANDROID_RELEASE_CERT_SHA256, the public SHA-256 of the app
# signing certificate from Play Console). A throwaway key, a missing pin, another keystore (for example the Play
# upload key) or more than one signer fails. Tests: android-signing-gate.test.sh.
#
# Usage: android-signing-gate.sh KIND EXPECTED_CERT_SHA256 APK
#   KIND                  sign-apk's output: release or debug
#   EXPECTED_CERT_SHA256  64 hex digits, colons and spaces allowed, any case
# APKSIGNER overrides the apksigner binary (default: the newest one under $ANDROID_HOME/build-tools).
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 KIND EXPECTED_CERT_SHA256 APK" >&2
  exit 2
fi
kind="$1"
expected="$(printf '%s' "$2" | tr -d ': ' | tr 'A-F' 'a-f')"
apk="$3"

if [ "${kind}" != "release" ]; then
  echo "::error::A release APK must be signed with the release key, not '${kind}'. Check the ANDROID_KEYSTORE_* secrets of the android-release environment."
  exit 1
fi
if [[ ! "${expected}" =~ ^[0-9a-f]{64}$ ]]; then
  echo "::error::ANDROID_RELEASE_CERT_SHA256 is not a SHA-256 fingerprint (64 hex digits). Set the repository variable to the app signing key certificate's SHA-256 from Play Console."
  exit 1
fi
if [ ! -f "${apk}" ]; then
  echo "::error::${apk} does not exist."
  exit 1
fi

apksigner="${APKSIGNER:-}"
if [ -z "${apksigner}" ]; then
  apksigner="$(ls -d "${ANDROID_HOME}"/build-tools/* | sort -V | tail -n 1)/apksigner"
fi
certs="$("${apksigner}" verify --print-certs "${apk}")"
digests="$(printf '%s\n' "${certs}" | sed -n 's/^Signer #[0-9][0-9]* certificate SHA-256 digest: *\([0-9A-Fa-f:]*\).*$/\1/p' | tr -d ':' | tr 'A-F' 'a-f')"
count="$(printf '%s\n' "${digests}" | grep -c . || true)"
if [ "${count}" != "1" ]; then
  echo "::error::Expected exactly one APK signer, found ${count}."
  exit 1
fi
if [ "${digests}" != "${expected}" ]; then
  echo "::error::The APK signing certificate SHA-256 ${digests} is not the pinned release certificate ${expected}."
  exit 1
fi
echo "Signed with the release key (certificate SHA-256 ${digests})."
