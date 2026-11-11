# Publish generated manifests on an isolated branch. Never update fork master or workflows.
# Keep the submission PAT separate from gh's read-only release-inspection token.

function Invoke-VoxelyWingetApi {
    param(
        [ValidateSet("GET", "POST")][string]$Method = "GET",
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
        $request.Body = ConvertTo-Json -InputObject $Body -Depth 20 -Compress
    }
    try { $response = Invoke-WebRequest @request }
    catch { throw "GitHub API $Method $Path could not be reached. Retry to inspect existing branches and pull requests." }
    $status = [int]$response.StatusCode
    if ($AllowNotFound -and $status -eq 404) { return $null }
    if ($status -lt 200 -or $status -ge 300) {
        # Don't print response bodies or request objects containing credentials.
        $failure = [InvalidOperationException]::new("GitHub API $Method $Path failed (HTTP $status). Check token permissions, expiration and GitHub service status; see docs/WINGET.md.")
        $failure.Data["StatusCode"] = $status
        throw $failure
    }
    return [pscustomobject]@{
        Data = (ConvertFrom-Json -InputObject $response.Content -NoEnumerate)
        Scopes = @(([string]($response.Headers["X-OAuth-Scopes"] -join ",")).Split(",") | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    }
}

function Get-VoxelyWingetBranchBase {
    param([Parameter(Mandatory)][ValidatePattern('^[a-zA-Z0-9-]+$')][string]$Owner)
    $fork = "$Owner/winget-pkgs"
    $upstream = 'microsoft/winget-pkgs'
    $repository = (Invoke-VoxelyWingetApi -Path "repos/$fork").Data
    if ($repository.private -ne $false -or -not $repository.fork -or
        $repository.parent.full_name -ne $upstream -or -not $repository.permissions.push) {
        throw "$fork must be a public writable fork of $upstream. No branch was changed."
    }
    $target = (Invoke-VoxelyWingetApi -Path "repos/$upstream/git/ref/heads/master").Data.object.sha
    $current = (Invoke-VoxelyWingetApi -Path "repos/$fork/git/ref/heads/master").Data.object.sha
    Assert-VoxelyWingetSha $target
    Assert-VoxelyWingetSha $current
    # Shared history avoids importing workflows, including from stale or diverged master.
    $comparison = (Invoke-VoxelyWingetApi -Path "repos/$upstream/compare/$current...$target").Data
    $base = $comparison.merge_base_commit.sha
    Assert-VoxelyWingetSha $base
    $commit = (Invoke-VoxelyWingetApi -Path "repos/$fork/git/commits/$base").Data
    if ($commit.sha -cne $base) { throw 'Unverifiable common ancestor.' }
    Assert-VoxelyWingetSha $commit.tree.sha
    return [pscustomobject]@{ Owner = $Owner; BaseSha = $base; BaseTree = $commit.tree.sha; UpstreamSha = $target }
}

function Assert-VoxelyWingetSha($Sha) {
    if ($Sha -isnot [string] -or $Sha -cnotmatch '^[a-f0-9]{40}$') { throw 'Invalid immutable GitHub SHA.' }
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
    $query = [Uri]::EscapeDataString("repo:$upstream is:pr author:$owner in:title `"$id version $Version`"")
    $search = (Invoke-VoxelyWingetApi -Path "search/issues?q=$query&per_page=100").Data
    if ($search.incomplete_results -or $search.total_count -gt 100) { throw "Incomplete WinGet PR search; refusing a potentially duplicate submission." }
    $closedMatch = $false
    foreach ($item in $search.items) {
        if ($item.title -notmatch "^(New version: )?$([regex]::Escape($id)) version $([regex]::Escape($Version))$") { continue }
        $pr = (Invoke-VoxelyWingetApi -Path "repos/$upstream/pulls/$($item.number)").Data
        if ($pr.head.repo.full_name -ne "$owner/winget-pkgs" -or $pr.base.repo.full_name -ne $upstream -or $pr.base.ref -ne "master") { continue }
        if ($pr.state -ne 'open') { $closedMatch = $true; continue }
        if ($pr.changed_files -ne 3) { throw "Existing WinGet PR has unexpected scope; inspect $($pr.html_url) before retrying." }
        $files = @((Invoke-VoxelyWingetApi -Path "repos/$upstream/pulls/$($item.number)/files?per_page=100").Data)
        $expected = @("$directory/$id.yaml", "$directory/$id.installer.yaml", "$directory/$id.locale.en-US.yaml")
        if ($files.Count -ne 3 -or @($files.filename | Select-Object -Unique).Count -ne 3 -or
            @($files | Where-Object { $_.filename -cnotin $expected -or $_.status -cne 'added' }).Count -gt 0) {
            throw "Existing WinGet PR has unexpected files; inspect $($pr.html_url) before retrying."
        }
        Assert-VoxelyWingetSha $pr.head.sha
        return [pscustomobject]@{ Status = "open-pr"; Url = $pr.html_url; ManifestRepo = "$owner/winget-pkgs"; ManifestRef = $pr.head.sha; PrNumber = $item.number; Branch = $pr.head.ref }
    }
    if ($closedMatch) { throw 'A closed WinGet version PR already exists. Inspect it before retrying; no duplicate was submitted.' }
    if ("public_repo" -notin $identity.Scopes -and "repo" -notin $identity.Scopes) {
        throw 'WINGET_CREATE_GITHUB_TOKEN requires a classic PAT with public_repo. Neither repo nor workflow is required.'
    }
    $context = Get-VoxelyWingetBranchBase -Owner $owner
    return [pscustomobject]@{ Status = 'ready'; Url = $null; Context = $context }
}

function Assert-VoxelyWingetManifestHash {
    param(
        [Parameter(Mandatory)][ValidatePattern('^[a-zA-Z0-9-]+/winget-pkgs$')][string]$Repo,
        [Parameter(Mandatory)][ValidatePattern('^(master|[a-fA-F0-9]{40})$')][string]$Ref,
        [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
        [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$Sha256,
        [Parameter(Mandatory)][ValidatePattern('^\d{4}-\d{2}-\d{2}$')][string]$ReleaseDate
    )
    $path = "repos/$Repo/contents/manifests/a/AryaPaw/Voxely/$Version/AryaPaw.Voxely.installer.yaml`?ref=$Ref"
    $response = (Invoke-VoxelyWingetApi -Path $path).Data
    if ($response.encoding -ne 'base64') { throw 'Existing WinGet installer manifest is not verifiable.' }
    $manifest = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($response.content))
    Assert-VoxelyWingetInstallerContent -Manifest $manifest -Version $Version -Sha256 $Sha256 -ReleaseDate $ReleaseDate
}

function Assert-VoxelyWingetInstallerContent {
    param([string]$Manifest, [string]$Version, [string]$Sha256, [string]$ReleaseDate)
    $dates = [regex]::Matches($Manifest, '(?m)^ReleaseDate:\s*([^\r\n]+)\r?$')
    if ($dates.Count -ne 1 -or $dates[0].Groups[1].Value.Trim() -cne $ReleaseDate) {
        throw 'WinGet ReleaseDate is missing or differs from the GitHub Release publication date.'
    }
    $hashes = [regex]::Matches($manifest, '(?m)^\s*InstallerSha256:\s*([A-Fa-f0-9]{64})\s*$')
    $urls = [regex]::Matches($manifest, '(?m)^\s*(?:-\s*)?InstallerUrl:\s*(\S+)\s*$')
    if ($hashes.Count -ne 1 -or $hashes[0].Groups[1].Value -ne $Sha256 -or
        $urls.Count -ne 1 -or $urls[0].Groups[1].Value -cne (Get-VoxelyVersionedInstallerUrl -Version $Version)) {
        throw 'Existing WinGet manifest differs from the release installer. Use a new version; no duplicate was submitted.'
    }
}

function Get-VoxelyWingetEntries {
    param([string]$Directory, [string]$Version, [string]$Sha256, [string]$ReleaseDate)
    $root = Get-Item -LiteralPath $Directory -Force
    for ($ancestor = $root; $null -ne $ancestor; $ancestor = $ancestor.Parent) {
        if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Manifest directory contains a reparse point.' }
    }
    $id = Get-VoxelyWingetPackageId
    $names = @("$id.yaml", "$id.installer.yaml", "$id.locale.en-US.yaml")
    $files = @(Get-ChildItem -LiteralPath $root.FullName -Force)
    if ($files.Count -ne 3) { throw 'Expected exactly three generated manifests.' }
    foreach ($file in $files) {
        if ($file.PSIsContainer -or $file.Name -cnotin $names -or $file.Length -gt 256KB -or
            ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unexpected generated manifest file.' }
        $content = [IO.File]::ReadAllText($file.FullName, [Text.UTF8Encoding]::new($false, $true))
        foreach ($field in @{ PackageIdentifier = $id; PackageVersion = $Version }.GetEnumerator()) {
            $matches = [regex]::Matches($content, "(?m)^$($field.Key):\s*([^\r\n]+)\r?$")
            if ($matches.Count -ne 1 -or $matches[0].Groups[1].Value.Trim() -cne $field.Value) { throw "Invalid manifest $($field.Key)." }
        }
        if ($file.Name -ceq "$id.installer.yaml") {
            Assert-VoxelyWingetInstallerContent -Manifest $content -Version $Version -Sha256 $Sha256 -ReleaseDate $ReleaseDate
        }
        $bytes = [Text.Encoding]::UTF8.GetBytes($content)
        $header = [Text.Encoding]::UTF8.GetBytes("blob $($bytes.Length)`0")
        $blob = [Convert]::ToHexString([Security.Cryptography.SHA1]::HashData([byte[]]($header + $bytes))).ToLowerInvariant()
        [pscustomobject]@{ Path = "manifests/a/AryaPaw/Voxely/$Version/$($file.Name)"; Content = $content; Sha = $blob }
    }
}

function Assert-VoxelyWingetDiff {
    param($Files, $Entries)
    if (@($Files).Count -ne 3) { throw 'Unexpected WinGet diff: exactly three additions are required.' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($file in $Files) {
        $entry = @($Entries | Where-Object Path -CEQ $file.filename)
        if ($entry.Count -ne 1 -or -not $seen.Add($file.filename) -or $file.status -cne 'added' -or
            $file.sha -cne $entry[0].Sha) { throw 'Unexpected WinGet diff: refusing unrelated or differing files.' }
    }
}

function Assert-VoxelyWingetPullSnapshot {
    param([Parameter(Mandatory)]$Submission)
    Assert-VoxelyWingetSha $Submission.ManifestRef
    $number = [int]$Submission.PrNumber
    if ($number -le 0 -or $Submission.ManifestRepo -cnotmatch '^[a-zA-Z0-9-]+/winget-pkgs$') { throw 'Invalid PR snapshot.' }
    $current = (Invoke-VoxelyWingetApi -Path "repos/microsoft/winget-pkgs/pulls/$number").Data
    if ($current.state -cne 'open' -or $current.head.sha -cne $Submission.ManifestRef -or
        $current.head.repo.full_name -cne $Submission.ManifestRepo -or $current.head.ref -cne $Submission.Branch -or
        $current.base.repo.full_name -cne 'microsoft/winget-pkgs' -or $current.base.ref -cne 'master' -or $current.changed_files -ne 3) {
        throw 'WinGet PR identity changed; inspect it before retrying.'
    }
}

function Publish-VoxelyWingetBranch {
    param($Context, [string]$Version, $Entries)
    $owner = $Context.Owner
    if ($owner -cnotmatch '^[a-zA-Z0-9-]+$' -or $Version -cnotmatch '^\d+\.\d+\.\d+$') { throw 'Invalid submission identity.' }
    foreach ($sha in @($Context.BaseSha, $Context.BaseTree, $Context.UpstreamSha)) { Assert-VoxelyWingetSha $sha }
    $fork = "$owner/winget-pkgs"
    $branch = "codex/winget-AryaPaw.Voxely-$Version"
    $refPath = "repos/$fork/git/ref/heads/$branch"
    $existing = Invoke-VoxelyWingetApi -Path $refPath -AllowNotFound
    if ($null -eq $existing) {
        $treeEntries = @($Entries | ForEach-Object { @{ path = $_.Path; mode = '100644'; type = 'blob'; content = $_.Content } })
        $tree = (Invoke-VoxelyWingetApi -Method POST -Path "repos/$fork/git/trees" -Body @{ base_tree = $Context.BaseTree; tree = $treeEntries }).Data
        Assert-VoxelyWingetSha $tree.sha
        $commit = (Invoke-VoxelyWingetApi -Method POST -Path "repos/$fork/git/commits" -Body @{
            message = "New version: AryaPaw.Voxely version $Version"; tree = $tree.sha; parents = @($Context.BaseSha)
        }).Data
        Assert-VoxelyWingetSha $commit.sha
        try {
            $null = Invoke-VoxelyWingetApi -Method POST -Path "repos/$fork/git/refs" -Body @{ ref = "refs/heads/$branch"; sha = $commit.sha }
        } catch {
            if ($_.Exception.Data['StatusCode'] -notin @(409, 422)) { throw }
            # A timed-out or concurrent attempt may already have created this branch.
            # Read and verify it below; never overwrite it.
        }
        # A successful ref creation can precede its visibility to subsequent GETs.
        # Retry reads only: never repeat writes or treat permission errors as absence.
        for ($read = 0; $read -lt 7; $read++) {
            $existing = Invoke-VoxelyWingetApi -Path $refPath -AllowNotFound
            if ($null -ne $existing) { break }
            if ($read -lt 6) { Start-Sleep -Seconds ([int][Math]::Pow(2, $read)) }
        }
        if ($null -eq $existing) {
            throw "WinGet branch $branch is not visible after bounded retries. Retry submission to inspect existing state; no ref was overwritten."
        }
    }
    $head = $existing.Data.object.sha
    Assert-VoxelyWingetSha $head
    $compare = (Invoke-VoxelyWingetApi -Path "repos/microsoft/winget-pkgs/compare/$($Context.UpstreamSha)...$head").Data
    Assert-VoxelyWingetDiff -Files $compare.files -Entries $Entries
    return [pscustomobject]@{ Owner = $owner; Branch = $branch; Sha = $head }
}

function Publish-VoxelyWingetSubmission {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
        [Parameter(Mandatory)][string]$ManifestDirectory,
        [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$Sha256,
        [Parameter(Mandatory)][ValidatePattern('^\d{4}-\d{2}-\d{2}$')][string]$ReleaseDate
    )
    $entries = @(Get-VoxelyWingetEntries -Directory $ManifestDirectory -Version $Version -Sha256 $Sha256 -ReleaseDate $ReleaseDate)
    $branch = Publish-VoxelyWingetBranch -Context $Context -Version $Version -Entries $entries
    $head = [Uri]::EscapeDataString("$($branch.Owner):$($branch.Branch)")
    $lookup = "repos/microsoft/winget-pkgs/pulls?state=all&base=master&head=$head&per_page=2"
    $pulls = @((Invoke-VoxelyWingetApi -Path $lookup).Data)
    if ($pulls.Count -gt 1) { throw 'Ambiguous existing WinGet pull requests.' }
    if ($pulls.Count -eq 0) {
        try {
            $created = (Invoke-VoxelyWingetApi -Method POST -Path 'repos/microsoft/winget-pkgs/pulls' -Body @{
                title = "New version: AryaPaw.Voxely version $Version"; base = 'master'; head = "$($branch.Owner):$($branch.Branch)"
                body = "Update AryaPaw.Voxely to $Version. Generated with wingetcreate and verified against the published installer."
            }).Data
            $pulls = @($created)
        } catch {
            if ($_.Exception.Data['StatusCode'] -ne 422) { throw }
            $pulls = @((Invoke-VoxelyWingetApi -Path $lookup).Data)
            if ($pulls.Count -ne 1) { throw 'PR creation failed; no unique matching PR was found.' }
        }
    }
    $number = [int]$pulls[0].number
    if ($number -le 0) { throw 'Invalid WinGet pull request number.' }
    $pr = (Invoke-VoxelyWingetApi -Path "repos/microsoft/winget-pkgs/pulls/$number").Data
    if ($pr.state -cne 'open' -or $pr.head.sha -cne $branch.Sha -or
        $pr.head.repo.full_name -cne "$($branch.Owner)/winget-pkgs" -or $pr.head.ref -cne $branch.Branch -or
        $pr.base.repo.full_name -cne 'microsoft/winget-pkgs' -or $pr.base.ref -cne 'master' -or $pr.changed_files -ne 3) {
        throw 'WinGet PR identity changed; inspect it before retrying.'
    }
    $files = (Invoke-VoxelyWingetApi -Path "repos/microsoft/winget-pkgs/pulls/$number/files?per_page=100").Data
    Assert-VoxelyWingetDiff -Files $files -Entries $entries
    Assert-VoxelyWingetPullSnapshot -Submission ([pscustomobject]@{
        PrNumber = $number; ManifestRepo = "$($branch.Owner)/winget-pkgs"; ManifestRef = $branch.Sha; Branch = $branch.Branch
    })
    return [string]$pr.html_url
}
