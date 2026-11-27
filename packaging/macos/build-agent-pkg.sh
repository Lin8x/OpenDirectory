#!/usr/bin/env bash
# Build the Endpoint Agent's macOS installer package (runs on macOS).
#
#   packaging/macos/build-agent-pkg.sh <version> <arm64-binary> <x64-binary> <out.pkg>
#
# Merges both architectures into one universal binary with lipo and installs it
# to /usr/local/bin/opendirectory-agent, the path the agent's launchd service
# uses. Deploy it with any MDM; see scripts/postinstall for unattended
# enrollment. Uninstall with: sudo opendirectory-agent uninstall
#
# The package isn't signed yet. Jamf, Munki and `installer -pkg` accept it;
# Intune requires a Developer ID Installer signature (productsign).
set -euo pipefail
version=${1:?usage: build-agent-pkg.sh <version> <arm64> <x64> <out.pkg>}
version=${version#v}
arm=${2:?arm64 binary}
x64=${3:?x64 binary}
out=${4:?out.pkg}
here=$(cd "$(dirname -- "$0")" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

mkdir -p "$root/usr/local/bin" "$(dirname -- "$out")"
lipo -create -output "$root/usr/local/bin/opendirectory-agent" "$arm" "$x64"
chmod 0755 "$root/usr/local/bin/opendirectory-agent"

pkgbuild --root "$root" \
  --identifier dev.opendirectory.agent \
  --version "$version" \
  --install-location / \
  --scripts "$here/scripts" \
  "$out"
ls -l "$out"
