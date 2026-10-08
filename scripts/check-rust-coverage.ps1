# Rust lib coverage diagnostics. Behavioral regressions are enforced separately.
# The complete gate requires the coverage tool, including local runs.

param(
    [switch]$SelfTest,
    [switch]$FailureInjection
)

$ErrorActionPreference = "Stop"
function Assert-CoverageExit([int]$Code) {
    if ($Code -ne 0) { throw "cargo llvm-cov failed: $Code" }
}
function Get-LineCoverage($Report) {
    if ($Report.type -cne 'llvm.coverage.json.export' -or @($Report.data).Count -ne 1) {
        throw 'Unsupported LLVM coverage report'
    }
    $lines = $Report.data[0].totals.lines
    if ($null -eq $lines -or
        ($lines.count -isnot [long] -and $lines.count -isnot [int]) -or
        ($lines.covered -isnot [long] -and $lines.covered -isnot [int]) -or
        $lines.count -le 0 -or $lines.covered -lt 0 -or $lines.covered -gt $lines.count -or
        ($lines.percent -isnot [double] -and $lines.percent -isnot [decimal] -and $lines.percent -isnot [long] -and $lines.percent -isnot [int]) -or
        [double]::IsNaN([double]$lines.percent) -or [double]::IsInfinity([double]$lines.percent) -or
        $lines.percent -lt 0 -or $lines.percent -gt 100 -or
        [Math]::Abs([double]$lines.percent - (100.0 * $lines.covered / $lines.count)) -gt 0.011) {
        throw 'Invalid LLVM line coverage summary'
    }
    if ($Report.data[0].files -isnot [array] -or $Report.data[0].files.Count -eq 0) { throw 'Missing LLVM per-file coverage' }
    foreach ($file in $Report.data[0].files) {
        if ($file.filename -isnot [string] -or [string]::IsNullOrWhiteSpace($file.filename) -or $null -eq $file.summary.lines) {
            throw 'Invalid LLVM per-file coverage'
        }
        $entry = $file.summary.lines
        if (($entry.count -isnot [long] -and $entry.count -isnot [int]) -or
            ($entry.covered -isnot [long] -and $entry.covered -isnot [int]) -or
            $entry.count -lt 0 -or $entry.covered -lt 0 -or $entry.covered -gt $entry.count -or
            ($entry.percent -isnot [double] -and $entry.percent -isnot [decimal] -and $entry.percent -isnot [long] -and $entry.percent -isnot [int]) -or
            [double]::IsNaN([double]$entry.percent) -or [double]::IsInfinity([double]$entry.percent) -or
            $entry.percent -lt 0 -or $entry.percent -gt 100 -or
            ($entry.count -eq 0 -and $entry.percent -ne 0) -or
            ($entry.count -gt 0 -and [Math]::Abs([double]$entry.percent - (100.0 * $entry.covered / $entry.count)) -gt 0.011)) {
            throw 'Invalid LLVM per-file line summary'
        }
    }
    return $lines
}
if ($FailureInjection) {
    & pwsh -NoProfile -Command 'exit 42'
    Assert-CoverageExit $LASTEXITCODE
    exit 0
}
if ($SelfTest) {
    & pwsh -NoProfile -File $PSCommandPath -FailureInjection
    if ($LASTEXITCODE -eq 0) { throw 'Failure injection did not fail the coverage gate' }
    foreach ($percent in @(0, 73.83, 100)) {
        $lines = @{ count = 10000; covered = [int]($percent * 100); percent = $percent }
        $fixture = @{ type = 'llvm.coverage.json.export'; data = @(@{ totals = @{ lines = $lines }; files = @(@{ filename = 'fixture.rs'; summary = @{ lines = $lines } }) }) }
        $null = Get-LineCoverage $fixture
    }
    $invalidRejected = 0
    foreach ($report in @(@{}, @{ type = 'llvm.coverage.json.export'; data = @() }, @{ type = 'llvm.coverage.json.export'; data = @(@{ totals = @{ lines = @{ count = 0; covered = 0; percent = 0 } } }) })) {
        try { $null = Get-LineCoverage $report } catch { $invalidRejected++ }
    }
    if ($invalidRejected -ne 3) { throw 'Invalid coverage report accepted' }
    foreach ($mutation in @('boolean-percent', 'string-count', 'fractional-count', 'inconsistent-percent', 'missing-files', 'invalid-file')) {
        $bad = $fixture | ConvertTo-Json -Depth 10 | ConvertFrom-Json
        switch ($mutation) {
            'boolean-percent' { $bad.data[0].totals.lines.percent = $true }
            'string-count' { $bad.data[0].totals.lines.count = '10000' }
            'fractional-count' { $bad.data[0].totals.lines.count = 1.2 }
            'inconsistent-percent' { $bad.data[0].totals.lines.covered = 0 }
            'missing-files' { $bad.data[0].files = @() }
            'invalid-file' { $bad.data[0].files[0].summary.lines.percent = $true }
        }
        $rejected = $false
        try { $null = Get-LineCoverage $bad } catch { $rejected = $true }
        if (-not $rejected) { throw "Invalid coverage fixture accepted: $mutation" }
    }
    Write-Host 'PASS: tool failures remain fatal; valid coverage percentages are diagnostic'
    exit 0
}
Set-Location (Split-Path -Parent $PSScriptRoot)
$reportDirectory = Join-Path (Get-Location) 'coverage/rust'
$reportPath = Join-Path $reportDirectory 'summary.json'
$markdownPath = Join-Path $reportDirectory 'summary.md'
# Discard only this command's generated reports, so an old result cannot look fresh.
foreach ($generatedReport in @($reportPath, $markdownPath)) {
    if (Test-Path -LiteralPath $generatedReport) { Remove-Item -LiteralPath $generatedReport -Force }
}
Set-Location "src-tauri"

$llvmCov = Get-Command cargo-llvm-cov -ErrorAction SilentlyContinue
if (-not $llvmCov) {
    $localTool = Join-Path (Split-Path -Parent $PSScriptRoot) '.local/tools/bin/cargo-llvm-cov.exe'
    if (Test-Path -LiteralPath $localTool -PathType Leaf) { $llvmCov = Get-Command $localTool }
}
if (-not $llvmCov) { throw 'cargo-llvm-cov is required: cargo install cargo-llvm-cov --locked --version 0.6.16' }

$previousTarget = $env:CARGO_TARGET_DIR
$previousIncremental = $env:CARGO_INCREMENTAL
$targetRoot = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) 'target'))
$covTarget = Join-Path $targetRoot ("llvm-cov-" + [guid]::NewGuid().ToString('N'))
$env:CARGO_TARGET_DIR = $covTarget
$env:CARGO_INCREMENTAL = "0"
try {
    New-Item -ItemType Directory -Path $reportDirectory -Force | Out-Null
    & $llvmCov.Source llvm-cov --locked --lib --json --summary-only --output-path $reportPath --ignore-filename-regex "benches|main.rs"
    Assert-CoverageExit $LASTEXITCODE
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    $lines = Get-LineCoverage $report
    $percentage = ([double]$lines.percent).ToString('F2', [Globalization.CultureInfo]::InvariantCulture)
    $summary = @(
        '# Rust coverage report',
        '',
        "Lines: $percentage% ($($lines.covered)/$($lines.count)).",
        '',
        'The global percentage is diagnostic. The complete Rust suite and the named critical regressions are mandatory.',
        'Coverage includes test code and does not prove native microphone, hotkey, HUD or insertion acceptance.',
        '',
        '| File | Covered lines | Total lines | Coverage |',
        '| --- | ---: | ---: | ---: |'
    )
    foreach ($file in $report.data[0].files) {
        $fileName = [IO.Path]::GetRelativePath((Get-Location), $file.filename).Replace('\', '/')
        $fileLines = $file.summary.lines
        $filePercentage = ([double]$fileLines.percent).ToString('F2', [Globalization.CultureInfo]::InvariantCulture)
        $summary += "| $fileName | $($fileLines.covered) | $($fileLines.count) | $filePercentage% |"
    }
    $summary | Set-Content -LiteralPath $markdownPath -Encoding UTF8
    if ($env:GITHUB_STEP_SUMMARY) { $summary | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Encoding UTF8 }
    Write-Host "Rust line coverage: $percentage% (diagnostic, no global threshold). Reports: $reportDirectory"
} finally {
    $env:CARGO_TARGET_DIR = $previousTarget
    $env:CARGO_INCREMENTAL = $previousIncremental
    if (-not ([System.IO.Path]::GetFullPath($covTarget).StartsWith($targetRoot + [System.IO.Path]::DirectorySeparatorChar))) { throw 'Coverage cleanup escaped target directory' }
    if (Test-Path -LiteralPath $covTarget) {
        Remove-Item -LiteralPath $covTarget -Recurse -Force
    }
}
