# Same as `cargo x setup`. Run `.\scripts\x.ps1 help` for every task.
& "$PSScriptRoot/x.ps1" setup @args
exit $LASTEXITCODE
