# Same lease and safety policy as managed builds. Runtime and unknown files are preserved.
param([switch]$Full)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'rust-build-cache.ps1')
Invoke-VoxelyBuildCache -Full:$Full -Action { Write-Host 'Cargo cache maintenance completed; runtime files are protected.' }
