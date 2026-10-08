# Rebuild and start the local daily-driver exe. Not a GitHub Release, tag, or installer.
param([switch]$SelfTest, [switch]$StopRunning)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "src-tauri\target\release\voxely.exe"

. (Join-Path $PSScriptRoot 'windows-independent-process.ps1')

if ($SelfTest) {
    if (-not (Test-SameExecutablePath 'C:\Voxely\voxely.exe' 'c:\voxely\voxely.exe')) {
        throw 'Same executable path was not recognized'
    }
    if (Test-SameExecutablePath 'C:\Voxely\voxely.exe' 'C:\Voxely\other.exe') {
        throw 'Different executable path was treated as the local app'
    }
    if (Test-SameExecutablePath '' $exe) { throw 'Empty process path was accepted' }
    Write-Host 'PASS: local rebuild identifies only the exact executable path'
    exit 0
}

$runningLocal = @(Get-Process -Name voxely -ErrorAction SilentlyContinue | Where-Object {
    Test-SameExecutablePath $_.Path $exe
})
if ($runningLocal.Count -gt 0) {
    if ($StopRunning) {
        & (Join-Path $PSScriptRoot 'stop-local-app.ps1')
    } else {
        $processIds = ($runningLocal | ForEach-Object { $_.Id }) -join ', '
        throw "Local Voxely is running (PID $processIds). Quit from its tray, or use -StopRunning for an idle-only graceful exit. Never change closeToTray to rebuild."
    }
}

$env:VOXELY_LOCAL_BUILD = "1"
Set-Location $root
bunx tauri build --no-bundle
if ($LASTEXITCODE -ne 0) {
    throw "local Tauri build failed with exit code $LASTEXITCODE"
}
if (-not (Test-Path $exe)) {
    throw "local app exe missing: $exe"
}

& (Join-Path $PSScriptRoot 'start-local-app.ps1')
