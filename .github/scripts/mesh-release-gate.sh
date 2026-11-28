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
# the workflows run, cargo's configuration and every build script, the Apple project
# and package, and the Android build scripts.
# Read without `mapfile` and associative arrays, which the macOS runners' bash 3.2 lacks (ios-testflight.yml runs
# this there).
files=()
while IFS= read -r file; do
    files+=("${file}")
done < <(
    {
        find .github/workflows -name '*.yml' ! -name ci.yml
        find .github/actions -type f
        find .github/scripts -type f ! -name mesh-release-gate.sh
        find .cargo -type f
        find . -name build.rs -not -path './target/*' -not -path '*/node_modules/*' -not -path './.git/*'
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

# Source as code, one lexer for every scan below: comments are removed (whole-line `//`, `///`, `//!`, `/* */` blocks and
# spans, a trailing `//`), and a `//` or `/*` inside a "..." string literal (`"http://x"`) stays: strings are read
# (escapes included, one line at a time), as are the char literals `'"'`, `'{'`, `'\"'`; in TypeScript a '...' string too.
# A line that held only a comment is dropped. With `skip-tests` a Rust file is read without its test code: a
# `#[cfg(test)]` item (a `mod`, a `use`, an `fn`, an `impl`) is skipped through its own closing brace (or its `;`),
# braces counted outside strings and comments, and the production code after it is read again; a `#![cfg(test)]` file
# is skipped whole.
lex() {
    local file="$1" skip="${2:-}"
    awk -v skiptests="${skip}" -v single="$([[ "${file}" == *.ts ]] && echo 1 || echo 0)" '
        # Starts or goes on skipping an item: bracket depth over the string-free shape of the line.
        function skip_item(shape,   k, ch) {
            for (k = 1; k <= length(shape); k++) {
                ch = substr(shape, k, 1)
                if (ch == "{" || ch == "(" || ch == "[") { depth++; if (ch == "{") opened = 1 }
                else if (ch == "}" || ch == ")" || ch == "]") depth--
                else if (ch == ";" && depth == 0) semi = 1
            }
            skipping = !(depth <= 0 && (opened || semi))
        }
        {
            line = $0; out = ""; shape = ""; hadc = (block ? 1 : 0); n = length(line); i = 1; instr = 0
            while (i <= n) {
                c = substr(line, i, 1); two = substr(line, i, 2)
                if (block) {
                    if (two == "*/") { block = 0; i += 2 } else { i++ }
                    continue
                }
                if (instr) {
                    out = out c
                    if (c == "\\") { out = out substr(line, i + 1, 1); i += 2; continue }
                    if (c == quote) { instr = 0; shape = shape c }
                    i++; continue
                }
                if (two == "//") { hadc = 1; break }
                if (two == "/*") { hadc = 1; block = 1; i += 2; continue }
                if (c == "\"" || (single && c == "\047")) { instr = 1; quote = c; out = out c; shape = shape c; i++; continue }
                if (c == "\047" && substr(line, i + 2, 1) == "\047") { out = out substr(line, i, 3); i += 3; continue }
                if (c == "\047" && substr(line, i + 1, 1) == "\\" && substr(line, i + 3, 1) == "\047") { out = out substr(line, i, 4); i += 4; continue }
                out = out c; shape = shape c; i++
            }
            if (hadc && out ~ /^[[:space:]]*$/) { next }
            if (skiptests == "") { print out; next }
            if (skipall) { next }
            if (skipping) { skip_item(shape); next }
            if (out ~ /^[[:space:]]*#!\[cfg\(test\)\]/) { skipall = 1; next }
            if (out ~ /^[[:space:]]*#\[cfg\(test\)\]/) {
                sub(/^[[:space:]]*#\[cfg\(test\)\]/, "", out); sub(/^[[:space:]]*#\[cfg\(test\)\]/, "", shape)
                pending = 1
                if (out ~ /^[[:space:]]*$/) { next }
            }
            if (pending) {
                if (out ~ /^[[:space:]]*$/ || out ~ /^[[:space:]]*#\[/) { next }
                pending = 0; depth = 0; opened = 0; semi = 0
                skip_item(shape)
                next
            }
            print out
        }
    ' "${file}"
}

# `lex` without test skipping; with a function name, only that function's body: from its `fn`/`func` line to the next
# declaration at the same or a shallower indent.
code_of() {
    local file="$1" function="${2:-}"
    if [ -z "${function}" ]; then
        lex "${file}"
        return
    fi
    lex "${file}" | awk -v name="${function}" '
        {
            line = $0
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
    '
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
    "crates/client-core/src/backend/mesh.rs|start_envelope|prepared_start(envelope, ENABLED)|1"
    "crates/client-core/src/backend/mesh.rs|prepared_start|allow_mesh_text(envelope, open)|1"
    "crates/client-core/src/backend/mesh.rs|engine_runs_mesh|runs_mesh(ENABLED)|1"
    "crates/client-core/src/backend/mesh.rs|runs_mesh|set_allow_mesh(&mut profile, open)|1"
    "crates/client-core/src/backend/mesh.rs|check_envelope|with_allow_mesh_text(envelope)|1"
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
# it (Rust: every `#[cfg(test)]` item is skipped through its closing brace, see `lex`; Swift, Kotlin and TypeScript:
# the folders named test, tests, Tests and androidTest). A line of code (comments stripped) that names the field or one
# of its helpers (`allow_mesh`, `allowMesh`, `AllowMesh`) and the word `true` is refused, which covers `allow_mesh: true`,
# `allowMesh = true`, `withAllowMesh(true)`, `withAllowMesh(dict, open: true)`, `set_allow_mesh(x, true)`,
# `allow_mesh_text(x, true)`, `Value::Bool(true)` on the insert line, `["allow_mesh"] = true` and an escaped
# `\"allow_mesh\":true`. The helpers that take a value are also refused when called with it spelled `true` on a line of
# their own (`prepared_start`, `start_gated`, `through_proxy_gated`, `runs_mesh`, `joined_into`). That only sees the literal
# written on one line; a value passed in a variable, `!false`, a constant, a macro, a multi-line `insert` or a renamed
# serde field gets past it. So the writers are also counted below: a new call to one of them in non-test code, which is
# how a new start path would get the gate open, changes a count and fails until this script is updated with it.
forbidden='[aA]llow_?[mM]esh.*(^|[^[:alnum:]_])true([^[:alnum:]_]|$)'
helpers='(prepared_start|start_gated|through_proxy_gated|runs_mesh|joined_into|settingAllowMesh|available)\(([^)]*[^[:alnum:]_])?true([^[:alnum:]_]|$)'
# The writers and how many calls of each the non-test code has, in every file together (a call is the name followed by
# `(`, its own `fn`/`func` line not counted): exactly, not at least.
writers=(
    "allow_mesh_text|5"
    "set_allow_mesh|4"
    "prepared_start|2"
    "start_envelope|1"
    "start_gated|1"
    "through_proxy_gated|1"
    "joined_into|1"
    "runs_mesh|1"
    "check_envelope|5"
    "with_allow_mesh|0"
    "with_allow_mesh_text|1"
    "withAllowMesh|2"
    "settingAllowMesh|2"
)
# Calls per writer, by index into `writers`.
writer_calls=()
for index in "${!writers[@]}"; do
    writer_calls[index]=0
done
while IFS= read -r file; do
    case "${file}" in
        */Tests/* | */test/* | */tests/* | */androidTest/* | */node_modules/*) continue ;;
    esac
    if [[ "${file}" == *.rs ]]; then
        text=$(lex "${file}" skip-tests)
    else
        text=$(lex "${file}")
    fi
    if printf '%s\n' "${text}" | grep -En "${forbidden}|${helpers}"; then
        echo "::error file=${file}::${file} hard-codes allow_mesh true; it is the release gate's value (mesh::with_allow_mesh, EmbeddedRuntimeConfig.withAllowMesh)."
        found=1
    fi
    for index in "${!writers[@]}"; do
        name="${writers[index]%%|*}"
        calls=$(printf '%s\n' "${text}" | grep -E "(^|[^[:alnum:]_])${name}[[:space:]]*\(" | grep -Evc "(fn|func)[[:space:]]+${name}([^[:alnum:]_]|$)" || true)
        writer_calls[index]=$(( writer_calls[index] + calls ))
    done
done < <(find crates apps -type f \( -name '*.rs' -o -name '*.swift' -o -name '*.kt' -o -name '*.ts' \) -not -path '*/target/*' -not -path '*/node_modules/*')
for index in "${!writers[@]}"; do
    name="${writers[index]%%|*}"
    expected="${writers[index]##*|}"
    if [ "${writer_calls[index]}" -ne "${expected}" ]; then
        echo "::error::${name}( is called ${writer_calls[index]} times in non-test code, expected exactly ${expected}; a new start path or writer of the engine's allow_mesh must be added to the 'writers' list in $(basename "$0") after review (and to the checks above), a removed one taken off it."
        found=1
    fi
done
exit "${found}"
