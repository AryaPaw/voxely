# Run the complete Rust suite and require the registered critical regressions to pass.
# The registry is the canonical test mapping. Changes need a behavioral justification and review.
#Requires -Version 7.0
param(
    [switch]$SelfTest,
    [switch]$FailureInjection
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$registryPath = Join-Path $PSScriptRoot 'critical-tests.json'
$repoRoot = Split-Path -Parent $PSScriptRoot

function Assert-ExactKeys($Value, [string[]]$Keys) {
    if ($Value -isnot [System.Collections.IDictionary]) { throw 'Expected a JSON object' }
    $actual = @($Value.Keys)
    if ($actual.Count -ne $Keys.Count) { throw 'Unexpected registry object fields' }
    foreach ($key in $Keys) {
        if (-not ($actual -ccontains $key)) { throw "Missing registry field: $key" }
    }
}

function Read-Registry([string]$Json) {
    $value = ConvertFrom-Json -InputObject $Json -AsHashtable
    Assert-ExactKeys $value @('schemaVersion', 'groups')
    if ($value.schemaVersion -isnot [long] -and $value.schemaVersion -isnot [int]) { throw 'Invalid registry schema version' }
    if ($value.schemaVersion -ne 1) { throw 'Unsupported registry schema version' }
    if ($value.groups -isnot [array] -or $value.groups.Count -eq 0) { throw 'Critical groups must be a nonempty array' }
    # Risk domains are the runner's contract; exact test IDs live only in the registry.
    $requiredDomains = @('capture-cancel', 'persistence-recovery', 'generation-insertion', 'retry', 'usage-history', 'retention', 'shutdown', 'insertion-delivery', 'compare', 'network')
    $domains = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $ids = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($group in $value.groups) {
        Assert-ExactKeys $group @('id', 'purpose', 'tests')
        if ($group.id -isnot [string] -or -not ($requiredDomains -ccontains $group.id)) { throw 'Unknown critical risk domain' }
        if (-not $domains.Add($group.id)) { throw "Duplicate risk domain: $($group.id)" }
        if ($group.purpose -isnot [string] -or [string]::IsNullOrWhiteSpace($group.purpose)) { throw 'Each group needs a behavioral purpose' }
        if ($group.tests -isnot [array] -or $group.tests.Count -eq 0) { throw 'Critical tests must be a nonempty array' }
        foreach ($id in $group.tests) {
            if ($id -isnot [string] -or $id -cnotmatch '\A[A-Za-z_][A-Za-z_0-9]*(?:::[A-Za-z_][A-Za-z_0-9]*)+\z') { throw 'Invalid fully qualified test ID' }
            if (-not $ids.Add($id)) { throw "Duplicate critical test ID: $id" }
        }
    }
    foreach ($domain in $requiredDomains) {
        if (-not $domains.Contains($domain)) { throw "Missing critical risk domain: $domain" }
    }
    return [pscustomobject]@{ Groups = $value.groups; TestIds = $ids }
}

function New-ResultState($Registry) {
    return [pscustomobject]@{
        Registry = $Registry
        Seen = [Collections.Generic.Dictionary[string, Collections.Generic.List[string]]]::new([StringComparer]::Ordinal)
    }
}

function Add-TestOutput($State, [string]$Line) {
    if ($Line -cmatch '\Atest (?<id>[A-Za-z_][A-Za-z_0-9]*(?:::[A-Za-z_][A-Za-z_0-9]*)+) \.\.\. (?<status>.+)\z') {
        $id = $Matches.id
        if ($State.Registry.TestIds.Contains($id)) {
            if (-not $State.Seen.ContainsKey($id)) { $State.Seen.Add($id, [Collections.Generic.List[string]]::new()) }
            $State.Seen[$id].Add($Matches.status)
        }
    }
}

function Assert-TestResults($State, [int]$ExitCode) {
    if ($ExitCode -ne 0) { throw "cargo test failed: $ExitCode" }
    $failures = [Collections.Generic.List[string]]::new()
    foreach ($id in $State.Registry.TestIds) {
        if (-not $State.Seen.ContainsKey($id)) { $failures.Add("missing: $id"); continue }
        $statuses = $State.Seen[$id]
        if ($statuses.Count -ne 1) { $failures.Add("duplicate: $id"); continue }
        if ($statuses[0] -cne 'ok') { $failures.Add("$($statuses[0]): $id") }
    }
    if ($failures.Count -gt 0) { throw "Critical regression checks failed:`n$($failures -join "`n")" }
}

function New-PassingState($Registry) {
    $state = New-ResultState $Registry
    foreach ($id in $Registry.TestIds) { Add-TestOutput $state "test $id ... ok" }
    return $state
}

function Assert-Rejected([string]$Name, [scriptblock]$Action) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (-not $rejected) { throw "Guard selftest did not reject: $Name" }
}

function Invoke-GuardSelfTest([string]$Json) {
    $registry = Read-Registry $Json
    Assert-TestResults (New-PassingState $registry) 0
    $firstId = @($registry.TestIds)[0]
    foreach ($status in @('ignored', 'ignored, native dependency unavailable', 'FAILED', 'ok trailing-text')) {
        $state = New-PassingState $registry
        $null = $state.Seen.Remove($firstId)
        Add-TestOutput $state "test $firstId ... $status"
        Assert-Rejected $status { Assert-TestResults $state 0 }
    }
    $state = New-PassingState $registry
    $null = $state.Seen.Remove($firstId)
    Assert-Rejected 'missing test' { Assert-TestResults $state 0 }
    Add-TestOutput $state "test $($firstId)_suffix ... ok"
    Assert-Rejected 'substring match' { Assert-TestResults $state 0 }
    Add-TestOutput $state "test $firstId ... ok"
    Add-TestOutput $state "test $firstId ... ok"
    Assert-Rejected 'duplicate test output' { Assert-TestResults $state 0 }
    Assert-Rejected 'nonzero cargo exit' { Assert-TestResults (New-PassingState $registry) 42 }
    Assert-Rejected 'malformed JSON' { Read-Registry '{' }
    Assert-Rejected 'empty registry' { Read-Registry '{"schemaVersion":1,"groups":[]}' }
    foreach ($mutation in @('schema', 'empty-purpose', 'empty-tests', 'duplicate-id', 'missing-domain', 'duplicate-domain', 'unknown-field')) {
        $bad = ConvertFrom-Json -InputObject $Json -AsHashtable
        switch ($mutation) {
            'schema' { $bad.schemaVersion = 2 }
            'empty-purpose' { $bad.groups[0].purpose = ' ' }
            'empty-tests' { $bad.groups[0].tests = @() }
            'duplicate-id' { $bad.groups[1].tests[0] = $bad.groups[0].tests[0] }
            'missing-domain' { $bad.groups = @($bad.groups | Select-Object -Skip 1) }
            'duplicate-domain' { $bad.groups[1].id = $bad.groups[0].id }
            'unknown-field' { $bad['unexpected'] = $true }
        }
        $badJson = ConvertTo-Json -InputObject $bad -Depth 10
        Assert-Rejected $mutation { Read-Registry $badJson }
    }
    $childOutput = @(& pwsh -NoProfile -NonInteractive -File $PSCommandPath -FailureInjection 2>&1)
    if ($LASTEXITCODE -eq 0 -or -not ($childOutput -match 'cargo test failed: 42')) { throw 'Actual child exit 42 did not fail the helper' }
    Write-Host 'PASS: critical guard selftests, including actual child exit 42'
}

$registryJson = Get-Content -LiteralPath $registryPath -Raw
if ($FailureInjection) {
    $state = New-PassingState (Read-Registry $registryJson)
    & pwsh -NoProfile -NonInteractive -Command 'exit 42'
    Assert-TestResults $state $LASTEXITCODE
    exit 0
}
Invoke-GuardSelfTest $registryJson
if ($SelfTest) { exit 0 }

$startedAt = [DateTime]::UtcNow
$state = New-ResultState (Read-Registry $registryJson)
$outputDir = Join-Path $repoRoot 'coverage/rust'
$null = New-Item -ItemType Directory -Path $outputDir -Force
$logPath = Join-Path $outputDir 'critical-tests-failure.log'
$tail = [Collections.Generic.Queue[byte[]]]::new()
$tailBytes = 0
$logLimit = 1MB
$cargoExit = $null
$failure = $null
try {
    & cargo test --locked --manifest-path (Join-Path $repoRoot 'src-tauri/Cargo.toml') --all -- --test-threads=8 --color never 2>&1 | ForEach-Object {
        $line = [string]$_
        Write-Host $line
        Add-TestOutput $state $line
        $bytes = [Text.Encoding]::UTF8.GetBytes($line + "`n")
        if ($bytes.Length -gt $logLimit) { $bytes = $bytes[($bytes.Length - $logLimit)..($bytes.Length - 1)] }
        $tail.Enqueue($bytes)
        $tailBytes += $bytes.Length
        while ($tailBytes -gt $logLimit) { $tailBytes -= $tail.Dequeue().Length }
    }
    $cargoExit = $LASTEXITCODE
    Assert-TestResults $state $cargoExit
} catch {
    $failure = $_.Exception.Message
} finally {
    $groups = foreach ($group in $state.Registry.Groups) {
        $tests = foreach ($id in $group.tests) {
            [string[]]$statuses = @()
            if ($state.Seen.ContainsKey($id)) { $statuses = @($state.Seen[$id]) }
            [pscustomobject]@{ id = $id; passed = ($statuses.Count -eq 1 -and $statuses[0] -ceq 'ok'); observedStatuses = @($statuses) }
        }
        [pscustomobject]@{ id = $group.id; purpose = $group.purpose; tests = @($tests) }
    }
    $report = [ordered]@{
        schemaVersion = 1
        status = $(if ($null -eq $failure) { 'PASS' } else { 'FAIL' })
        startedAtUtc = $startedAt.ToString('o')
        finishedAtUtc = [DateTime]::UtcNow.ToString('o')
        registrySha256 = (Get-FileHash -LiteralPath $registryPath -Algorithm SHA256).Hash
        cargoExitCode = $cargoExit
        failure = $failure
        groups = @($groups)
    }
    $report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $outputDir 'critical-tests.json') -Encoding utf8
    if ($null -ne $failure) {
        $stream = [IO.File]::Create($logPath)
        try { foreach ($bytes in $tail) { $stream.Write($bytes, 0, $bytes.Length) } } finally { $stream.Dispose() }
    } elseif (Test-Path -LiteralPath $logPath) {
        Remove-Item -LiteralPath $logPath
    }
}
if ($null -ne $failure) { throw $failure }
Write-Host "PASS: complete Rust suite and $($state.Registry.TestIds.Count) registered critical regressions"
