# Drop Cargo caches that are not the daily-driver sources.
# Default: incremental, MSRV, and llvm-cov dirs. -Full wipes src-tauri/target.

param(
    [switch]$Full
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "src-tauri/target"

if (-not (Test-Path $target)) {
    Write-Host "No src-tauri/target"
    exit 0
}

if ($Full) {
    cargo clean --manifest-path (Join-Path $root "src-tauri/Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "cargo clean failed: $LASTEXITCODE" }
    Write-Host "Removed src-tauri/target"
    exit 0
}

$removed = @()
foreach ($rel in @("debug/incremental", "llvm-cov", "msrv-1.89")) {
    $path = Join-Path $target $rel
    if (Test-Path $path) {
        Remove-Item -LiteralPath $path -Recurse -Force
        $removed += $rel
    }
}
Get-ChildItem $target -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like "msrv-*" } |
    ForEach-Object {
        Remove-Item -LiteralPath $_.FullName -Recurse -Force
        $removed += $_.Name
    }

if ($removed.Count -eq 0) {
    Write-Host "No incremental/MSRV/llvm-cov caches"
} else {
    Write-Host ("Removed " + ($removed -join ", "))
}
