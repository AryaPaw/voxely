# Offline API fixtures: no real credentials, repositories or network writes.
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "winget-lib.ps1")

$savedPat = $env:WINGET_CREATE_GITHUB_TOKEN
$savedGh = $env:GH_TOKEN
$env:WINGET_CREATE_GITHUB_TOKEN = "fixture-pat"
$env:GH_TOKEN = "fixture-read-only-release-token"
$script:responses = [Collections.Generic.Queue[object]]::new()
$script:calls = [Collections.Generic.List[object]]::new()
$script:checks = 0
$script:target = "a" * 40
$script:old = "b" * 40
$script:other = "c" * 40
$script:version = "0.3.1"
$script:upstream = "repos/microsoft/winget-pkgs"
$script:fork = "repos/fixture-user/winget-pkgs"
$script:manifest = "$script:upstream/contents/manifests/a/AryaPaw/Voxely/$script:version/AryaPaw.Voxely.yaml?ref=master"

function Assert-True($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
    $script:checks++
}

function Add-Response([string]$Path, $Data, [int]$Status = 200, [string]$Method = "GET", [string]$Scopes = "public_repo, workflow") {
    $script:responses.Enqueue([pscustomobject]@{
        Path = $Path; Method = $Method
        Response = [pscustomobject]@{ StatusCode = $Status; Content = (ConvertTo-Json -InputObject $Data -Depth 20 -Compress); Headers = @{ "X-OAuth-Scopes" = @($Scopes) } }
    })
}

function Invoke-WebRequest {
    param($Uri, $Method, $Headers, $Body, $ContentType, $UserAgent, $TimeoutSec, $MaximumRedirection, $SkipHttpErrorCheck)
    if ($script:responses.Count -eq 0) { throw "Unexpected API request: $Method $Uri" }
    $fixture = $script:responses.Dequeue()
    Assert-True ($Uri -eq "https://api.github.com/$($fixture.Path)") "Unexpected request path: $Uri"
    Assert-True ($Method -eq $fixture.Method) "Unexpected method: $Method"
    Assert-True ($Headers.Authorization -eq "Bearer fixture-pat") "Submission must use the dedicated PAT"
    Assert-True ($TimeoutSec -eq 30 -and $MaximumRedirection -eq 0) "Requests must be bounded and must not redirect credentials"
    $script:calls.Add([pscustomobject]@{ Path = $fixture.Path; Method = $Method; Body = $Body })
    return $fixture.Response
}

function Start-Case {
    Assert-True ($script:responses.Count -eq 0) "Previous case did not consume its API responses"
    $script:calls.Clear()
}

function Add-NewSubmission([string]$Scopes = "public_repo, workflow") {
    Add-Response $script:manifest @{ message = "Not Found" } 404
    Add-Response "user" @{ login = "fixture-user" } -Scopes $Scopes
    $query = [Uri]::EscapeDataString("repo:microsoft/winget-pkgs is:pr is:open author:fixture-user in:title `"AryaPaw.Voxely version $script:version`"")
    Add-Response "search/issues?q=$query&per_page=100" @{ incomplete_results = $false; total_count = 0; items = @() }
}

function Add-Fork {
    Add-Response $script:fork @{ fork = $true; parent = @{ full_name = "microsoft/winget-pkgs" }; default_branch = "master"; permissions = @{ push = $true } }
}

function Add-Comparison([int]$Ahead = 0, [int]$Behind = 7301) {
    Add-Response "$script:upstream/git/ref/heads/master" @{ object = @{ sha = $script:target } }
    Add-Response "$script:fork/git/ref/heads/master" @{ object = @{ sha = $script:old } }
    Add-Response "$script:upstream/compare/$script:target...$script:old" @{ ahead_by = $Ahead; behind_by = $Behind }
}

function Expect-Failure([scriptblock]$Action, [string]$Pattern) {
    $message = ""
    try { & $Action | Out-Null }
    catch { $message = $_.Exception.Message }
    Assert-True ($message -match $Pattern) "Expected failure '$Pattern', got '$message'"
    Assert-True (-not $message.Contains("fixture-pat") -and -not $message.Contains("fixture-read-only-release-token")) "Credentials leaked in error"
}

try {
    Start-Case
    Add-Response $script:manifest @{ name = "AryaPaw.Voxely.yaml" }
    $result = Initialize-VoxelyWingetSubmission -Version $script:version
    Assert-True ($result.Status -eq "published" -and $script:calls.Count -eq 1) "Published version must skip all writes"

    Start-Case
    Add-Response $script:manifest @{} 404
    Add-Response "user" @{ login = "fixture-user" } -Scopes "public_repo"
    $query = [Uri]::EscapeDataString("repo:microsoft/winget-pkgs is:pr is:open author:fixture-user in:title `"AryaPaw.Voxely version $script:version`"")
    Add-Response "search/issues?q=$query&per_page=100" @{ incomplete_results = $false; total_count = 1; items = @(@{ number = 42; title = "New version: AryaPaw.Voxely version $script:version" }) }
    $pr = @{ state = "open"; head = @{ sha = $script:target; repo = @{ full_name = "fixture-user/winget-pkgs" } }; base = @{ repo = @{ full_name = "microsoft/winget-pkgs" }; ref = "master" }; changed_files = 1; html_url = "https://github.com/microsoft/winget-pkgs/pull/42" }
    Add-Response "$script:upstream/pulls/42" $pr
    Add-Response "$script:upstream/pulls/42/files?per_page=100" @(@{ filename = "manifests/a/AryaPaw/Voxely/$script:version/AryaPaw.Voxely.yaml" })
    $result = Initialize-VoxelyWingetSubmission -Version $script:version
    Assert-True ($result.Status -eq "open-pr" -and $result.Url -eq $pr.html_url) "Existing exact PR must skip sync and submission"
    Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 0) "Existing PR must not mutate fork"

    foreach ($scopes in @("public_repo", "workflow", "")) {
        Start-Case
        Add-NewSubmission -Scopes $scopes
        Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "classic PAT with public_repo and workflow"
        Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 0) "Insufficient scopes must fail before writes"
    }

    Start-Case
    Add-NewSubmission -Scopes "repo, workflow"
    Add-Fork
    Add-Response "$script:upstream/git/ref/heads/master" @{ object = @{ sha = $script:target } }
    Add-Response "$script:fork/git/ref/heads/master" @{ object = @{ sha = $script:target } }
    Assert-True ((Initialize-VoxelyWingetSubmission -Version $script:version).Status -eq "ready") "Current fork should be ready"
    Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 0) "Current fork must not be rewritten"

    Start-Case
    Add-NewSubmission
    Add-Fork
    Add-Comparison
    Add-Response "$script:fork/git/refs/heads/master" @{ object = @{ sha = $script:target } } -Method PATCH
    Add-Response "$script:fork/git/ref/heads/master" @{ object = @{ sha = $script:target } }
    Assert-True ((Initialize-VoxelyWingetSubmission -Version $script:version).Status -eq "ready") "Behind-only fork should fast-forward"
    $patch = @($script:calls | Where-Object Method -eq PATCH)
    $body = $patch[0].Body | ConvertFrom-Json
    Assert-True ($patch.Count -eq 1 -and $body.sha -eq $script:target -and $body.force -ceq $false) "Sync must update only captured SHA with force=false"

    foreach ($behind in @(0, 100)) {
        Start-Case
        Add-NewSubmission
        Add-Fork
        Add-Comparison -Ahead 1 -Behind $behind
        Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "unique commits"
        Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 0) "Ahead or diverged fork must be preserved"
    }

    Start-Case
    Add-NewSubmission
    Add-Response $script:fork @{ fork = $false; parent = @{ full_name = "other/repo" }; default_branch = "master"; permissions = @{ push = $true } }
    Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "must be a writable fork"

    Start-Case
    Add-NewSubmission
    Add-Fork
    Add-Comparison
    Add-Response "$script:fork/git/refs/heads/master" @{ message = "fixture-pat must never appear" } 403 PATCH
    Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "HTTP 403"

    foreach ($status in @(409, 422)) {
        Start-Case
        Add-NewSubmission
        Add-Fork
        for ($attempt = 1; $attempt -le 3; $attempt++) {
            Add-Comparison
            Add-Response "$script:fork/git/refs/heads/master" @{} $status PATCH
        }
        Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "HTTP $status"
        Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 3) "Concurrent sync retries must be bounded"
    }

    Start-Case
    Add-NewSubmission
    Add-Fork
    Add-Comparison
    Add-Response "$script:fork/git/refs/heads/master" @{} 422 PATCH
    Add-Response "$script:upstream/git/ref/heads/master" @{ object = @{ sha = $script:target } }
    Add-Response "$script:fork/git/ref/heads/master" @{ object = @{ sha = $script:target } }
    Assert-True ((Initialize-VoxelyWingetSubmission -Version $script:version).Status -eq "ready") "Concurrent successful sync should become a no-op on retry"

    Start-Case
    Add-NewSubmission
    Add-Fork
    Add-Comparison
    Add-Response "$script:fork/git/refs/heads/master" @{} -Method PATCH
    Add-Response "$script:fork/git/ref/heads/master" @{ object = @{ sha = $script:other } }
    Add-Response "$script:upstream/git/ref/heads/master" @{ object = @{ sha = $script:target } }
    Add-Response "$script:fork/git/ref/heads/master" @{ object = @{ sha = $script:other } }
    Add-Response "$script:upstream/compare/$script:target...$script:other" @{ ahead_by = 1; behind_by = 100 }
    Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "unique commits"
    Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 1) "A new concurrent unique commit must prevent another PATCH"

    foreach ($searchFailure in @("incomplete", "server-error")) {
        Start-Case
        Add-Response $script:manifest @{} 404
        Add-Response "user" @{ login = "fixture-user" }
        if ($searchFailure -eq "incomplete") {
            Add-Response "search/issues?q=$query&per_page=100" @{ incomplete_results = $true; total_count = 0; items = @() }
            Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "Incomplete WinGet PR search"
        }
        else {
            Add-Response "search/issues?q=$query&per_page=100" @{} 500
            Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "HTTP 500"
        }
        Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 0) "Unreliable PR search must prevent writes"
    }

    Start-Case
    Add-Response $script:manifest @{} 404
    Add-Response "user" @{ login = "fixture-user" }
    Add-Response "search/issues?q=$query&per_page=100" @{ incomplete_results = $false; total_count = 1; items = @(@{ number = 42; title = "New version: AryaPaw.Voxely version $script:version" }) }
    Add-Response "$script:upstream/pulls/42" $pr
    Add-Response "$script:upstream/pulls/42/files?per_page=100" @(@{ filename = "manifests/a/AnotherPublisher/App/1.0.0/App.yaml" })
    Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "unexpected files"
    Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 0) "PR containing unrelated files must require inspection"

    Start-Case
    Add-Response $script:manifest @{} 500
    Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "HTTP 500"
    Assert-True ($script:calls.Count -eq 1) "API errors must not be treated as an absent manifest"

    Start-Case
    Expect-Failure { Initialize-VoxelyWingetSubmission -Version '0.3.1"; bad' } "pattern"
    Assert-True ($script:calls.Count -eq 0) "Invalid version must not reach API"

    foreach ($hashMatches in @($true, $false)) {
        Start-Case
        $expectedHash = 'a' * 64
        $manifestHash = if ($hashMatches) { $expectedHash } else { 'b' * 64 }
        $yaml = "Installers:`n- InstallerUrl: $(Get-VoxelyVersionedInstallerUrl -Version $script:version)`n  InstallerSha256: $manifestHash`n"
        Add-Response "$script:upstream/contents/manifests/a/AryaPaw/Voxely/$script:version/AryaPaw.Voxely.installer.yaml?ref=master" @{ encoding = 'base64'; content = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($yaml)) }
        if ($hashMatches) {
            Assert-VoxelyWingetManifestHash -Repo 'microsoft/winget-pkgs' -Ref master -Version $script:version -Sha256 $expectedHash
        } else {
            Expect-Failure { Assert-VoxelyWingetManifestHash -Repo 'microsoft/winget-pkgs' -Ref master -Version $script:version -Sha256 $expectedHash } 'differs from the release installer'
        }
        Assert-True (@($script:calls | Where-Object Method -eq PATCH).Count -eq 0) 'Manifest verification must be read-only'
    }

    Start-Case
    $env:WINGET_CREATE_GITHUB_TOKEN = ""
    Expect-Failure { Initialize-VoxelyWingetSubmission -Version $script:version } "requires env"
    Assert-True ($script:calls.Count -eq 0) "Missing PAT must not fall back to gh credentials"
    Assert-True ($env:GH_TOKEN -eq "fixture-read-only-release-token") "gh release token must remain unchanged"
    Write-Host "winget-github offline tests passed ($script:checks assertions)"
}
finally {
    $env:WINGET_CREATE_GITHUB_TOKEN = $savedPat
    $env:GH_TOKEN = $savedGh
}
