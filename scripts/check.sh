#!/usr/bin/env sh
# Same as `cargo x check`. Run `./scripts/x.sh help` for every task.
exec "$(dirname -- "$0")/x.sh" check "$@"
