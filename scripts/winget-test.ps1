# Local WinGet validation / optional install. Does not submit PRs.

param(
    [Parameter(Mandatory = $true)]
    [string]$ManifestDir,
    [switch]$Install,
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "winget-lib.ps1")

if (-not (Test-Path $ManifestDir)) { throw "Manifest directory missing: $ManifestDir" }

Write-Host "winget validate --manifest $ManifestDir"
winget validate --manifest $ManifestDir
if ($LASTEXITCODE -ne 0) { throw "winget validate failed" }

$id = Get-VoxelyWingetPackageId
Write-Host "Next (admin once): winget settings --enable LocalManifestFiles"
Write-Host "Then: winget install --manifest `"$ManifestDir`" --silent --accept-package-agreements"
Write-Host "Then: winget list --id $id"
Write-Host "Then: winget uninstall --id $id --silent"

if ($Install) {
    winget settings --enable LocalManifestFiles
    winget install --manifest $ManifestDir --silent --accept-package-agreements --disable-interactivity
    if ($LASTEXITCODE -ne 0) { throw "winget install failed" }
    winget list --id $id
}

if ($Uninstall) {
    winget uninstall --id $id --silent --disable-interactivity
    if ($LASTEXITCODE -ne 0) { throw "winget uninstall failed" }
}
