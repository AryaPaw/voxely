# Keep shared verification commands in package.json; own only their cache lifecycle.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'rust-build-cache.ps1')
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
Invoke-VoxelyBuildCache -Action {
    $package = Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw | ConvertFrom-Json
    foreach ($command in $package.scripts.'verify:checks'.Split(' && ', [StringSplitOptions]::None)) {
        if ($command -notmatch '^bun run (verify:[\w:-]+)$') { throw "Invalid shared verification command: $command" }
        & bun run $Matches[1]
        if ($LASTEXITCODE -ne 0) { throw "Verification failed: $command (exit $LASTEXITCODE)." }
    }
}
