param(
  [Parameter(Mandatory = $true)]
  [string]$Version
)

$ErrorActionPreference = "Stop"
$nsis = Get-ChildItem -Recurse "src-tauri/target/release/bundle/nsis" -Filter "*-setup.exe" | Select-Object -First 1
if (-not $nsis) { throw "NSIS installer not found" }
$sig = Get-ChildItem -Recurse "src-tauri/target/release/bundle/nsis" -Filter "*.sig" | Select-Object -First 1
if (-not $sig) { throw "Updater signature (.sig) not found" }
Write-Host "Installer $($nsis.FullName)"
Write-Host "Signature $($sig.FullName)"

# NSIS is a GUI-subsystem exe. `&` returns immediately and leaves $LASTEXITCODE unset.
$proc = Start-Process -FilePath $nsis.FullName -ArgumentList @("/S", "/NS") -Wait -PassThru
if ($null -eq $proc) { throw "Silent install did not start" }
if ($proc.ExitCode -ne 0) { throw "Silent install failed with exit $($proc.ExitCode)" }

$installDir = Join-Path $env:LOCALAPPDATA "Voxely"
$exe = Get-ChildItem $installDir -Recurse -Filter "voxely.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $exe) { throw "voxely.exe missing after install in $installDir" }
Write-Host "Installed $($exe.FullName) version gate $Version"
