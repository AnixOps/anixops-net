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

# Source without comment lines (`//`, `///`, `//!`): a call named in a comment is not a call. With a function name, only
# that function's body: from its `fn`/`func` line to the next declaration at the same or a shallower indent.
code_of() {
    local file="$1" function="${2:-}"
    awk -v name="${function}" '
        /^[[:space:]]*\/\// { next }
        name == "" { print; next }
        {
            if (!inside && match($0, "(fn|func) " name "[(<]")) {
                inside = 1
                match($0, /^[[:space:]]*/)
                indent = RLENGTH
                print
                next
            }
            if (inside) {
                match($0, /^[[:space:]]*/)
                if (RLENGTH <= indent && $0 ~ /(^|[[:space:]])(fn|func) /) {
                    inside = 0
                    next
                }
                print
            }
        }
    ' "${file}"
}

# The gate that refuses a profile with a mesh outbound while it is closed must still be called by every start path:
# a deleted call leaves the flags closed and the engine (mesh compiled in) starting it. And every envelope handed to an
# engine must carry the engine's own gate, `allow_mesh` set to the release gate (the helper never takes a value): the
# kernel's default is closed, so a deleted call fails closed, but mesh would then never run in a preview build.
# file|function (empty: whole file)|pattern|calls at least. Comment lines do not count.
checks=(
    "crates/client-core/src/backend/engine.rs|start|mesh::refuse_gated_envelope(|2"
    "crates/client-core/src/backend/engine.rs|start|mesh::with_allow_mesh_text(|2"
    "crates/client-core/src/backend/engine.rs|through_temporary_proxy|mesh::with_allow_mesh_text(|1"
    "crates/client-core/src/backend/mesh.rs|with_joined|with_allow_mesh(&mut meshed)|1"
    "apps/windows-gui/src/gui/tun_mode.rs|start_with|refuse_gated(envelope)|1"
    "apps/windows-gui/src/gui/tun_mode.rs|start_with|mesh::with_allow_mesh_bytes(|1"
    "apps/windows-gui/src/gui/actions/connection.rs|start_embedded|tun_mode::refuse_gated(envelope)|1"
    "apps/windows-gui/src/gui/actions/connection.rs|start_embedded|mesh::with_allow_mesh_bytes(|1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift|beginTunnel|EmbeddedRuntimeConfig.meshGateFailure(of: |1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift|beginMeshOnly|EmbeddedRuntimeConfig.meshGateFailure(of: |1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift|handleAppMessage|EmbeddedRuntimeConfig.meshGateControlRefusal(|1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift||EngineBudget.encoded(|4"
    "apps/ios/Sources/NetworkCoreBridge/EngineBudget.swift|encoded|config.withAllowMesh().encoded()|1"
    "apps/ios/Sources/NetworkCoreAppHost/EngineCapabilities.swift|answer|EmbeddedRuntimeConfig.withAllowMesh(|1"
)
for check in "${checks[@]}"; do
    IFS='|' read -r file function pattern least <<< "${check}"
    calls=$(code_of "${file}" "${function}" | grep -cF -- "${pattern}" || true)
    if [ "${calls}" -lt "${least}" ]; then
        echo "::error file=${file}::${file}${function:+ (${function})} calls '${pattern}' ${calls} times, expected at least ${least}; the 组网 release gate must guard every engine start."
        found=1
    fi
done

# No source writes the engine's gate open by itself: `allow_mesh` is only ever the release gate's value. Tests may spell
# it (Rust: everything after the first `#[cfg(test)]` line; Swift and Kotlin: the test folders).
while IFS= read -r file; do
    case "${file}" in
        */Tests/* | */test/* | */tests/* | */node_modules/*) continue ;;
    esac
    if [[ "${file}" == *.rs ]]; then
        text=$(awk '/^[[:space:]]*#\[cfg\(test\)\]/ { exit } { print }' "${file}")
    else
        text=$(cat "${file}")
    fi
    if printf '%s\n' "${text}" | grep -Ev '^[[:space:]]*//' | grep -En 'allow_?[mM]esh"?[[:space:]]*[:=][[:space:]]*true'; then
        echo "::error file=${file}::${file} hard-codes allow_mesh true; it is the release gate's value (mesh::with_allow_mesh, EmbeddedRuntimeConfig.withAllowMesh)."
        found=1
    fi
done < <(find crates apps -type f \( -name '*.rs' -o -name '*.swift' -o -name '*.kt' -o -name '*.ts' \) -not -path '*/target/*' -not -path '*/node_modules/*')
exit "${found}"
