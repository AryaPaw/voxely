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
$releaseDay = Get-VoxelyReleaseDate -PublishedAt '2026-10-30T12:11:28Z'
if ($releaseDay -cne '2026-10-30') { throw 'Release date must come from GitHub publication' }
if ((Get-VoxelyReleaseDate -PublishedAt '2026-01-01T00:30:00+03:00') -cne '2025-12-31') { throw 'Release date must use UTC across midnight/year boundaries' }
if ((Get-VoxelyReleaseDate -PublishedAt ([datetime]::new(2026,10,30,12,11,28,[DateTimeKind]::Utc))) -cne '2026-10-30') { throw 'JSON DateTime parsing changed release date' }
foreach ($bad in @($null, '', 'not-a-date')) {
    $failed = $false
    try { Get-VoxelyReleaseDate -PublishedAt $bad | Out-Null } catch { $failed = $true }
    if (-not $failed) { throw 'Missing/invalid publication timestamp was accepted' }
}
$metadata = [pscustomobject]@{ ReleaseDate = $releaseDay; HtmlUrl = 'https://github.com/AryaPaw/voxely/releases/tag/v0.4.0' }
$arguments = @(Get-VoxelyWingetUpdateArguments -Version '0.4.0' -OutDir 'C:\fixture with spaces' -Release $metadata)
foreach ($pair in @(@('--release-date','2026-10-30'),@('--release-notes-url',$metadata.HtmlUrl),@('--out','C:\fixture with spaces'),@('--urls',(Get-VoxelyWingetCreateUrlArg -Version '0.4.0')))) {
    $indexes = @(0..($arguments.Count - 1) | Where-Object { $arguments[$_] -ceq $pair[0] })
    if ($indexes.Count -ne 1 -or $arguments[$indexes[0]+1] -cne $pair[1]) { throw "Missing/incorrect generator argument $($pair[0])" }
}
if ('--submit' -in $arguments) { throw 'Generation must never submit through wingetcreate' }
function gh {
    $global:LASTEXITCODE = 0
    return '{"tagName":"v0.4.0","url":"https://github.com/AryaPaw/voxely/releases/tag/v0.4.0","publishedAt":"2020-01-01T00:30:00+03:00","assets":[{"name":"Voxely_0.4.0_x64-setup.exe","url":"https://github.com/AryaPaw/voxely/releases/download/v0.4.0/Voxely_0.4.0_x64-setup.exe"}]}'
}
try {
    $historical = Assert-VoxelyGithubRelease -Version '0.4.0'
    if ($historical.ReleaseDate -cne '2019-12-31') { throw 'Release metadata used the current day instead of API publication time' }
} finally { Remove-Item Function:gh }
Write-Host 'PASS: release publication date, UTC boundaries and generator metadata arguments'
& (Join-Path $PSScriptRoot "winget-github.test.ps1")
& (Join-Path $PSScriptRoot "winget-tool.test.ps1")
