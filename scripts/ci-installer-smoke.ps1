param(
  [string]$Version,
  [switch]$SelfTest,
  [int]$TimeoutSeconds = 60
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot 'installed-runtime.ps1')
function Assert-InstalledRuntime($ActualVersion, $ExpectedVersion, $WindowHandle) {
  if ($ActualVersion -ne $ExpectedVersion) { throw "Installed version mismatch: $ActualVersion != $ExpectedVersion" }
  if ($WindowHandle -eq 0) { throw "Installed runtime did not show a window" }
}
if ($SelfTest) {
  Test-VoxelyInstalledRuntimeHelpers
  Assert-InstalledRuntime '0.2.14' '0.2.14' 123
  foreach ($case in @(@('0.2.13', '0.2.14', 123), @('0.2.14', '0.2.14', 0))) {
    $rejected = $false
    try { Assert-InstalledRuntime $case[0] $case[1] $case[2] } catch { $rejected = $true }
    if (-not $rejected) { throw 'Invalid installed runtime was accepted' }
  }
  Write-Host 'PASS: smoke rejects mismatched versions and missing windows; no installer launched'
  exit 0
}
if ($env:CI -ne 'true') { throw 'Installer smoke requires an isolated CI Windows runner; host installation refused' }
if (-not $Version) { throw 'Version is required' }
$nsis = Get-ChildItem -Recurse "src-tauri/target/release/bundle/nsis" -Filter "*-setup.exe" | Select-Object -First 1
if (-not $nsis) { throw "NSIS installer not found" }
$sig = Get-ChildItem -Recurse "src-tauri/target/release/bundle/nsis" -Filter "*.sig" | Select-Object -First 1
if (-not $sig) { throw "Updater signature (.sig) not found" }
Write-Host "Installer $($nsis.FullName)"
Write-Host "Signature $($sig.FullName)"

# NSIS is a GUI-subsystem exe. `&` returns immediately and leaves $LASTEXITCODE unset.
$proc = Start-Process -FilePath $nsis.FullName -ArgumentList @("/S", "/NS") -WindowStyle Hidden -PassThru
if ($null -eq $proc) { throw "Silent install did not start" }
if (-not $proc.WaitForExit($TimeoutSeconds * 1000)) { $proc.Kill(); throw 'Silent install timed out' }
if ($proc.ExitCode -ne 0) { throw "Silent install failed with exit $($proc.ExitCode)" }

$installDir = Join-Path $env:LOCALAPPDATA "Voxely"
$exe = Get-ChildItem $installDir -Recurse -Filter "voxely.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $exe) { throw "voxely.exe missing after install in $installDir" }
$actualVersion = ([System.Diagnostics.FileVersionInfo]::GetVersionInfo($exe.FullName).ProductVersion -split '\+')[0]
# The application window is the acceptance target on this disposable runner.
$runtime = Start-Process -FilePath $exe.FullName -PassThru -WindowStyle Normal
try {
  $ready = Wait-VoxelyInstalledRuntime $runtime $exe.FullName $Version (Join-Path $env:APPDATA 'Voxely') $TimeoutSeconds
  Assert-InstalledRuntime $actualVersion $Version $ready.Handle
  Write-Host "PASS: installed $Version, PID $($runtime.Id), main HWND $($ready.Handle), ready run $($ready.RunId)"
} finally {
  if (-not $runtime.HasExited) {
    $null = $runtime.CloseMainWindow()
    if (-not $runtime.WaitForExit(5000)) { $runtime.Kill() }
  }
}
