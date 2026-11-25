# Same as `cargo x updater-secrets`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" updater-secrets @args
exit $LASTEXITCODE
