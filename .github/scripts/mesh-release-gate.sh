#!/usr/bin/env bash
# 组网 stays out of every release until kernel 0.3 is complete and audited (docs/architecture/mesh-client.md): the
# release gate is client-core's `ANIXOPS_MESH_PREVIEW=1` build variable and the Apple `MESH_PREVIEW` compilation
# condition, and only the CI test steps (ci.yml) may open it. This fails when anything a release build reads sets
# either one, or when the variable is set in the environment that runs it.
# Usage: mesh-release-gate.sh   (from the repository root)
set -euo pipefail

flag="MESH_""PREVIEW"
if [ -n "${ANIXOPS_MESH_PREVIEW:-}" ]; then
    echo "::error::ANIXOPS_${flag} is set in the release environment; 组网 must stay closed in releases."
    exit 1
fi

# Everything that configures a release build: the workflows other than ci.yml, the composite actions, cargo's
# configuration, the Apple project and package, and the Android build scripts.
mapfile -t files < <(
    {
        find .github/workflows -name '*.yml' ! -name ci.yml
        find .github/actions -type f
        find .cargo -type f
        echo apps/ios/project.yml
        echo apps/ios/Package.swift
        find apps/android-native -name '*.gradle.kts' -not -path '*/build/*'
        find apps/android-native -name 'gradle.properties' -not -path '*/build/*'
    } 2> /dev/null | sort -u
)
found=0
for file in "${files[@]}"; do
    [ -f "${file}" ] || continue
    # Comments (`#`, `//`) may name the gate; settings may not.
    if grep -n "${flag}" "${file}" | grep -Ev '^[0-9]+:[[:space:]]*(#|//)'; then
        echo "::error file=${file}::${file} opens the 组网 release gate (${flag}); only ci.yml test steps may."
        found=1
    fi
done
exit "${found}"
