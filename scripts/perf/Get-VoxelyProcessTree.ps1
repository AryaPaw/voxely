param(
    [string]$ProcessName = "voxely",
    [string]$Scenario = "manual",
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug",
    [string]$OutFile
)

$ErrorActionPreference = "Stop"

function Get-OwnedTree {
    param([string]$Name)

    $roots = @(Get-Process -Name $Name -ErrorAction SilentlyContinue)
    if ($roots.Count -eq 0) {
        return @()
    }

    $all = @(Get-CimInstance Win32_Process)
    $byParent = @{}
    foreach ($row in $all) {
        $parent = [int]$row.ParentProcessId
        if (-not $byParent.ContainsKey($parent)) {
            $byParent[$parent] = @()
        }
        $byParent[$parent] += $row
    }

    $owned = @{}
    $queue = New-Object System.Collections.Queue
    foreach ($root in $roots) {
        $owned[$root.Id] = $true
        $queue.Enqueue([int]$root.Id)
    }

    while ($queue.Count -gt 0) {
        $processId = [int]$queue.Dequeue()
        if (-not $byParent.ContainsKey($processId)) {
            continue
        }
        foreach ($child in $byParent[$processId]) {
            $childId = [int]$child.ProcessId
            if ($owned.ContainsKey($childId)) {
                continue
            }
            $childName = [string]$child.Name
            if ($childName -eq "voxely.exe" -or $childName -eq "msedgewebview2.exe") {
                $owned[$childId] = $true
                $queue.Enqueue($childId)
            }
        }
    }

    Get-Process -ErrorAction SilentlyContinue | Where-Object { $owned.ContainsKey($_.Id) }
}

function Get-Row {
    param($Process)

    $wmi = Get-CimInstance Win32_Process -Filter "ProcessId=$($Process.Id)"
    [pscustomobject]@{
        pid           = $Process.Id
        name          = $Process.ProcessName
        privateBytes  = [int64]$Process.PrivateMemorySize64
        workingSet    = [int64]$Process.WorkingSet64
        commit        = [int64]$Process.PagedMemorySize64
        handles       = [int]$Process.HandleCount
        threads       = [int]$Process.Threads.Count
        cpuSeconds    = [double]$Process.CPU
        readBytes     = [int64]$wmi.ReadTransferCount
        writeBytes    = [int64]$wmi.WriteTransferCount
    }
}

$owned = @(Get-OwnedTree -Name $ProcessName)
$rows = @($owned | ForEach-Object { Get-Row $_ })
$webview = @($rows | Where-Object { $_.name -eq "msedgewebview2" }).Count

$tree = [ordered]@{
    processCount  = $rows.Count
    webview2Count = $webview
    privateBytes  = [int64](($rows | Measure-Object privateBytes -Sum).Sum)
    workingSet    = [int64](($rows | Measure-Object workingSet -Sum).Sum)
    commit        = [int64](($rows | Measure-Object commit -Sum).Sum)
    handles       = [int](($rows | Measure-Object handles -Sum).Sum)
    threads       = [int](($rows | Measure-Object threads -Sum).Sum)
}

$git = "unknown"
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
try {
    $git = (git -C $repo rev-parse --short HEAD).Trim()
} catch {
    $git = "unknown"
}

$payload = [ordered]@{
    schema   = "voxely-perf/v1"
    capturedAt = [DateTime]::UtcNow.ToString("o")
    build    = [ordered]@{
        profile = $Profile
        git     = $git
        exe     = if ($rows.Count -gt 0) { ($owned | Where-Object { $_.ProcessName -eq $ProcessName } | Select-Object -First 1).Path } else { $null }
    }
    machine  = [ordered]@{
        computer = $env:COMPUTERNAME
        os       = [System.Environment]::OSVersion.VersionString
    }
    scenario = $Scenario
    tree     = $tree
    processes = $rows
}

$json = $payload | ConvertTo-Json -Depth 6
if ($OutFile) {
    $dir = Split-Path -Parent $OutFile
    if ($dir) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }
    Set-Content -Path $OutFile -Value $json -Encoding utf8
}

$json
