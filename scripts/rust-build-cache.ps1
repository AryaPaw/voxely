# One lifecycle owner for the repository's disposable Cargo artifacts.
Set-StrictMode -Version Latest

function Get-VoxelyBuildCache {
    param([string]$RepoRoot = (Split-Path -Parent $PSScriptRoot))
    $root = [IO.Path]::GetFullPath($RepoRoot)
    if (-not (Test-Path -LiteralPath (Join-Path $root 'package.json') -PathType Leaf)) { throw 'Cache owner must be a repository root.' }
    $target = [IO.Path]::GetFullPath((Join-Path $root 'src-tauri/target'))
    $cursor = $target
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse point in cache path: $cursor" }
        }
        $parent = [IO.Directory]::GetParent($cursor)
        $cursor = if ($parent) { $parent.FullName } else { $null }
    }
    return [pscustomobject]@{ Root = $root; Target = $target; Lock = (Join-Path $target '.voxely-cache.lock'); BudgetBytes = 15GB; ReserveBytes = 10GB }
}

function Get-VoxelyCacheTree {
    param([Parameter(Mandatory)]$Cache, [Parameter(Mandatory)][string]$Path)
    $absolute = [IO.Path]::GetFullPath($Path)
    if ($absolute -ne $Cache.Target -and -not $absolute.StartsWith($Cache.Target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Cache path escaped target: $absolute" }
    if (-not (Test-Path -LiteralPath $absolute)) { return }
    $items = @((Get-Item -LiteralPath $absolute -Force)) + @(Get-ChildItem -LiteralPath $absolute -Force -Recurse -ErrorAction Stop)
    foreach ($item in $items) {
        if ($null -eq $item) { continue }
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing cache tree containing a reparse point: $($item.FullName)" }
        $item
    }
}

function Get-VoxelyCacheBytes {
    param([Parameter(Mandatory)]$Cache)
    $sum = 0L
    foreach ($item in @(Get-VoxelyCacheTree -Cache $Cache -Path $Cache.Target)) {
        if (-not $item.PSIsContainer) { $sum += $item.Length }
    }
    return $sum
}

function Get-VoxelyCargoLockPaths {
    param([Parameter(Mandatory)]$Cache)
    # Cargo locks profiles, not process names. Validate before creating any lock scaffold.
    $tree = @(Get-VoxelyCacheTree -Cache $Cache -Path $Cache.Target)
    $paths = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $roots = @($Cache.Target) + @($tree | Where-Object {
        $_.PSIsContainer -and $_.Parent.FullName -eq $Cache.Target -and (Test-VoxelyTemporaryTargetName $_.Name)
    } | ForEach-Object FullName)
    foreach ($root in $roots) {
        foreach ($profileName in @('debug', 'release')) {
            $directory = Join-Path $root $profileName
            New-Item -ItemType Directory -Path $directory -Force | Out-Null
            $null = $paths.Add((Join-Path $directory '.cargo-lock'))
        }
    }
    foreach ($entry in $tree | Where-Object Name -eq '.cargo-lock') {
        if ($entry.PSIsContainer) { throw "Invalid Cargo lock directory: $($entry.FullName)" }
        $null = $paths.Add($entry.FullName)
    }
    return @($paths | Sort-Object)
}

function Exit-VoxelyCargoLocks {
    param([Parameter(Mandatory)]$CargoLocks)
    foreach ($stream in $CargoLocks.Streams) { $stream.Dispose() }
}

function Open-VoxelyCargoLock {
    param([Parameter(Mandatory)][string]$Path)
    # Never truncate locks, including hardlinks. Denying Delete keeps the same
    # file object available to Cargo waiters for the whole cleanup operation.
    $stream = [IO.File]::Open($Path, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::ReadWrite)
    try {
        $stream.Lock(0, 1)
        return $stream
    } catch { $stream.Dispose(); throw }
}

function Enter-VoxelyCargoLocks {
    param([Parameter(Mandatory)]$Cache, [switch]$WaitForBuilders, [ValidateRange(0, 900)][int]$TimeoutSeconds = 900)
    if (-not $IsWindows) { throw 'Cargo cache maintenance currently requires Windows file-lock semantics.' }
    $wait = [Diagnostics.Stopwatch]::StartNew()
    $announced = $false
    while ($true) {
        $streams = [Collections.Generic.List[IO.FileStream]]::new()
        $busyPath = $null
        try {
            $paths = @(Get-VoxelyCargoLockPaths -Cache $Cache)
            foreach ($path in $paths) {
                try {
                    $stream = Open-VoxelyCargoLock -Path $path
                    $streams.Add($stream)
                } catch {
                    $errorCode = $_.Exception.GetBaseException().HResult -band 0xffff
                    if ($errorCode -notin @(32, 33)) { throw }
                    $busyPath = $path
                    break
                }
            }
            if (-not $busyPath) {
                $null = @(Get-VoxelyCacheTree -Cache $Cache -Path $Cache.Target)
                Assert-VoxelyCacheRuntimeIdle -Cache $Cache
                return [pscustomobject]@{ Target = $Cache.Target; Paths = $paths; Streams = $streams.ToArray() }
            }
        } catch {
            foreach ($stream in $streams) { $stream.Dispose() }
            throw
        }
        # Release the whole set before waiting: Cargo may acquire host/target locks
        # in a different order. Never wait for one while holding another.
        foreach ($stream in $streams) { $stream.Dispose() }
        if (-not $WaitForBuilders -or $wait.Elapsed.TotalSeconds -ge $TimeoutSeconds) {
            throw "Build cache is busy: Cargo holds $busyPath"
        }
        if (-not $announced) {
            Write-Host "Waiting for Cargo using this Voxely target (up to $TimeoutSeconds seconds)."
            $announced = $true
        }
        Start-Sleep -Milliseconds 250
    }
}

function Assert-VoxelyCacheRuntimeIdle {
    param([Parameter(Mandatory)]$Cache)
    foreach ($process in @(Get-Process -Name voxely,voxely_lib* -ErrorAction SilentlyContinue)) {
        try { $processPath = $process.Path } catch { throw 'Cannot establish running Voxely executable ownership; cache cleanup refused.' }
        if (-not $processPath) { throw 'Cannot establish running Voxely executable ownership; cache cleanup refused.' }
        $protected = Join-Path $Cache.Target 'release/voxely.exe'
        if ($processPath.StartsWith($Cache.Target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -and $processPath -ne $protected) { throw "A cache executable is running (PID $($process.Id)); cleanup refused." }
    }
}

function Assert-VoxelyCacheIdle {
    param([Parameter(Mandatory)]$Cache, [switch]$WaitForBuilders, [ValidateRange(0, 900)][int]$TimeoutSeconds = 900)
    $locks = Enter-VoxelyCargoLocks -Cache $Cache -WaitForBuilders:$WaitForBuilders -TimeoutSeconds $TimeoutSeconds
    Exit-VoxelyCargoLocks $locks
}

function Test-VoxelyTemporaryTargetName {
    param([string]$Name)
    return $Name -eq 'llvm-cov' -or $Name -eq 'local-fix-verification' -or $Name -match '^llvm-cov-[0-9a-f]{32}$' -or $Name -match '^msrv-\d+\.\d+(?:\.\d+)?(?:-[0-9a-f]{32})?$'
}

function Remove-VoxelyCacheDirectory {
    param([Parameter(Mandatory)]$Cache, [Parameter(Mandatory)][string]$Path, [switch]$WaitForBuilders, $CargoLocks = $null)
    if (-not (Test-VoxelyBuildCacheSession -Cache $Cache)) { throw 'Cache deletion requires the active managed-build lease.' }
    $absolute = [IO.Path]::GetFullPath($Path)
    $relative = [IO.Path]::GetRelativePath($Cache.Target, $absolute).Replace('\', '/')
    $allowed = $relative -in @('debug', 'debug/incremental', 'release/deps', 'release/build', 'release/.fingerprint', 'release/incremental', 'release/examples') -or (Test-VoxelyTemporaryTargetName $relative)
    if (-not $allowed) { throw "Not an owned disposable cache directory: $relative" }
    if (-not (Test-Path -LiteralPath $absolute)) { return }
    $ownsLocks = $null -eq $CargoLocks
    if ($ownsLocks) { $CargoLocks = Enter-VoxelyCargoLocks -Cache $Cache -WaitForBuilders:$WaitForBuilders }
    try {
        if ($CargoLocks.Target -ne $Cache.Target) { throw 'Cargo locks belong to another cache.' }
        $null = @(Get-VoxelyCacheTree -Cache $Cache -Path $absolute)
        if (Test-VoxelyTemporaryTargetName $relative) {
            $artifactRoots = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
            $null = $artifactRoots.Add($absolute)
            # Only profiles covered by our held locks are disposable. A newly
            # created/unlocked profile is never recursively removed with the root.
            foreach ($lockPath in $CargoLocks.Paths) {
                if ($lockPath.StartsWith($absolute + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                    $profileDirectory = Split-Path -Parent $lockPath
                    Clear-VoxelyLockedCachePath -Cache $Cache -Path $profileDirectory -CargoLocks $CargoLocks
                    # cargo-llvm-cov also writes profraw/profdata beside profiles
                    # under its nested llvm-cov-target namespace.
                    $artifactRoot = Split-Path -Parent $profileDirectory
                    if ($artifactRoot.StartsWith($absolute + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                        $null = $artifactRoots.Add($artifactRoot)
                    }
                }
            }
            foreach ($artifactRoot in $artifactRoots) {
                foreach ($file in @(Get-ChildItem -LiteralPath $artifactRoot -File -Force)) {
                    if ($file.Name -notin @('.cargo-lock', 'CACHEDIR.TAG')) { Remove-Item -LiteralPath $file.FullName -Force -ErrorAction Stop }
                }
            }
        } else {
            Clear-VoxelyLockedCachePath -Cache $Cache -Path $absolute -CargoLocks $CargoLocks
        }
    } finally { if ($ownsLocks) { Exit-VoxelyCargoLocks $CargoLocks } }
    Write-Host "Cleared Cargo artifacts: $relative"
}

function Clear-VoxelyLockedCachePath {
    param([Parameter(Mandatory)]$Cache, [Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$CargoLocks)
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $tree = @(Get-VoxelyCacheTree -Cache $Cache -Path $Path)
    $prefix = $Path + [IO.Path]::DirectorySeparatorChar
    $containsLock = @($CargoLocks.Paths | Where-Object { $_.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
    if (-not $containsLock) {
        # An independently started nested Cargo may have introduced a new target
        # after lock discovery. Preserve that whole subtree instead of deleting
        # through a profile whose lock isn't part of this maintenance operation.
        if (@($tree | Where-Object Name -eq '.cargo-lock').Count) { return }
        Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
        return
    }
    # Cargo uses the same remove-siblings pattern. Keep the lock and all its
    # ancestors so a waiting Cargo never resumes through a replaced lock file.
    foreach ($entry in @(Get-ChildItem -LiteralPath $Path -Force)) {
        if ($entry.FullName -in $CargoLocks.Paths) { continue }
        if ($entry.PSIsContainer) { Clear-VoxelyLockedCachePath -Cache $Cache -Path $entry.FullName -CargoLocks $CargoLocks }
        else { Remove-Item -LiteralPath $entry.FullName -Force -ErrorAction Stop }
    }
}

function Get-VoxelyCacheFreeBytes {
    param([Parameter(Mandatory)]$Cache)
    return ([IO.DriveInfo]::new([IO.Path]::GetPathRoot($Cache.Target))).AvailableFreeSpace
}

function Repair-VoxelyBuildCache {
    param([Parameter(Mandatory)]$Cache, [switch]$BeforeBuild, [switch]$Full, [switch]$WaitForBuilders)
    if (-not (Test-VoxelyBuildCacheSession -Cache $Cache)) { throw 'Cache maintenance requires the active managed-build lease.' }
    $cargoLocks = Enter-VoxelyCargoLocks -Cache $Cache -WaitForBuilders:$WaitForBuilders
    try {
        $null = @(Get-VoxelyCacheTree -Cache $Cache -Path $Cache.Target)
        foreach ($file in @(Get-ChildItem -LiteralPath $Cache.Target -File -Force)) {
            if ($file.Name -match '^\.voxely-cache-session-([0-9a-f]{32})\.json$' -and $Matches[1] -cne $env:VOXELY_BUILD_CACHE_SESSION) {
                [IO.File]::Delete($file.FullName)
            }
        }
        foreach ($directory in @(Get-ChildItem -LiteralPath $Cache.Target -Directory -Force)) {
            if (Test-VoxelyTemporaryTargetName $directory.Name) { Remove-VoxelyCacheDirectory -Cache $Cache -Path $directory.FullName -CargoLocks $cargoLocks }
        }
        foreach ($relative in @('debug/incremental', 'release/incremental')) { Remove-VoxelyCacheDirectory -Cache $Cache -Path (Join-Path $Cache.Target $relative) -CargoLocks $cargoLocks }
        $bytes = Get-VoxelyCacheBytes -Cache $Cache
        # Evict whole coherent profiles, never individual hashed dependency files.
        foreach ($unit in @(@('debug'), @('release/deps', 'release/build', 'release/.fingerprint', 'release/incremental', 'release/examples'))) {
            $needSpace = $BeforeBuild -and (Get-VoxelyCacheFreeBytes -Cache $Cache) -lt $Cache.ReserveBytes
            if (-not $Full -and $bytes -le $Cache.BudgetBytes -and -not $needSpace) { break }
            foreach ($relative in $unit) { Remove-VoxelyCacheDirectory -Cache $Cache -Path (Join-Path $Cache.Target $relative) -CargoLocks $cargoLocks }
            $bytes = Get-VoxelyCacheBytes -Cache $Cache
        }
        Write-Host ("Cargo cache: {0:F2} GiB / {1:F0} GiB budget (file lengths; hardlinks counted conservatively)." -f ($bytes / 1GB), ($Cache.BudgetBytes / 1GB))
        if ($bytes -gt $Cache.BudgetBytes) { throw 'Cache budget exceeded by protected or unrecognized files; inspect manually. No unknown data was deleted.' }
        if ($BeforeBuild -and (Get-VoxelyCacheFreeBytes -Cache $Cache) -lt $Cache.ReserveBytes) { throw 'Less than 10 GiB free after safe cache eviction; build was not started.' }
    } finally { Exit-VoxelyCargoLocks $cargoLocks }
}

function Test-VoxelyBuildCacheSession {
    param([Parameter(Mandatory)]$Cache)
    if ($env:VOXELY_BUILD_CACHE_SESSION -notmatch '^[0-9a-f]{32}$' -or -not (Test-Path -LiteralPath $Cache.Lock)) { return $false }
    try {
        $recordPath = Join-Path $Cache.Target (".voxely-cache-session-$env:VOXELY_BUILD_CACHE_SESSION.json")
        if ((Get-Item -LiteralPath $Cache.Lock -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { return $false }
        if ((Get-Item -LiteralPath $recordPath -Force -ErrorAction Stop).Attributes -band [IO.FileAttributes]::ReparsePoint) { return $false }
        $record = Get-Content -LiteralPath $recordPath -Raw | ConvertFrom-Json
        if ($record.token -cne $env:VOXELY_BUILD_CACHE_SESSION -or $record.target -ne $Cache.Target) { return $false }
        $owner = Get-Process -Id $record.pid -ErrorAction Stop
        if ($owner.StartTime.ToUniversalTime().Ticks -ne $record.startTicks) { return $false }
        try {
            $probe = [IO.File]::Open($recordPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read)
            $probe.Dispose()
            return $false
        } catch [IO.IOException] { }
        try {
            $probe = [IO.File]::Open($Cache.Lock, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read)
            $probe.Dispose()
            return $false
        } catch [IO.IOException] { return $true }
    } catch { return $false }
}

function Invoke-VoxelyBuildCache {
    param([Parameter(Mandatory)][scriptblock]$Action, [string]$RepoRoot = (Split-Path -Parent $PSScriptRoot), [switch]$Full)
    $cache = Get-VoxelyBuildCache -RepoRoot $RepoRoot
    if (Test-VoxelyBuildCacheSession -Cache $cache) { & $Action $cache; return }
    if ($env:CARGO_TARGET_DIR -and [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) -ne $cache.Target) { throw 'Unset external CARGO_TARGET_DIR before a managed build.' }
    if ($env:CARGO_BUILD_TARGET_DIR -or $env:CARGO_BUILD_BUILD_DIR) { throw 'Unset external Cargo output directory overrides before a managed build.' }
    New-Item -ItemType Directory -Path $cache.Target -Force | Out-Null
    $lockEntry = Get-Item -LiteralPath $cache.Lock -Force -ErrorAction SilentlyContinue
    if ($null -ne $lockEntry) {
        if ($lockEntry.PSIsContainer -or ($lockEntry.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Invalid cache lock: reparse points and directories are refused.' }
    }
    # Never truncate or write an existing lock: it could be a hardlink to unrelated data.
    try { $lease = [IO.File]::Open($cache.Lock, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read) }
    catch [IO.IOException] { throw 'Another managed build or cache cleanup owns this target. Wait for it to finish.' }
    $previousSession = $env:VOXELY_BUILD_CACHE_SESSION
    $previousTarget = $env:CARGO_TARGET_DIR
    $failure = $null
    $recordPath = $null
    $recordCreated = $false
    $recordStream = $null
    try {
        $env:VOXELY_BUILD_CACHE_SESSION = [guid]::NewGuid().ToString('N')
        $env:CARGO_TARGET_DIR = $cache.Target
        $record = @{ token = $env:VOXELY_BUILD_CACHE_SESSION; target = $cache.Target; pid = $PID; startTicks = (Get-Process -Id $PID).StartTime.ToUniversalTime().Ticks } | ConvertTo-Json -Compress
        $buffer = [Text.Encoding]::UTF8.GetBytes($record)
        $recordPath = Join-Path $cache.Target (".voxely-cache-session-$env:VOXELY_BUILD_CACHE_SESSION.json")
        $recordStream = [IO.File]::Open($recordPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read)
        $recordCreated = $true
        $recordStream.Write($buffer, 0, $buffer.Length)
        $recordStream.Flush($true)
        Repair-VoxelyBuildCache -Cache $cache -BeforeBuild -Full:$Full
        & $Action $cache
    } catch { $failure = $_ }
    finally {
        try {
            if ($null -eq $failure) { Assert-VoxelyCacheIdle -Cache $cache -WaitForBuilders }
            Repair-VoxelyBuildCache -Cache $cache -Full:$Full -WaitForBuilders:($null -eq $failure)
        }
        catch {
            if ($null -ne $failure) { Write-Warning "Post-build cache cleanup failed: $($_.Exception.Message)" }
            else { $failure = $_ }
        }
        try {
            if ($null -ne $recordStream) { $recordStream.Dispose() }
            if ($recordCreated) { [IO.File]::Delete($recordPath) }
        }
        catch {
            if ($null -ne $failure) { Write-Warning "Cache session cleanup failed: $($_.Exception.Message)" }
            else { $failure = $_ }
        }
        finally {
            $env:VOXELY_BUILD_CACHE_SESSION = $previousSession
            $env:CARGO_TARGET_DIR = $previousTarget
            $lease.Dispose()
        }
    }
    if ($null -ne $failure) { throw $failure }
}
