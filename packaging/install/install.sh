#!/bin/sh
# Install the OpenDirectory Admin Center on Linux or macOS.
#
#   curl -fsSL https://github.com/Lin8x/OpenDirectory/releases/latest/download/install.sh | sh
#
# Options (after `sh -s --`):
#   --format deb|rpm|pacman|appimage|dmg   pick the package instead of detecting it
#   --version v0.13.0                     install a specific release (default: latest)
#   --no-repo                             don't add the OpenDirectory APT/DNF repository
#   --dry-run                             download and verify, but only print the install steps
#
# Linux: uses the .deb with apt, the .rpm with dnf, yum or zypper, the Arch
# package with pacman, and otherwise the AppImage in ~/.local/bin. With apt,
# dnf, yum and zypper it also adds the signed OpenDirectory repository, so
# routine system updates keep the Admin Center current.
# macOS: copies the app from the .dmg into /Applications.
# Every download is checked against the release's SHA256SUMS.
set -eu

repo=Lin8x/OpenDirectory
base=${OD_RELEASE_BASE:-}
version=latest
format=
dry_run=0
use_repo=1
repo_url=${OD_REPO_URL:-https://lin8x.github.io/OpenDirectory}

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }
run() {
  if [ "$dry_run" = 1 ]; then say "+ $*"; else "$@"; fi
}

while [ $# -gt 0 ]; do
  case "$1" in
    --format) format=${2:?--format needs a value}; shift 2 ;;
    --format=*) format=${1#*=}; shift ;;
    --version) version=${2:?--version needs a value}; shift 2 ;;
    --version=*) version=${1#*=}; shift ;;
    --dry-run) dry_run=1; shift ;;
    --no-repo) use_repo=0; shift ;;
    -h | --help)
      say "Usage: install.sh [--format deb|rpm|pacman|appimage|dmg] [--version vX.Y.Z] [--no-repo] [--dry-run]"
      exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
done

if [ -z "$base" ]; then
  if [ "$version" = latest ]; then
    base="https://github.com/$repo/releases/latest/download"
  else
    base="https://github.com/$repo/releases/download/v${version#v}"
  fi
fi

sudo=
if [ "$(id -u)" != 0 ]; then
  if command -v sudo >/dev/null 2>&1; then sudo=sudo
  elif command -v doas >/dev/null 2>&1; then sudo=doas
  fi
fi
as_root() {
  if [ "$(id -u)" != 0 ] && [ -z "$sudo" ]; then die "this step needs root; install sudo or run as root"; fi
  run $sudo "$@"
}

have() { command -v "$1" >/dev/null 2>&1; }

os=$(uname -s)
arch=$(uname -m)
if [ -z "$format" ]; then
  case "$os" in
    Darwin) format=dmg ;;
    Linux)
      [ "$arch" = x86_64 ] || die "the Admin Center is built for x86_64 Linux only (this is $arch)"
      if have apt-get && have dpkg; then format=deb
      elif have dnf || have yum || have zypper; then format=rpm
      elif have pacman; then format=pacman
      else format=appimage
      fi ;;
    *) die "unsupported OS: $os (on Windows use install.ps1)" ;;
  esac
fi

case "$format" in
  deb) asset=OpenDirectory-AdminCenter-linux-amd64.deb ;;
  rpm) asset=OpenDirectory-AdminCenter-linux-x86_64.rpm ;;
  pacman) asset=OpenDirectory-AdminCenter-linux-x86_64.pkg.tar.zst ;;
  appimage) asset=OpenDirectory-AdminCenter-linux-x86_64.AppImage ;;
  dmg) asset=OpenDirectory-AdminCenter-macos-universal.dmg ;;
  *) die "unknown format: $format (deb, rpm, pacman, appimage or dmg)" ;;
esac

have curl || die "curl is required"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
file="$tmp/$asset"

say "Downloading $asset from $base"
# HTTPS only, unless OD_RELEASE_BASE points somewhere else (local testing).
proto="--proto =https"
[ -n "${OD_RELEASE_BASE:-}" ] && proto=
# shellcheck disable=SC2086
curl -fL -# $proto --retry 3 -o "$file" "$base/$asset" || die "download failed: $base/$asset"

# shellcheck disable=SC2086
if curl -fsSL $proto -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" 2>/dev/null; then
  want=$(awk -v f="$asset" '$2 == f || $2 == "*" f {print $1}' "$tmp/SHA256SUMS")
  [ -n "$want" ] || die "$asset isn't listed in SHA256SUMS"
  if have sha256sum; then got=$(sha256sum "$file" | cut -d' ' -f1)
  else got=$(shasum -a 256 "$file" | cut -d' ' -f1)
  fi
  [ "$want" = "$got" ] || die "checksum mismatch for $asset (expected $want, got $got)"
  say "Checksum verified."
else
  say "No SHA256SUMS for this release; skipping checksum verification."
fi

# Add the signed APT/DNF/Zypper repository for future updates, if it's online.
add_repo() {
  [ "$use_repo" = 1 ] || return 0
  curl -fsI "$repo_url/opendirectory.asc" >/dev/null 2>&1 || {
    say "The OpenDirectory package repository isn't reachable; installing without it."
    return 0
  }
  case "$1" in
    deb)
      curl -fsSL "$repo_url/opendirectory.gpg" -o "$tmp/opendirectory.gpg"
      as_root install -m 0644 "$tmp/opendirectory.gpg" /usr/share/keyrings/opendirectory.gpg
      printf 'deb [signed-by=/usr/share/keyrings/opendirectory.gpg] %s/apt ./\n' "$repo_url" >"$tmp/opendirectory.list"
      as_root install -m 0644 "$tmp/opendirectory.list" /etc/apt/sources.list.d/opendirectory.list ;;
    rpm)
      if have zypper && ! have dnf; then
        as_root zypper --non-interactive addrepo --refresh "$repo_url/opendirectory.repo" ||
          say "(the repository may already be added)"
        as_root rpm --import "$repo_url/opendirectory.asc"
      else
        curl -fsSL "$repo_url/opendirectory.repo" -o "$tmp/opendirectory.repo"
        as_root install -m 0644 "$tmp/opendirectory.repo" /etc/yum.repos.d/opendirectory.repo
      fi ;;
  esac
  say "Added the OpenDirectory repository: system updates now include the Admin Center."
}

case "$format" in
  deb)
    add_repo deb
    as_root apt-get install -y "$file" ;;
  rpm)
    add_repo rpm
    if have dnf; then as_root dnf install -y "$file"
    elif have yum; then as_root yum install -y "$file"
    else as_root zypper --non-interactive install --allow-unsigned-rpm "$file"
    fi ;;
  pacman)
    as_root pacman -U --noconfirm "$file" ;;
  appimage)
    dest="$HOME/.local/bin/opendirectory-admin-center"
    run mkdir -p "$HOME/.local/bin"
    run install -m 0755 "$file" "$dest"
    say "Installed $dest. Make sure ~/.local/bin is on your PATH."
    say "AppImages need FUSE 2 (libfuse2) to run." ;;
  dmg)
    mnt="$tmp/mnt"
    run mkdir -p "$mnt"
    run hdiutil attach -nobrowse -quiet -mountpoint "$mnt" "$file"
    app="OpenDirectory Admin Center.app"
    dest=/Applications
    [ -w "$dest" ] || dest_sudo=$sudo
    run ${dest_sudo:-} rm -rf "$dest/$app"
    run ${dest_sudo:-} cp -R "$mnt/$app" "$dest/"
    run hdiutil detach -quiet "$mnt"
    say "Installed $dest/$app."
    say "It isn't notarized yet: the first time, right-click it in Applications and choose Open." ;;
esac

case "$format" in
  appimage | dmg) say "OpenDirectory Admin Center is installed. It updates itself from GitHub Releases." ;;
  *) say "OpenDirectory Admin Center is installed. Launch it from your applications menu." ;;
esac
