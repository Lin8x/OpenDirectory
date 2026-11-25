# Same as `cargo x doctor`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" doctor @args
exit $LASTEXITCODE
