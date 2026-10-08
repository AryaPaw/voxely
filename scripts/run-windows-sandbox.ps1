param([string]$InstallerPath, [ValidateRange(1, 600)][int]$TimeoutSeconds = 300, [switch]$PrepareRuntime, [switch]$SelfTest)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-AcceptanceResult($Result, $Request) {
  if ($Result.schema -ne 1 -or $Result.runId -cne $Request.runId -or $Result.version -cne $Request.version -or $Result.installer -cne $Request.installer -or $Result.sha256 -cne $Request.sha256) { throw 'Sandbox result belongs to a different candidate or run' }
  if ($Result.status -cne 'PASS' -or $Result.stage -cne 'complete' -or $Result.error -or -not $Result.installedPath -or $Result.processId -le 0 -or $Result.visibleHwnd -le 0 -or $Result.windowTitle -cne 'Voxely' -or $Result.runtimeRunId -notmatch '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$') { throw "Sandbox acceptance failed at $($Result.stage): $($Result.error)" }
  $requestedPreparation = $Request.PSObject.Properties['prepareRuntime'] -and $Request.prepareRuntime
  $hasPreparation = $Result.PSObject.Properties['runtimePreparation'] -and $null -ne $Result.runtimePreparation
  if ([bool]$requestedPreparation -ne [bool]$hasPreparation) { throw 'Sandbox runtime prerequisite differs from the requested environment' }
}

if ($SelfTest) {
  & (Join-Path $PSScriptRoot 'sandbox-install.ps1') -SelfTest
  $request = [pscustomobject]@{ runId = ('a' * 32); version = '0.3.0'; installer = 'Voxely_0.3.0_x64-setup.exe'; sha256 = ('B' * 64) }
  $fixture = [pscustomobject]@{ schema = 1; runId = $request.runId; version = $request.version; installer = $request.installer; sha256 = $request.sha256; status = 'PASS'; stage = 'complete'; error = $null; installedPath = 'C:\fixture\voxely.exe'; processId = 123; visibleHwnd = 456; windowTitle = 'Voxely'; runtimeRunId = '12345678-1234-1234-1234-123456789abc' }
  Assert-AcceptanceResult $fixture $request
  $rejected = 0
  foreach ($field in @('runId', 'version', 'installer', 'sha256', 'status', 'stage', 'error', 'visibleHwnd', 'processId', 'windowTitle', 'runtimeRunId')) {
    $bad = $fixture | ConvertTo-Json | ConvertFrom-Json
    if ($field -in @('visibleHwnd', 'processId')) { $bad.$field = 0 } else { $bad.$field = 'invalid' }
    try { Assert-AcceptanceResult $bad $request } catch { $rejected++ }
  }
  if ($rejected -ne 11) { throw 'Negative result fixture accepted' }
  $prepared = $fixture | ConvertTo-Json | ConvertFrom-Json
  $prepared | Add-Member runtimePreparation ([pscustomobject]@{changed=$true})
  try { Assert-AcceptanceResult $prepared $request; throw 'Unrequested runtime preparation accepted' } catch { if ($_.Exception.Message -cne 'Sandbox runtime prerequisite differs from the requested environment') { throw } }
  $request | Add-Member prepareRuntime $true
  try { Assert-AcceptanceResult $fixture $request; throw 'Missing runtime preparation accepted' } catch { if ($_.Exception.Message -cne 'Sandbox runtime prerequisite differs from the requested environment') { throw } }
  Assert-AcceptanceResult $prepared $request
  Write-Output 'PASS: result helper fixtures only; Windows Sandbox acceptance NOT RUN'
  return
}

$sandbox = Get-Command WindowsSandbox.exe -ErrorAction SilentlyContinue
if (-not $sandbox) { throw 'BLOCKED: Windows Sandbox is unavailable' }
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$version = (Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Unsupported release version format' }
if (-not $InstallerPath) { $InstallerPath = Join-Path $root "src-tauri\target\release\bundle\nsis\Voxely_${version}_x64-setup.exe" }
$source = Get-Item -LiteralPath $InstallerPath
if ($source.PSIsContainer -or $source.Extension -ine '.exe') { throw 'InstallerPath must name one NSIS executable' }
$runId = [Guid]::NewGuid().ToString('N')
$run = Join-Path $root "src-tauri\target\sandbox-runs\$runId"
$mapped = Join-Path $run 'mapped'
New-Item -ItemType Directory -Path $mapped | Out-Null
Copy-Item -LiteralPath $source.FullName -Destination (Join-Path $mapped $source.Name)
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'sandbox-install.ps1') -Destination (Join-Path $mapped 'sandbox-install.ps1')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'installed-runtime.ps1') -Destination (Join-Path $mapped 'installed-runtime.ps1')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'sandbox-prepare-webview.ps1') -Destination (Join-Path $mapped 'sandbox-prepare-webview.ps1')
$request = [pscustomobject][ordered]@{ schema = 1; runId = $runId; version = $version; installer = $source.Name; sha256 = (Get-FileHash -LiteralPath (Join-Path $mapped $source.Name) -Algorithm SHA256).Hash; prepareRuntime = [bool]$PrepareRuntime }
$request | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $mapped 'request.json') -Encoding UTF8
$template = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'voxely-sandbox.wsb') -Raw
$generated = Join-Path $run 'voxely-sandbox.wsb'
$template.Replace('__HOST_BUNDLE__', [Security.SecurityElement]::Escape($mapped)) | Set-Content -LiteralPath $generated -Encoding UTF8
$resultPath = Join-Path $mapped 'result.json'
Write-Host "Sandbox run $runId; evidence: $run"
$verdict = [ordered]@{ schema = 1; runId = $runId; version = $version; installer = $request.installer; sha256 = $request.sha256; status = 'FAIL'; error = $null; resultPath = $resultPath; finishedAt = $null }
try {
  Start-Process -FilePath $sandbox.Source -ArgumentList ('"{0}"' -f $generated) -WindowStyle Hidden | Out-Null
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  while ([DateTime]::UtcNow -lt $deadline) {
    if (Test-Path -LiteralPath $resultPath -PathType Leaf) {
      $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
      Assert-AcceptanceResult $result $request
      $verdict.status = 'PASS'
      Write-Output "PASS: Windows Sandbox install, exact version, visible window and uninstall; prepared runtime: $([bool]$PrepareRuntime); evidence: $resultPath"
      return
    }
    Start-Sleep -Milliseconds 500
  }
  throw "Windows Sandbox produced no acceptance result within $TimeoutSeconds seconds"
} catch {
  $verdict.error = $_.Exception.Message
  throw "FAIL: $($verdict.error); evidence: $run"
} finally {
  $verdict.finishedAt = (Get-Date).ToUniversalTime().ToString('o')
  $verdictPath = Join-Path $run 'host-verdict.json'
  $verdict | ConvertTo-Json | Set-Content -LiteralPath "$verdictPath.tmp" -Encoding UTF8
  Move-Item -LiteralPath "$verdictPath.tmp" -Destination $verdictPath -Force
}
