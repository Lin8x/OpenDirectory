# Same as `cargo x version`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" version @args
exit $LASTEXITCODE
