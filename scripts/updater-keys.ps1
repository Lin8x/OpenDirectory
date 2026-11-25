# Same as `cargo x updater-keys`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" updater-keys @args
exit $LASTEXITCODE
