param(
  [string]$Expected = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$package = Get-Content (Join-Path $root "package.json") -Raw | ConvertFrom-Json
$tauri = Get-Content (Join-Path $root "src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json
$cargoLine = Select-String -Path (Join-Path $root "src-tauri/Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $cargoLine) { throw "Cargo.toml version not found" }
$cargoVersion = $cargoLine.Matches[0].Groups[1].Value
if ($tauri.version -ne "../package.json") {
  throw "tauri.conf.json version must be the path ../package.json (got $($tauri.version))"
}
if ($package.version -ne $cargoVersion) {
  throw "Version mismatch: package.json=$($package.version) Cargo.toml=$cargoVersion"
}
if ($Expected -and $package.version -ne $Expected) {
  throw "Manifest version $($package.version) does not match expected $Expected"
}
Write-Host "Version SSOT $($package.version)"
