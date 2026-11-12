# A minimum-version build owns a temporary target, never the daily-driver cache.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'rust-build-cache.ps1')
$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root 'src-tauri/Cargo.toml'
$match = Select-String -LiteralPath $manifest -Pattern '^rust-version\s*=\s*"(\d+\.\d+\.\d+)"' | Select-Object -First 1
if (-not $match) { throw 'Cargo.toml must declare an exact rust-version.' }
$minimum = $match.Matches[0].Groups[1].Value
Invoke-VoxelyBuildCache -Action {
    param($msrvCache)
    $temporary = Join-Path $msrvCache.Target "msrv-$minimum"
    $previousTarget = $env:CARGO_TARGET_DIR
    $completed = $false
    try {
        $env:CARGO_TARGET_DIR = $temporary
        & cargo "+$minimum" build --locked --all-targets --manifest-path $manifest
        if ($LASTEXITCODE -ne 0) { throw "MSRV $minimum build failed: $LASTEXITCODE" }
        $completed = $true
    } finally {
        $env:CARGO_TARGET_DIR = $previousTarget
        try { Remove-VoxelyCacheDirectory -Cache $msrvCache -Path $temporary -WaitForBuilders:$completed }
        catch {
            if ($completed) { throw }
            Write-Warning "MSRV cache cleanup failed: $($_.Exception.Message)"
        }
    }
}
