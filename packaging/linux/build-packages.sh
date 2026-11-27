#!/usr/bin/env bash
# Build Linux packages for the Endpoint Agent, the Management Server and the
# od CLI.
#
#   packaging/linux/build-packages.sh <version> <bin-dir> <out-dir>
#
# <bin-dir> holds opendirectory-agent-linux-x64, opendirectory-server-linux-x64
# and od-linux-x64 (the names the release attaches). Writes one package per
# component and format, with stable names so that
# releases/latest/download/<name> always points at the newest build:
#
#   opendirectory-agent-linux-amd64.deb           Debian, Ubuntu, Mint, Pop!_OS
#   opendirectory-agent-linux-x86_64.rpm          Fedora, RHEL, Rocky, Alma, openSUSE
#   opendirectory-agent-linux-x86_64.pkg.tar.zst  Arch, Manjaro, EndeavourOS
#
# Packages install the binary to /usr/bin. The agent package enrolls
# unattended when /etc/opendirectory/enroll.conf exists (see
# agent-postinstall.sh); otherwise `enroll --install` creates the service, as on
# every other OS. The server package leaves `install-service` to the admin.
#
# No Alpine .apk: the binaries link against glibc and Alpine uses musl.
#
# Needs nfpm (https://nfpm.goreleaser.com) on PATH.
set -euo pipefail

version=${1:?usage: build-packages.sh <version> <bin-dir> <out-dir>}
version=${version#v}
bin_dir=$(cd "${2:?bin-dir}" && pwd)
out_dir=${3:?out-dir}
mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)
here=$(cd "$(dirname -- "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

noop="$work/noop.sh"
printf '#!/bin/sh\nexit 0\n' >"$noop"

# package | binary | release asset | description | next step shown after install
components=(
  "opendirectory-agent|opendirectory-agent|opendirectory-agent-linux-x64|OpenDirectory Endpoint Agent: applies Group Policy and reports inventory to the Management Server|"
  "opendirectory-server|opendirectory-server|opendirectory-server-linux-x64|OpenDirectory Management Server: directory, Group Policy and endpoint management|sudo opendirectory-server install-service"
  "opendirectory-cli|od|od-linux-x64|od, the OpenDirectory command-line client for the Management Server|od --help"
)

for c in "${components[@]}"; do
  IFS='|' read -r pkg bin asset desc hint <<<"$c"
  src="$bin_dir/$asset"
  [ -f "$src" ] || { echo "missing $src" >&2; exit 1; }
  chmod 0755 "$src"

  if [ "$pkg" = opendirectory-agent ]; then
    post="$here/agent-postinstall.sh"
    pre="$here/agent-preremove.sh"
  else
    post="$work/$pkg.postinstall"
    printf '#!/bin/sh\necho "%s %s installed. Next step: %s"\n' "$pkg" "$version" "$hint" >"$post"
    pre="$noop"
  fi

  export PKG_NAME=$pkg BIN_NAME=$bin BIN_PATH=$src PKG_DESCRIPTION=$desc \
    VERSION=$version README="$root/README.md" POSTINSTALL=$post PREREMOVE=$pre

  nfpm package -f "$here/nfpm.yaml" -p deb -t "$out_dir/$pkg-linux-amd64.deb"
  nfpm package -f "$here/nfpm.yaml" -p rpm -t "$out_dir/$pkg-linux-x86_64.rpm"
  nfpm package -f "$here/nfpm.yaml" -p archlinux -t "$out_dir/$pkg-linux-x86_64.pkg.tar.zst"
done

ls -l "$out_dir"
