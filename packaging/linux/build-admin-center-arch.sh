#!/usr/bin/env bash
# Build the Admin Center's Arch Linux package from the release .deb.
#
#   packaging/linux/build-admin-center-arch.sh <version> <admin-center.deb> <out-dir>
#
# Writes <out-dir>/OpenDirectory-AdminCenter-linux-x86_64.pkg.tar.zst. Needs nfpm.
set -euo pipefail
version=${1:?usage: build-admin-center-arch.sh <version> <deb> <out-dir>}
deb=$(cd "$(dirname -- "${2:?deb}")" && pwd)/$(basename -- "$2")
out_dir=${3:?out-dir}
mkdir -p "$out_dir"
here=$(cd "$(dirname -- "$0")" && pwd)
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
dpkg-deb -x "$deb" "$ROOT"
export ROOT VERSION=${version#v}
nfpm package -f "$here/admin-center-arch.yaml" -p archlinux \
  -t "$out_dir/OpenDirectory-AdminCenter-linux-x86_64.pkg.tar.zst"
