#!/usr/bin/env sh
# Runs an OpenDirectory developer task, so you never have to type `cargo`.
#
#   ./scripts/x.sh <task> [options]     e.g. ./scripts/x.sh dev --browser
#
# Every other script in this folder (dev.sh, test.sh, release.sh, ...) is a
# one-line shortcut for this one. The tasks themselves live in xtask/src/main.rs.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# None of these tasks need root. Under sudo, cargo would leave root-owned files
# in target/ and ~/.cargo that break later runs, and sudo also hides the
# rustup install in your home folder. So run the task as you instead.
if [ "$(id -u)" = 0 ] && [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ]; then
  echo "Note: sudo isn't needed for these scripts. Running as $SUDO_USER instead." >&2
  exec sudo -u "$SUDO_USER" -- "$0" "$@"
fi

# rustup installs into ~/.cargo/bin. Pick it up even if this shell was opened
# before Rust was installed and PATH hasn't been reloaded yet, or if HOME
# points somewhere else (as it can under sudo).
if ! command -v cargo >/dev/null 2>&1; then
  my_home=$(getent passwd "$(id -un)" 2>/dev/null | cut -d: -f6 || true)
  if [ -x "${CARGO_HOME:-$HOME/.cargo}/bin/cargo" ]; then
    PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
  elif [ -n "$my_home" ] && [ -x "$my_home/.cargo/bin/cargo" ]; then
    HOME=$my_home
    PATH="$my_home/.cargo/bin:$PATH"
    export HOME
  fi
  export PATH
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust isn't installed yet. OpenDirectory's tools are written in Rust, so it's needed once." >&2
  echo "Install it (takes a couple of minutes, no admin rights needed):" >&2
  echo >&2
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
  echo >&2
  echo "then open a new terminal and run ./scripts/setup.sh. More at https://rustup.rs" >&2
  exit 1
fi

exec cargo x "$@"
