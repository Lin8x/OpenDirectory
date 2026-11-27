# Build the Endpoint Agent's Windows Installer package.
#
#   pwsh packaging/windows/build-agent-msi.ps1 -Version 0.13.0 -AgentExe out\opendirectory-agent-windows-x64.exe -Out out\OpenDirectory-Agent-windows-x64.msi
#
# Needs the WiX Toolset v5 .NET tool:  dotnet tool install --global wix --version 5.0.2
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $AgentExe,
  [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
$Version = $Version.TrimStart('v')
$wxs = Join-Path $PSScriptRoot 'agent.wxs'
$exe = (Resolve-Path $AgentExe).Path
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Out) | Out-Null

wix build $wxs -arch x64 -d "Version=$Version" -d "AgentExe=$exe" -o $Out
if ($LASTEXITCODE -ne 0) { throw "wix build failed ($LASTEXITCODE)" }
Get-Item $Out | Format-Table Name, Length
