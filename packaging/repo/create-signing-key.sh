#!/usr/bin/env bash
# One-time setup: create the key that signs the APT and DNF repositories and
# hand it to GitHub Actions.
#
#   packaging/repo/create-signing-key.sh [backup-dir]
#
# Creates an Ed25519 OpenPGP key with no passphrase (CI signs unattended),
# stores it as the REPO_SIGNING_KEY Actions secret with `gh secret set`, and
# writes a backup of the private key to [backup-dir] (default: the current
# directory). Keep that backup somewhere safe and out of the repository: if the
# key is lost, every installed machine has to trust a new one.
set -euo pipefail

backup_dir=${1:-.}
repo=${GH_REPO:-Lin8x/OpenDirectory}
command -v gh >/dev/null || { echo "Install the GitHub CLI (gh) and sign in first." >&2; exit 1; }

GNUPGHOME=$(mktemp -d)
export GNUPGHOME
trap 'rm -rf "$GNUPGHOME"' EXIT

gpg --batch --passphrase '' --quick-generate-key \
  "OpenDirectory Packages <opendirectory@users.noreply.github.com>" ed25519 sign never
fpr=$(gpg --batch --list-secret-keys --with-colons | awk -F: '/^fpr:/ {print $10; exit}')

mkdir -p "$backup_dir"
private="$backup_dir/opendirectory-repo-signing-key.private.asc"
(umask 077 && gpg --batch --armor --export-secret-keys "$fpr" >"$private")
gpg --batch --armor --export "$fpr" >"$backup_dir/opendirectory-repo-signing-key.public.asc"

gh secret set REPO_SIGNING_KEY --repo "$repo" <"$private"

echo
echo "Created signing key $fpr and saved it as the REPO_SIGNING_KEY secret on $repo."
echo "Private key backup: $private (keep it safe, never commit it)."
