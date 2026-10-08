# Temporary metadata-only observer. Never restarts, terminates or debugs Voxely.
param(
    [Parameter(Mandatory = $true)][int]$TargetProcessId,
    [Parameter(Mandatory = $true)][long]$ExpectedStartTicks,
    [ValidateRange(1, 168)][int]$MaxHours = 168,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$expectedExe = Join-Path $root 'src-tauri\target\release\voxely.exe'
$diagnostics = Join-Path $env:APPDATA 'Voxely\diagnostics\process-exits'
$log = Join-Path $diagnostics "exit-$TargetProcessId-$ExpectedStartTicks.jsonl"
$observerStartTicks = [Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().Ticks

function Write-Observation {
    param([string]$Event, [hashtable]$Fields = @{})
    # Reserve terminal evidence even when the periodic sample budget is exhausted.
    if ($Event -in @('sample', 'sample-unavailable') -and (Get-Item -LiteralPath $log).Length -ge 262144) { return }
    $record = [ordered]@{ timestamp = [DateTime]::UtcNow.ToString('o'); event = $Event; processId = $TargetProcessId; startTicks = $ExpectedStartTicks; observerPid = $PID; observerStartTicks = $observerStartTicks }
    foreach ($key in $Fields.Keys) { $record[$key] = $Fields[$key] }
    [IO.File]::AppendAllText($log, (($record | ConvertTo-Json -Compress) + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
}

function Get-ObservedExit {
    param([System.Diagnostics.Process]$Process, [int]$TimeoutMs)
    # Holding the original handle retains the exit status and prevents PID reuse ambiguity.
    $null = $Process.Handle
    if (-not $Process.WaitForExit($TimeoutMs)) { return $null }
    $signedCode = [int]$Process.ExitCode
    $unsignedCode = [BitConverter]::ToUInt32([BitConverter]::GetBytes($signedCode), 0)
    return @{ exitCode = $signedCode; exitCodeHex = ('0x{0:X8}' -f $unsignedCode); exitTime = $Process.ExitTime.ToUniversalTime().ToString('o') }
}

if ($SelfTest) {
    $child = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]@{
        FileName = [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
        Arguments = '-NoProfile -NonInteractive -WindowStyle Hidden -Command "exit 7"'
        UseShellExecute = $false; CreateNoWindow = $true
    })
    try {
        $exit = Get-ObservedExit $child 10000
        if (-not $exit -or $exit.exitCode -ne 7 -or $exit.exitCodeHex -ne '0x00000007') { throw 'Wrong exit status' }
        Write-Output 'PASS: held process handle retains exit code 7'
    } finally { $child.Dispose() }
    return
}

$process = Get-Process -Id $TargetProcessId -ErrorAction Stop
$null = $process.Handle
if ($process.StartTime.ToUniversalTime().Ticks -ne $ExpectedStartTicks -or
    -not [string]::Equals($process.Path, $expectedExe, [StringComparison]::OrdinalIgnoreCase)) {
    $process.Dispose()
    throw 'Observer target does not match the expected local app instance.'
}
$mutex = [Threading.Mutex]::new($false, "Local\VoxelyExitObserver-$TargetProcessId-$ExpectedStartTicks")
$owned = $false
try {
    $owned = $mutex.WaitOne(0)
    if (-not $owned) { return }
    New-Item -ItemType Directory -Path $diagnostics -Force | Out-Null
    # Only our own old, exact-format metadata files are eligible for retention.
    $old = @(Get-ChildItem -LiteralPath $diagnostics -File | Where-Object { $_.Name -match '^exit-\d+-\d+\.jsonl$' -and $_.FullName -ne $log } | Sort-Object LastWriteTime -Descending)
    foreach ($file in @($old | Select-Object -Skip 15)) { Remove-Item -LiteralPath $file.FullName }
    Write-Observation 'observer-attached' @{ maxHours = $MaxHours }
    $deadline = [DateTime]::UtcNow.AddHours($MaxHours)
    $nextSample = [DateTime]::UtcNow
    while ([DateTime]::UtcNow -lt $deadline) {
        $exit = Get-ObservedExit $process 1000
        if ($exit) { Write-Observation 'process-exited' $exit; return }
        if ([DateTime]::UtcNow -ge $nextSample) {
            try {
                $process.Refresh()
                if ($process.HasExited) { continue }
                Write-Observation 'sample' @{ privateBytes = $process.PrivateMemorySize64; workingSet = $process.WorkingSet64; handles = $process.HandleCount; cpuMs = [long]$process.TotalProcessorTime.TotalMilliseconds }
            } catch {
                if ($process.HasExited) { continue }
                Write-Observation 'sample-unavailable'
            }
            $nextSample = [DateTime]::UtcNow.AddSeconds(60)
        }
    }
    Write-Observation 'observer-expired'
} catch {
    if (Test-Path -LiteralPath $diagnostics) {
        # Exception messages may contain sensitive data; retain the type only.
        Write-Observation 'observer-failed' @{ errorType = $_.Exception.GetType().FullName }
    }
    throw
} finally {
    if ($owned) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
    $process.Dispose()
}
