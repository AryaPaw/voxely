# Launch the daily driver independently of the invoking terminal's Windows Job.
# Microsoft: https://learn.microsoft.com/windows/win32/procthread/job-objects
param()

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-independent-process.ps1')
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'src-tauri\target\release\voxely.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "Local app exe missing: $exe"
}
$running = @(Get-Process -Name voxely -ErrorAction SilentlyContinue | Where-Object {
    [string]::Equals($_.Path, $exe, [StringComparison]::OrdinalIgnoreCase)
})
if ($running.Count -gt 0) {
    throw 'Local Voxely is already running. Finish any dictation and quit from the tray before relaunching.'
}

# WMI launches outside the caller's job; BREAKAWAY also excludes the WMI host's job.
# Never silently fall back to Start-Process: that restores the lifetime coupling.
$appProcessId = Start-IndependentWindowsProcess -CommandLine ('"' + $exe + '"') -WorkingDirectory $root

$deadline = [DateTime]::UtcNow.AddSeconds(15)
do {
    $appProcess = Get-Process -Id $appProcessId -ErrorAction Stop
    if (-not [string]::Equals($appProcess.Path, $exe, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Started process path does not match the local executable.'
    }
    Assert-IndependentWindowsProcess $appProcess
    $expectedTitle = $appProcess.MainWindowTitle -in @('Voxely (local)', 'Voxely (локальная версия)')
    if ($expectedTitle -and [Voxely.LocalLaunch.ProcessJob]::IsWindowVisible($appProcess.MainWindowHandle)) {
        $observerEvidence = & (Join-Path $PSScriptRoot 'start-exit-monitor.ps1') -TargetProcessId $appProcess.Id
        [pscustomobject]@{
            ProcessId = $appProcess.Id
            Path = $appProcess.Path
            StartTime = $appProcess.StartTime
            MainWindowTitle = $appProcess.MainWindowTitle
            MainWindowHandle = $appProcess.MainWindowHandle.ToInt64()
            InWindowsJob = $false
            ObserverPid = $observerEvidence.ObserverPid
            ObserverJournal = $observerEvidence.Journal
        }
        return
    }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $deadline)
throw 'Voxely launched independently, but its main window did not appear within 15 seconds.'
