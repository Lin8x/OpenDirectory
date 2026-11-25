# Same as `cargo x build`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" build @args
exit $LASTEXITCODE
