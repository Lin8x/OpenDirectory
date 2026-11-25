#!/usr/bin/env sh
# Same as `cargo x clean`. Run `./scripts/x.sh help` for every task.
exec "$(dirname -- "$0")/x.sh" clean "$@"
