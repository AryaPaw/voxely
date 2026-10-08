param([switch]$SelfTest)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'installed-runtime.ps1')

function Assert-Request($Request) {
  if ($Request.schema -ne 1 -or $Request.runId -notmatch '^[0-9a-f]{32}$' -or $Request.version -notmatch '^\d+\.\d+\.\d+$' -or $Request.sha256 -notmatch '^[0-9A-Fa-f]{64}$') { throw 'Invalid acceptance request identity' }
  if ([IO.Path]::GetFileName($Request.installer) -cne $Request.installer -or $Request.installer -notmatch '^[^/\\:]+\.exe$') { throw 'Installer must be an exact executable filename' }
  if ($Request.PSObject.Properties['prepareRuntime'] -and $Request.prepareRuntime -isnot [bool]) { throw 'prepareRuntime must be a boolean' }
}

function Assert-FileVersion([string]$Actual, [string]$Expected) {
  $actualVersion = [version]$Actual
  $expectedVersion = [version]$Expected
  if ($actualVersion.Major -ne $expectedVersion.Major -or $actualVersion.Minor -ne $expectedVersion.Minor -or $actualVersion.Build -ne $expectedVersion.Build -or $actualVersion.Revision -gt 0) { throw "File version mismatch: $Actual, expected $Expected" }
}

function Invoke-BoundedProcess([string]$Path, [int]$TimeoutSeconds) {
  $process = Start-Process -FilePath $Path -ArgumentList '/S' -WindowStyle Hidden -PassThru
  try {
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) { throw "Process timed out: $Path" }
    if ($process.ExitCode -ne 0) { throw "Process exited with $($process.ExitCode): $Path" }
  } finally { $process.Dispose() }
}

if ($SelfTest) {
  $fixture = [pscustomobject]@{ schema = 1; runId = ('a' * 32); version = '0.3.0'; sha256 = ('b' * 64); installer = 'Voxely_0.3.0_x64-setup.exe' }
  Test-VoxelyInstalledRuntimeHelpers
  Assert-Request $fixture
  Assert-FileVersion '0.3.0.0' '0.3.0'
  $rejected = 0
  foreach ($field in @('runId', 'version', 'sha256', 'installer')) {
    $bad = $fixture | ConvertTo-Json | ConvertFrom-Json
    $bad.$field = '../invalid.exe'
    try { Assert-Request $bad } catch { $rejected++ }
  }
  try { Assert-FileVersion '0.2.14.0' '0.3.0' } catch { $rejected++ }
  $bad = $fixture | ConvertTo-Json | ConvertFrom-Json
  $bad | Add-Member prepareRuntime 'true'
  try { Assert-Request $bad } catch { $rejected++ }
  if ($rejected -ne 6) { throw 'Negative fixture accepted' }
  Write-Output 'PASS: request/version helper fixtures only; Windows Sandbox acceptance NOT RUN'
  return
}

# Keep all installation and filesystem mutations behind the Sandbox account guard.
if ($env:USERNAME -cne 'WDAGUtilityAccount' -or [Security.Principal.WindowsIdentity]::GetCurrent().Name.Split('\')[-1] -cne 'WDAGUtilityAccount') { throw 'This installer acceptance script runs only as WDAGUtilityAccount inside Windows Sandbox' }
$bundle = 'C:\Users\WDAGUtilityAccount\Desktop\VoxelyBundle'
$resultPath = Join-Path $bundle 'result.json'
$log = Join-Path $bundle 'sandbox-run.log'
$result = [ordered]@{ schema = 1; runId = $null; version = $null; installer = $null; sha256 = $null; status = 'FAIL'; stage = 'request'; error = $null; installedPath = $null; processId = $null; visibleHwnd = $null; runtimeRunId = $null; windowTitle = $null; runtimePreparation = $null; startedAt = (Get-Date).ToUniversalTime().ToString('o'); finishedAt = $null }
try {
  $request = Get-Content -LiteralPath (Join-Path $bundle 'request.json') -Raw | ConvertFrom-Json
  Assert-Request $request
  foreach ($field in @('runId', 'version', 'installer', 'sha256')) { $result[$field] = $request.$field }
  $setup = Join-Path $bundle $request.installer
  if (-not (Test-Path -LiteralPath $setup -PathType Leaf)) { throw 'Requested installer is missing' }
  if ((Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash -cne $request.sha256.ToUpperInvariant()) { throw 'Installer SHA256 mismatch' }
  if ($request.PSObject.Properties['prepareRuntime'] -and $request.prepareRuntime) {
    $result.stage = 'runtime-prerequisite'
    $result.runtimePreparation = & (Join-Path $PSScriptRoot 'sandbox-prepare-webview.ps1')
  }
  $result.stage = 'install'
  Invoke-BoundedProcess $setup 120
  $candidates = @((Join-Path $env:LOCALAPPDATA 'Voxely\voxely.exe'), (Join-Path $env:LOCALAPPDATA 'Programs\Voxely\voxely.exe'))
  $installed = @($candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf })
  if ($installed.Count -ne 1) { throw "Expected one installed executable; found $($installed.Count)" }
  $exe = [IO.Path]::GetFullPath($installed[0])
  $result.installedPath = $exe
  Assert-FileVersion ([Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileVersion) $request.version
  $result.stage = 'visible-window'
  # The application window is the acceptance target inside this disposable VM.
  $app = Start-Process -FilePath $exe -PassThru -WindowStyle Normal
  $result.processId = $app.Id
  $runtime = Wait-VoxelyInstalledRuntime $app $exe $request.version (Join-Path $env:APPDATA 'Voxely')
  $result.visibleHwnd = $runtime.Handle
  $result.runtimeRunId = $runtime.RunId
  $result.windowTitle = $runtime.Title
  $result.stage = 'uninstall'
  $uninstaller = Join-Path (Split-Path -Parent $exe) 'uninstall.exe'
  if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) { throw 'Uninstaller is missing' }
  Invoke-BoundedProcess $uninstaller 60
  $deadline = [DateTime]::UtcNow.AddSeconds(20)
  do {
    $remaining = @(Get-Process -Name voxely -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq $exe })
    if ($remaining.Count -eq 0 -and -not (Test-Path -LiteralPath $exe)) { break }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)
  if ($remaining.Count -ne 0 -or (Test-Path -LiteralPath $exe)) { throw 'Uninstall left the executable or its running process behind' }
  $result.stage = 'complete'
  $result.status = 'PASS'
} catch {
  $result.error = $_.Exception.Message
} finally {
  $result.finishedAt = (Get-Date).ToUniversalTime().ToString('o')
  $result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$resultPath.tmp" -Encoding UTF8
  Move-Item -LiteralPath "$resultPath.tmp" -Destination $resultPath -Force
  "$(Get-Date -Format o) $($result.status) $($result.stage) $($result.error)" | Add-Content -LiteralPath $log
}
if ($result.status -ne 'PASS') { exit 1 }
