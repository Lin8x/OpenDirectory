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

# rustup installs into ~/.cargo/bin. Pick it up even if this shell was opened
# before Rust was installed and PATH hasn't been reloaded yet.
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
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
