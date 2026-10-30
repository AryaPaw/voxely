# Offline download/cache fixtures. The production hash pins the exact official executable.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'winget-lib.ps1')
$release = Get-VoxelyWingetCreateRelease
if ($release.Url -notmatch '^https://github.com/microsoft/winget-create/releases/download/v[\d.]+/wingetcreate.exe$' -or $release.Sha256 -notmatch '^[a-f0-9]{64}$') { throw 'Official tool pin is invalid' }
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) "voxely-tool-test-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$script:toolPath = Join-Path $fixtureRoot 'wingetcreate.exe'
$valid = Join-Path $fixtureRoot 'valid.fixture'
[IO.File]::WriteAllText($valid, 'known official fixture content')
$script:fixtureHash = (Get-FileHash -LiteralPath $valid).Hash
$script:downloads = 0
$script:downloadMode = 'valid'
function Get-VoxelyWingetCreatePath { return $script:toolPath }
function Get-VoxelyWingetCreateRelease { return @{ Version = 'fixture'; Url = 'https://example.invalid/tool'; Sha256 = $script:fixtureHash } }
function curl.exe {
    $script:downloads++
    $downloadPath = $args[[Array]::IndexOf($args, '--output') + 1]
    if ('--max-time' -notin $args -or '--connect-timeout' -notin $args) { throw 'Download time bounds missing' }
    switch ($script:downloadMode) {
        'failed' { [IO.File]::WriteAllBytes($downloadPath, [byte[]]::new(10000)); $global:LASTEXITCODE = 56 }
        'corrupt' { [IO.File]::WriteAllBytes($downloadPath, [byte[]]::new(10000)); $global:LASTEXITCODE = 0 }
        'valid' { [IO.File]::WriteAllText($downloadPath, 'known official fixture content'); $global:LASTEXITCODE = 0 }
    }
}
try {
    [IO.File]::WriteAllBytes($script:toolPath, [byte[]]::new(20000))
    foreach ($mode in 'failed', 'corrupt') {
        $script:downloadMode = $mode
        $failed = $false
        try { Install-VoxelyWingetCreate | Out-Null } catch { $failed = $true }
        if (-not $failed) { throw "Accepted $mode download" }
        if ((Get-Item -LiteralPath $script:toolPath).Length -ne 20000) { throw 'Failed download replaced existing cache' }
        if (@(Get-ChildItem -LiteralPath $fixtureRoot -Filter '*.tmp').Count) { throw 'Temporary download leaked' }
    }
    $script:downloadMode = 'valid'
    $result = Install-VoxelyWingetCreate
    if ($result -ne $script:toolPath -or -not (Test-VoxelyToolHash -Path $result -Sha256 $script:fixtureHash)) { throw 'Retry did not repair corrupt cache' }
    $previousDownloads = $script:downloads
    Install-VoxelyWingetCreate | Out-Null
    if ($script:downloads -ne $previousDownloads) { throw 'Verified cache was downloaded again' }
    Write-Host 'PASS: pinned tool cache, partial download, curl failure, hash mismatch, recovery, reuse and cleanup'
} finally {
    $resolved = (Resolve-Path -LiteralPath $fixtureRoot).Path
    if ((Split-Path -Leaf $resolved) -notlike 'voxely-tool-test-*' -or (Split-Path -Parent $resolved) -ne [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')) { throw 'Unexpected fixture root' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
