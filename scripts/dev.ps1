# Same as `cargo x dev`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" dev @args
exit $LASTEXITCODE
