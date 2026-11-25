# Same as `cargo x fmt`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" fmt @args
exit $LASTEXITCODE
