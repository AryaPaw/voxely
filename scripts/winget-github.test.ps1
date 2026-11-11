# Offline API fixtures: no real credentials, repositories or network writes.
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "winget-lib.ps1")

$savedPat = $env:WINGET_CREATE_GITHUB_TOKEN
$savedGh = $env:GH_TOKEN
$env:WINGET_CREATE_GITHUB_TOKEN = "fixture-pat"
$env:GH_TOKEN = "fixture-read-only-release-token"
$script:responses = [Collections.Generic.Queue[object]]::new()
$script:calls = [Collections.Generic.List[object]]::new()
$script:delays = [Collections.Generic.List[int]]::new()
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

function Add-Response([string]$Path, $Data, [int]$Status = 200, [string]$Method = "GET", [string]$Scopes = "public_repo") {
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
    if ($fixture.Response.StatusCode -eq -1) { throw 'fixture-pat lost response' }
    return $fixture.Response
}

function Start-Case {
    Assert-True ($script:responses.Count -eq 0) "Previous case did not consume its API responses"
    $script:calls.Clear()
    $script:delays.Clear()
}

function Start-Sleep {
    param([int]$Seconds)
    $script:delays.Add($Seconds)
}

function Add-NewSubmission([string]$Scopes = "public_repo") {
    Add-Response $script:manifest @{ message = "Not Found" } 404
    Add-Response "user" @{ login = "fixture-user" } -Scopes $Scopes
    $query = [Uri]::EscapeDataString("repo:microsoft/winget-pkgs is:pr author:fixture-user in:title `"AryaPaw.Voxely version $script:version`"")
    Add-Response "search/issues?q=$query&per_page=100" @{ incomplete_results = $false; total_count = 0; items = @() }
}

function Add-Fork {
    Add-Response $script:fork @{ private = $false; fork = $true; parent = @{ full_name = "microsoft/winget-pkgs" }; default_branch = "master"; permissions = @{ push = $true } }
}

function Expect-Failure([scriptblock]$Action, [string]$Pattern) {
    $message = ""
    try { & $Action | Out-Null }
    catch { $message = $_.Exception.Message }
    Assert-True ($message -match $Pattern) "Expected failure '$Pattern', got '$message'"
    Assert-True (-not $message.Contains("fixture-pat") -and -not $message.Contains("fixture-read-only-release-token")) "Credentials leaked in error"
}

$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('voxely-winget-api-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$version=$script:version; $up=$script:upstream; $fork=$script:fork
$day='2026-10-30'; $hash='e'*64; $headSha='d'*40; $treeSha='c'*40
$branch="codex/winget-AryaPaw.Voxely-$version"; $ref="$fork/git/ref/heads/$branch"
$yaml="PackageIdentifier: AryaPaw.Voxely`nPackageVersion: $version`n"
[IO.File]::WriteAllText((Join-Path $fixtureRoot 'AryaPaw.Voxely.yaml'), $yaml)
[IO.File]::WriteAllText((Join-Path $fixtureRoot 'AryaPaw.Voxely.locale.en-US.yaml'), $yaml + "Description: Unicode Кириллица`r`n")
[IO.File]::WriteAllText((Join-Path $fixtureRoot 'AryaPaw.Voxely.installer.yaml'), $yaml + "Installers:`n- InstallerUrl: $(Get-VoxelyVersionedInstallerUrl -Version $version)`n  InstallerSha256: $hash`nReleaseDate: $day`n")
$context=[pscustomobject]@{Owner='fixture-user';BaseSha=$script:old;BaseTree=$treeSha;UpstreamSha=$script:target}
$entries=@(Get-VoxelyWingetEntries -Directory $fixtureRoot -Version $version -Sha256 $hash -ReleaseDate $day)
$files=@($entries|ForEach-Object {@{filename=$_.Path;status='added';sha=$_.Sha}})
$lookup="$up/pulls?state=all&base=master&head=$([Uri]::EscapeDataString("fixture-user:$branch"))&per_page=2"
$pr=@{number=42;state='open';head=@{sha=$headSha;ref=$branch;repo=@{full_name='fixture-user/winget-pkgs'}};base=@{ref='master';repo=@{full_name='microsoft/winget-pkgs'}};changed_files=3;html_url='https://github.com/microsoft/winget-pkgs/pull/42'}
function Add-Branch([bool]$Existing=$false,[int]$RefStatus=201,$Diff=$files,[int]$MissingReads=0,[int]$ReadStatus=200) {
    if($Existing){Add-Response $ref @{object=@{sha=$headSha}}}else{
        Add-Response $ref @{} 404
        Add-Response "$fork/git/trees" @{sha=$treeSha} -Method POST
        Add-Response "$fork/git/commits" @{sha=$headSha} -Method POST
        Add-Response "$fork/git/refs" @{object=@{sha=$headSha}} $RefStatus POST
        for($read=0;$read -lt $MissingReads;$read++){Add-Response $ref @{} 404}
        if($MissingReads -ge 7){return}
        Add-Response $ref @{object=@{sha=$headSha}} $ReadStatus
        if($ReadStatus -ne 200){return}
    }
    Add-Response "$up/compare/$script:target...$headSha" @{files=@($Diff)}
}
function Add-Pull([bool]$Existing=$false,[bool]$Conflict=$false) {
    $data = @()
    if ($Existing) { $data = @($pr) }
    Add-Response $lookup -Data $data
    if(-not $Existing){
        Add-Response "$up/pulls" $pr $(if($Conflict){422}else{201}) POST
        if($Conflict){Add-Response $lookup @($pr)}
    }
    Add-Response "$up/pulls/42" $pr
    Add-Response "$up/pulls/42/files?per_page=100" $files
    Add-Response "$up/pulls/42" $pr
}
function Submit-Fixture { Publish-VoxelyWingetSubmission -Context $context -Version $version -ManifestDirectory $fixtureRoot -Sha256 $hash -ReleaseDate $day }
try {
    foreach($entry in $entries) {
        $independent = git hash-object --no-filters -- (Join-Path $fixtureRoot (Split-Path -Leaf $entry.Path))
        Assert-True ($LASTEXITCODE -eq 0 -and $independent -ceq $entry.Sha) 'Git blob hash differs from git hash-object'
    }
    Start-Case
    Add-Response $script:manifest @{}
    Assert-True ((Initialize-VoxelyWingetSubmission -Version $version).Status -eq 'published') 'Published version must skip writes'
    foreach ($wrongFiles in @($false, $true)) {
        Start-Case
        Add-Response $script:manifest @{} 404
        Add-Response user @{login='fixture-user'}
        $query=[Uri]::EscapeDataString("repo:microsoft/winget-pkgs is:pr author:fixture-user in:title `"AryaPaw.Voxely version $version`"")
        Add-Response "search/issues?q=$query&per_page=100" @{incomplete_results=$false;total_count=1;items=@(@{number=42;title="New version: AryaPaw.Voxely version $version"})}
        Add-Response "$up/pulls/42" $pr
        $existingFiles=@($files|ForEach-Object {$_.Clone()})
        if($wrongFiles){$existingFiles[0].filename='.github/workflows/ci.yml'}
        Add-Response "$up/pulls/42/files?per_page=100" $existingFiles
        if($wrongFiles){Expect-Failure {Initialize-VoxelyWingetSubmission -Version $version} 'unexpected files'}
        else{Assert-True ((Initialize-VoxelyWingetSubmission -Version $version).Status -eq 'open-pr') 'Existing PR must be read-only without workflow'}
        Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Existing PR modified fork'
    }
    foreach($failure in @('incomplete','server')){
        Start-Case
        Add-Response $script:manifest @{} 404
        Add-Response user @{login='fixture-user'}
        if($failure -eq 'incomplete'){
            Add-Response "search/issues?q=$query&per_page=100" @{incomplete_results=$true;total_count=0;items=@()}
            Expect-Failure {Initialize-VoxelyWingetSubmission -Version $version} 'Incomplete'
        }else{
            Add-Response "search/issues?q=$query&per_page=100" @{} 500
            Expect-Failure {Initialize-VoxelyWingetSubmission -Version $version} 'HTTP 500'
        }
        Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Unreliable lookup allowed writes'
    }
    foreach($withOpen in @($false,$true)) {
        Start-Case
        Add-Response $script:manifest @{} 404
        Add-Response user @{login='fixture-user'}
        $closed=$pr.Clone(); $closed.state='closed'
        $items=@(@{number=42;title="New version: AryaPaw.Voxely version $version"})
        if($withOpen){$items+=@{number=43;title="New version: AryaPaw.Voxely version $version"}}
        Add-Response "search/issues?q=$query&per_page=100" @{incomplete_results=$false;total_count=$items.Count;items=$items}
        Add-Response "$up/pulls/42" $closed
        if($withOpen){
            $open=$pr.Clone(); $open.number=43; $open.html_url='https://github.com/microsoft/winget-pkgs/pull/43'
            Add-Response "$up/pulls/43" $open
            Add-Response "$up/pulls/43/files?per_page=100" $files
            Assert-True ((Initialize-VoxelyWingetSubmission -Version $version).Url -eq $open.html_url) 'Closed-first search must reuse valid open PR'
        }else{Expect-Failure {Initialize-VoxelyWingetSubmission -Version $version} 'closed WinGet version PR'}
        Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Closed PR recovery wrote remote state'
    }
    foreach($current in @($script:old,('f'*40))) {
        Start-Case; Add-NewSubmission; Add-Fork
        Add-Response "$up/git/ref/heads/master" @{object=@{sha=$script:target}}
        Add-Response "$fork/git/ref/heads/master" @{object=@{sha=$current}}
        Add-Response "$up/compare/$current...$script:target" @{merge_base_commit=@{sha=$script:old}}
        Add-Response "$fork/git/commits/$script:old" @{sha=$script:old;tree=@{sha=$treeSha}}
        $ready=Initialize-VoxelyWingetSubmission -Version $version
        Assert-True ($ready.Status -eq 'ready' -and $ready.Context.BaseSha -eq $script:old) 'Public-only stale/diverged fork must use shared history'
        Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Initialization wrote remote state'
    }
    foreach($scopes in @('', 'workflow')) {
        Start-Case; Add-NewSubmission -Scopes $scopes
        Expect-Failure {Initialize-VoxelyWingetSubmission -Version $version} 'classic PAT with public_repo'
    }
    Start-Case; Add-NewSubmission
    Add-Response $fork @{private=$true;fork=$true;parent=@{full_name='microsoft/winget-pkgs'};permissions=@{push=$true}}
    Expect-Failure {Initialize-VoxelyWingetSubmission -Version $version} 'public writable fork'
    Start-Case; Add-Branch; Add-Pull
    Assert-True ((Submit-Fixture) -eq $pr.html_url) 'New submission failed'
    $writes=@($script:calls|Where-Object Method -eq POST)
    Assert-True ($writes.Count -eq 4) 'Expected tree, commit, new ref and PR only'
    $body=$writes[0].Body|ConvertFrom-Json
    Assert-True ($body.base_tree -eq $treeSha -and $body.tree.Count -eq 3) 'Wrong tree/entry count'
    foreach($entry in $entries){
        $sent=@($body.tree|Where-Object path -CEQ $entry.Path)
        Assert-True ($sent.Count -eq 1 -and $sent[0].content -ceq $entry.Content -and $sent[0].mode -ceq '100644' -and $sent[0].type -ceq 'blob') 'Nested JSON/content changed'
    }
    Assert-True (($writes[1].Body|ConvertFrom-Json).parents[0] -eq $script:old) 'Wrong parent'
    Assert-True (($writes[2].Body|ConvertFrom-Json).ref -ceq "refs/heads/$branch") 'Wrong ref'
    Assert-True (@($script:calls|Where-Object {$_.Method -eq 'PATCH' -or ($_.Method -eq 'POST' -and $_.Path -match 'master|workflow')}).Count -eq 0) 'Master/workflow write attempted'
    Start-Case; Add-Branch -Existing $true; Add-Pull -Existing $true
    Assert-True ((Submit-Fixture) -eq $pr.html_url) 'Retry must reuse branch and PR'
    Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Retry rewrote state'
    foreach($status in @(409,422)){
        Start-Case; Add-Branch -RefStatus $status; Add-Pull -Conflict $true
        Assert-True ((Submit-Fixture) -eq $pr.html_url) 'Concurrent ref/PR recovery failed'
    }
    foreach($status in @(201,409,422)){
        Start-Case; Add-Branch -RefStatus $status -MissingReads 3; Add-Pull
        Assert-True ((Submit-Fixture) -eq $pr.html_url) 'Delayed branch visibility must recover'
        Assert-True (($script:delays -join ',') -ceq '1,2,4') 'Branch visibility retries must use bounded exponential backoff'
        Assert-True (@($script:calls|Where-Object {$_.Method -eq 'POST' -and $_.Path -eq "$fork/git/refs"}).Count -eq 1) 'Delayed visibility recreated the ref'
    }
    Start-Case; Add-Branch -MissingReads 7
    Expect-Failure {Submit-Fixture} 'not visible after bounded retries'
    Assert-True (($script:delays -join ',') -ceq '1,2,4,8,16,32') 'Missing branch retries must stop after seven reads'
    Assert-True (@($script:calls|Where-Object {$_.Method -eq 'POST' -and $_.Path -eq "$up/pulls"}).Count -eq 0) 'Invisible branch reached PR creation'
    foreach($status in @(401,403,500)){
        Start-Case; Add-Branch -ReadStatus $status
        Expect-Failure {Submit-Fixture} "HTTP $status"
        Assert-True ($script:delays.Count -eq 0) 'Permission/server errors must not be retried as propagation delays'
    }
    foreach($kind in @('extra','modified','hash','path')){
        Start-Case; $bad=@($files|ForEach-Object {$_.Clone()})
        switch($kind){
            extra {$bad+=@{filename='.github/workflows/ci.yml';status='added';sha=$script:old}}
            modified {$bad[0].status='modified'}
            hash {$bad[0].sha=$script:old}
            path {$bad[0].filename='.github/workflows/ci.yml'}
        }
        Add-Branch -Existing $true -Diff $bad
        Expect-Failure {Submit-Fixture} 'Unexpected WinGet diff'
        Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Branch collision triggered writes'
    }
    foreach($status in @(401,403,500)){
        Start-Case; Add-Response $ref @{} $status
        Expect-Failure {Submit-Fixture} "HTTP $status"
        Assert-True ($script:calls.Count -eq 1) 'API errors treated as absence'
    }
    Start-Case
    Add-Response $ref @{} 404
    Add-Response "$fork/git/trees" @{sha=$treeSha} -Method POST
    Add-Response "$fork/git/commits" @{sha=$headSha} -Method POST
    Add-Response "$fork/git/refs" @{} -1 POST
    Expect-Failure {Submit-Fixture} 'could not be reached'
    Start-Case; Add-Branch -Existing $true; Add-Pull
    Assert-True ((Submit-Fixture) -eq $pr.html_url) 'Lost ref response retry failed'
    Assert-True (@($script:calls|Where-Object {$_.Method -eq 'POST' -and $_.Path -ne "$up/pulls"}).Count -eq 0) 'Lost ref response caused branch overwrite'
    Start-Case; Add-Branch -Existing $true
    Add-Response $lookup -Data @()
    Add-Response "$up/pulls" @{} -1 POST
    Expect-Failure {Submit-Fixture} 'could not be reached'
    Start-Case; Add-Branch -Existing $true; Add-Pull -Existing $true
    Assert-True ((Submit-Fixture) -eq $pr.html_url) 'Lost PR network response retry failed'
    Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Lost PR network response created duplicate'
    Start-Case
    Add-Response $ref @{} 404
    Add-Response "$fork/git/trees" @{message='fixture-pat must not leak'} 500 POST
    Expect-Failure {Submit-Fixture} 'HTTP 500'
    Assert-True ($script:calls.Count -eq 2) 'Failed tree allowed a commit/PR'
    Start-Case
    Add-Branch -Existing $true
    Add-Response $lookup -Data @()
    Add-Response "$up/pulls" @{} 500 POST
    Expect-Failure {Submit-Fixture} 'HTTP 500'
    Start-Case; Add-Branch -Existing $true; Add-Pull -Existing $true
    Assert-True ((Submit-Fixture) -eq $pr.html_url) 'Lost PR response retry failed'
    Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Lost PR response caused duplicate submission'
    Start-Case; Add-Branch -Existing $true
    Add-Response $lookup @($pr)
    $changedPr=$pr.Clone(); $changedPr.head=$pr.head.Clone(); $changedPr.head.sha=$script:other
    Add-Response "$up/pulls/42" $changedPr
    Expect-Failure {Submit-Fixture} 'identity changed'
    foreach($changed in @($false,$true)) {
        Start-Case
        $snapshot=[pscustomobject]@{PrNumber=42;ManifestRepo='fixture-user/winget-pkgs';ManifestRef=$headSha;Branch=$branch}
        $current=$pr.Clone(); $current.head=$pr.head.Clone()
        if($changed){$current.head.sha=$script:other}
        Add-Response "$up/pulls/42" $current
        if($changed){Expect-Failure {Assert-VoxelyWingetPullSnapshot -Submission $snapshot} 'identity changed'}
        else{Assert-VoxelyWingetPullSnapshot -Submission $snapshot}
        Assert-True (@($script:calls|Where-Object Method -ne GET).Count -eq 0) 'Snapshot check wrote remote state'
    }
    Start-Case
    foreach($matching in @($true,$false)) {
        $manifestHash=if($matching){$hash}else{'f'*64}
        $yaml="Installers:`n- InstallerUrl: $(Get-VoxelyVersionedInstallerUrl -Version $version)`n  InstallerSha256: $manifestHash`nReleaseDate: $day`n"
        Add-Response "$up/contents/manifests/a/AryaPaw/Voxely/$version/AryaPaw.Voxely.installer.yaml?ref=master" @{encoding='base64';content=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($yaml))}
        if($matching){Assert-VoxelyWingetManifestHash -Repo microsoft/winget-pkgs -Ref master -Version $version -Sha256 $hash -ReleaseDate $day}
        else{Expect-Failure {Assert-VoxelyWingetManifestHash -Repo microsoft/winget-pkgs -Ref master -Version $version -Sha256 $hash -ReleaseDate $day} 'differs from the release installer'}
    }
    Start-Case
    Expect-Failure {Publish-VoxelyWingetSubmission -Context $context -Version 'bad/version' -ManifestDirectory $fixtureRoot -Sha256 $hash -ReleaseDate $day} 'pattern'
    Expect-Failure {Publish-VoxelyWingetSubmission -Context $context -Version $version -ManifestDirectory $fixtureRoot -Sha256 ('f'*64) -ReleaseDate $day} 'differs from the release installer'
    [IO.File]::WriteAllText((Join-Path $fixtureRoot 'unexpected.yaml'), 'not publishable')
    Expect-Failure {Submit-Fixture} 'exactly three'
    Remove-Item -LiteralPath (Join-Path $fixtureRoot 'unexpected.yaml')
    Assert-True ($script:calls.Count -eq 0) 'Bad files reached API'
    $installerPath = Join-Path $fixtureRoot 'AryaPaw.Voxely.installer.yaml'
    $originalInstaller = [IO.File]::ReadAllText($installerPath)
    foreach ($badDate in @('', 'ReleaseDate: 2026-10-29', "ReleaseDate: $day`nReleaseDate: $day")) {
        [IO.File]::WriteAllText($installerPath, $originalInstaller.Replace("ReleaseDate: $day", $badDate))
        Expect-Failure {Submit-Fixture} 'ReleaseDate'
        Assert-True ($script:calls.Count -eq 0) 'Missing/wrong/duplicate ReleaseDate reached GitHub'
    }
    [IO.File]::WriteAllText($installerPath, $originalInstaller)
    Start-Case; $env:WINGET_CREATE_GITHUB_TOKEN=''
    Expect-Failure {Initialize-VoxelyWingetSubmission -Version $version} 'requires env'
    Assert-True ($script:calls.Count -eq 0 -and $env:GH_TOKEN -ceq 'fixture-read-only-release-token') 'Credential separation broken'
    $prepare=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'winget-prepare.ps1') -Raw
    Assert-True ($prepare -notmatch '"--submit"' -and $prepare -match 'Publish-VoxelyWingetSubmission -Context \$submission.Context') 'Generator must never submit/sync'
    Write-Host "PASS: public-only WinGet API ($script:checks assertions)"
} finally {
    $env:WINGET_CREATE_GITHUB_TOKEN=$savedPat; $env:GH_TOKEN=$savedGh
    $resolved=[IO.Path]::GetFullPath($fixtureRoot)
    if((Split-Path -Parent $resolved) -ne [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') -or (Split-Path -Leaf $resolved) -notlike 'voxely-winget-api-test-*'){throw 'Unexpected fixture cleanup path'}
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
