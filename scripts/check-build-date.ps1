# Exercise Cargo's real build-script cache against disposable Git repositories.
param([string]$BuildSupport = (Join-Path $PSScriptRoot '../src-tauri/build_support/build_date.rs'))
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$BuildSupport = (Resolve-Path -LiteralPath $BuildSupport).Path
$supportHash = (Get-FileHash -LiteralPath $BuildSupport).Hash
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('voxely-build-date-' + [guid]::NewGuid().ToString('N'))
$evidence = Join-Path $temporary 'evidence'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$results = [Collections.Generic.List[object]]::new()
$commandNumber = 0
$pin = (Select-String -LiteralPath (Join-Path $repo 'rust-toolchain.toml') -Pattern '^channel\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value

function Invoke-FixtureProcess([string]$Tool, [string]$WorkingDirectory, [string[]]$Arguments, [hashtable]$Environment = @{}) {
    $script:commandNumber++
    $name = '{0:D3}-{1}' -f $script:commandNumber, $Tool
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Command $Tool -ErrorAction Stop).Source)
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    foreach ($key in @('CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR', 'VOXELY_BUILD_CACHE_SESSION', 'VOXELY_BUILD_DATE', 'SOURCE_DATE_EPOCH', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_AUTHOR_NAME', 'GIT_AUTHOR_EMAIL', 'GIT_COMMITTER_NAME', 'GIT_COMMITTER_EMAIL', 'GIT_AUTHOR_DATE', 'GIT_COMMITTER_DATE')) {
        $null = $start.Environment.Remove($key)
    }
    $start.Environment['CARGO_HOME'] = Join-Path $temporary 'cargo-home'
    $start.Environment['RUSTUP_TOOLCHAIN'] = $pin
    foreach ($key in $Environment.Keys) { $start.Environment[$key] = $Environment[$key] }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        $null = $process.Start()
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $timedOut = -not $process.WaitForExit(30000)
        if ($timedOut) { $process.Kill($true); if (-not $process.WaitForExit(5000)) { throw "$Tool fixture did not stop after timeout." } }
        if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdout, $stderr), 5000)) { throw "$Tool fixture output did not drain." }
        $outText = $stdout.Result
        $errText = $stderr.Result
        [IO.File]::WriteAllText((Join-Path $evidence "$name.stdout.log"), $outText.Substring([Math]::Max(0, $outText.Length - 65536)))
        [IO.File]::WriteAllText((Join-Path $evidence "$name.stderr.log"), $errText.Substring([Math]::Max(0, $errText.Length - 65536)))
        if ($timedOut) { throw "$Tool fixture exceeded 30 seconds." }
        if ($process.ExitCode -ne 0) { throw "$Tool fixture failed with exit $($process.ExitCode): $errText" }
        return $outText.Trim()
    } finally { $process.Dispose() }
}
function Assert-DateFixture($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Commit-DateFixture([string]$Directory, [string]$Day) {
    $null = Invoke-FixtureProcess git $Directory @('commit', '--allow-empty', '-m', "fixture $Day") @{
        GIT_AUTHOR_DATE = "${Day}T12:00:00+00:00"; GIT_COMMITTER_DATE = "${Day}T12:00:00+00:00"
    }
}
function Build-DateFixture([string]$Name, [string]$Directory, [string]$Target, [string]$Expected, [string]$Rerun = 'one', [hashtable]$Environment = @{}) {
    $before = if (Test-Path -LiteralPath $runs) { @(Get-Content -LiteralPath $runs).Count } else { 0 }
    $actual = Invoke-FixtureProcess cargo $Directory @('run', '--quiet', '--offline', '--locked', '--manifest-path', (Join-Path $Directory 'Cargo.toml'), '--target-dir', $Target) $Environment
    $after = @(Get-Content -LiteralPath $runs).Count
    Assert-DateFixture ($actual -ceq $Expected) "$Name returned '$actual', expected '$Expected'."
    if ($Rerun -eq 'one') { Assert-DateFixture ($after -eq $before + 1) "$Name must execute build.rs exactly once (before=$before, after=$after)." }
    if ($Rerun -eq 'none') { Assert-DateFixture ($after -eq $before) "$Name unexpectedly reran build.rs (before=$before, after=$after)." }
    $results.Add([pscustomobject]@{name=$Name;date=$actual;buildScriptRunsBefore=$before;buildScriptRunsAfter=$after;status='PASS'})
}
$failure = $null
$archive = $null
try {
    $identity = (Invoke-FixtureProcess git $repo @('log', '-1', '--format=%an%n%ae')) -split '\r?\n'
    Assert-DateFixture ($identity.Count -eq 2 -and -not [string]::IsNullOrWhiteSpace($identity[0]) -and -not [string]::IsNullOrWhiteSpace($identity[1])) 'Cannot establish the existing HEAD author for fixture commits.'
    $humanName = $identity[0]
    $humanEmail = $identity[1]
    $project = Join-Path $temporary 'project'
    $target = Join-Path $temporary 'target'
    $runs = Join-Path $temporary 'build-script-runs.log'
    New-Item -ItemType Directory -Path (Join-Path $project 'src') -Force | Out-Null
    @'
[package]
name = "voxely_build_date_fixture"
version = "0.0.0"
edition = "2021"
'@ | Set-Content -LiteralPath (Join-Path $project 'Cargo.toml')
    @'
version = 4
[[package]]
name = "voxely_build_date_fixture"
version = "0.0.0"
'@ | Set-Content -LiteralPath (Join-Path $project 'Cargo.lock')
    'fn main() { println!("{}", env!("VOXELY_BUILD_DATE")); }' | Set-Content -LiteralPath (Join-Path $project 'src/main.rs')
    $supportLiteral = $BuildSupport.Replace('\', '/')
    $runsLiteral = $runs.Replace('\', '/')
    @"
#[path = r###"$supportLiteral"###]
mod build_date;
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=VOXELY_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    use std::io::Write;
    writeln!(std::fs::OpenOptions::new().create(true).append(true).open(r###"$runsLiteral"###).unwrap(), "run").unwrap();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    build_date::emit_git_dependencies(repo);
    println!("cargo:rustc-env=VOXELY_BUILD_DATE={}", build_date::resolve_build_date(repo));
}
"@ | Set-Content -LiteralPath (Join-Path $project 'build.rs')
    $null = Invoke-FixtureProcess git $project @('init', '--initial-branch=main')
    foreach ($setting in @(@('user.name', $humanName), @('user.email', $humanEmail), @('commit.gpgsign', 'false'), @('core.autocrlf', 'false'), @('core.hooksPath', (Join-Path $temporary 'no-hooks')))) {
        $null = Invoke-FixtureProcess git $project @('config', '--local', $setting[0], $setting[1])
    }
    $null = Invoke-FixtureProcess git $project @('add', '--all')
    Commit-DateFixture $project '2026-01-02'
    $initialCommit = Invoke-FixtureProcess git $project @('rev-parse', 'HEAD')
    Build-DateFixture 'initial branch build' $project $target '2026-01-02'
    Build-DateFixture 'unchanged branch reuses build script' $project $target '2026-01-02' 'none'
    Commit-DateFixture $project '2026-01-03'
    Build-DateFixture 'branch commit advances date without source changes' $project $target '2026-01-03'
    $null = Invoke-FixtureProcess git $project @('pack-refs', '--all', '--prune')
    Assert-DateFixture (-not (Test-Path -LiteralPath (Join-Path $project '.git/refs/heads/main'))) 'pack-refs did not remove the loose branch reference.'
    Build-DateFixture 'packed branch baseline' $project $target '2026-01-03' 'any'
    Build-DateFixture 'unchanged packed branch reuses build script' $project $target '2026-01-03' 'none'
    Commit-DateFixture $project '2026-01-04'
    Assert-DateFixture (Test-Path -LiteralPath (Join-Path $project '.git/refs/heads/main')) 'Commit did not create the loose branch reference.'
    Build-DateFixture 'packed reference advances through a new loose commit' $project $target '2026-01-04'
    $null = Invoke-FixtureProcess git $project @('checkout', '--detach')
    Build-DateFixture 'detached HEAD baseline' $project $target '2026-01-04'
    Commit-DateFixture $project '2026-01-05'
    Build-DateFixture 'detached HEAD commit advances date' $project $target '2026-01-05'
    $worktree = Join-Path $temporary 'linked-worktree'
    $worktreeTarget = Join-Path $temporary 'worktree-target'
    $null = Invoke-FixtureProcess git $project @('worktree', 'add', '-b', 'linked', $worktree, 'HEAD')
    Assert-DateFixture (Test-Path -LiteralPath (Join-Path $worktree '.git') -PathType Leaf) 'Linked worktree must use a .git file.'
    $gitFileHash = (Get-FileHash -LiteralPath (Join-Path $worktree '.git')).Hash
    Build-DateFixture 'linked worktree baseline' $worktree $worktreeTarget '2026-01-05'
    Build-DateFixture 'unchanged linked worktree reuses build script' $worktree $worktreeTarget '2026-01-05' 'none'
    Commit-DateFixture $worktree '2026-01-06'
    Build-DateFixture 'linked worktree branch reference advances date' $worktree $worktreeTarget '2026-01-06'
    Assert-DateFixture ((Get-FileHash -LiteralPath (Join-Path $worktree '.git')).Hash -ceq $gitFileHash) 'The linked worktree .git file must stay unchanged during the commit.'
    $null = Invoke-FixtureProcess git $worktree @('diff', '--exit-code', $initialCommit, 'HEAD')
    Build-DateFixture 'explicit date precedes epoch and Git' $worktree $worktreeTarget '2040-02-03' 'one' @{VOXELY_BUILD_DATE='2040-02-03';SOURCE_DATE_EPOCH='0'}
    Build-DateFixture 'epoch precedes Git when explicit date is invalid' $worktree $worktreeTarget '1970-01-01' 'one' @{VOXELY_BUILD_DATE='invalid';SOURCE_DATE_EPOCH='0'}
    Build-DateFixture 'removing overrides restores Git date' $worktree $worktreeTarget '2026-01-06'
    Assert-DateFixture ((Get-FileHash -LiteralPath $BuildSupport).Hash -ceq $supportHash) 'Production build support changed during the fixture.'
} catch { $failure = $_ }
finally {
    [pscustomobject]@{status=$(if($failure){'FAIL'}else{'PASS'});buildSupport=$BuildSupport;buildSupportSha256=$supportHash;cases=$results.ToArray();error=$(if($failure){$failure.Exception.Message}else{$null})} |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidence 'results.json') -Encoding utf8
    $output = Join-Path $repo '.local/build-date-fix'
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $archive = Join-Path $output ('evidence-' + [guid]::NewGuid().ToString('N') + '.zip')
    Compress-Archive -LiteralPath $evidence -DestinationPath $archive
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try { Assert-DateFixture (@($zip.Entries | Where-Object FullName -match 'results\.json$').Count -eq 1) 'Build-date evidence archive is missing its results.' }
    finally { $zip.Dispose() }
    $absolute = [IO.Path]::GetFullPath($temporary)
    $parent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
    Assert-DateFixture ([IO.Path]::GetDirectoryName($absolute) -eq $parent -and [IO.Path]::GetFileName($absolute) -match '^voxely-build-date-[0-9a-f]{32}$') 'Unsafe fixture cleanup path.'
    Assert-DateFixture (@(Get-ChildItem -LiteralPath $absolute -Recurse -Force | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count -eq 0) 'Build-date fixture contains a reparse point.'
    Remove-Item -LiteralPath $absolute -Recurse -Force
}
Write-Host "Build-date evidence: $archive"
if ($failure) { throw $failure }
Write-Host "PASS: real Git/Cargo build-date invalidation ($($results.Count) cases)."
