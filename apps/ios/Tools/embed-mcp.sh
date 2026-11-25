#!/usr/bin/env bash
# Xcode build phase of NetworkCoreMac: copies anixops-mcp (built by .github/scripts/build-macos-mcp.sh) into
# Contents/MacOS next to the `anixops` command-line tool and signs it before Xcode signs the app. Like the tool, it
# carries the app group entitlement so it can reach the app's socket in the group container, and the hardened
# runtime for notarization. Unsigned builds (CODE_SIGNING_ALLOWED=NO, as in CI) only copy it.
set -euo pipefail

source="${SRCROOT}/Generated/anixops-mcp"
destination="${TARGET_BUILD_DIR}/${EXECUTABLE_FOLDER_PATH}/anixops-mcp"

if [[ ! -x "${source}" ]]; then
  echo "error: ${source} is missing; run .github/scripts/build-macos-mcp.sh first" >&2
  exit 1
fi

mkdir -p "$(dirname "${destination}")"
cp -f "${source}" "${destination}"
chmod 755 "${destination}"

if [[ "${CODE_SIGNING_ALLOWED:-NO}" != YES || -z "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]]; then
  exit 0
fi

entitlements="${DERIVED_FILE_DIR}/anixops-mcp.entitlements"
mkdir -p "${DERIVED_FILE_DIR}"
sed "s/\$(TeamIdentifierPrefix)/${DEVELOPMENT_TEAM}./g" "${SRCROOT}/Entitlements/NetworkCoreCLI.entitlements" \
  > "${entitlements}"

flags=(--force --options runtime --entitlements "${entitlements}" --sign "${EXPANDED_CODE_SIGN_IDENTITY}")
# Ad-hoc signatures ("-") cannot carry a secure timestamp.
if [[ "${EXPANDED_CODE_SIGN_IDENTITY}" != "-" ]]; then
  flags+=(--timestamp)
fi
/usr/bin/codesign "${flags[@]}" "${destination}"
