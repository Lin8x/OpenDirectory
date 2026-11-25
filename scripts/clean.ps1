# Same as `cargo x clean`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" clean @args
exit $LASTEXITCODE
