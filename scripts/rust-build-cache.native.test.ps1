# Real Cargo interoperability in disposable, std-only Windows fixtures.
param([string]$Library = (Join-Path $PSScriptRoot 'rust-build-cache.ps1'))
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Native Cargo cache tests require Windows.' }
$repo = Split-Path -Parent $PSScriptRoot
$Library = (Resolve-Path -LiteralPath $Library).Path
. $Library
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('voxely-cache-native-' + [guid]::NewGuid().ToString('N'))
$evidence = Join-Path $temporary 'evidence'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$script:ownedCargo = [Collections.Generic.List[object]]::new()
$script:results = [Collections.Generic.List[object]]::new()
$script:removalHook = $null
$script:reverseCargo = $null
$script:reverseHookRan = $false
$savedEnvironment = @{}
foreach ($name in @('CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR', 'VOXELY_BUILD_CACHE_SESSION')) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    [Environment]::SetEnvironmentVariable($name, $null, 'Process')
}
function Assert-Native($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function New-NativeRoot([string]$Name) {
    $path = Join-Path $temporary $Name
    New-Item -ItemType Directory -Path (Join-Path $path 'src-tauri/target/release') -Force | Out-Null
    '{}' | Set-Content -LiteralPath (Join-Path $path 'package.json')
    'protected runtime' | Set-Content -LiteralPath (Join-Path $path 'src-tauri/target/release/voxely.exe')
    return $path
}
function Write-NativeSentinel([string]$Root) {
    $path = Join-Path $Root 'src-tauri/target/debug/incremental/retain-until-idle.txt'
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    'owned sentinel' | Set-Content -LiteralPath $path
    return $path
}
function Start-NativeCargo([string]$Name, [string]$Target, [string[]]$CargoArguments = @('build')) {
    $gate = Join-Path $temporary ('gate-' + $Name)
    New-Item -ItemType Directory -Path $gate -Force | Out-Null
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Command cargo -ErrorAction Stop).Source)
    $start.WorkingDirectory = $project
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    # CI can force ANSI color, but the fixture reads Cargo's lock status from stderr.
    foreach ($argument in @($CargoArguments) + @('--color', 'never', '--offline', '--locked', '--manifest-path', (Join-Path $project 'Cargo.toml'), '--target-dir', $Target)) {
        $start.ArgumentList.Add($argument)
    }
    foreach ($nameToRemove in @('CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR', 'VOXELY_BUILD_CACHE_SESSION', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER')) {
        $null = $start.Environment.Remove($nameToRemove)
    }
    $start.Environment['CARGO_HOME'] = Join-Path $temporary 'cargo-home'
    $start.Environment['VOXELY_NATIVE_GATE'] = $gate
    $pin = Select-String -LiteralPath (Join-Path $repo 'rust-toolchain.toml') -Pattern '^channel\s*=\s*"([^"]+)"'
    if ($pin) { $start.Environment['RUSTUP_TOOLCHAIN'] = $pin.Matches[0].Groups[1].Value }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    $stdoutPath = Join-Path $evidence ($Name + '.stdout.log')
    $stderrPath = Join-Path $evidence ($Name + '.stderr.log')
    $stdout = [IO.FileStream]::new($stdoutPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite, 1, $true)
    $stderr = [IO.FileStream]::new($stderrPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite, 1, $true)
    try { $null = $process.Start() } catch { $stdout.Dispose(); $stderr.Dispose(); $process.Dispose(); throw }
    $entry = [pscustomobject]@{
        Process=$process; Gate=$gate; Name=$Name; Stdout=$stdout; Stderr=$stderr;
        StdoutTask=$process.StandardOutput.BaseStream.CopyToAsync($stdout);
        StderrTask=$process.StandardError.BaseStream.CopyToAsync($stderr);
        StderrPath=$stderrPath; Closed=$false
    }
    $script:ownedCargo.Add($entry)
    return $entry
}
function Wait-NativeMarker($Entry, [string]$Marker, [ValidateRange(1,30)][int]$Seconds = 25) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while (-not (Test-Path -LiteralPath (Join-Path $Entry.Gate $Marker))) {
        if ($Entry.Process.HasExited) { throw "Cargo $($Entry.Name) exited before $Marker (exit $($Entry.Process.ExitCode))." }
        if ([DateTime]::UtcNow -ge $deadline) { throw "Cargo $($Entry.Name) timed out before $Marker." }
        Start-Sleep -Milliseconds 50
    }
}
function Release-NativeCargo($Entry) { [IO.File]::WriteAllText((Join-Path $Entry.Gate 'release'), 'release') }
function Complete-NativeCargo($Entry) {
    if (-not $Entry.Process.WaitForExit(30000)) { throw "Cargo $($Entry.Name) did not finish within 30 seconds." }
    if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($Entry.StdoutTask, $Entry.StderrTask), 5000)) { throw 'Cargo output did not drain.' }
    if (-not $Entry.Closed) { $Entry.Stdout.Dispose(); $Entry.Stderr.Dispose(); $Entry.Closed=$true }
    if ($Entry.Process.ExitCode -ne 0) { throw "Cargo $($Entry.Name) failed with exit $($Entry.Process.ExitCode)." }
}
function Wait-NativeCargoLock($Entry) {
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        if ($Entry.Process.HasExited) { throw "Cargo failed instead of waiting for the cleanup lock: $($Entry.Process.ExitCode)." }
        if (Test-Path -LiteralPath $Entry.StderrPath) {
            if ((Get-Content -LiteralPath $Entry.StderrPath -Raw) -match '(?i)blocking waiting for file lock') { return }
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'Cargo did not confirm waiting for the native profile lock.'
}
function Invoke-NativeCase([string]$Name, [scriptblock]$Action) {
    $started = [DateTime]::UtcNow
    try {
        & $Action
        $script:results.Add([pscustomobject]@{name=$Name;status='PASS';seconds=([DateTime]::UtcNow-$started).TotalSeconds})
    } catch {
        $script:results.Add([pscustomobject]@{name=$Name;status='FAIL';error=$_.Exception.Message;seconds=([DateTime]::UtcNow-$started).TotalSeconds})
        throw
    }
}
function Remove-Item {
    [CmdletBinding(DefaultParameterSetName='Literal')]
    param([Parameter(ParameterSetName='Literal')][string[]]$LiteralPath, [Parameter(ParameterSetName='Path')][string[]]$Path, [switch]$Recurse, [switch]$Force)
    if ($null -ne $script:removalHook) { & $script:removalHook $LiteralPath }
    Microsoft.PowerShell.Management\Remove-Item @PSBoundParameters
}
$failure = $null
$cleanupFailure = $null
$archive = $null
try {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class VoxelyNativeCacheFileId {
    [StructLayout(LayoutKind.Sequential)] struct Info {
        public uint Attributes; public System.Runtime.InteropServices.ComTypes.FILETIME Creation;
        public System.Runtime.InteropServices.ComTypes.FILETIME Access; public System.Runtime.InteropServices.ComTypes.FILETIME Write;
        public uint Volume; public uint SizeHigh; public uint SizeLow; public uint Links; public uint IndexHigh; public uint IndexLow;
    }
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle file, out Info info);
    public static string Read(string path) {
        using (var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete)) {
            Info info; if (!GetFileInformationByHandle(file.SafeFileHandle, out info)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            return info.Volume.ToString("X8") + ":" + info.IndexHigh.ToString("X8") + info.IndexLow.ToString("X8");
        }
    }
}
'@
    $project = Join-Path $temporary 'project'
    New-Item -ItemType Directory -Path (Join-Path $project 'src') -Force | Out-Null
    @'
[package]
name = "voxely_cache_native_fixture"
version = "0.0.0"
edition = "2021"
'@ | Set-Content -LiteralPath (Join-Path $project 'Cargo.toml')
    @'
version = 4
[[package]]
name = "voxely_cache_native_fixture"
version = "0.0.0"
'@ | Set-Content -LiteralPath (Join-Path $project 'Cargo.lock')
    'fn main() {}' | Set-Content -LiteralPath (Join-Path $project 'src/main.rs')
    @'
use std::{env, fs, path::PathBuf, thread, time::{Duration, Instant}};
fn main() {
    println!("cargo:rerun-if-env-changed=VOXELY_NATIVE_GATE");
    let gate = PathBuf::from(env::var_os("VOXELY_NATIVE_GATE").expect("test gate"));
    fs::write(gate.join("entered"), b"entered").unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    while !gate.join("release").exists() {
        assert!(Instant::now() < deadline, "fixture gate release timed out");
        thread::sleep(Duration::from_millis(20));
    }
}
'@ | Set-Content -LiteralPath (Join-Path $project 'build.rs')

    foreach ($freshScaffold in @($false, $true)) {
        Invoke-NativeCase "Cargo clean accepts retained nested target tags, freshManagedScaffold=$freshScaffold" {
            $caseName = "cache-tag-$freshScaffold"
            $root = New-NativeRoot $caseName
            $sentinel = Join-Path $root 'src-tauri/target/release/voxely.exe'
            $sentinelHash = (Get-FileHash -LiteralPath $sentinel).Hash
            $target = Join-Path $root 'src-tauri/target/llvm-cov/llvm-cov-target'
            $tag = Join-Path $target 'CACHEDIR.TAG'
            if ($freshScaffold) {
                Invoke-VoxelyBuildCache -RepoRoot $root -Action { }
                Assert-Native (Test-Path -LiteralPath (Join-Path $root 'src-tauri/target/debug/.cargo-lock')) 'Fresh managed cleanup must create its main target lock scaffold.'
                Assert-Native (-not (Test-Path -LiteralPath $target)) 'Fresh managed cleanup must leave the future nested Cargo target absent.'
            }
            $build = Start-NativeCargo ($caseName + '-build') $target
            Release-NativeCargo $build
            Complete-NativeCargo $build
            Assert-Native (Test-Path -LiteralPath $tag) 'Cargo did not create its own cache tag on the first build.'
            $tagHash = (Get-FileHash -LiteralPath $tag).Hash
            Invoke-VoxelyBuildCache -RepoRoot $root -Action { }
            $clean = Start-NativeCargo ($caseName + '-clean') $target @('clean', '--package', 'voxely_cache_native_fixture')
            Complete-NativeCargo $clean
            Assert-Native ((Get-FileHash -LiteralPath $tag).Hash -ceq $tagHash) 'Managed cleanup changed the Cargo-generated cache tag.'
            Assert-Native ((Get-FileHash -LiteralPath $sentinel).Hash -ceq $sentinelHash) 'Cleanup or cargo clean changed the daily-driver sentinel.'
        }
    }

    Invoke-NativeCase 'foreign Cargo target does not block Voxely cleanup' {
        $root = New-NativeRoot 'foreign-case'
        $sentinel = Write-NativeSentinel $root
        $cargo = Start-NativeCargo 'foreign' (Join-Path $temporary 'foreign-target')
        try {
            Wait-NativeMarker $cargo 'entered'
            $timer = [Diagnostics.Stopwatch]::StartNew()
            Invoke-VoxelyBuildCache -RepoRoot $root -Action { }
            Assert-Native ($timer.Elapsed.TotalSeconds -lt 10) 'Foreign Cargo delayed unrelated cleanup.'
            Assert-Native (-not $cargo.Process.HasExited) 'Foreign build must still be held while cleanup finishes.'
            Assert-Native (-not (Test-Path -LiteralPath $sentinel)) 'Own incremental artifacts were not cleaned.'
        } finally { Release-NativeCargo $cargo }
        Complete-NativeCargo $cargo
    }

    Invoke-NativeCase 'same target Cargo prevents deletion and preserves lock identity' {
        $root = New-NativeRoot 'shared-case'
        $target = Join-Path $root 'src-tauri/target'
        $cargo = Start-NativeCargo 'shared' $target
        try {
            Wait-NativeMarker $cargo 'entered'
            $sentinel = Write-NativeSentinel $root
            $lockPath = Join-Path $target 'debug/.cargo-lock'
            $identity = [VoxelyNativeCacheFileId]::Read($lockPath)
            $message = ''
            try { Invoke-VoxelyBuildCache -RepoRoot $root -Action { throw 'Busy action must not execute.' } }
            catch { $message = $_.Exception.Message }
            Assert-Native ($message -match 'Build cache is busy') "Same-target cleanup was not rejected by its lock: $message"
            Assert-Native ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ceq 'owned sentinel') 'Busy cleanup modified a protected artifact.'
            $timedOut = $false
            try { Assert-VoxelyCacheIdle -Cache (Get-VoxelyBuildCache -RepoRoot $root) -WaitForBuilders -TimeoutSeconds 0 }
            catch { $timedOut = $_.Exception.Message -match 'Build cache is busy' }
            Assert-Native $timedOut 'Same-target bounded wait did not reject the busy lock.'
        } finally { Release-NativeCargo $cargo }
        Complete-NativeCargo $cargo
        Invoke-VoxelyBuildCache -RepoRoot $root -Full -Action { }
        Assert-Native (-not (Test-Path -LiteralPath $sentinel)) 'Released profile artifacts were not evicted.'
        Assert-Native ([VoxelyNativeCacheFileId]::Read($lockPath) -ceq $identity) 'Cleanup replaced the Cargo lock file.'
        Assert-Native (Test-Path -LiteralPath (Join-Path $target 'release/voxely.exe')) 'Cleanup removed the daily-driver sentinel.'
    }

    Invoke-NativeCase 'Cargo waits for the held cleanup lock and resumes through the same file' {
        $root = New-NativeRoot 'reverse-case'
        Invoke-VoxelyBuildCache -RepoRoot $root -Action {
            param($cache)
            $sentinel = Write-NativeSentinel $root
            $incremental = Split-Path -Parent $sentinel
            $lockPath = Join-Path $cache.Target 'debug/.cargo-lock'
            $identity = [VoxelyNativeCacheFileId]::Read($lockPath)
            $script:removalHook = {
                param($paths)
                if (-not $script:reverseHookRan -and @($paths | Where-Object { $_ -eq $incremental -or $_.StartsWith($incremental + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) }).Count) {
                    $script:reverseHookRan = $true
                    $script:reverseCargo = Start-NativeCargo 'reverse' $cache.Target
                    Wait-NativeCargoLock $script:reverseCargo
                    Assert-Native (-not (Test-Path -LiteralPath (Join-Path $script:reverseCargo.Gate 'entered'))) 'Cargo entered build.rs while cleanup still held its profile lock.'
                }
            }
            try { Repair-VoxelyBuildCache -Cache $cache }
            finally { $script:removalHook = $null }
            Assert-Native $script:reverseHookRan 'The cleanup deletion hook did not execute.'
            Assert-Native ([VoxelyNativeCacheFileId]::Read($lockPath) -ceq $identity) 'A waiting Cargo would resume through a replaced lock.'
            try { Wait-NativeMarker $script:reverseCargo 'entered' }
            finally { Release-NativeCargo $script:reverseCargo }
            Complete-NativeCargo $script:reverseCargo
        }
    }

    Invoke-NativeCase 'new unlocked custom and target-triple profiles survive the lock snapshot' {
        $root = New-NativeRoot 'new-profile-case'
        Invoke-VoxelyBuildCache -RepoRoot $root -Action {
            param($cache)
            $owned = Join-Path $cache.Target 'llvm-cov'
            $oldArtifact = Join-Path $owned 'debug/old.rlib'
            New-Item -ItemType Directory -Path (Split-Path -Parent $oldArtifact) -Force | Out-Null
            'old owned artifact' | Set-Content -LiteralPath $oldArtifact
            $newProfiles = @('custom-profile/deps/retain.rlib', 'x86_64-fixture/debug/deps/retain.rlib', 'debug/deps/nested-target/debug/deps/retain.rlib') |
                ForEach-Object { Join-Path $owned $_ }
            $locks = Enter-VoxelyCargoLocks -Cache $cache
            try {
                # The actual production lock snapshot is already held. A newly
                # created profile must not borrow protection from an ancestor.
                foreach ($artifact in $newProfiles) {
                    New-Item -ItemType Directory -Path (Split-Path -Parent $artifact) -Force | Out-Null
                    'new unlocked artifact' | Set-Content -LiteralPath $artifact
                }
                $nestedLock = Join-Path $owned 'debug/deps/nested-target/debug/.cargo-lock'
                [IO.File]::WriteAllText($nestedLock, '')
                Assert-Native ($nestedLock -notin $locks.Paths) 'The new nested profile was unexpectedly part of the original lock snapshot.'
                Remove-VoxelyCacheDirectory -Cache $cache -Path $owned -CargoLocks $locks
                Assert-Native (-not (Test-Path -LiteralPath $oldArtifact)) 'The covered profile was not cleaned.'
                Assert-Native (Test-Path -LiteralPath $nestedLock) 'Cleanup removed a new nested Cargo lock.'
                foreach ($artifact in $newProfiles) {
                    Assert-Native ((Get-Content -LiteralPath $artifact -Raw).Trim() -ceq 'new unlocked artifact') 'Cleanup removed a profile absent from its lock snapshot.'
                }
            } finally { Exit-VoxelyCargoLocks $locks }
        }
    }
} catch { $failure = $_ }
finally {
    $script:removalHook = $null
    foreach ($entry in $script:ownedCargo) {
        try {
            Release-NativeCargo $entry
            if (-not $entry.Process.WaitForExit(30000)) { $entry.Process.Kill($true); if (-not $entry.Process.WaitForExit(5000)) { throw 'Owned fixture Cargo could not be stopped.' } }
            $null = [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($entry.StdoutTask, $entry.StderrTask), 5000)
            if (-not $entry.Closed) { $entry.Stdout.Dispose(); $entry.Stderr.Dispose(); $entry.Closed=$true }
            $entry.Process.Dispose()
        } catch { if ($null -eq $cleanupFailure) { $cleanupFailure = $_ } }
    }
    foreach ($name in $savedEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process') }
    try {
        foreach ($log in @(Get-ChildItem -LiteralPath $evidence -File -Filter '*.log')) {
            if ($log.Length -gt 65536) {
                $logStream = [IO.File]::OpenRead($log.FullName)
                try { $null=$logStream.Seek(-65536,[IO.SeekOrigin]::End); $tail=[byte[]]::new(65536); $null=$logStream.Read($tail,0,$tail.Length) }
                finally { $logStream.Dispose() }
                [IO.File]::WriteAllBytes($log.FullName, $tail)
            }
        }
        [pscustomobject]@{status=$(if($failure -or $cleanupFailure){'FAIL'}else{'PASS'}); library=$Library; cases=$script:results.ToArray(); error=$(if($failure){$failure.Exception.Message}else{$null}); cleanupError=$(if($cleanupFailure){$cleanupFailure.Exception.Message}else{$null})} |
            ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $evidence 'results.json') -Encoding utf8
        $output = Join-Path $repo '.local/cache-isolation'
        New-Item -ItemType Directory -Path $output -Force | Out-Null
        $archive = Join-Path $output ('native-evidence-' + [guid]::NewGuid().ToString('N') + '.zip')
        Compress-Archive -LiteralPath $evidence -DestinationPath $archive
        $zip = [IO.Compression.ZipFile]::OpenRead($archive)
        try { Assert-Native (@($zip.Entries | Where-Object FullName -match 'results\.json$').Count -eq 1) 'Evidence archive is missing its results.' }
        finally { $zip.Dispose() }
        if ($null -eq $cleanupFailure) {
            $absolute = [IO.Path]::GetFullPath($temporary)
            $parent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
            if ([IO.Path]::GetDirectoryName($absolute) -ne $parent -or [IO.Path]::GetFileName($absolute) -notmatch '^voxely-cache-native-[0-9a-f]{32}$') { throw 'Unsafe fixture cleanup path.' }
            if (@(Get-ChildItem -LiteralPath $absolute -Recurse -Force | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) { throw 'Fixture cleanup refused a reparse point.' }
            Microsoft.PowerShell.Management\Remove-Item -LiteralPath $absolute -Recurse -Force
        }
    } catch { if ($null -eq $cleanupFailure) { $cleanupFailure = $_ } }
}
if ($archive) { Write-Host "Native Cargo evidence: $archive" }
if ($failure) { throw $failure }
if ($cleanupFailure) { throw $cleanupFailure }
Write-Host 'PASS: real foreign/shared Cargo targets, cleanup lock interoperability and lock-file identity.'
