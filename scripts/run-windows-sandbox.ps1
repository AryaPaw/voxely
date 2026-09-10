$ErrorActionPreference = "Stop"
$wsb = Join-Path $PSScriptRoot "voxely-sandbox.wsb"
$bundle = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle\nsis"
if (-not (Test-Path $bundle)) {
  throw "Build NSIS first (bun run tauri build). Mapped folder missing: $bundle"
}
Copy-Item (Join-Path $PSScriptRoot "sandbox-install.ps1") $bundle -Force
$template = Get-Content (Join-Path $PSScriptRoot "voxely-sandbox.wsb") -Raw
$resolved = [System.IO.Path]::GetFullPath($bundle)
$generated = Join-Path $env:TEMP "voxely-sandbox.wsb"
$template.Replace("__HOST_BUNDLE__", $resolved) | Set-Content $generated
$sandbox = Get-Command WindowsSandbox.exe -ErrorAction SilentlyContinue
if (-not $sandbox) {
  Write-Host "BLOCKED: Windows Sandbox is not available on this machine."
  exit 2
}
& $sandbox.Source $generated
