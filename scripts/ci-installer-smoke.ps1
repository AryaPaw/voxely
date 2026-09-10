param(
  [Parameter(Mandatory = $true)]
  [string]$Version
)

$ErrorActionPreference = "Stop"
$nsis = Get-ChildItem -Recurse "src-tauri/target/release/bundle/nsis" -Filter "*.exe" | Select-Object -First 1
if (-not $nsis) { throw "NSIS installer not found" }
$sig = Get-ChildItem -Recurse "src-tauri/target/release/bundle/nsis" -Filter "*.sig" | Select-Object -First 1
if (-not $sig) { throw "Updater signature (.sig) not found" }
$installDir = Join-Path $env:RUNNER_TEMP "voxely-smoke"
New-Item -ItemType Directory -Force $installDir | Out-Null
Write-Host "Installer $($nsis.FullName)"
Write-Host "Signature $($sig.FullName)"
& $nsis.FullName "/S" "/D=$installDir"
if ($LASTEXITCODE -ne 0) { throw "Silent install failed" }
$exe = Get-ChildItem $installDir -Recurse -Filter "voxely.exe" | Select-Object -First 1
if (-not $exe) { throw "voxely.exe missing after install" }
Write-Host "Installed $($exe.FullName) version gate $Version"
