# Same as `cargo x commit`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" commit @args
exit $LASTEXITCODE
