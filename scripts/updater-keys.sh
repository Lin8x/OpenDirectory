#!/usr/bin/env sh
# Same as `cargo x updater-keys`. Run `./scripts/x.sh help` for every task.
exec "$(dirname -- "$0")/x.sh" updater-keys "$@"
