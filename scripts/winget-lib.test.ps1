$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "winget-lib.ps1")

$root = Get-VoxelyRepoRoot
$package = Get-Content (Join-Path $root "package.json") -Raw | ConvertFrom-Json
$version = Get-VoxelyPackageVersion -Root $root
if ($version -ne $package.version) { throw "SSOT mismatch $version vs $($package.version)" }

$id = Get-VoxelyWingetPackageId
if ($id -ne "AryaPaw.Voxely") { throw "unexpected package id $id" }

$name = Get-VoxelyNsisInstallerName -Version "0.2.12"
if ($name -ne "Voxely_0.2.12_x64-setup.exe") { throw "unexpected installer name $name" }

$url = Get-VoxelyVersionedInstallerUrl -Version "0.2.12"
if ($url -notmatch '/releases/download/v0.2.12/Voxely_0.2.12_x64-setup.exe$') {
    throw "URL must be version-specific, got $url"
}
if ($url -match '/latest/') { throw "must not use latest download URL" }

$arg = Get-VoxelyWingetCreateUrlArg -Version "0.2.12"
if ($arg -ne "$url|x64|user") { throw "override must be |x64|user, got $arg" }

Write-Host "winget-lib tests passed (version $version)"
