param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [ValidatePattern('^[a-zA-Z0-9_-]{1,80}$')]
    [string]$Scenario = "fresh-tray-or-running",
    [string]$OutDir = "",
    [ValidateRange(3, 10)][int]$Trials = 3,
    [ValidateRange(3, 20)][int]$SamplesPerTrial = 5,
    [ValidateRange(1, 10)][int]$IntervalSeconds = 2,
    [ValidateRange(0, 30)][int]$SettleSeconds = 10,
    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"
function Get-NearestRank {
    param([double[]]$Values, [ValidateRange(0.01, 1)][double]$Percentile)
    if ($Values.Count -eq 0) { throw 'Cannot summarize an empty series.' }
    $ordered = @($Values | Sort-Object)
    $ordered[[int][Math]::Ceiling($ordered.Count * $Percentile) - 1]
}
if ($SelfTest) {
    if ((Get-NearestRank @(5, 1, 3, 2, 4) 0.5) -ne 3) { throw 'Median failed.' }
    if ((Get-NearestRank @(5, 1, 3, 2, 4) 0.95) -ne 5) { throw 'p95 failed.' }
    $emptyRejected = $false
    try { Get-NearestRank @() 0.5 } catch { $emptyRejected = $true }
    if (-not $emptyRejected) { throw 'Empty series accepted.' }
    Write-Host 'PASS: campaign quantiles and empty-series rejection'
    return
}
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $OutDir) {
    $OutDir = Join-Path $repo "docs/perf/trials"
}

$exe = Join-Path $repo "src-tauri/target/$Profile/voxely.exe"
$roots = @(Get-Process -Name voxely -ErrorAction SilentlyContinue | Where-Object {
    [string]::Equals($_.Path, $exe, [StringComparison]::OrdinalIgnoreCase)
})
if ($roots.Count -ne 1) { throw "Expected one running $exe process, found $($roots.Count)." }
$target = $roots[0]
$targetStart = $target.StartTime.ToUniversalTime()
$stamp = Get-Date -Format "yyyyMMdd-HHmmss-fff"
$series = @()
for ($trial = 1; $trial -le $Trials; $trial++) {
    Start-Sleep -Seconds $SettleSeconds
    for ($sample = 1; $sample -le $SamplesPerTrial; $sample++) {
        $current = Get-Process -Id $target.Id -ErrorAction Stop
        if ($current.StartTime.ToUniversalTime() -ne $targetStart) { throw 'Target process restarted during the campaign.' }
        $out = Join-Path $OutDir "$Profile-$Scenario-$stamp-$trial-$sample.json"
        $row = & (Join-Path $PSScriptRoot 'Get-VoxelyProcessTree.ps1') -Profile $Profile -Scenario $Scenario -TargetProcessId $target.Id -OutFile $out | ConvertFrom-Json
        $series += [pscustomobject]@{ trial = $trial; sample = $sample; capturedAt = $row.capturedAt; tree = $row.tree; source = $out }
        if ($sample -lt $SamplesPerTrial) { Start-Sleep -Seconds $IntervalSeconds }
    }
    Write-Host "Completed trial $trial/$Trials for PID $($target.Id) ($Scenario)."
}
$summary = [ordered]@{
    schema = 'voxely-perf-campaign/v1'
    scenario = $Scenario
    profile = $Profile
    processId = $target.Id
    processStartedAt = $targetStart.ToString('o')
    executableModifiedAt = (Get-Item -LiteralPath $exe).LastWriteTimeUtc.ToString('o')
    trials = $Trials
    samplesPerTrial = $SamplesPerTrial
    privateBytesMedian = Get-NearestRank @($series | ForEach-Object { $_.tree.privateBytes }) 0.5
    privateBytesP95 = Get-NearestRank @($series | ForEach-Object { $_.tree.privateBytes }) 0.95
    workingSetMedian = Get-NearestRank @($series | ForEach-Object { $_.tree.workingSet }) 0.5
    workingSetP95 = Get-NearestRank @($series | ForEach-Object { $_.tree.workingSet }) 0.95
    operationLatency = 'NOT RUN: memory sampling does not measure dictation or UI latency'
    series = $series
}
$summaryPath = Join-Path $OutDir "$Profile-$Scenario-$stamp-summary.json"
$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding utf8
Write-Host "Wrote $summaryPath"
$summary | ConvertTo-Json -Depth 8
