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

# Source without comments (whole-line `//`, `///`, `//!`, `/* */` blocks and spans, and a trailing ` // ...`): a call named in a
# comment is not a call. With a function name, only that function's body: from its `fn`/`func` line to the next
# declaration at the same or a shallower indent.
code_of() {
    local file="$1" function="${2:-}"
    awk -v name="${function}" '
        {
            line = $0
            if (block) {
                if (match(line, /\*\//)) { line = substr(line, RSTART + 2); block = 0 } else { next }
            }
            while (match(line, /\/\*/)) {
                rest = substr(line, RSTART + 2)
                if (match(rest, /\*\//)) {
                    line = substr(line, 1, RSTART - 1) substr(rest, RSTART + 2)
                } else {
                    line = substr(line, 1, RSTART - 1)
                    block = 1
                    break
                }
            }
            if (line ~ /^[[:space:]]*\/\//) { next }
            sub(/[[:space:]]\/\/.*$/, "", line)
            if (block && line ~ /^[[:space:]]*$/) { next }
        }
        name == "" { print line; next }
        {
            if (!inside && match(line, "(fn|func) " name "[(<]")) {
                inside = 1
                match(line, /^[[:space:]]*/)
                indent = RLENGTH
                print line
                next
            }
            if (inside) {
                match(line, /^[[:space:]]*/)
                if (RLENGTH <= indent && line ~ /(^|[[:space:]])(fn|func) /) {
                    inside = 0
                    next
                }
                print line
            }
        }
    ' "${file}"
}

# The gate that refuses a profile with a mesh outbound while it is closed must still be called by every start path:
# a deleted call leaves the flags closed and the engine (mesh compiled in) starting it. And every envelope handed to an
# engine must carry the engine's own gate, `allow_mesh` set to the release gate (the helper never takes a value): the
# kernel's default is closed, so a deleted call fails closed, but mesh would then never run in a preview build.
# file|function (empty: whole file)|pattern|calls at least. Comments do not count. The two helpers that take the gate as a value
# (Rust `set_allow_mesh`, Swift `settingAllowMesh`) are called only with the release gate (`ENABLED`, `MeshSupport.enabled`)
# and are never public: the checks above pin each call site and each default.
checks=(
    "crates/client-core/src/backend/engine.rs|start|self.start_gated(envelope, node_id, port, super::mesh::ENABLED)|1"
    "crates/client-core/src/backend/engine.rs|start|super::mesh::start_envelope(envelope)|1"
    "crates/client-core/src/backend/engine.rs|start_gated|super::mesh::prepared_start(envelope, open)|1"
    "crates/client-core/src/backend/engine.rs|through_temporary_proxy|through_proxy_gated(envelope, super::mesh::ENABLED, probe)|1"
    "crates/client-core/src/backend/engine.rs|through_proxy_gated|super::mesh::allow_mesh_text(envelope, open)|1"
    "crates/client-core/src/backend/mesh.rs|with_joined|joined_into(dir, envelope, ENABLED)|1"
    "crates/client-core/src/backend/mesh.rs|joined_into|set_allow_mesh(&mut meshed, open)|1"
    "crates/client-core/src/backend/mesh.rs|with_allow_mesh|set_allow_mesh(config, ENABLED)|1"
    "crates/client-core/src/backend/mesh.rs|with_allow_mesh_text|allow_mesh_text(envelope, ENABLED)|1"
    "crates/client-core/src/backend/mesh.rs|with_allow_mesh_bytes|allow_mesh_bytes(envelope, ENABLED)|1"
    "crates/client-core/src/backend/mesh.rs|start_envelope|prepared_start(envelope, ENABLED)|1"
    "crates/client-core/src/backend/mesh.rs|prepared_start|allow_mesh_text(envelope, open)|1"
    "crates/client-core/src/backend/mesh.rs|engine_runs_mesh|runs_mesh(ENABLED)|1"
    "crates/client-core/src/backend/mesh.rs|runs_mesh|set_allow_mesh(&mut profile, open)|1"
    "crates/client-core/src/backend/mesh.rs|check_envelope|with_allow_mesh_text(envelope)|1"
    "apps/windows-gui/src/gui/tun_mode.rs|start_with|refuse_gated(envelope)|1"
    "apps/windows-gui/src/gui/tun_mode.rs|start_with|mesh::with_allow_mesh_bytes(|1"
    "apps/windows-gui/src/gui/actions/connection.rs|start_embedded|gated_envelope(envelope)|1"
    "apps/windows-gui/src/gui/actions/connection.rs|gated_envelope|tun_mode::refuse_gated(envelope)|1"
    "apps/windows-gui/src/gui/actions/connection.rs|gated_envelope|mesh::with_allow_mesh_bytes(envelope)|1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift|beginTunnel|EmbeddedRuntimeConfig.meshGateFailure(of: |1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift|beginMeshOnly|EmbeddedRuntimeConfig.meshGateFailure(of: |1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift|handleAppMessage|EmbeddedRuntimeConfig.meshGateControlRefusal(|1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift|beginMeshOnly|EngineBudget.encoded(|1"
    "apps/ios/Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift|makeRuntimeSession|EngineBudget.encoded(|3"
    "apps/ios/Sources/NetworkCoreBridge/EngineBudget.swift|encoded|config.withAllowMesh().encoded()|1"
    "apps/ios/Sources/NetworkCoreBridge/EmbeddedRuntimeConfig.swift|withAllowMesh|settingAllowMesh(MeshSupport.enabled)|1"
    "apps/ios/Sources/NetworkCoreBridge/EmbeddedRuntimeConfig.swift|withAllowMesh|settingAllowMesh(object, MeshSupport.enabled)|1"
    "apps/ios/Sources/NetworkCoreBridge/MeshTunnel.swift|available|enabled && engineRunsMesh|1"
    "apps/ios/Sources/NetworkCoreBridge/MeshTunnel.swift||available(enabled: enabled)|1"
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
# it (Rust: everything from a `#[cfg(test)] mod` on; Swift and Kotlin: the test folders). A line of code (comments
# stripped) that names the field or one of its helpers (`allow_mesh`, `allowMesh`, `AllowMesh`) and the word `true` is
# refused, which covers `allow_mesh: true`, `allowMesh = true`, `withAllowMesh(true)`, `withAllowMesh(dict, open: true)`,
# `set_allow_mesh(x, true)`, `allow_mesh_text(x, true)`, `Value::Bool(true)` on the insert line, `["allow_mesh"] = true`
# and an escaped `\"allow_mesh\":true`. The helpers that take a value are also refused when called with it spelled `true`
# on a line of their own (`prepared_start`, `start_gated`, `through_proxy_gated`, `runs_mesh`, `joined_into`).
# A `#[cfg(test)]` before anything but a `mod` or a `use` would hide the code after it from the scan, so it is refused.
forbidden='[aA]llow_?[mM]esh.*(^|[^[:alnum:]_])true([^[:alnum:]_]|$)'
helpers='(prepared_start|start_gated|through_proxy_gated|runs_mesh|joined_into|settingAllowMesh|available)\(([^)]*[^[:alnum:]_])?true([^[:alnum:]_]|$)'
while IFS= read -r file; do
    case "${file}" in
        */Tests/* | */test/* | */tests/* | */node_modules/*) continue ;;
    esac
    if [[ "${file}" == *.rs ]]; then
        text=$(awk '
            /^[[:space:]]*#\[cfg\(test\)\]/ { pending = 1; next }
            pending && /^[[:space:]]*(#\[|\/\/)/ { next }
            pending && /^[[:space:]]*(pub(\([a-z]+\))?[[:space:]]+)?mod[[:space:]]/ { exit }
            pending && /^[[:space:]]*use[[:space:]]/ { pending = 0; next }
            pending { print "@@cfg-test-item"; pending = 0 }
            { print }
        ' "${file}")
        if printf '%s\n' "${text}" | grep -q '^@@cfg-test-item$'; then
            echo "::error file=${file}::${file} has a #[cfg(test)] item that is neither a mod nor a use; the scan for a hard-coded allow_mesh true would skip what follows it."
            found=1
        fi
    else
        text=$(cat "${file}")
    fi
    stripped=$(printf '%s\n' "${text}" | awk -v RS='\n' '
        /^[[:space:]]*\/\// { next }
        { sub(/[[:space:]]\/\/.*$/, ""); print }
    ')
    if printf '%s\n' "${stripped}" | grep -En "${forbidden}|${helpers}"; then
        echo "::error file=${file}::${file} hard-codes allow_mesh true; it is the release gate's value (mesh::with_allow_mesh, EmbeddedRuntimeConfig.withAllowMesh)."
        found=1
    fi
done < <(find crates apps -type f \( -name '*.rs' -o -name '*.swift' -o -name '*.kt' -o -name '*.ts' \) -not -path '*/target/*' -not -path '*/node_modules/*')
exit "${found}"
