# Attach to an already-running daily driver, with no interruption of dictation.
param([Parameter(Mandatory = $true)][int]$TargetProcessId, [switch]$SelfTest)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-independent-process.ps1')
$root = Split-Path -Parent $PSScriptRoot
function Get-ObserverAttachment {
    if (-not (Test-Path -LiteralPath $log)) { return $null }
    # Attachment is at the start, before periodic samples. The observer limits samples
    # to 256 KiB; read up to 2048 records so later samples cannot hide its identity.
    # Ignore a partially appended record when a writer is active.
    Get-Content -LiteralPath $log -TotalCount 2048 | ForEach-Object {
        try { $_ | ConvertFrom-Json } catch { $null }
    } | Where-Object { $_ -and $_.event -eq 'observer-attached' } | Select-Object -Last 1
}
if ($SelfTest) {
    $log = Join-Path ([IO.Path]::GetTempPath()) ('voxely-attachment-test-' + [Guid]::NewGuid().ToString('N') + '.jsonl')
    try {
        $lines = @('{"event":"observer-attached","observerPid":123,"observerStartTicks":456}')
        $lines += 1..40 | ForEach-Object { '{"event":"sample"}' }
        $lines += '{"partial":'
        [IO.File]::WriteAllLines($log, [string[]]$lines, [Text.UTF8Encoding]::new($false))
        $attachment = Get-ObserverAttachment
        if ($attachment.observerPid -ne 123 -or $attachment.observerStartTicks -ne 456) { throw 'Observer identity was hidden by samples.' }
        Write-Output 'PASS: observer identity survives 40 samples and a partial record'
    } finally { if (Test-Path -LiteralPath $log) { Remove-Item -LiteralPath $log } }
    return
}
$appProcess = Get-Process -Id $TargetProcessId -ErrorAction Stop
$expectedExe = Join-Path $root 'src-tauri\target\release\voxely.exe'
if (-not [string]::Equals($appProcess.Path, $expectedExe, [StringComparison]::OrdinalIgnoreCase)) { throw 'Not the local Voxely process.' }
$ticks = $appProcess.StartTime.ToUniversalTime().Ticks
$watcher = Join-Path $PSScriptRoot 'watch-local-app.ps1'
$shell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$log = Join-Path $env:APPDATA "Voxely\diagnostics\process-exits\exit-$TargetProcessId-$ticks.jsonl"
if (Test-Path -LiteralPath $log) {
    $previous = Get-ObserverAttachment
    $existing = if ($previous) { Get-Process -Id $previous.observerPid -ErrorAction SilentlyContinue } else { $null }
    if ($existing -and $previous.observerStartTicks -and
        $existing.StartTime.ToUniversalTime().Ticks -eq $previous.observerStartTicks -and
        [string]::Equals($existing.Path, $shell, [StringComparison]::OrdinalIgnoreCase)) {
        Assert-IndependentWindowsProcess $existing
        [pscustomobject]@{ ObserverPid = $existing.Id; TargetProcessId = $TargetProcessId; Journal = $log; InWindowsJob = $false }
        return
    }
}
$command = '"' + $shell + '" -NoProfile -NonInteractive -WindowStyle Hidden -File "' + $watcher + '" -TargetProcessId ' + $TargetProcessId + ' -ExpectedStartTicks ' + $ticks
$observerPid = Start-IndependentWindowsProcess -CommandLine $command -WorkingDirectory $root -Hidden
$observer = Get-Process -Id $observerPid -ErrorAction Stop
Assert-IndependentWindowsProcess $observer
$deadline = [DateTime]::UtcNow.AddSeconds(10)
do {
    if (Test-Path -LiteralPath $log) {
        $attached = Get-ObserverAttachment
        if ($attached -and $attached.event -eq 'observer-attached' -and $attached.processId -eq $TargetProcessId -and
            $attached.observerPid -eq $observerPid -and $attached.observerStartTicks -eq $observer.StartTime.ToUniversalTime().Ticks) {
            [pscustomobject]@{ ObserverPid = $attached.observerPid; TargetProcessId = $TargetProcessId; Journal = $log; InWindowsJob = $false }
            return
        }
    }
    if ($observer.HasExited) { throw 'Exit observer stopped before attachment was confirmed.' }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $deadline)
throw 'Exit observer did not confirm attachment within 10 seconds.'
