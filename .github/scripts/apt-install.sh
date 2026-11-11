#!/usr/bin/env bash
# Installs Ubuntu packages on a GitHub-hosted runner, without hanging on a broken mirror.
# apt gets short timeouts and retries; when the runner's default mirrors (azure/archive/security.ubuntu.com)
# cannot be reached, the sources switch to the kernel.org Ubuntu mirror and the install is tried again.
#
# Usage: apt-install.sh <package>...
set -euo pipefail

apt_opts=(-o Acquire::Retries=3 -o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20)
fallback=http://mirrors.edge.kernel.org/ubuntu

attempt() {
  timeout 600 sudo apt-get "${apt_opts[@]}" update &&
    timeout 900 sudo apt-get "${apt_opts[@]}" install -y --no-install-recommends "$@"
}

if attempt "$@"; then
  exit 0
fi

echo "::warning::Ubuntu mirrors unreachable, switching to ${fallback}"
for file in /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do
  [ -f "${file}" ] || continue
  sudo sed -i -E "s#https?://([a-z0-9.-]+\.)?(archive|security)\.ubuntu\.com/ubuntu/?#${fallback}#g" "${file}"
done
attempt "$@"
