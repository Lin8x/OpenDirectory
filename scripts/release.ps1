# Same as `cargo x release`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" release @args
exit $LASTEXITCODE
