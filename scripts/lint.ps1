# Same as `cargo x lint`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" lint @args
exit $LASTEXITCODE
