# Same as `cargo x help`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" help @args
exit $LASTEXITCODE
