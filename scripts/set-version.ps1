param(
  [Parameter(Mandatory = $true, Position = 0)]
  [string]$To
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$packagePath = Join-Path $root "package.json"
$cargoPath = Join-Path $root "src-tauri/Cargo.toml"
$lockPath = Join-Path $root "src-tauri/Cargo.lock"
$package = Get-Content $packagePath -Raw | ConvertFrom-Json

function Get-BumpedVersion([string]$current, [string]$kind) {
  if ($current -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
    throw "package.json version must be X.Y.Z, got $current"
  }
  $major = [int]$Matches[1]
  $minor = [int]$Matches[2]
  $patch = [int]$Matches[3]
  switch ($kind) {
    "patch" { return "$major.$minor.$($patch + 1)" }
    "minor" { return "$major.$($minor + 1).0" }
    "major" { return "$($major + 1).0.0" }
    default { throw "unknown bump $kind" }
  }
}

if ($To -match '^(patch|minor|major)$') {
  $version = Get-BumpedVersion $package.version $To
} elseif ($To -match '^\d+\.\d+\.\d+$') {
  $version = $To
} else {
  throw "Usage: set-version.ps1 -To <X.Y.Z|patch|minor|major>"
}

$utf8 = New-Object System.Text.UTF8Encoding $false
$packageJson = Get-Content $packagePath -Raw
$packageJson = [regex]::Replace(
  $packageJson,
  '"version"\s*:\s*"[^"]+"',
  ('"version": "' + $version + '"')
)
[System.IO.File]::WriteAllText($packagePath, $packageJson, $utf8)

$cargo = Get-Content $cargoPath -Raw
$cargo = [regex]::Replace($cargo, '(?m)^version\s*=\s*"[^"]+"', ('version = "' + $version + '"'))
[System.IO.File]::WriteAllText($cargoPath, $cargo, $utf8)

$lock = Get-Content $lockPath -Raw
$lockRe = New-Object System.Text.RegularExpressions.Regex '(name = "voxely"\r?\nversion = ")[^"]+'
$lock = $lockRe.Replace($lock, ('${1}' + $version), 1)
[System.IO.File]::WriteAllText($lockPath, $lock, $utf8)

& (Join-Path $PSScriptRoot "check-version.ps1") -Expected $version
Write-Host "Set version $version (package.json + Cargo.toml). Date is not stored in source."
