param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug",
    [string]$Scenario = "fresh-tray-or-running",
    [string]$OutDir = ""
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $OutDir) {
    $OutDir = Join-Path $repo "docs/perf/trials"
}

$exe = if ($Profile -eq "debug") {
    Join-Path $repo "src-tauri/target/debug/voxely.exe"
} else {
    Join-Path $repo "src-tauri/target/release/voxely.exe"
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $OutDir "$Profile-$Scenario-$stamp.json"

& (Join-Path $PSScriptRoot "Get-VoxelyProcessTree.ps1") -Profile $Profile -Scenario $Scenario -OutFile $out | Out-Null

Write-Host "Wrote $out"
if (-not (Get-Process -Name voxely -ErrorAction SilentlyContinue)) {
    Write-Host "voxely.exe was not running. Snapshot is empty. Start $exe with VOXELY_DATA_DIR=%APPDATA%\VoxelyPerf (debug only) without touching %APPDATA%\Voxely."
}

Get-Content $out
