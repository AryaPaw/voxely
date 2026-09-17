# Rust lib coverage floor for CI.
# Locally missing cargo-llvm-cov is not a failure.

param(
    [int]$FailUnder = 40
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
Set-Location "src-tauri"

$llvmCov = Get-Command cargo-llvm-cov -ErrorAction SilentlyContinue
if (-not $llvmCov) {
    if ($env:CI -eq "true") {
        throw "cargo-llvm-cov is required in CI"
    }
    Write-Host "cargo-llvm-cov not installed; skip local rust coverage gate"
    exit 0
}

cargo llvm-cov --locked --lib --fail-under-lines $FailUnder --ignore-filename-regex "benches|main.rs"
