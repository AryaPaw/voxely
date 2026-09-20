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

$covTarget = Join-Path (Get-Location) "target/llvm-cov"
$env:CARGO_TARGET_DIR = $covTarget
$env:CARGO_INCREMENTAL = "0"
try {
    cargo llvm-cov --locked --lib --fail-under-lines $FailUnder --ignore-filename-regex "benches|main.rs"
    if ($LASTEXITCODE -ne 0) { throw "cargo llvm-cov failed: $LASTEXITCODE" }
} finally {
    if (Test-Path $covTarget) {
        Remove-Item -LiteralPath $covTarget -Recurse -Force
    }
}
