# Disposable fixtures only. No real target deletion or compilation in this test.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'rust-build-cache.ps1')
$script:actualCacheContext = ${function:Get-VoxelyBuildCache}
$script:budget = 1MB
$script:free = 100GB
$script:busy = $false
$script:busyChecksRemaining = 0
$script:busySentinel = $null
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
    if ('cargo' -in $Name -and $script:busy) { return [pscustomobject]@{ProcessName='cargo';Id=123} }
    if ('cargo' -in $Name -and $script:busyChecksRemaining -gt 0) {
        Assert-Cache (Test-Path -LiteralPath $script:busySentinel) 'Cleanup must retain artifacts while any compiler is busy'
        $script:busyChecksRemaining--
        return [pscustomobject]@{ProcessName='cargo';Id=123}
    }
    if ('voxely' -in $Name -and $null -ne $script:runningApp) { return $script:runningApp }
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
        Assert-Cache (-not (Test-Path (Join-Path $cache.Target 'msrv-1.90'))) 'MSRV should be cleaned before action'
        $null = Write-CacheFixture $fixture ('llvm-cov-' + ('c' * 32) + '/debug/temporary.rlib')
    }
    Assert-Cache (Test-Path $exe) 'Runtime executable must remain'
    Assert-Cache (Test-Path $dll) 'Adjacent runtime DLL must remain'
    Assert-Cache (Test-Path $unknown) 'Unknown data must remain'
    Assert-Cache (@(Get-ChildItem (Join-Path $fixture 'src-tauri/target') -Directory -Filter 'llvm-cov*').Count -eq 0) 'Post-build temporary targets must be removed'

    $fixture = New-CacheFixture
    $exe = Write-CacheFixture $fixture 'release/voxely.exe'
    $null = Write-CacheFixture $fixture 'debug/deps/large.rlib' 8192
    $script:budget = 4096
    Invoke-VoxelyBuildCache -RepoRoot $fixture -Action { }
    Assert-Cache (-not (Test-Path (Join-Path $fixture 'src-tauri/target/debug'))) 'Over-budget debug profile must be evicted coherently'
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
    Assert-Cache (@(Get-ChildItem (Join-Path $fixture 'src-tauri/target') -Directory -Filter 'llvm-cov*').Count -eq 0) 'Failure must still clean temporary targets'
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
    Assert-Cache (-not (Test-Path (Join-Path $fixture 'src-tauri/target/debug'))) 'Full maintenance must evict debug'

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
    New-Item -ItemType Directory -Path (Join-Path $fixture 'src-tauri') | Out-Null
    'rust-version = "1.90.0"' | Set-Content (Join-Path $fixture 'src-tauri/Cargo.toml')
    $child = Join-Path $fixture 'fake-msrv.ps1'
    @'
param($Entry)
$ErrorActionPreference = 'Stop'
function Get-Process {
    param([string[]]$Name, [int]$Id, $ErrorAction)
    if ($PSBoundParameters.ContainsKey('Id')) { return Microsoft.PowerShell.Management\Get-Process -Id $Id -ErrorAction Stop }
}
function cargo {
    if ($args[0] -ne '+1.90.0' -or $args[1] -ne 'build' -or '--locked' -notin $args -or '--all-targets' -notin $args) { throw 'Wrong MSRV arguments' }
    if ([IO.Path]::GetFileName($env:CARGO_TARGET_DIR) -notmatch '^msrv-1\.90\.0-[0-9a-f]{32}$') { throw 'MSRV target was not isolated' }
    New-Item -ItemType Directory -Path $env:CARGO_TARGET_DIR -Force | Out-Null
    'artifact' | Set-Content (Join-Path $env:CARGO_TARGET_DIR 'partial.rlib')
    $global:LASTEXITCODE = 42
}
try { & $Entry; throw 'Expected MSRV failure' }
catch { if ($_.Exception.Message -notmatch 'MSRV 1.90.0 build failed: 42') { throw }; if ($env:CARGO_TARGET_DIR -or $env:VOXELY_BUILD_CACHE_SESSION) { throw 'Environment not restored' }; exit 42 }
'@ | Set-Content $child
    & pwsh -NoProfile -File $child -Entry (Join-Path $fixtureScripts 'check-msrv.ps1') 2>$null
    Assert-Cache ($LASTEXITCODE -eq 42) 'MSRV must retain native failure and restore environment'
    Assert-Cache (@(Get-ChildItem (Join-Path $fixture 'src-tauri/target') -Directory -Filter 'msrv-*').Count -eq 0) 'MSRV failure must remove its temporary target'
    $fixture = New-CacheFixture
    $fixtureScripts = Join-Path $fixture 'scripts'
    New-Item -ItemType Directory -Path $fixtureScripts | Out-Null
    foreach ($name in @('rust-build-cache.ps1', 'check-rust-coverage.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $fixtureScripts }
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
    if ('cargo' -in $Name -and $global:coverageWasCalled) { return [pscustomobject]@{ProcessName='cargo';Id=123} }
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
    Assert-Cache (@(Get-ChildItem (Join-Path $fixture 'src-tauri/target') -Directory -Filter 'llvm-cov-*').Count -eq 1) 'Busy cleanup must preserve its temporary target'
    Write-Host "PASS: Rust cache lifecycle ($script:assertions assertions, disposable fixtures only)."
} finally {
    foreach ($name in $savedEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process') }
    $absolute = [IO.Path]::GetFullPath($script:temporary)
    $expectedParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if ([IO.Path]::GetDirectoryName($absolute) -ne $expectedParent -or [IO.Path]::GetFileName($absolute) -notmatch '^voxely-cache-test-[0-9a-f]{32}$') { throw 'Unsafe fixture cleanup path' }
    Remove-Item -LiteralPath $absolute -Recurse -Force
}
