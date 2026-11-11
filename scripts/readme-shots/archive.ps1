param([Parameter(Mandatory)][string]$Work)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$ownedRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../.local/readme-shots'))
$Work = (Resolve-Path -LiteralPath $Work).Path

function Assert-OwnedRun([string]$Path) {
    $resolved = [IO.Path]::GetFullPath($Path)
    if ([IO.Path]::GetDirectoryName($resolved) -ne $ownedRoot -or [IO.Path]::GetFileName($resolved) -notmatch '^run-[0-9a-f-]{36}$') { throw 'Not an owned screenshot run.' }
    $marker = Get-Content -LiteralPath (Join-Path $resolved 'session.json') -Raw | ConvertFrom-Json
    if ($marker.format -ne 'voxely-native-readme' -or $marker.version -ne 1) { throw 'Unknown screenshot workspace.' }
    $cursor = $resolved
    while ($cursor) {
        if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse point in capture path: $cursor" }
        $parent = [IO.Directory]::GetParent($cursor)
        $cursor = if ($null -ne $parent) { $parent.FullName } else { $null }
    }
    foreach ($entry in @((Get-Item -LiteralPath $resolved -Force)) + @(Get-ChildItem -LiteralPath $resolved -Force -Recurse)) {
        if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing reparse point: $($entry.FullName)" }
    }
    return $resolved
}

$Work = Assert-OwnedRun $Work
$archive = Join-Path $Work 'artifacts.zip'
$files = @(Get-ChildItem -LiteralPath $Work -File | Where-Object { $_.Extension -in '.png','.json','.jsonl','.log' })
if (($files | Measure-Object Length -Sum).Sum -gt 50MB) { throw 'Screenshot diagnostics exceeded the 50 MiB bound.' }
Compress-Archive -LiteralPath $files.FullName -DestinationPath $archive -Force
$zip = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    foreach ($file in $files) {
        $entry = $zip.GetEntry($file.Name)
        if ($null -eq $entry) { throw "Missing archived artifact: $($file.Name)" }
        $stream = $entry.Open()
        try { $archivedHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)) }
        finally { $stream.Dispose() }
        if ($archivedHash -ne (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash) { throw "Archive verification failed: $($file.Name)" }
    }
} finally { $zip.Dispose() }
$verified = Join-Path $Work 'archive.verified'
[IO.File]::WriteAllText($verified, (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash)

# Delete only generated profiles/assets after the result archive was verified.
foreach ($name in @('assets','profile','webview')) {
    $target = [IO.Path]::GetFullPath((Join-Path $Work $name))
    if ([IO.Path]::GetDirectoryName($target) -ne $Work) { throw 'Generated directory escaped its run.' }
    if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Recurse -Force }
}

# Keep three archived tool-owned runs. Unmarked/unfinished foreign directories
# aren't cleanup candidates; originals in docs/images never enter this cleanup.
$runs = @(Get-ChildItem -LiteralPath $ownedRoot -Directory -Filter 'run-*' | Where-Object {
    (Test-Path -LiteralPath (Join-Path $_.FullName 'session.json')) -and (Test-Path -LiteralPath (Join-Path $_.FullName 'artifacts.zip')) -and (Test-Path -LiteralPath (Join-Path $_.FullName 'archive.verified'))
} | Sort-Object CreationTimeUtc -Descending)
foreach ($old in @($runs | Select-Object -Skip 3)) {
    $target = Assert-OwnedRun $old.FullName
    if ([IO.File]::ReadAllText((Join-Path $target 'archive.verified')) -ne (Get-FileHash -LiteralPath (Join-Path $target 'artifacts.zip') -Algorithm SHA256).Hash) { throw 'Archived run verification marker no longer matches.' }
    Remove-Item -LiteralPath $target -Recurse -Force
}
Write-Host "Archived capture results: $archive"
