# Daily-driver runtime matrix for HWND/insert/overlay claims.
# This script does not insert text or change user WAV/SQLite. It only reports process evidence.

$ErrorActionPreference = "Stop"
$exe = Join-Path (Split-Path -Parent $PSScriptRoot) "src-tauri\target\debug\voxely.exe"

Write-Host "Daily driver: $exe"
if (-not (Test-Path $exe)) {
    Write-Host "BLOCKED: debug exe missing. Build with: bunx tauri build -d --no-bundle"
    exit 2
}

$info = Get-Item $exe
Write-Host ("exe LastWriteTime={0:o}" -f $info.LastWriteTimeUtc)

$proc = Get-CimInstance Win32_Process -Filter "Name='voxely.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.ExecutablePath -and ($_.ExecutablePath -ieq $exe) }

if (-not $proc) {
    Write-Host "BLOCKED: src-tauri/target/debug/voxely.exe is not running"
    Write-Host "Start that exe after rebuild. Do not use tauri dev or LocalAppData install."
    exit 2
}

foreach ($item in @($proc)) {
    Write-Host ("running pid={0} path={1} CreationDate={2}" -f $item.ProcessId, $item.ExecutablePath, $item.CreationDate)
}

Write-Host "Manual matrix still required on this build:"
Write-Host "- short / long / truncated capture: HUD, WAV, Completed"
Write-Host "- insert: Notepad, Cursor, Chrome/Edge, Word; Cyrillic/emoji; live focus; clipboard without Ctrl+V"
Write-Host "- Idle Escape vs busy cancel; DPI 100/150/200"
Write-Host "- crash mid-capture: .raw.wav.tmp -> Interrupted"
Write-Host "- delete failed clip and history retry with busy/cancel"
Write-Host "Installer/Sandbox is a separate gate."
exit 0
