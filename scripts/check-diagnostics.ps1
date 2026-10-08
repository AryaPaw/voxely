# Isolated diagnostics checks only: no Voxely launch, WER writes or user-data changes.
$ErrorActionPreference = 'Stop'
$scripts = @('windows-independent-process.ps1','start-local-app.ps1','stop-local-app.ps1','rebuild-local-app.ps1','start-exit-monitor.ps1','watch-local-app.ps1','configure-crash-dumps.ps1','collect-crash-diagnostics.ps1','installed-runtime.ps1','sandbox-install.ps1','run-windows-sandbox.ps1','ci-installer-smoke.ps1','check-diagnostics.ps1')
foreach ($name in $scripts) {
    $tokens = $null
    $parseErrors = $null
    $null = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $name), [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count -gt 0) { throw "Diagnostics script parse failed: $name" }
}
$shells = @([Diagnostics.Process]::GetCurrentProcess().MainModule.FileName, (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe')) | Select-Object -Unique
foreach ($runtimeShell in $shells) {
    foreach ($installerScript in @('run-windows-sandbox.ps1', 'ci-installer-smoke.ps1')) {
        & $runtimeShell -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot $installerScript) -SelfTest
        if ($LASTEXITCODE -ne 0) { throw 'Installer acceptance helper selftest failed.' }
    }
    foreach ($localScript in @('stop-local-app.ps1', 'rebuild-local-app.ps1')) {
        & $runtimeShell -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot $localScript) -SelfTest
        if ($LASTEXITCODE -ne 0) { throw 'Local lifecycle script selftest failed.' }
    }
    & $runtimeShell -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot 'watch-local-app.ps1') -TargetProcessId 0 -ExpectedStartTicks 0 -SelfTest
    if ($LASTEXITCODE -ne 0) { throw 'Exit observer selftest failed.' }
    & $runtimeShell -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot 'start-exit-monitor.ps1') -TargetProcessId 0 -SelfTest
    if ($LASTEXITCODE -ne 0) { throw 'Observer attachment selftest failed.' }
    & $runtimeShell -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot 'collect-crash-diagnostics.ps1') -SelfTest
    if ($LASTEXITCODE -ne 0) { throw 'Collector selftest failed.' }
}
Write-Output 'PASS: diagnostic scripts parse and isolated checks'
