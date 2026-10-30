# GitHub preflight for wingetcreate. Only the fork's master ref may be updated.
# Keep the submission PAT separate from gh's read-only release-inspection token.

function Invoke-VoxelyWingetApi {
    param(
        [ValidateSet("GET", "PATCH")][string]$Method = "GET",
        [Parameter(Mandatory)][string]$Path,
        [hashtable]$Body,
        [switch]$AllowNotFound
    )
    if (-not $env:WINGET_CREATE_GITHUB_TOKEN) {
        throw "Submit requires env WINGET_CREATE_GITHUB_TOKEN. Never pass the token on the command line."
    }
    $request = @{
        Uri = "https://api.github.com/$Path"
        Method = $Method
        Headers = @{
            Authorization = "Bearer $env:WINGET_CREATE_GITHUB_TOKEN"
            Accept = "application/vnd.github+json"
            "X-GitHub-Api-Version" = "2022-11-28"
        }
        UserAgent = "Voxely-WinGet"
        TimeoutSec = 30
        MaximumRedirection = 0
        SkipHttpErrorCheck = $true
    }
    if ($Body) {
        $request.ContentType = "application/json"
        $request.Body = ConvertTo-Json -InputObject $Body -Compress
    }
    try { $response = Invoke-WebRequest @request }
    catch { throw "GitHub API $Method $Path could not be reached. No submission was attempted by this preflight." }
    $status = [int]$response.StatusCode
    if ($AllowNotFound -and $status -eq 404) { return $null }
    if ($status -lt 200 -or $status -ge 300) {
        # Don't print response bodies or request objects containing credentials.
        $failure = [InvalidOperationException]::new("GitHub API $Method $Path failed (HTTP $status). Check token permissions, expiration and GitHub service status; see docs/WINGET.md.")
        $failure.Data["StatusCode"] = $status
        throw $failure
    }
    return [pscustomobject]@{
        Data = ($response.Content | ConvertFrom-Json)
        Scopes = @(([string]($response.Headers["X-OAuth-Scopes"] -join ",")).Split(",") | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    }
}

function Sync-VoxelyWingetFork {
    param([Parameter(Mandatory)][string]$Owner)
    $fork = "$Owner/winget-pkgs"
    $upstream = "microsoft/winget-pkgs"
    $repository = (Invoke-VoxelyWingetApi -Path "repos/$fork").Data
    if (-not $repository.fork -or $repository.parent.full_name -ne $upstream -or $repository.default_branch -ne "master" -or -not $repository.permissions.push) {
        throw "$fork must be a writable fork of $upstream with default branch master. No branch was changed."
    }
    # Capture immutable SHAs. force=false is the final protection against concurrent writers.
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        $target = (Invoke-VoxelyWingetApi -Path "repos/$upstream/git/ref/heads/master").Data.object.sha
        $current = (Invoke-VoxelyWingetApi -Path "repos/$fork/git/ref/heads/master").Data.object.sha
        if ($target -eq $current) { return }
        $comparison = (Invoke-VoxelyWingetApi -Path "repos/$upstream/compare/$target...$current").Data
        if ($comparison.ahead_by -gt 0) {
            throw "$fork/master has $($comparison.ahead_by) unique commits. Refusing to merge, reset or force-push. Reconcile that branch manually; PR branches are untouched."
        }
        if ($comparison.behind_by -le 0) { throw "Unexpected fork comparison; refusing to change $fork/master." }
        Write-Host "Fast-forwarding $fork/master ($($comparison.behind_by) commits behind)."
        try {
            $null = Invoke-VoxelyWingetApi -Method PATCH -Path "repos/$fork/git/refs/heads/master" -Body @{ sha = $target; force = $false }
        }
        catch {
            if ($_.Exception.Data["StatusCode"] -in @(409, 422) -and $attempt -lt 3) { continue }
            throw
        }
        $actual = (Invoke-VoxelyWingetApi -Path "repos/$fork/git/ref/heads/master").Data.object.sha
        if ($actual -eq $target) { return }
        # A concurrent writer advanced the ref. Recheck ancestry before another attempt.
    }
    throw "$fork/master kept changing during synchronization. Retry after the other writer finishes."
}

function Initialize-VoxelyWingetSubmission {
    param([Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version)
    $id = Get-VoxelyWingetPackageId
    $upstream = "microsoft/winget-pkgs"
    $directory = "manifests/a/AryaPaw/Voxely/$Version"
    $published = Invoke-VoxelyWingetApi -Path "repos/$upstream/contents/$directory/$id.yaml`?ref=master" -AllowNotFound
    if ($null -ne $published) {
        return [pscustomobject]@{ Status = "published"; Url = "https://github.com/$upstream/tree/master/$directory"; ManifestRepo = $upstream; ManifestRef = 'master' }
    }
    $identity = Invoke-VoxelyWingetApi -Path "user"
    $owner = [string]$identity.Data.login
    if ($owner -notmatch '^[a-zA-Z0-9-]+$') { throw "Unexpected GitHub login in WinGet token identity." }
    $query = [Uri]::EscapeDataString("repo:$upstream is:pr is:open author:$owner in:title `"$id version $Version`"")
    $search = (Invoke-VoxelyWingetApi -Path "search/issues?q=$query&per_page=100").Data
    if ($search.incomplete_results -or $search.total_count -gt 100) { throw "Incomplete WinGet PR search; refusing a potentially duplicate submission." }
    foreach ($item in $search.items) {
        if ($item.title -notmatch "^(New version: )?$([regex]::Escape($id)) version $([regex]::Escape($Version))$") { continue }
        $pr = (Invoke-VoxelyWingetApi -Path "repos/$upstream/pulls/$($item.number)").Data
        if ($pr.state -ne "open" -or $pr.head.repo.full_name -ne "$owner/winget-pkgs" -or $pr.base.repo.full_name -ne $upstream -or $pr.base.ref -ne "master") { continue }
        if ($pr.changed_files -gt 100) { throw "Existing WinGet PR has unexpected scope; inspect $($pr.html_url) before retrying." }
        $files = @((Invoke-VoxelyWingetApi -Path "repos/$upstream/pulls/$($item.number)/files?per_page=100").Data)
        if ($files.Count -ne $pr.changed_files -or $files.Count -eq 0 -or @($files | Where-Object { -not $_.filename.StartsWith("$directory/", [StringComparison]::Ordinal) }).Count -gt 0) {
            throw "Existing WinGet PR has unexpected files; inspect $($pr.html_url) before retrying."
        }
        return [pscustomobject]@{ Status = "open-pr"; Url = $pr.html_url; ManifestRepo = "$owner/winget-pkgs"; ManifestRef = $pr.head.sha }
    }
    # GitHub may reject fork updates that bring in upstream workflow changes.
    # Require these scopes before any write, even if the fork happens to be current today.
    if (("public_repo" -notin $identity.Scopes -and "repo" -notin $identity.Scopes) -or "workflow" -notin $identity.Scopes) {
        throw "WINGET_CREATE_GITHUB_TOKEN needs a classic PAT with public_repo and workflow scopes. Update the existing Actions secret once; do not delete the fork or grant delete_repo. See docs/WINGET.md."
    }
    Sync-VoxelyWingetFork -Owner $owner
    return [pscustomobject]@{ Status = "ready"; Url = $null }
}

function Assert-VoxelyWingetManifestHash {
    param(
        [Parameter(Mandatory)][ValidatePattern('^[a-zA-Z0-9-]+/winget-pkgs$')][string]$Repo,
        [Parameter(Mandatory)][ValidatePattern('^(master|[a-fA-F0-9]{40})$')][string]$Ref,
        [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
        [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$Sha256
    )
    $path = "repos/$Repo/contents/manifests/a/AryaPaw/Voxely/$Version/AryaPaw.Voxely.installer.yaml`?ref=$Ref"
    $response = (Invoke-VoxelyWingetApi -Path $path).Data
    if ($response.encoding -ne 'base64') { throw 'Existing WinGet installer manifest is not verifiable.' }
    $manifest = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($response.content))
    $hashes = [regex]::Matches($manifest, '(?m)^\s*InstallerSha256:\s*([A-Fa-f0-9]{64})\s*$')
    $urls = [regex]::Matches($manifest, '(?m)^\s*(?:-\s*)?InstallerUrl:\s*(\S+)\s*$')
    if ($hashes.Count -ne 1 -or $hashes[0].Groups[1].Value -ne $Sha256 -or
        $urls.Count -ne 1 -or $urls[0].Groups[1].Value -cne (Get-VoxelyVersionedInstallerUrl -Version $Version)) {
        throw 'Existing WinGet manifest differs from the release installer. Use a new version; no duplicate was submitted.'
    }
}
