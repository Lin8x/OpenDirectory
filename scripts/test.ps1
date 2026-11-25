# Same as `cargo x test`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" test @args
exit $LASTEXITCODE
