# Disposable fixtures only. No real target deletion or compilation in this test.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'rust-build-cache.ps1')
$script:actualCacheContext = ${function:Get-VoxelyBuildCache}
$script:actualOpenCargoLock = ${function:Open-VoxelyCargoLock}
$script:actualCargoLockPaths = ${function:Get-VoxelyCargoLockPaths}
$script:budget = 1MB
$script:free = 100GB
$script:busy = $false
$script:busyChecksRemaining = 0
$script:idleChecksBeforeNewBuilder = -1
$script:busySentinel = $null
$script:onBusyCheck = $null
$script:forbidSleep = $false
$script:sleepCount = 0
$script:runningApp = $null
$script:assertions = 0
function Get-VoxelyBuildCache {
    param([string]$RepoRoot)
    $context = & $script:actualCacheContext -RepoRoot $RepoRoot
    $context.BudgetBytes = $script:budget
    $context.ReserveBytes = 10GB
    return $context
}
function Get-VoxelyCacheFreeBytes { param($Cache) return $script:free }
function Get-Process {
    param([string[]]$Name, [int]$Id, $ErrorAction)
    if ($PSBoundParameters.ContainsKey('Id')) { return Microsoft.PowerShell.Management\Get-Process -Id $Id -ErrorAction Stop }
    if ('cargo' -in $Name -or 'rustc' -in $Name -or 'link' -in $Name) { throw 'Cache maintenance must not scan global compiler names' }
    if ('voxely' -in $Name -and $null -ne $script:runningApp) { return $script:runningApp }
}
function Get-VoxelyCargoLockPaths {
    param($Cache)
    $paths = @(& $script:actualCargoLockPaths -Cache $Cache)
    $script:firstLockOfAttempt = $true
    if ($script:idleChecksBeforeNewBuilder -ge 0) {
        if ($script:idleChecksBeforeNewBuilder -eq 0) {
            $script:busyChecksRemaining = 2
            $script:idleChecksBeforeNewBuilder = -1
        } else { $script:idleChecksBeforeNewBuilder-- }
    }
    return $paths
}
function Open-VoxelyCargoLock {
    param([string]$Path)
    if ($script:firstLockOfAttempt) {
        $script:firstLockOfAttempt = $false
        if ($script:busy) { throw [IO.IOException]::new('fixture-owned-profile-lock-busy', [int]0x80070021) }
        if ($script:busyChecksRemaining -gt 0) {
        Assert-Cache (Test-Path -LiteralPath $script:busySentinel) 'Cleanup must retain artifacts while this target profile is locked'
        if ($null -ne $script:onBusyCheck) { & $script:onBusyCheck }
        $script:busyChecksRemaining--
        throw [IO.IOException]::new('fixture-owned-profile-lock-busy', [int]0x80070021)
        }
    }
    return & $script:actualOpenCargoLock -Path $Path
}
function Start-Sleep {
    param([int]$Milliseconds)
    $script:sleepCount++
    if ($script:forbidSleep) { throw 'Unexpected wait after an existing failure' }
    Microsoft.PowerShell.Utility\Start-Sleep -Milliseconds $Milliseconds
}
function Assert-Cache($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
    $script:assertions++
}
function Expect-CacheFailure([scriptblock]$Action, [string]$Pattern) {
    $message = ''
    try { & $Action | Out-Null } catch { $message = $_.Exception.Message }
    Assert-Cache ($message -match $Pattern) "Expected '$Pattern', got '$message'"
}
function New-CacheFixture {
    $path = Join-Path $script:temporary ([guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $path | Out-Null
    '{}' | Set-Content -LiteralPath (Join-Path $path 'package.json')
    return $path
}
function Write-CacheFixture([string]$Root, [string]$Relative, [int]$Bytes = 16) {
    $file = Join-Path $Root "src-tauri/target/$Relative"
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $file) | Out-Null
    [IO.File]::WriteAllBytes($file, [byte[]]::new($Bytes))
    return $file
}
function Get-CacheArtifactCount([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    return @(Get-ChildItem -LiteralPath $Path -File -Recurse -Force | Where-Object Name -ne '.cargo-lock').Count
}
function Get-TemporaryArtifactCount([string]$Root, [string]$Pattern) {
    $count = 0
    foreach ($directory in @(Get-ChildItem -LiteralPath (Join-Path $Root 'src-tauri/target') -Directory -Filter $Pattern)) {
        $count += Get-CacheArtifactCount $directory.FullName
    }
    return $count
}
$script:temporary = Join-Path ([IO.Path]::GetTempPath()) ('voxely-cache-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $script:temporary | Out-Null
$savedEnvironment = @{}
foreach ($name in @('CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR', 'VOXELY_BUILD_CACHE_SESSION')) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    [Environment]::SetEnvironmentVariable($name, $null, 'Process')
}
try {
    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    $dll = Write-CacheFixture $fixture 'release/runtime.dll'
    $unknown = Write-CacheFixture $fixture 'notes/owner-data.txt'
    $debug = Write-CacheFixture $fixture 'debug/deps/current.rlib'
    foreach ($name in @('msrv-1.90', ('msrv-1.90.0-' + ('a' * 32)), ('llvm-cov-' + ('b' * 32)), 'llvm-cov', 'local-fix-verification')) {
        $null = Write-CacheFixture $fixture "$name/debug/deps/temp.rlib"
    }
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        param($cache)
        Assert-Cache (Test-VoxelyBuildCacheSession $cache) 'Lease must be valid inside action'
        Assert-Cache (Test-Path -LiteralPath $debug) 'Under-budget cache must remain'
        Assert-Cache ((Get-CacheArtifactCount (Join-Path $cache.Target 'msrv-1.90')) -eq 0) 'MSRV artifacts should be cleaned before action'
        $null = Write-CacheFixture $fixture ('llvm-cov-' + ('c' * 32) + '/debug/temporary.rlib')
    }
    Assert-Cache (Test-Path $exe) 'Runtime executable must remain'
    Assert-Cache (Test-Path $dll) 'Adjacent runtime DLL must remain'
    Assert-Cache (Test-Path $unknown) 'Unknown data must remain'
    Assert-Cache ((Get-TemporaryArtifactCount $fixture 'llvm-cov*') -eq 0) 'Post-build temporary artifacts must be removed while Cargo lock scaffolds remain'

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    $instrumented = Join-Path $fixture 'src-tauri/target/llvm-cov'
    $outsideInstrumentation = Write-CacheFixture $fixture 'owner-data/keep.profraw' 64
    $outsideInstrumentationHash = (Get-FileHash -LiteralPath $outsideInstrumentation).Hash
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        # Real cargo-llvm-cov keeps its profile below llvm-cov-target and its
        # instrumentation/metadata files beside that profile, not inside it.
        $null = Write-CacheFixture $fixture 'llvm-cov/llvm-cov-target/debug/.cargo-lock' 0
        foreach ($artifact in @(
            'llvm-cov/top-level.profraw', 'llvm-cov/top-level.profdata', 'llvm-cov/top-level.json',
            'llvm-cov/llvm-cov-target/unit-1.profraw', 'llvm-cov/llvm-cov-target/unit-2.profraw',
            'llvm-cov/llvm-cov-target/unit-3.profraw', 'llvm-cov/llvm-cov-target/merged.profdata',
            'llvm-cov/llvm-cov-target/coverage.json', 'llvm-cov/llvm-cov-target/.rustc_info.json',
            'llvm-cov/llvm-cov-target/debug/deps/fixture.rlib',
            'llvm-cov/llvm-cov-target/debug/.fingerprint/fixture/hash'
        )) { $null = Write-CacheFixture $fixture $artifact 64 }
    }
    Assert-Cache ((Get-CacheArtifactCount $instrumented) -eq 0) 'Nested coverage cleanup must remove profraw/profdata and metadata beside the locked profile'
    $instrumentedLocks = @(Get-ChildItem -LiteralPath $instrumented -File -Recurse -Force)
    Assert-Cache ($instrumentedLocks.Count -ge 3 -and @($instrumentedLocks | Where-Object { $_.Name -ne '.cargo-lock' -or $_.Length -ne 0 }).Count -eq 0) 'Nested coverage cleanup must leave only zero-length Cargo locks'
    Assert-Cache (Test-Path -LiteralPath $exe) 'Nested instrumentation cleanup must preserve the daily driver'
    Assert-Cache ((Get-FileHash -LiteralPath $outsideInstrumentation).Hash -ceq $outsideInstrumentationHash) 'Cleanup must preserve same-extension files outside its private temporary namespace'
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        $null = Write-CacheFixture $fixture 'llvm-cov/llvm-cov-target/next-run.profraw' 64
    }
    Assert-Cache ((Get-CacheArtifactCount $instrumented) -eq 0) 'Reusing the bounded coverage namespace must not accumulate parent instrumentation files'

    $fixture = New-CacheFixture
    $null = Write-CacheFixture $fixture 'llvm-cov/llvm-cov-target/debug/.cargo-lock' 0
    $unknown = Write-CacheFixture $fixture 'llvm-cov/llvm-cov-target/unrelated-owner/sentinel.bin' 32
    $unknownHash = (Get-FileHash -LiteralPath $unknown).Hash
    $raw = Write-CacheFixture $fixture 'llvm-cov/llvm-cov-target/owned.profraw' 64
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { }
    Assert-Cache (-not (Test-Path -LiteralPath $raw)) 'Known coverage namespace files must be reclaimed beside unknown directories'
    Assert-Cache ((Get-FileHash -LiteralPath $unknown).Hash -ceq $unknownHash) 'Parent instrumentation cleanup must preserve unknown subdirectories unchanged'

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    $null = Write-CacheFixture $fixture 'debug/deps/large.rlib' 8192
    $script:budget = 4096
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { }
    Assert-Cache ((Get-CacheArtifactCount (Join-Path $fixture 'src-tauri/target/debug')) -eq 0) 'Over-budget debug artifacts must be evicted coherently'
    Assert-Cache (Test-Path -LiteralPath (Join-Path $fixture 'src-tauri/target/debug/.cargo-lock')) 'Debug eviction must preserve the Cargo lock identity'
    Assert-Cache (Test-Path $exe) 'Budget eviction must preserve exe'

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    $null = Write-CacheFixture $fixture 'release/deps/large.rlib' 8192
    $null = Write-CacheFixture $fixture 'release/.fingerprint/library/hash'
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { }
    Assert-Cache (-not (Test-Path (Join-Path $fixture 'src-tauri/target/release/deps'))) 'Release intermediates should be evicted'
    Assert-Cache (-not (Test-Path (Join-Path $fixture 'src-tauri/target/release/.fingerprint'))) 'Release fingerprints must be evicted with dependencies'
    Assert-Cache (Test-Path $exe) 'Release eviction must preserve exe'
    $script:budget = 1MB

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    Expect-CacheFailure {
        Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
            $null = Write-CacheFixture $fixture ('llvm-cov-' + ('d' * 32) + '/debug/failure.rlib')
            throw 'fixture-command-exit-23'
        }
    } 'fixture-command-exit-23'
    Assert-Cache ((Get-TemporaryArtifactCount $fixture 'llvm-cov*') -eq 0) 'Failure must still clean temporary artifacts'
    Assert-Cache (-not $env:VOXELY_BUILD_CACHE_SESSION -and -not $env:CARGO_TARGET_DIR) 'Environment must restore after failure'
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { }

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        param($cache)
        $script:busy = $true
        try {
            Expect-CacheFailure { Repair-VoxelyBuildCache $cache } 'busy'
            Expect-CacheFailure { Assert-VoxelyCacheIdle $cache -WaitForBuilders -TimeoutSeconds 0 } 'busy'
            Assert-Cache (Test-Path -LiteralPath $exe) 'Expired wait must preserve the runtime executable'
        }
        finally { $script:busy = $false }
        $script:runningApp = [pscustomobject]@{Path=(Join-Path $cache.Target 'debug/voxely.exe');Id=321}
        try { Expect-CacheFailure { Repair-VoxelyBuildCache $cache } 'cache executable is running' }
        finally { $script:runningApp = $null }
        $script:runningApp = [pscustomobject]@{Path=$exe;Id=322}
        try { Repair-VoxelyBuildCache $cache } finally { $script:runningApp = $null }
        Expect-CacheFailure { Remove-VoxelyCacheDirectory $cache $exe } 'Not an owned'
        Expect-CacheFailure { Remove-VoxelyCacheDirectory $cache (Join-Path $fixture 'outside') } 'Not an owned'
    }

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        param($cache)
        $script:busySentinel = Write-CacheFixture $fixture ('llvm-cov-' + ('f' * 32) + '/debug/retain-until-idle.rlib')
        $script:busyChecksRemaining = 3
    }
    Assert-Cache ($script:busyChecksRemaining -eq 0) 'Post-build cleanup must wait for transient compiler activity'
    Assert-Cache (-not (Test-Path -LiteralPath $script:busySentinel)) 'Post-build cleanup must reclaim owned temporary files after compilers exit'
    Assert-Cache (Test-Path -LiteralPath $exe) 'Waiting cleanup must preserve the daily driver'
    $script:busySentinel = $null

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        param($cache)
        $owned = Join-Path $cache.Target ('llvm-cov-' + ('a' * 32))
        $script:busySentinel = Write-CacheFixture $fixture ('llvm-cov-' + ('a' * 32) + '/debug/new-builder.rlib')
        Assert-VoxelyCacheIdle -Cache $cache -WaitForBuilders
        # A new external builder starts after the caller's successful idle check.
        $script:busyChecksRemaining = 2
        $waitsBeforeRemoval = $script:sleepCount
        Remove-VoxelyCacheDirectory -Cache $cache -Path $owned -WaitForBuilders
        Assert-Cache ($script:sleepCount -eq $waitsBeforeRemoval + 2) 'Removal recheck must wait for a newly started builder'
        Assert-Cache (-not (Test-Path -LiteralPath $script:busySentinel)) 'Removal must reclaim its own artifacts only after the new builder exits'

        $script:busySentinel = Write-CacheFixture $fixture ('llvm-cov-' + ('b' * 32) + '/debug/new-repair-builder.rlib')
        # Repair holds one native lock set throughout all of its deletions.
        $script:busyChecksRemaining = 2
        $waitsBeforeRepair = $script:sleepCount
        Repair-VoxelyBuildCache -Cache $cache -WaitForBuilders
        Assert-Cache ($script:sleepCount -eq $waitsBeforeRepair + 2) 'Repair must wait for this target profile before acquiring the lock set for all deletions'
        Assert-Cache ($script:busyChecksRemaining -eq 0) 'Repair must observe the complete delayed builder lifecycle'
        Assert-Cache (-not (Test-Path -LiteralPath $script:busySentinel)) 'Repair must not delete artifacts before its new builder becomes idle'
    }
    Assert-Cache (Test-Path -LiteralPath $exe) 'Delayed-builder cleanup must preserve the daily driver'
    $script:busySentinel = $null

    $fixture = New-CacheFixture
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        param($cache)
        $owned = Join-Path $cache.Target ('llvm-cov-' + ('c' * 32))
        $script:busySentinel = Write-CacheFixture $fixture ('llvm-cov-' + ('c' * 32) + '/debug/recheck-tree.rlib')
        $script:busyJunction = Join-Path $owned 'new-junction'
        $script:busyJunctionTarget = Join-Path $fixture 'outside-cache'
        New-Item -ItemType Directory -Path $script:busyJunctionTarget | Out-Null
        $outsideSentinel = Join-Path $script:busyJunctionTarget 'preserve.txt'
        'outside sentinel' | Set-Content -LiteralPath $outsideSentinel
        $script:busyChecksRemaining = 2
        $script:onBusyCheck = {
            New-Item -ItemType Junction -Path $script:busyJunction -Target $script:busyJunctionTarget | Out-Null
            $script:onBusyCheck = $null
        }
        try {
            Expect-CacheFailure { Remove-VoxelyCacheDirectory -Cache $cache -Path $owned -WaitForBuilders } 'reparse point'
            Assert-Cache (Test-Path -LiteralPath $script:busySentinel) 'A reparse point created during waiting must reject deletion of the owned tree'
            Assert-Cache ((Get-Content -LiteralPath $outsideSentinel) -eq 'outside sentinel') 'The new junction destination must remain untouched'
        } finally {
            $script:onBusyCheck = $null
            if (Test-Path -LiteralPath $script:busyJunction) { [IO.Directory]::Delete($script:busyJunction) }
        }
    }
    $script:busySentinel = $null

    $fixture = New-CacheFixture
    $null = Write-CacheFixture $fixture 'release/voxely.exe'
    $script:forbidSleep = $true
    $script:sleepCount = 0
    $script:busy = $true
    try {
        Expect-CacheFailure { Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { throw 'Rejected action must not start' } } 'Build cache is busy'
        Assert-Cache ($script:sleepCount -eq 0) 'Initial busy rejection must not enter post-build waiting'
    } finally { $script:busy = $false }
    try {
        Expect-CacheFailure {
            Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
                $script:busy = $true
                throw 'primary-coverage-timeout'
            }
        } 'primary-coverage-timeout'
        Assert-Cache ($script:sleepCount -eq 0) 'A failed action must not restart the cleanup wait'
    } finally { $script:busy = $false; $script:forbidSleep = $false }

    $fixture = New-CacheFixture
    $protectedData = Write-CacheFixture $fixture 'owner-data/large.bin' 8192
    $script:budget = 4096
    Expect-CacheFailure { Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { throw 'Action must not execute' } } 'protected or unrecognized'
    Assert-Cache (Test-Path $protectedData) 'Unknown over-budget data must not be deleted'
    $script:budget = 1MB
    $script:free = 1GB
    Expect-CacheFailure { Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { throw 'Action must not execute' } } 'Less than 10 GiB free'
    $script:free = 100GB

    $fixture = New-CacheFixture
    $outside = Join-Path $script:temporary 'outside'
    New-Item -ItemType Directory -Path $outside | Out-Null
    $sentinel = Join-Path $outside 'sentinel.txt'
    'preserve' | Set-Content $sentinel
    $null = Write-CacheFixture $fixture 'debug/deps/current.rlib'
    $junction = Join-Path $fixture 'src-tauri/target/debug/foreign'
    New-Item -ItemType Junction -Path $junction -Target $outside | Out-Null
    try {
        Expect-CacheFailure { Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { throw 'Action must not execute' } } 'reparse point'
        Assert-Cache ((Get-Content $sentinel) -eq 'preserve') 'Junction destination must remain unchanged'
    } finally { [IO.Directory]::Delete($junction) }

    $fixture = New-CacheFixture
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        param($cache)
        Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { param($nested) Assert-Cache (Test-VoxelyBuildCacheSession $nested) 'Nested build must reuse held lease' }
        $savedSession = $env:VOXELY_BUILD_CACHE_SESSION
        $env:VOXELY_BUILD_CACHE_SESSION = $null
        try { Expect-CacheFailure { Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { throw 'Action must not execute' } } 'Another managed build' }
        finally { $env:VOXELY_BUILD_CACHE_SESSION = $savedSession }
    }

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    $dll = Write-CacheFixture $fixture 'release/required.dll'
    $null = Write-CacheFixture $fixture 'debug/deps/current.rlib'
    $null = Write-CacheFixture $fixture 'release/deps/current.rlib'
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Full -Action { }
    Assert-Cache (Test-Path $exe) 'Full maintenance must preserve runtime exe'
    Assert-Cache (Test-Path $dll) 'Full maintenance must preserve runtime DLLs'
    Assert-Cache ((Get-CacheArtifactCount (Join-Path $fixture 'src-tauri/target/debug')) -eq 0) 'Full maintenance must evict debug artifacts'
    Assert-Cache (Test-Path -LiteralPath (Join-Path $fixture 'src-tauri/target/debug/.cargo-lock')) 'Full maintenance must preserve the Cargo lock scaffold'

    $fixture = New-CacheFixture
    $null = Write-CacheFixture $fixture 'release/voxely.exe'
    $externalFile = Join-Path $script:temporary 'external-file.txt'
    'never truncate' | Set-Content $externalFile
    $lockFile = Join-Path $fixture 'src-tauri/target/.voxely-cache.lock'
    New-Item -ItemType HardLink -Path $lockFile -Target $externalFile | Out-Null
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { }
    Assert-Cache ((Get-Content $externalFile) -eq 'never truncate') 'Hardlinked lock must never be truncated or written'

    $fixture = New-CacheFixture
    $null = Write-CacheFixture $fixture 'release/voxely.exe'
    $lockJunction = Join-Path $fixture 'src-tauri/target/.voxely-cache.lock'
    New-Item -ItemType Junction -Path $lockJunction -Target $outside | Out-Null
    try {
        Expect-CacheFailure { Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { throw 'Action must not execute' } } 'Invalid cache lock'
        Assert-Cache ((Get-Content $sentinel) -eq 'preserve') 'Lock reparse destination must be preserved'
    } finally { [IO.Directory]::Delete($lockJunction) }

    $fixture = New-CacheFixture
    New-Item -ItemType Directory -Path (Join-Path $fixture 'src-tauri') | Out-Null
    $targetJunction = Join-Path $fixture 'src-tauri/target'
    New-Item -ItemType Junction -Path $targetJunction -Target $outside | Out-Null
    try {
        Expect-CacheFailure { Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { throw 'Action must not execute' } } 'Reparse point in cache path'
        Assert-Cache (-not (Test-Path (Join-Path $outside '.voxely-cache.lock'))) 'Target junction must be rejected before creating metadata'
    } finally { [IO.Directory]::Delete($targetJunction) }

    $fixture = New-CacheFixture
    $null = Write-CacheFixture $fixture 'release/voxely.exe'
    $cache = Get-VoxelyBuildCache $fixture
    [IO.File]::WriteAllText($cache.Lock, '')
    $staleToken = 'e' * 32
    $staleRecord = Join-Path $cache.Target ".voxely-cache-session-$staleToken.json"
    @{token=$staleToken;target=$cache.Target;pid=$PID;startTicks=(Microsoft.PowerShell.Management\Get-Process -Id $PID).StartTime.ToUniversalTime().Ticks} | ConvertTo-Json | Set-Content $staleRecord
    $env:VOXELY_BUILD_CACHE_SESSION = $staleToken
    Assert-Cache (-not (Test-VoxelyBuildCacheSession $cache)) 'A live PID without a held write handle must not be a lease'
    $differentLease = [IO.File]::Open($cache.Lock, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read)
    try {
        Assert-Cache (-not (Test-VoxelyBuildCacheSession $cache)) 'A stale token must be rejected when a different session holds the global lock'
    } finally { $differentLease.Dispose() }
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { }
    Assert-Cache (-not (Test-Path $staleRecord)) 'Next run must remove stale session metadata'
    $env:VOXELY_BUILD_CACHE_SESSION = $null

    $fixture = New-CacheFixture
    $script:budget = 4096
    Expect-CacheFailure {
        Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
            $null = Write-CacheFixture $fixture 'unknown/retain.bin' 8192
            throw 'primary-child-failure'
        }
    } 'primary-child-failure'
    Assert-Cache (Test-Path (Join-Path $fixture 'src-tauri/target/unknown/retain.bin')) 'Post-cleanup failure must preserve unknown data and original error'
    $script:budget = 1MB

    $fixture = New-CacheFixture
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action {
        param($cache)
        $child = Join-Path $fixture 'child.ps1'
        @'
param($Library, $Root)
$ErrorActionPreference = 'Stop'
. $Library
Invoke-VoxelyBuildCache -RepoRoot $Root -Action { param($childCache) if (-not (Test-VoxelyBuildCacheSession $childCache)) { throw 'Missing child lease' } }
'@ | Set-Content $child
        & pwsh -NoProfile -File $child -Library (Join-Path $PSScriptRoot 'rust-build-cache.ps1') -Root $fixture
        Assert-Cache ($LASTEXITCODE -eq 0) 'Separate child process must inherit and reuse the real held lease'
        $savedSession = $env:VOXELY_BUILD_CACHE_SESSION
        $env:VOXELY_BUILD_CACHE_SESSION = $null
        try {
            & pwsh -NoProfile -File $child -Library (Join-Path $PSScriptRoot 'rust-build-cache.ps1') -Root $fixture 2>$null
            Assert-Cache ($LASTEXITCODE -ne 0) 'Separate process without token must be denied while lease is held'
        } finally { $env:VOXELY_BUILD_CACHE_SESSION = $savedSession }
    }

    $fixture = New-CacheFixture
    $fixtureScripts = Join-Path $fixture 'scripts'
    New-Item -ItemType Directory -Path $fixtureScripts | Out-Null
    foreach ($name in @('rust-build-cache.ps1', 'check-msrv.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $fixtureScripts }
    Add-Content -LiteralPath (Join-Path $fixtureScripts 'rust-build-cache.ps1') -Value @'
$script:fixtureOpenCargoLock = ${function:Open-VoxelyCargoLock}
function Open-VoxelyCargoLock {
    param([string]$Path)
    if ($global:msrvWasCalled -and $global:msrvBusyChecks -gt 0) {
        if (-not (Test-Path -LiteralPath $global:msrvArtifact)) { throw 'MSRV deleted artifacts while its profile was locked' }
        $global:msrvBusyChecks--
        throw [IO.IOException]::new('fixture-owned-msrv-profile-busy', [int]0x80070021)
    }
    return & $script:fixtureOpenCargoLock -Path $Path
}
'@
    New-Item -ItemType Directory -Path (Join-Path $fixture 'src-tauri') | Out-Null
    'rust-version = "1.90.0"' | Set-Content (Join-Path $fixture 'src-tauri/Cargo.toml')
    $child = Join-Path $fixture 'fake-msrv.ps1'
    @'
param($Entry, [ValidateSet('failure', 'busy-failure', 'success')][string]$Scenario = 'failure')
$ErrorActionPreference = 'Stop'
$global:msrvWasCalled = $false
$global:msrvSleepCount = 0
$global:msrvBusyChecks = 0
$global:msrvArtifact = $null
function Get-Process {
    param([string[]]$Name, [int]$Id, $ErrorAction)
    if ($PSBoundParameters.ContainsKey('Id')) { return Microsoft.PowerShell.Management\Get-Process -Id $Id -ErrorAction Stop }
    if ('cargo' -in $Name) { throw 'MSRV must not scan global compilers' }
}
function Start-Sleep {
    $global:msrvSleepCount++
    if ($Scenario -ne 'success') { throw 'Failed MSRV must not restart the cleanup wait' }
}
function cargo {
    if ($args[0] -ne '+1.90.0' -or $args[1] -ne 'build' -or '--locked' -notin $args -or '--all-targets' -notin $args) { throw 'Wrong MSRV arguments' }
    if ([IO.Path]::GetFileName($env:CARGO_TARGET_DIR) -cne 'msrv-1.90.0') { throw 'MSRV target was not isolated in its bounded namespace' }
    New-Item -ItemType Directory -Path $env:CARGO_TARGET_DIR -Force | Out-Null
    $global:msrvArtifact = Join-Path $env:CARGO_TARGET_DIR 'partial.rlib'
    'artifact' | Set-Content $global:msrvArtifact
    $global:msrvWasCalled = $true
    $global:msrvBusyChecks = if ($Scenario -eq 'failure') { 0 } else { 2 }
    $global:LASTEXITCODE = if ($Scenario -eq 'success') { 0 } else { 42 }
}
try {
    & $Entry
    if ($Scenario -ne 'success') { throw 'Expected MSRV failure' }
    if ($global:msrvSleepCount -ne 2) { throw 'Successful MSRV did not wait for the newly started builder' }
    if ($env:CARGO_TARGET_DIR -or $env:VOXELY_BUILD_CACHE_SESSION) { throw 'Environment not restored' }
    exit 0
}
catch {
    if ($_.Exception.Message -notmatch 'MSRV 1.90.0 build failed: 42') { throw }
    if ($global:msrvSleepCount -ne 0) { throw 'Failed MSRV entered cleanup waiting' }
    if ($env:CARGO_TARGET_DIR -or $env:VOXELY_BUILD_CACHE_SESSION) { throw 'Environment not restored' }
    exit 42
}
'@ | Set-Content $child
    & pwsh -NoProfile -File $child -Entry (Join-Path $fixtureScripts 'check-msrv.ps1') 2>$null
    Assert-Cache ($LASTEXITCODE -eq 42) 'MSRV must retain native failure and restore environment'
    Assert-Cache ((Get-TemporaryArtifactCount $fixture 'msrv-*') -eq 0) 'MSRV failure must remove its temporary artifacts'
    & pwsh -NoProfile -File $child -Entry (Join-Path $fixtureScripts 'check-msrv.ps1') -Scenario busy-failure 2>$null
    Assert-Cache ($LASTEXITCODE -eq 42) 'MSRV must retain the original Cargo failure without waiting when cleanup becomes busy'
    Assert-Cache ((Get-TemporaryArtifactCount $fixture 'msrv-*') -eq 1) 'Busy MSRV failure cleanup must preserve its temporary artifact'
    & pwsh -NoProfile -File $child -Entry (Join-Path $fixtureScripts 'check-msrv.ps1') -Scenario success 2>$null
    Assert-Cache ($LASTEXITCODE -eq 0) 'Successful MSRV must wait for a newly started builder before cleanup'
    Assert-Cache ((Get-TemporaryArtifactCount $fixture 'msrv-*') -eq 0) 'Successful MSRV cleanup must reclaim owned artifacts after the profile lock is released'
    $fixture = New-CacheFixture
    $fixtureScripts = Join-Path $fixture 'scripts'
    New-Item -ItemType Directory -Path $fixtureScripts | Out-Null
    foreach ($name in @('rust-build-cache.ps1', 'check-rust-coverage.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $fixtureScripts }
    Add-Content -LiteralPath (Join-Path $fixtureScripts 'rust-build-cache.ps1') -Value @'
$script:fixtureOpenCargoLock = ${function:Open-VoxelyCargoLock}
function Open-VoxelyCargoLock {
    param([string]$Path)
    if ($global:coverageWasCalled) { throw [IO.IOException]::new('fixture-owned-coverage-profile-busy', [int]0x80070021) }
    return & $script:fixtureOpenCargoLock -Path $Path
}
'@
    New-Item -ItemType Directory -Path (Join-Path $fixture 'src-tauri') | Out-Null
    $fakeTool = Join-Path $fixture 'fake-llvm-cov.ps1'
    @'
$global:coverageWasCalled = $true
New-Item -ItemType Directory -Path $env:CARGO_TARGET_DIR -Force | Out-Null
'artifact' | Set-Content (Join-Path $env:CARGO_TARGET_DIR 'partial.rlib')
$global:LASTEXITCODE = 42
'@ | Set-Content $fakeTool
    $child = Join-Path $fixture 'fake-coverage.ps1'
    @'
param($Entry, $FakeTool)
$ErrorActionPreference = 'Stop'
$global:coverageWasCalled = $false
$global:coverageSleepCount = 0
function Get-Command { param($Name, $ErrorAction) return [pscustomobject]@{Source=$FakeTool} }
function Get-Process {
    param([string[]]$Name, [int]$Id, $ErrorAction)
    if ($PSBoundParameters.ContainsKey('Id')) { return Microsoft.PowerShell.Management\Get-Process -Id $Id -ErrorAction Stop }
    if ('cargo' -in $Name) { throw 'Coverage must not scan global compilers' }
}
function Start-Sleep { $global:coverageSleepCount++; throw 'Failed coverage must not wait again during cleanup' }
try { & $Entry; throw 'Expected coverage failure' }
catch {
    if ($_.Exception.Message -notmatch 'cargo llvm-cov failed: 42') { throw }
    if ($global:coverageSleepCount -ne 0) { throw 'Failed coverage entered cleanup waiting' }
    if ($env:CARGO_TARGET_DIR -or $env:VOXELY_BUILD_CACHE_SESSION) { throw 'Environment not restored' }
    exit 42
}
'@ | Set-Content $child
    & pwsh -NoProfile -File $child -Entry (Join-Path $fixtureScripts 'check-rust-coverage.ps1') -FakeTool $fakeTool 2>$null
    Assert-Cache ($LASTEXITCODE -eq 42) 'Coverage tool error must survive a concurrent cleanup failure without another wait'
    Assert-Cache ((Get-TemporaryArtifactCount $fixture 'llvm-cov*') -eq 1) 'Busy cleanup must preserve its temporary artifact'
    Write-Host "PASS: Rust cache lifecycle ($script:assertions assertions, disposable fixtures only)."
} finally {
    foreach ($name in $savedEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process') }
    $absolute = [IO.Path]::GetFullPath($script:temporary)
    $expectedParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if ([IO.Path]::GetDirectoryName($absolute) -ne $expectedParent -or [IO.Path]::GetFileName($absolute) -notmatch '^voxely-cache-test-[0-9a-f]{32}$') { throw 'Unsafe fixture cleanup path' }
    Remove-Item -LiteralPath $absolute -Recurse -Force
}
