# Same as `cargo x check`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" check @args
exit $LASTEXITCODE
