#!/usr/bin/env bash
# Fill in the store and package-manager manifests for a release.
#
#   packaging/render-manifests.sh <version> <assets-dir> <out-dir>
#
# <assets-dir> holds the release's stable-named Admin Center installers
# (OpenDirectory-AdminCenter-*). Writes, under <out-dir>:
#
#   flatpak/io.github.lin8x.OpenDirectory.yml          Flathub
#   flatpak/io.github.lin8x.OpenDirectory.metainfo.xml
#   aur/opendirectory-admin-center-bin/PKGBUILD         AUR
#   aur/opendirectory-admin-center-bin/.SRCINFO
#   homebrew/Casks/opendirectory-admin-center.rb       Homebrew tap
#   nix/sources.json                                    Nix flake pin
set -euo pipefail

version=${1:?usage: render-manifests.sh <version> <assets-dir> <out-dir>}
version=${version#v}
assets=${2:?assets-dir}
out=${3:?out-dir}
here=$(cd "$(dirname -- "$0")" && pwd)

# A missing asset leaves its checksum empty, so a job that only has the .deb
# can still render the Flatpak manifest.
sha() { [ -f "$assets/$1" ] && sha256sum "$assets/$1" | cut -d' ' -f1 || true; }
sri() { [ -f "$assets/$1" ] && printf 'sha256-%s' "$(openssl dgst -sha256 -binary "$assets/$1" | base64 -w0)" || true; }

deb=OpenDirectory-AdminCenter-linux-amd64.deb
dmg=OpenDirectory-AdminCenter-macos-universal.dmg
appimage=OpenDirectory-AdminCenter-linux-x86_64.AppImage
deb_sha=$(sha "$deb")
dmg_sha=$(sha "$dmg")
date=$(date -u +%Y-%m-%d)

fill() { # template output
  mkdir -p "$(dirname -- "$2")"
  sed -e "s/@VERSION@/$version/g" -e "s/@DATE@/$date/g" \
    -e "s/@DEB_SHA256@/$deb_sha/g" -e "s/@DMG_SHA256@/$dmg_sha/g" "$1" >"$2"
}

fill "$here/flatpak/io.github.lin8x.OpenDirectory.yml" "$out/flatpak/io.github.lin8x.OpenDirectory.yml"
fill "$here/flatpak/io.github.lin8x.OpenDirectory.metainfo.xml" "$out/flatpak/io.github.lin8x.OpenDirectory.metainfo.xml"
fill "$here/aur/opendirectory-admin-center-bin/PKGBUILD.in" "$out/aur/opendirectory-admin-center-bin/PKGBUILD"
fill "$here/aur/opendirectory-admin-center-bin/.SRCINFO.in" "$out/aur/opendirectory-admin-center-bin/.SRCINFO"
fill "$here/homebrew/Casks/opendirectory-admin-center.rb.in" "$out/homebrew/Casks/opendirectory-admin-center.rb"

mkdir -p "$out/nix"
cat >"$out/nix/sources.json" <<JSON
{
  "version": "$version",
  "url": "https://github.com/Lin8x/OpenDirectory/releases/download/v$version/$appimage",
  "hash": "$(sri "$appimage")"
}
JSON

find "$out" -type f | sort
