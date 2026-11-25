#!/usr/bin/env sh
# Same as `cargo x lint`. Run `./scripts/x.sh help` for every task.
exec "$(dirname -- "$0")/x.sh" lint "$@"
