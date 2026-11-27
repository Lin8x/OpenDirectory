# Install the OpenDirectory Admin Center on Windows.
#
#   irm https://github.com/Lin8x/OpenDirectory/releases/latest/download/install.ps1 | iex
#
# With options:
#   & ([scriptblock]::Create((irm https://github.com/Lin8x/OpenDirectory/releases/latest/download/install.ps1))) -Format exe -Version v0.13.0
#
#   -Format msi|exe   Windows Installer package (default) or the setup .exe
#   -Version vX.Y.Z   a specific release (default: latest)
#   -DryRun           download and verify, but don't run the installer
#
# The download is checked against the release's SHA256SUMS. The installer runs
# with a progress bar and no prompts; Windows asks for administrator approval
# when the installer needs it (run this as your usual user, not as admin).
param(
  [ValidateSet('msi', 'exe')] [string] $Format = 'msi',
  [string] $Version = 'latest',
  [switch] $DryRun
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue' # Invoke-WebRequest is much faster without it

$repo = 'Lin8x/OpenDirectory'
$base = $env:OD_RELEASE_BASE
if (-not $base) {
  if ($Version -eq 'latest') { $base = "https://github.com/$repo/releases/latest/download" }
  else { $base = "https://github.com/$repo/releases/download/v$($Version.TrimStart('v'))" }
}

if (-not [Environment]::Is64BitOperatingSystem) { throw 'The Admin Center needs 64-bit Windows.' }
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$asset = if ($Format -eq 'msi') { 'OpenDirectory-AdminCenter-windows-x64.msi' } else { 'OpenDirectory-AdminCenter-windows-x64-setup.exe' }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("opendirectory-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
$file = Join-Path $tmp $asset

try {
  Write-Host "Downloading $asset from $base"
  Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $file

  $sums = $null
  try { $sums = (Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS").Content } catch { }
  if ($sums) {
    if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
    $line = ($sums -split "`n") | Where-Object { ($_ -split '\s+')[1] -in @($asset, "*$asset") } | Select-Object -First 1
    if (-not $line) { throw "$asset isn't listed in SHA256SUMS" }
    $want = ($line -split '\s+')[0].ToLowerInvariant()
    $got = (Get-FileHash -Algorithm SHA256 -Path $file).Hash.ToLowerInvariant()
    if ($want -ne $got) { throw "Checksum mismatch for $asset (expected $want, got $got)" }
    Write-Host 'Checksum verified.'
  } else {
    Write-Host 'No SHA256SUMS for this release; skipping checksum verification.'
  }

  if ($Format -eq 'msi') {
    $exe = 'msiexec.exe'
    $argList = @('/i', "`"$file`"", '/passive', '/norestart')
  } else {
    $exe = $file
    $argList = @('/P') # passive mode: progress bar, no prompts
  }
  if ($DryRun) {
    Write-Host "+ $exe $($argList -join ' ')"
    return
  }
  $p = Start-Process -FilePath $exe -ArgumentList $argList -Wait -PassThru
  if ($p.ExitCode -notin 0, 3010) { throw "The installer exited with code $($p.ExitCode)." }
  Write-Host 'OpenDirectory Admin Center is installed. Open it from the Start menu; it updates itself from GitHub Releases.'
} finally {
  Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tmp
}
