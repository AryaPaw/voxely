param([Parameter(Mandatory)][string]$Work, [switch]$Pilot)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$Work = (Resolve-Path -LiteralPath $Work).Path
if ([IO.Path]::GetDirectoryName($Work) -ne (Join-Path $root '.local/readme-shots') -or [IO.Path]::GetFileName($Work) -notlike 'run-*') { throw 'Invalid screenshot work directory.' }
if (@(Get-ChildItem -LiteralPath $Work -Filter '*.ready.json').Count) { throw 'This capture workspace has already been used. Run bun run docs:shots again.' }
Set-Location $root
. (Join-Path $root 'scripts/rust-build-cache.ps1')

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class ReadmeCapture {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
}
"@
$previousWork = $env:VOXELY_SHOTS_WORK
$previousPilot = $env:VOXELY_SHOTS_PILOT
$fixture = $null
$fixtureHwnd = [IntPtr]::Zero
function Stop-ReadmeFixture {
    if ($null -ne $script:fixture -and -not $script:fixture.HasExited) {
        if ($script:fixtureHwnd -ne [IntPtr]::Zero) { $null = [ReadmeCapture]::PostMessage($script:fixtureHwnd, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) }
        if (-not $script:fixture.WaitForExit(3000)) { $script:fixture.Kill() }
        $script:fixture.WaitForExit()
    }
}
try {
    $env:VOXELY_SHOTS_WORK = $Work
    $env:VOXELY_SHOTS_PILOT = if ($Pilot) { '1' } else { '0' }
    Invoke-VoxelyBuildCache -RepoRoot $root -Action {
      try {
        Write-Host 'Building isolated native screenshot tool...'
        & cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --no-run --message-format=json 1> (Join-Path $Work 'build.jsonl') 2> (Join-Path $Work 'build.log')
        if ($LASTEXITCODE -ne 0) { Get-Content (Join-Path $Work 'build.log') -Tail 35; throw "Native screenshot build failed ($LASTEXITCODE)." }
        $artifacts = @(Get-Content (Join-Path $Work 'build.jsonl') | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.executable -and $_.target.name -eq 'voxely_lib' })
        if ($artifacts.Count -ne 1) { throw 'Expected exactly one native test executable.' }
        $script:fixture = Start-Process -FilePath $artifacts[0].executable -ArgumentList @('app::session::readme_shots::capture_native_readme', '--ignored', '--exact', '--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $Work 'native.log') -RedirectStandardError (Join-Path $Work 'native-errors.log')
        $scenes = @(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'scenes.json') -Raw | ConvertFrom-Json)
        $sections = if ($Pilot) { @($scenes[0].section) } else { @($scenes.section) }
        $metadata = @()
        foreach ($section in $sections) {
            $ready = Join-Path $Work "$section.ready.json"
            $captured = Join-Path $Work "$section.captured.json"
            $deadline = [DateTime]::UtcNow.AddSeconds(40)
            while (-not (Test-Path -LiteralPath $captured)) {
                if ($fixture.HasExited) { Get-Content (Join-Path $Work 'native-errors.log') -Tail 20; throw "Native fixture exited before $section." }
                if ([DateTime]::UtcNow -gt $deadline) { throw "Timed out waiting for $section. See $Work." }
                if (Test-Path -LiteralPath $ready) {
                    $report = Get-Content -LiteralPath $ready -Raw | ConvertFrom-Json
                    if ($report.report.PSObject.Properties.Name -contains 'error') { throw $report.report.error }
                }
                Start-Sleep -Milliseconds 100
            }
            $state = Get-Content -LiteralPath $ready -Raw | ConvertFrom-Json
            $script:fixtureHwnd = [IntPtr][long]$state.hwnd
            if ($state.pid -ne $fixture.Id -or $state.section -ne $section) { throw 'Native readiness ownership mismatch.' }
            if ($state.report.PSObject.Properties.Name -contains 'error') { throw $state.report.error }
            $configured = (Get-Content -LiteralPath (Join-Path $root 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json).app.windows | Where-Object label -eq main
            if ($section -ne 'hud' -and ($state.report.width -ne $configured.width -or $state.report.height -ne $configured.height)) { throw "Unexpected logical viewport: $($state.report.width)x$($state.report.height)." }
            $size = Get-Content -LiteralPath $captured -Raw | ConvertFrom-Json
            $metadata += [pscustomobject]@{ section = $section; width = $size.width; height = $size.height; scale = $state.scaleFactor; viewport = $state.report }
            Write-Host "Captured $section ($($size.width)x$($size.height), native Tauri/WebView2)."
            [IO.File]::WriteAllText((Join-Path $Work "$section.done"), 'ok')
        }
        if (-not $fixture.WaitForExit(10000)) { throw 'Screenshot fixture failed to exit.' }
        if ($fixture.ExitCode -ne 0) { throw "Screenshot fixture failed ($($fixture.ExitCode))." }
        $metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $Work 'capture.json') -Encoding utf8
      } finally { Stop-ReadmeFixture }
    }
} finally {
    Stop-ReadmeFixture
    $env:VOXELY_SHOTS_WORK = $previousWork
    $env:VOXELY_SHOTS_PILOT = $previousPilot
}
