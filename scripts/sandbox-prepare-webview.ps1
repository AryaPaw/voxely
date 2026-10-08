param([switch]$SelfTest)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-SandboxIdentity([string]$EnvironmentUser, [string]$IdentityUser) {
  if ($EnvironmentUser -cne 'WDAGUtilityAccount' -or $IdentityUser -cne 'WDAGUtilityAccount') { throw 'WebView2 fixture preparation runs only inside Windows Sandbox' }
}

if ($SelfTest) {
  Assert-SandboxIdentity 'WDAGUtilityAccount' 'WDAGUtilityAccount'
  foreach ($users in @(@('WDAGUtilityAccount','host-user'), @('host-user','WDAGUtilityAccount'), @('host-user','host-user'))) {
    $rejected = $false
    try { Assert-SandboxIdentity $users[0] $users[1] } catch { $rejected = $true }
    if (-not $rejected) { throw 'Non-Sandbox identity accepted' }
  }
  Write-Output 'PASS: Sandbox identity fixtures only; no registry changes'
  return
}

# This is an explicit test-environment prerequisite, never an app startup repair.
Assert-SandboxIdentity $env:USERNAME ([Security.Principal.WindowsIdentity]::GetCurrent().Name.Split('\')[-1])
$root = Join-Path ${env:ProgramFiles(x86)} 'Microsoft\EdgeWebView\Application'
$folders = @(Get-ChildItem -LiteralPath $root -Directory | Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' })
if ($folders.Count -ne 1) { throw 'Expected exactly one installed Sandbox runtime; refusing to choose a version' }
$folder = $folders[0]
foreach ($relative in @('msedgewebview2.exe', 'EBWebView\x64\EmbeddedBrowserWebView.dll')) {
  $path = Join-Path $folder.FullName $relative
  $file = Get-Item -LiteralPath $path
  $signature = Get-AuthenticodeSignature -LiteralPath $path
  if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation' -or $file.VersionInfo.FileVersion -cne $folder.Name) { throw 'Sandbox runtime signature or file version mismatch' }
}
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, [Microsoft.Win32.RegistryView]::Registry32)
$clients = $null
$state = $null
try {
  $id = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
  $clients = $base.OpenSubKey("SOFTWARE\Microsoft\EdgeUpdate\Clients\$id", $true)
  $state = $base.OpenSubKey("SOFTWARE\Microsoft\EdgeUpdate\ClientState\$id", $true)
  if (-not $clients -or -not $state) { throw 'Existing Sandbox runtime registration is required' }
  $oldPath = [string]$state.GetValue('EBWebView')
  $oldVersion = [string]$clients.GetValue('pv')
  $oldStateVersion = [string]$state.GetValue('pv')
  if (-not $oldPath.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetDirectoryName($oldPath) -ine $root) { throw 'Registered runtime path is outside the expected Sandbox directory' }
  $changed = $oldPath -ine $folder.FullName -or $oldVersion -cne $folder.Name -or $oldStateVersion -cne $folder.Name
  if ($oldPath -ine $folder.FullName -and (Test-Path -LiteralPath $oldPath)) { throw 'Registered runtime still exists; refusing to replace it' }
  if ($changed) {
    $clients.SetValue('pv', $folder.Name, [Microsoft.Win32.RegistryValueKind]::String)
    $state.SetValue('pv', $folder.Name, [Microsoft.Win32.RegistryValueKind]::String)
    $state.SetValue('EBWebView', $folder.FullName, [Microsoft.Win32.RegistryValueKind]::String)
  }
  [pscustomobject]@{ changed = $changed; previousPath = $oldPath; previousVersion = $oldVersion; previousStateVersion = $oldStateVersion; runtimePath = $folder.FullName; version = $folder.Name }
} finally {
  if ($clients) { $clients.Dispose() }
  if ($state) { $state.Dispose() }
  $base.Dispose()
}
