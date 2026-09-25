# Rebuild and start the local daily-driver exe. Not a GitHub Release, tag, or installer.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "src-tauri\target\release\voxely.exe"

Get-Process voxely -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

$env:VOXELY_LOCAL_BUILD = "1"
Set-Location $root
bunx tauri build --no-bundle
if (-not (Test-Path $exe)) {
    throw "local app exe missing: $exe"
}

Start-Process $exe
Write-Host "Started local app: $exe"
