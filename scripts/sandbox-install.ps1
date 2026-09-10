$ErrorActionPreference = "Stop"
$bundle = "C:\Users\WDAGUtilityAccount\Desktop\VoxelyBundle"
$log = Join-Path $bundle "sandbox-run.log"
function Write-Log([string]$Message) {
  Add-Content -Path $log -Value ("{0} {1}" -f (Get-Date -Format o), $Message)
}
Write-Log "start"
$setup = Get-ChildItem $bundle -Filter "*setup*.exe" | Select-Object -First 1
if (-not $setup) {
  $setup = Get-ChildItem $bundle -Filter "*.exe" | Select-Object -First 1
}
if (-not $setup) { throw "NSIS installer not mapped into the Sandbox" }
Write-Log "install $($setup.FullName)"
& $setup.FullName "/S"
Start-Sleep -Seconds 8
$exe = @(
  "$env:LOCALAPPDATA\Programs\Voxely\voxely.exe",
  "$env:LOCALAPPDATA\Programs\voxely\voxely.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) { throw "voxely.exe not found after install" }
Write-Log "launch $exe"
Start-Process $exe
Start-Sleep -Seconds 5
Get-Process voxely -ErrorAction SilentlyContinue | Out-File -FilePath $log -Append
$unins = Get-ChildItem (Split-Path $exe) -Filter "uninstall.exe" | Select-Object -First 1
if ($unins) {
  Write-Log "uninstall $($unins.FullName)"
  & $unins.FullName "/S"
  Start-Sleep -Seconds 8
}
if (Get-Process voxely -ErrorAction SilentlyContinue) {
  throw "voxely.exe still running after uninstall"
}
if (Test-Path $exe) { throw "install directory leftover $exe" }
Write-Log "pass"
