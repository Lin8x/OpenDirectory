# Runs an OpenDirectory developer task, so you never have to type `cargo`.
#
#   .\scripts\x.ps1 <task> [options]     e.g. .\scripts\x.ps1 dev --browser
#
# Every other script in this folder (dev.ps1, test.ps1, release.ps1, ...) is a
# one-line shortcut for this one. The tasks themselves live in xtask\src\main.rs.
$ErrorActionPreference = 'Stop'

Set-Location (Split-Path -Parent $PSScriptRoot)

# rustup installs into ~\.cargo\bin. Pick it up even if this window was opened
# before Rust was installed and PATH hasn't been reloaded yet.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $HOME '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
        $env:PATH = "$cargoBin;$env:PATH"
    }
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Rust isn't installed yet. OpenDirectory's tools are written in Rust, so it's needed once." -ForegroundColor Yellow
    Write-Host "Install it (takes a few minutes):"
    Write-Host ""
    if ($env:OS -eq 'Windows_NT') {
        Write-Host "  winget install Rustlang.Rustup"
        Write-Host ""
        Write-Host "or download rustup-init.exe from https://rustup.rs, then open a new terminal and run .\scripts\setup.ps1"
    } else {
        Write-Host "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        Write-Host ""
        Write-Host "then open a new terminal and run ./scripts/setup.sh. More at https://rustup.rs"
    }
    exit 1
}

& cargo x @args
exit $LASTEXITCODE
