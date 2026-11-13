#!/usr/bin/env bash
# Cold start and client health check of the built Linux client, run under Xvfb by CI's linux-gui job.
#
# 1. Lays the binary out as the release does (bin/anixops-net + share/anixops-net/integrity.sha256).
# 2. Cold start: launches it RUNS times with an empty config directory and ANIXOPS_STARTUP_PROBE set, so it
#    quits as soon as the window's page has loaded. The wall time of each launch (exec to exit) is measured
#    here; the median must stay within BUDGET_MS.
# 3. Health: launches it once more without the probe and checks the loopback Prometheus endpoint, including
#    the install integrity result.
#
# Usage: linux-client-health.sh <path to anixops-net>   (inside xvfb-run)
set -euo pipefail

binary="$1"
runs="${RUNS:-7}"
budget_ms="${BUDGET_MS:-1500}"
port=9477

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
install -d "${work}/install/bin" "${work}/install/share/anixops-net"
install -m 0755 "${binary}" "${work}/install/bin/anixops-net"
(cd "${work}/install/bin" && sha256sum anixops-net) > "${work}/install/share/anixops-net/integrity.sha256"
app="${work}/install/bin/anixops-net"

summary="${GITHUB_STEP_SUMMARY:-/dev/stdout}"
times=()
{
  echo "### Linux client cold start"
  echo
  echo "| Run | Exec to exit (ms) | Main to page load (ms) |"
  echo "| --- | --- | --- |"
} >> "${summary}"
for run in $(seq 1 "${runs}"); do
  config="${work}/config-${run}"
  mkdir -p "${config}"
  probe="${work}/probe-${run}.json"
  start=$(date +%s%N)
  if ! XDG_CONFIG_HOME="${config}" ANIXOPS_STARTUP_PROBE="${probe}" timeout 60 "${app}" > "${work}/run-${run}.log" 2>&1; then
    echo "::error::cold start run ${run} did not finish"
    cat "${work}/run-${run}.log"
    exit 1
  fi
  end=$(date +%s%N)
  elapsed=$(( (end - start) / 1000000 ))
  if [ ! -s "${probe}" ]; then
    echo "::error::cold start run ${run} exited without loading the window"
    cat "${work}/run-${run}.log"
    exit 1
  fi
  ready=$(jq -r .ready_ms "${probe}")
  times+=("${elapsed}")
  echo "| ${run} | ${elapsed} | ${ready} |" >> "${summary}"
done
median=$(printf '%s\n' "${times[@]}" | sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}')
echo >> "${summary}"
echo "Median ${median} ms, budget ${budget_ms} ms." >> "${summary}"
echo "cold start median: ${median} ms (budget ${budget_ms} ms)"
if [ "${median}" -gt "${budget_ms}" ]; then
  echo "::error::cold start median ${median} ms is over the ${budget_ms} ms budget"
  exit 1
fi

# Health endpoint and integrity.
config="${work}/config-health"
mkdir -p "${config}"
XDG_CONFIG_HOME="${config}" "${app}" > "${work}/health.log" 2>&1 &
pid=$!
metrics=""
for _ in $(seq 1 60); do
  if metrics="$(curl -fsS "http://127.0.0.1:${port}/metrics" 2>/dev/null)" &&
    grep -q 'anixops_client_integrity{state="verified"} 1' <<< "${metrics}" &&
    grep -q '^anixops_client_cold_start_seconds ' <<< "${metrics}"; then
    break
  fi
  metrics=""
  sleep 0.5
done
root_status="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${port}/" || true)"
kill "${pid}" 2>/dev/null || true
wait "${pid}" 2>/dev/null || true
if [ -z "${metrics}" ]; then
  echo "::error::the metrics endpoint did not report a verified install and a cold start"
  curl -sS "http://127.0.0.1:${port}/metrics" || true
  cat "${work}/health.log"
  exit 1
fi
for line in \
  '# TYPE anixops_client_info gauge' \
  'anixops_client_info{platform="linux",' \
  'anixops_client_connected 0' \
  'anixops_client_crash_reports_enabled 1' \
  'anixops_client_cold_start_seconds '; do
  if ! grep -qF "${line}" <<< "${metrics}"; then
    echo "::error::metrics endpoint is missing: ${line}"
    echo "${metrics}"
    exit 1
  fi
done
if [ "${root_status}" != "404" ]; then
  echo "::error::the endpoint should answer 404 outside /metrics, got ${root_status}"
  exit 1
fi
echo "metrics endpoint and integrity check OK"
