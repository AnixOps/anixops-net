#!/usr/bin/env bash
# 组网 stays out of every release until kernel 0.3 is complete and audited, and its client 1.2 parts (regions, host
# groups, external networks) until kernel 0.4 is (docs/architecture/mesh-client.md, 发布闸门): the release gates are
# client-core's `ANIXOPS_MESH_PREVIEW=1` and `ANIXOPS_MESH04_PREVIEW=1` build variables and the Apple `MESH_PREVIEW` and
# `MESH04_PREVIEW` compilation conditions, and only the CI test steps (ci.yml) may open them. This fails when anything a
# release build reads sets any of them, or when a variable is set in the environment that runs it.
# Usage: mesh-release-gate.sh   (from the repository root)
set -euo pipefail

# The names are split so that this file does not match its own search.
flags=("MESH_""PREVIEW" "MESH04_""PREVIEW")
if [ -n "${ANIXOPS_MESH_PREVIEW:-}" ] || [ -n "${ANIXOPS_MESH04_PREVIEW:-}" ]; then
    echo "::error::ANIXOPS_${flags[0]} or ANIXOPS_${flags[1]} is set in the release environment; 组网 must stay closed in releases."
    exit 1
fi

# Everything that configures a release build: the workflows other than ci.yml, the composite actions, the scripts
# the workflows run, cargo's configuration and every build script, the desktop Tauri configuration, the Apple project
# and package, and the Android build scripts.
mapfile -t files < <(
    {
        find .github/workflows -name '*.yml' ! -name ci.yml
        find .github/actions -type f
        find .github/scripts -type f ! -name mesh-release-gate.sh
        find .cargo -type f
        find . -name build.rs -not -path './target/*' -not -path '*/node_modules/*' -not -path './.git/*'
        find apps -name 'tauri.conf*.json' -not -path '*/node_modules/*'
        echo apps/ios/project.yml
        echo apps/ios/Package.swift
        find apps/android-native -name '*.gradle.kts' -not -path '*/build/*'
        find apps/android-native -name 'gradle.properties' -not -path '*/build/*'
    } 2> /dev/null | sort -u
)
found=0
for file in "${files[@]}"; do
    [ -f "${file}" ] || continue
    for flag in "${flags[@]}"; do
        # Comments (`#`, `//`) may name the gate; settings may not.
        if grep -n "${flag}" "${file}" | grep -Ev '^[0-9]+:[[:space:]]*(#|//)'; then
            echo "::error file=${file}::${file} opens the 组网 release gate (${flag}); only ci.yml test steps may."
            found=1
        fi
    done
done
exit "${found}"
