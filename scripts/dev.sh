#!/usr/bin/env sh
# Same as `cargo x dev`. Run `./scripts/x.sh help` for every task.
exec "$(dirname -- "$0")/x.sh" dev "$@"
