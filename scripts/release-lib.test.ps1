# Offline release fixtures. No real tokens, remote writes or builds.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-lib.ps1')
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) "voxely-release-test-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$script:candidate = 'a' * 40
$script:tagSha = $null
$script:releaseFixture = $null
$script:annotatedTag = $false
$script:failUpload = $false
$script:listPrefix = @()
$script:duplicateRelease = $false
$script:tagEndpointDraftMisses = 0
$script:writes = [Collections.Generic.List[object]]::new()
$script:version = '1.2.3'
$script:paths = @('Voxely_1.2.3_x64-setup.exe', 'Voxely_1.2.3_x64-setup.exe.sig', 'latest.json') | ForEach-Object {
    $path = Join-Path $fixtureRoot $_
    [IO.File]::WriteAllText($path, "fixture content: $_")
    $path
}
function Invoke-VoxelyReleaseApi {
    param($Method = 'GET', $Path, $Body, $UploadPath, [switch]$AllowNotFound)
    if ($Method -ne 'GET') { $script:writes.Add(@{ Method = $Method; Path = $Path; Body = $Body }) }
    if ($Path -eq 'repos/fixture/repo/git/ref/tags/v1.2.3') {
        if ($null -eq $script:tagSha) { return $null }
        if ($script:annotatedTag) { return @{ object = @{ type = 'tag'; sha = ('c' * 40) } } }
        return @{ object = @{ type = 'commit'; sha = $script:tagSha } }
    }
    if ($Path -eq "repos/fixture/repo/git/tags/$('c' * 40)") { return @{ object = @{ type = 'commit'; sha = $script:tagSha } } }
    if ($Path -eq 'repos/fixture/repo/git/refs' -and $Method -eq 'POST') {
        if ($Body.sha -ne $script:candidate -or $Body.ref -ne 'refs/tags/v1.2.3') { throw 'Wrong new tag identity' }
        $script:tagSha = $Body.sha
        return @{}
    }
    if ($Path -eq 'repos/fixture/repo/releases' -and $Method -eq 'POST') {
        if ($Body.target_commitish -ne $script:candidate -or -not $Body.draft) { throw 'New release must be a draft tied to candidate SHA' }
        if ($null -ne $script:releaseFixture) { throw 'HTTP 422: release already exists' }
        $script:releaseFixture = [pscustomobject]@{ id = 42; tag_name = 'v1.2.3'; draft = $true; assets = @() }
        return $script:releaseFixture
    }
    if ($Path -match '^repos/fixture/repo/releases/42/assets\?name=(.+)$' -and $Method -eq 'POST') {
        if ($script:failUpload -and $script:releaseFixture.assets.Count -eq 1) { throw 'Injected upload interruption' }
        $name = [Uri]::UnescapeDataString($Matches[1])
        if (@($script:releaseFixture.assets | Where-Object name -EQ $name).Count) { throw 'Attempted asset replacement' }
        $file = Get-Item -LiteralPath $UploadPath
        $script:releaseFixture.assets += [pscustomobject]@{ name = $name; size = $file.Length; state = 'uploaded'; digest = "sha256:$((Get-FileHash -LiteralPath $file.FullName).Hash.ToLowerInvariant())" }
        return @{}
    }
    if ($Path -eq 'repos/fixture/repo/releases/42' -and $Method -eq 'PATCH') {
        if ($Body.draft -or $Body.make_latest -ne 'true' -or $script:releaseFixture.assets.Count -ne 3) { throw 'Premature release publication' }
        $script:releaseFixture.draft = $false
        return $script:releaseFixture
    }
    if ($Method -eq 'GET' -and $Path -eq 'repos/fixture/repo/releases/tags/v1.2.3') {
        if ($null -ne $script:releaseFixture -and $script:releaseFixture.draft) {
            $script:tagEndpointDraftMisses++
            return $null # GitHub tag lookup is published-only, despite authorized draft access.
        }
        return $script:releaseFixture
    }
    if ($Method -eq 'GET' -and $Path -match '^repos/fixture/repo/releases\?per_page=100&page=(\d+)$') {
        $page = [int]$Matches[1]
        $all = @($script:listPrefix)
        if ($null -ne $script:releaseFixture) { $all += $script:releaseFixture }
        if ($script:duplicateRelease) { $all += [pscustomobject]@{ id = 43; tag_name = 'v1.2.3'; draft = $true; assets = @() } }
        return @($all | Select-Object -Skip (($page - 1) * 100) -First 100)
    }
    if ($Method -eq 'GET' -and $Path -eq 'repos/fixture/repo/releases/42') { return $script:releaseFixture }
    throw "Unexpected fixture request $Method $Path"
}
function Publish-Fixture { Publish-VoxelyRelease -Repo 'fixture/repo' -Version $script:version -CandidateSha $script:candidate -ArtifactPaths $script:paths }
function Expect-Failure([scriptblock]$Action, [string]$Pattern) {
    $before = $script:writes.Count
    $message = ''
    try { & $Action | Out-Null } catch { $message = $_.Exception.Message }
    if ($message -notmatch $Pattern) { throw "Expected $Pattern, got $message" }
    if ($script:writes.Count -ne $before) { throw 'Invalid existing release caused writes' }
}
try {
    $script:failUpload = $true
    $failure = ''
    try { Publish-Fixture | Out-Null } catch { $failure = $_.Exception.Message }
    if ($failure -notmatch 'upload interruption' -or -not $script:releaseFixture.draft -or $script:releaseFixture.assets.Count -ne 1) { throw 'Interrupted upload did not preserve a recoverable draft' }
    if ($null -ne (Invoke-VoxelyReleaseApi -Path 'repos/fixture/repo/releases/tags/v1.2.3' -AllowNotFound) -or $script:tagEndpointDraftMisses -ne 1) { throw 'Fixture must emulate GitHub returning 404 for draft tag lookup' }
    $script:listPrefix = @(1..100 | ForEach-Object { [pscustomobject]@{ id = 1000 + $_; tag_name = "v0.0.$_"; draft = $false; assets = @() } })
    $script:failUpload = $false
    if ((Publish-Fixture) -ne 'published' -or $script:releaseFixture.draft) { throw 'New release failed' }
    $before = $script:writes.Count
    if ((Publish-Fixture) -ne 'already-published' -or $script:writes.Count -ne $before) { throw 'Identical publication must be read-only and idempotent' }
    $script:duplicateRelease = $true
    Expect-Failure { Publish-Fixture } 'Multiple releases'
    $script:duplicateRelease = $false
    $script:annotatedTag = $true
    if ((Publish-Fixture) -ne 'already-published') { throw 'Annotated tag was not resolved to the candidate' }
    $script:annotatedTag = $false
    $savedDigest = $script:releaseFixture.assets[0].digest
    $script:releaseFixture.assets[0].digest = "sha256:$('0' * 64)"
    Expect-Failure { Publish-Fixture } 'differs'
    $script:releaseFixture.assets[0].digest = $savedDigest
    $saved = $script:releaseFixture.assets
    $script:releaseFixture.assets = @($saved[0])
    Expect-Failure { Publish-Fixture } 'incomplete'
    $script:releaseFixture.draft = $true
    if ((Publish-Fixture) -ne 'published' -or $script:releaseFixture.assets.Count -ne 3) { throw 'Partial matching draft did not recover' }
    $script:tagSha = 'b' * 40
    Expect-Failure { Publish-Fixture } 'does not point to candidate'
    $script:tagSha = $script:candidate
    $script:releaseFixture.draft = $true
    $script:releaseFixture.assets[0].digest = 'sha256:unverifiable'
    Expect-Failure { Publish-Fixture } 'differs'
    $workflow = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../.github/workflows/release.yml') -Raw
    if ($workflow -notmatch 'group: release-\$\{\{ github.repository \}\}' -or $workflow -notmatch 'ref: \$\{\{ github.sha \}\}' -or $workflow -match 'softprops/action-gh-release') { throw 'Release concurrency/SHA/append-only guards missing' }
    Write-Host 'PASS: exact candidate/tag, immutable assets, identical rerun, incomplete published rejection and matching draft recovery'
} finally {
    $resolved = (Resolve-Path -LiteralPath $fixtureRoot).Path
    if ((Split-Path -Leaf $resolved) -notlike 'voxely-release-test-*' -or (Split-Path -Parent $resolved) -ne [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')) { throw 'Unexpected fixture root' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
