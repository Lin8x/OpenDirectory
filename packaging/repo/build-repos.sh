#!/usr/bin/env bash
# Build the signed APT and DNF/Zypper repositories that GitHub Pages serves.
#
#   REPO_SIGNING_KEY="$(cat private.asc)" packaging/repo/build-repos.sh <pkg-dir> <out-dir>
#
# <pkg-dir> holds the release's .deb and .rpm files. <out-dir> becomes the Pages
# site root:
#
#   apt/                 flat APT repository (Packages, Release, InRelease)
#   rpm/                 DNF/Zypper repository (repodata, signed repomd.xml)
#   opendirectory.asc    the signing key (ASCII armor)
#   opendirectory.gpg    the signing key (binary keyring for APT signed-by)
#   opendirectory.repo   repository definition for /etc/yum.repos.d and zypper
#
# Needs apt-ftparchive (apt-utils), createrepo_c and gpg.
set -euo pipefail

pkg_dir=$(cd "${1:?usage: build-repos.sh <pkg-dir> <out-dir>}" && pwd)
out=${2:?out-dir}
mkdir -p "$out"
out=$(cd "$out" && pwd)
: "${REPO_SIGNING_KEY:?REPO_SIGNING_KEY must hold the armored private signing key}"
base_url=${REPO_BASE_URL:-https://lin8x.github.io/OpenDirectory}

GNUPGHOME=$(mktemp -d)
export GNUPGHOME
trap 'rm -rf "$GNUPGHOME"' EXIT
printf '%s\n' "$REPO_SIGNING_KEY" | gpg --batch --quiet --import
key_id=$(gpg --batch --list-secret-keys --with-colons | awk -F: '/^fpr:/ {print $10; exit}')
sign() { gpg --batch --yes --pinentry-mode loopback --passphrase '' --local-user "$key_id" "$@"; }

gpg --batch --armor --export "$key_id" >"$out/opendirectory.asc"
gpg --batch --export "$key_id" >"$out/opendirectory.gpg"

# --- APT (flat repository: deb [signed-by=...] <url>/apt ./)
mkdir -p "$out/apt"
cp "$pkg_dir"/*.deb "$out/apt/"
(
  cd "$out/apt"
  apt-ftparchive packages . >Packages
  gzip -9kf Packages
  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=OpenDirectory \
    -o APT::FTPArchive::Release::Label=OpenDirectory \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    release . >Release
  sign --clearsign -o InRelease Release
  sign --armor --detach-sign -o Release.gpg Release
)

# --- DNF / Zypper
mkdir -p "$out/rpm"
cp "$pkg_dir"/*.rpm "$out/rpm/"
createrepo_c --quiet "$out/rpm"
sign --armor --detach-sign -o "$out/rpm/repodata/repomd.xml.asc" "$out/rpm/repodata/repomd.xml"

cat >"$out/opendirectory.repo" <<REPO
[opendirectory]
name=OpenDirectory
baseurl=$base_url/rpm
enabled=1
# Repository metadata is signed and lists every package's SHA-256.
repo_gpgcheck=1
gpgcheck=0
gpgkey=$base_url/opendirectory.asc
REPO

find "$out" -maxdepth 2 -type f | sort
