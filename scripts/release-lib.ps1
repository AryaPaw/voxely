# Candidate identity and append-only release publication. Never replace existing assets or tags.
Set-StrictMode -Version Latest

function Invoke-VoxelyReleaseApi {
    param(
        [ValidateSet('GET', 'POST', 'PATCH')][string]$Method = 'GET',
        [Parameter(Mandatory)][string]$Path,
        [hashtable]$Body,
        [string]$UploadPath,
        [switch]$AllowNotFound
    )
    if (-not $env:GH_TOKEN) { throw 'GH_TOKEN is required for release operations.' }
    $hostName = if ($UploadPath) { 'uploads.github.com' } else { 'api.github.com' }
    $request = @{
        Uri = "https://$hostName/$Path"; Method = $Method
        Headers = @{ Authorization = "Bearer $env:GH_TOKEN"; Accept = 'application/vnd.github+json'; 'X-GitHub-Api-Version' = '2022-11-28' }
        UserAgent = 'Voxely-Release'; TimeoutSec = 180; MaximumRedirection = 0; SkipHttpErrorCheck = $true
    }
    if ($UploadPath) {
        $request.InFile = $UploadPath
        $request.ContentType = 'application/octet-stream'
    } elseif ($Body) {
        $request.Body = ConvertTo-Json -InputObject $Body -Depth 8 -Compress
        $request.ContentType = 'application/json'
    }
    try { $response = Invoke-WebRequest @request }
    catch { throw "Release API $Method $Path could not be reached. Existing assets and tags were not replaced." }
    $status = [int]$response.StatusCode
    if ($AllowNotFound -and $status -eq 404) { return $null }
    if ($status -lt 200 -or $status -ge 300) { throw "Release API $Method $Path failed (HTTP $status)." }
    return $response.Content | ConvertFrom-Json
}

function Assert-VoxelyReleaseIdentity {
    param(
        [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')][string]$Repo,
        [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
        [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{40}$')][string]$CandidateSha,
        [switch]$AllowMissing
    )
    $reference = Invoke-VoxelyReleaseApi -Path "repos/$Repo/git/ref/tags/v$Version" -AllowNotFound
    if ($null -eq $reference) {
        if ($AllowMissing) { return $false }
        throw "Release tag v$Version is missing."
    }
    $object = $reference.object
    for ($depth = 0; $object.type -eq 'tag' -and $depth -lt 8; $depth++) {
        $object = (Invoke-VoxelyReleaseApi -Path "repos/$Repo/git/tags/$($object.sha)").object
    }
    if ($object.type -ne 'commit' -or $object.sha -ne $CandidateSha) {
        throw "Tag v$Version does not point to candidate $CandidateSha. Use a new version; tags are never moved."
    }
    return $true
}

function Assert-VoxelyReleaseAssets {
    param([Parameter(Mandatory)]$Release, [Parameter(Mandatory)][hashtable]$Files, [switch]$AllowMissing)
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($asset in $Release.assets) {
        if (-not $seen.Add([string]$asset.name) -or @($Files.Keys) -cnotcontains [string]$asset.name) {
            throw "Release contains unexpected or duplicate asset $($asset.name). No assets were replaced."
        }
        $local = $Files[$asset.name]
        if ($asset.state -ne 'uploaded' -or [long]$asset.size -ne $local.Length -or
            -not $asset.PSObject.Properties['digest'] -or [string]$asset.digest -ne "sha256:$($local.Sha256)") {
            throw "Release asset $($asset.name) differs from the verified candidate or has no usable digest. Use a new version; assets are never replaced."
        }
    }
    if (-not $AllowMissing -and $seen.Count -ne $Files.Count) { throw 'Published release is incomplete; refusing to modify it.' }
}

function Get-VoxelyReleaseForTag {
    param([Parameter(Mandatory)][string]$Repo, [Parameter(Mandatory)][string]$Tag)
    # The tag endpoint only returns published releases. The authenticated list also
    # includes drafts, so an interrupted upload can resume without creating a duplicate.
    $matching = [Collections.Generic.List[object]]::new()
    for ($page = 1; $page -le 100; $page++) {
        $releases = @(Invoke-VoxelyReleaseApi -Path "repos/$Repo/releases?per_page=100&page=$page")
        foreach ($release in $releases) {
            if ($release.tag_name -ceq $Tag) { $matching.Add($release) }
        }
        if ($matching.Count -gt 1) { throw "Multiple releases use tag $Tag. Refusing ambiguous draft recovery." }
        if ($releases.Count -lt 100) {
            if ($matching.Count -eq 0) { return $null }
            $release = Invoke-VoxelyReleaseApi -Path "repos/$Repo/releases/$($matching[0].id)"
            if ($release.tag_name -cne $Tag) { throw 'Release tag changed during lookup.' }
            return $release
        }
    }
    throw 'Release listing exceeded 100 pages. Refusing an incomplete identity check.'
}

function Publish-VoxelyRelease {
    param(
        [Parameter(Mandatory)][string]$Repo,
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$CandidateSha,
        [Parameter(Mandatory)][string[]]$ArtifactPaths
    )
    $tagExists = Assert-VoxelyReleaseIdentity -Repo $Repo -Version $Version -CandidateSha $CandidateSha -AllowMissing
    $files = @{}
    foreach ($path in $ArtifactPaths) {
        $file = Get-Item -LiteralPath $path
        if ($file.PSIsContainer -or $file.Length -eq 0 -or $files.ContainsKey($file.Name)) { throw 'Invalid release artifact set.' }
        $files[$file.Name] = @{ Path = $file.FullName; Length = $file.Length; Sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    $installer = "Voxely_${Version}_x64-setup.exe"
    if ($files.Count -ne 3 -or -not $files.ContainsKey($installer) -or -not $files.ContainsKey("$installer.sig") -or -not $files.ContainsKey('latest.json')) { throw 'Release requires exactly the installer, its signature and latest.json.' }
    $release = Get-VoxelyReleaseForTag -Repo $Repo -Tag "v$Version"
    if ($null -ne $release) {
        if (-not $tagExists) { throw 'Existing release has no verifiable tag.' }
        Assert-VoxelyReleaseAssets -Release $release -Files $files -AllowMissing:$release.draft
        if (-not $release.draft) { return 'already-published' }
    } else {
        if (-not $tagExists) {
            $null = Invoke-VoxelyReleaseApi -Method POST -Path "repos/$Repo/git/refs" -Body @{ ref = "refs/tags/v$Version"; sha = $CandidateSha }
            $null = Assert-VoxelyReleaseIdentity -Repo $Repo -Version $Version -CandidateSha $CandidateSha
        }
        $release = Invoke-VoxelyReleaseApi -Method POST -Path "repos/$Repo/releases" -Body @{
            tag_name = "v$Version"; target_commitish = $CandidateSha; name = "v$Version"; draft = $true
            body = "Download the per-user NSIS installer. Administrator rights aren't required.`n`nUpdates are signed with the Tauri updater key. Windows SmartScreen may still warn because Authenticode isn't used yet."
        }
    }
    # A partial draft may only gain missing assets; verified existing bytes remain untouched.
    foreach ($name in $files.Keys) {
        if (@($release.assets | Where-Object name -CEQ $name).Count) { continue }
        $encoded = [Uri]::EscapeDataString($name)
        $null = Invoke-VoxelyReleaseApi -Method POST -Path "repos/$Repo/releases/$($release.id)/assets?name=$encoded" -UploadPath $files[$name].Path
    }
    $release = Invoke-VoxelyReleaseApi -Path "repos/$Repo/releases/$($release.id)"
    Assert-VoxelyReleaseAssets -Release $release -Files $files
    $null = Assert-VoxelyReleaseIdentity -Repo $Repo -Version $Version -CandidateSha $CandidateSha
    $null = Invoke-VoxelyReleaseApi -Method PATCH -Path "repos/$Repo/releases/$($release.id)" -Body @{ draft = $false; make_latest = 'true' }
    return 'published'
}
