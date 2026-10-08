# Explicit opt-in only. Never called from application startup or the launcher.
# Microsoft: https://learn.microsoft.com/windows/win32/wer/collecting-user-mode-dumps
param(
    [ValidateSet('Status', 'Enable', 'Disable')][string]$Mode = 'Status',
    [string]$DiagnosticsRoot = (Join-Path $env:APPDATA 'Voxely\diagnostics')
)
$ErrorActionPreference = 'Stop'
$key = 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\voxely.exe'
$dumpFolder = Join-Path ([IO.Path]::GetFullPath($DiagnosticsRoot)) 'dumps'
$stateFile = Join-Path $DiagnosticsRoot 'wer-config.json'

function Get-DumpStatus {
    $settings = Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
    [pscustomobject]@{
        RegistryKey = $key
        Enabled = ($null -ne $settings -and $settings.DumpFolder -eq $dumpFolder -and $settings.DumpType -eq 1 -and $settings.DumpCount -eq 3)
        DumpFolder = if ($settings) { $settings.DumpFolder } else { $null }
        DumpType = if ($settings) { $settings.DumpType } else { $null }
        DumpCount = if ($settings) { $settings.DumpCount } else { $null }
    }
}
if ($Mode -eq 'Status') { Get-DumpStatus; return }
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Configuring Windows Error Reporting requires an elevated PowerShell window.'
}

if ($Mode -eq 'Enable') {
    if (Test-Path -LiteralPath $key) {
        if ((Get-DumpStatus).Enabled) { Get-DumpStatus; return }
        throw 'Existing per-app WER configuration found. It was preserved; inspect it before changing it.'
    }
    New-Item -ItemType Directory -Path $dumpFolder -Force | Out-Null
    # Prior key is absent. Do not change global WER settings or other applications.
    @{ owner = 'voxely-diagnostics-v1'; priorKeyExisted = $false; dumpFolder = $dumpFolder; status = 'pending'; enabledAt = [DateTime]::UtcNow.ToString('o') } |
        ConvertTo-Json | Set-Content -LiteralPath $stateFile
    New-Item -Path $key -Force | Out-Null
    New-ItemProperty -LiteralPath $key -Name DumpFolder -PropertyType ExpandString -Value $dumpFolder -Force | Out-Null
    New-ItemProperty -LiteralPath $key -Name DumpCount -PropertyType DWord -Value 3 -Force | Out-Null
    New-ItemProperty -LiteralPath $key -Name DumpType -PropertyType DWord -Value 1 -Force | Out-Null
    if (-not (Get-DumpStatus).Enabled) { throw 'WER configuration verification failed.' }
    @{ owner = 'voxely-diagnostics-v1'; priorKeyExisted = $false; dumpFolder = $dumpFolder; status = 'enabled'; enabledAt = [DateTime]::UtcNow.ToString('o') } |
        ConvertTo-Json | Set-Content -LiteralPath $stateFile
    Get-DumpStatus
    return
}

if (-not (Test-Path -LiteralPath $key)) { Get-DumpStatus; return }
if (-not (Test-Path -LiteralPath $stateFile)) { throw 'No ownership record. Existing WER configuration was preserved.' }
$state = Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
$registryKey = Get-Item -LiteralPath $key
if ($state.owner -ne 'voxely-diagnostics-v1' -or $state.priorKeyExisted -ne $false -or $state.dumpFolder -ne $dumpFolder -or
    -not (Get-DumpStatus).Enabled -or $registryKey.GetSubKeyNames().Count -ne 0 -or
    @($registryKey.GetValueNames() | Where-Object { $_ -notin @('DumpFolder', 'DumpCount', 'DumpType') }).Count -gt 0) {
    throw 'WER configuration changed or ownership is unknown. No settings were removed.'
}
# Remove only the exact per-app key this script created; keep all collected dumps.
Remove-Item -LiteralPath $key
$state.status = 'disabled'
$state | ConvertTo-Json | Set-Content -LiteralPath $stateFile
Get-DumpStatus
