# Explicit idle-only exit through the running local build. Never force-kills or edits settings.
param([switch]$SelfTest)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-independent-process.ps1')
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'src-tauri\target\release\voxely.exe'

function Get-LocalQuitArgument([int]$TargetProcessId) {
    if ($TargetProcessId -le 0) { throw 'A positive target PID is required.' }
    return "--quit-for-local-rebuild=$TargetProcessId"
}

if ($SelfTest) {
    if ((Get-LocalQuitArgument 42) -cne '--quit-for-local-rebuild=42') { throw 'Wrong control argument.' }
    $rejected = $false
    try { $null = Get-LocalQuitArgument 0 } catch { $rejected = $true }
    if (-not $rejected) { throw 'Zero target PID was accepted.' }
    if (-not (Test-SameExecutablePath 'C:\Voxely\voxely.exe' 'c:\voxely\voxely.exe')) { throw 'Equivalent paths were rejected.' }
    if (Test-SameExecutablePath 'C:\Voxely\voxely.exe' 'C:\Other\voxely.exe') { throw 'Unrelated exe was accepted.' }
    Write-Output 'PASS: idle-only local quit arguments and exact process selection'
    exit 0
}

$running = @(Get-Process -Name voxely -ErrorAction SilentlyContinue | Where-Object { Test-SameExecutablePath $_.Path $exe })
if ($running.Count -eq 0) { Write-Output 'Local Voxely is already stopped.'; return }
if ($running.Count -ne 1) { throw 'Ambiguous local process selection; no exit requested.' }
$target = $running[0]
try {
    # Open and retain this process handle before sending the PID-bound request.
    $null = $target.Handle
    if (-not (Test-SameExecutablePath $target.MainModule.FileName $exe)) { throw 'Target executable changed.' }
    $control = Start-Process -FilePath $exe -ArgumentList (Get-LocalQuitArgument $target.Id) -WorkingDirectory $root -WindowStyle Hidden -PassThru
    try {
        if (-not $target.WaitForExit(40000)) {
            throw 'Voxely did not accept the idle-only exit within 40 seconds. It may be busy or an older build. Use Quit in the tray after finishing work. No force-stop or settings change was performed.'
        }
        if ($target.ExitCode -ne 0) { throw "Local Voxely exited with code $($target.ExitCode); rebuild stopped." }
        Write-Output "PASS: local Voxely PID $($target.Id) exited gracefully with code 0."
    } finally { $control.Dispose() }
} finally { $target.Dispose() }
