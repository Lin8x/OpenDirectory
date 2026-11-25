# Same as `cargo x hooks`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" hooks @args
exit $LASTEXITCODE
