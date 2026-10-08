# Create a bounded local archive. History/settings/audio files aren't read directly.
# Existing application logs and opt-in dumps may contain private data; review before sharing.
param([switch]$IncludeDumps, [switch]$SelfTest)
$ErrorActionPreference = 'Stop'
function Copy-DiagnosticBytes {
    param([IO.Stream]$InputStream, [IO.Stream]$OutputStream, [long]$Limit)
    $buffer = [byte[]]::new(65536)
    [long]$copied = 0
    while ($copied -lt $Limit) {
        $read = $InputStream.Read($buffer, 0, [int][Math]::Min($buffer.Length, $Limit - $copied))
        if ($read -eq 0) { break }
        $OutputStream.Write($buffer, 0, $read)
        $copied += $read
    }
    return $copied
}
function Read-DiagnosticTail {
    param([string]$Path, [int]$Lines = 400, [int]$ByteBudget = 131072)
    $inputStream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    $memory = [IO.MemoryStream]::new()
    try {
        $offset = [Math]::Max(0, $inputStream.Length - $ByteBudget)
        $null = $inputStream.Seek($offset, [IO.SeekOrigin]::Begin)
        $null = Copy-DiagnosticBytes $inputStream $memory $ByteBudget
        $tail = [Text.Encoding]::UTF8.GetString($memory.ToArray())
        if ($offset -gt 0) {
            $newline = $tail.IndexOf("`n")
            if ($newline -ge 0) { $tail = $tail.Substring($newline + 1) }
        }
        return (($tail -split "`n" | Select-Object -Last $Lines) -join "`n")
    } finally { $inputStream.Dispose(); $memory.Dispose() }
}
if ($SelfTest) {
    $inputStream = [IO.MemoryStream]::new([byte[]](1,2,3,4,5,6))
    $outputStream = [IO.MemoryStream]::new()
    try {
        $copied = Copy-DiagnosticBytes $inputStream $outputStream 4
        if ($copied -ne 4 -or $outputStream.Length -ne 4 -or $inputStream.Position -ne 4 -or $outputStream.ToArray()[3] -ne 4) { throw 'Bounded copy failed.' }
        $copied = Copy-DiagnosticBytes $inputStream $outputStream 4
        if ($copied -ne 2 -or $outputStream.Length -ne 6) { throw 'Short stream copy failed.' }
        $fixture = Join-Path ([IO.Path]::GetTempPath()) ('voxely-tail-test-' + [Guid]::NewGuid().ToString('N') + '.log')
        try {
            [IO.File]::WriteAllText($fixture, ('x' * 524288), [Text.UTF8Encoding]::new($false))
            $tail = Read-DiagnosticTail $fixture
            if ([Text.Encoding]::UTF8.GetByteCount($tail) -ne 131072) { throw 'Long-line byte budget failed.' }
            [IO.File]::WriteAllText($fixture, "1`n2`n3`n4", [Text.UTF8Encoding]::new($false))
            if ((Read-DiagnosticTail $fixture -Lines 2) -ne "3`n4") { throw 'Tail line selection failed.' }
            $writer = [IO.File]::Open($fixture, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite)
            try {
                $busyRejected = $false
                try {
                    $reader = [IO.File]::Open($fixture, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
                    $reader.Dispose()
                } catch [IO.IOException] { $busyRejected = $true }
                if (-not $busyRejected) { throw 'An actively written dump would not be skipped.' }
            } finally { $writer.Dispose() }
        } finally { if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture } }
        Write-Output 'PASS: byte budgets, EOF, long-line, tail selection and active-writer exclusion'
    } finally { $inputStream.Dispose(); $outputStream.Dispose() }
    return
}
Add-Type -AssemblyName System.IO.Compression
$root = Split-Path -Parent $PSScriptRoot
$data = Join-Path $env:APPDATA 'Voxely'
$output = Join-Path $root '.local\diagnostics'
New-Item -ItemType Directory -Path $output -Force | Out-Null
$archivePath = Join-Path $output ('voxely-diagnostics-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8) + '.zip')
$stream = [IO.File]::Open($archivePath, [IO.FileMode]::CreateNew)
$archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $false)
function Add-DiagnosticText {
    param([string]$Name, [string]$Text)
    $entry = $archive.CreateEntry($Name)
    $writer = [IO.StreamWriter]::new($entry.Open(), [Text.UTF8Encoding]::new($false))
    try { $writer.Write($Text) } finally { $writer.Dispose() }
}
function Add-DiagnosticTail {
    param([string]$Name, [string]$Path, [int]$Lines = 400)
    try { Add-DiagnosticText $Name (Read-DiagnosticTail $Path -Lines $Lines) }
    catch [IO.IOException] { Add-DiagnosticText ($Name + '.unavailable.txt') 'File unavailable during collection; it may have rotated.' }
    catch [UnauthorizedAccessException] { Add-DiagnosticText ($Name + '.unavailable.txt') 'File access denied during collection.' }
}
try {
    $exe = Join-Path $root 'src-tauri\target\release\voxely.exe'
    $processes = @(Get-Process -Name voxely -ErrorAction SilentlyContinue | Select-Object Id,Path,StartTime,MainWindowTitle,Responding,PrivateMemorySize64,WorkingSet64)
    Add-DiagnosticText 'processes.json' ($processes | ConvertTo-Json -Depth 3)
    if (Test-Path -LiteralPath $exe) {
        Add-DiagnosticText 'binary.json' ((Get-Item -LiteralPath $exe | Select-Object FullName,Length,LastWriteTimeUtc) | ConvertTo-Json)
    }
    $logs = Join-Path $data 'logs'
    if (Test-Path -LiteralPath $logs) {
        Get-ChildItem -LiteralPath $logs -File -Filter 'voxely.*.log' | Sort-Object LastWriteTime -Descending | Select-Object -First 2 | ForEach-Object {
            Add-DiagnosticTail ('logs/' + $_.Name) $_.FullName -Lines 300
        }
    }
    $diagnostics = Join-Path $data 'diagnostics'
    if (Test-Path -LiteralPath $diagnostics) {
        # Exact owned patterns; do not recursively walk the user's data directory.
        Get-ChildItem -LiteralPath $diagnostics -File -Filter 'runtime-*.jsonl' | Sort-Object LastWriteTime -Descending | Select-Object -First 8 | ForEach-Object {
            Add-DiagnosticTail ('runtime/' + $_.Name) $_.FullName
        }
        $exits = Join-Path $diagnostics 'process-exits'
        if (Test-Path -LiteralPath $exits) {
            Get-ChildItem -LiteralPath $exits -File -Filter 'exit-*.jsonl' | Sort-Object LastWriteTime -Descending | Select-Object -First 16 | ForEach-Object {
                Add-DiagnosticTail ('process-exits/' + $_.Name) $_.FullName
            }
        }
    }
    Add-DiagnosticText 'wer-status.json' ((& (Join-Path $PSScriptRoot 'configure-crash-dumps.ps1') -Mode Status) | ConvertTo-Json)
    $eventJob = Start-Job -ScriptBlock {
        Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = (Get-Date).AddHours(-24); Id = 1000,1001,1002 } -MaxEvents 80 -ErrorAction Stop |
            Where-Object { $_.ProviderName -like 'CodexSandboxService*' -or $_.Message -match '(?i)voxely\.exe' } |
            Select-Object -First 30 TimeCreated,Id,ProviderName,@{Name='Message';Expression={if ($_.Message.Length -gt 8000) { $_.Message.Substring(0,8000) } else { $_.Message }}},@{Name='Xml';Expression={$xml=$_.ToXml();if($xml.Length -gt 12000){$xml.Substring(0,12000)}else{$xml}}}
    }
    try {
        if (Wait-Job $eventJob -Timeout 20) {
            $events = @(Receive-Job $eventJob -ErrorAction SilentlyContinue -ErrorVariable eventErrors)
            Add-DiagnosticText 'application-events.json' ($events | ConvertTo-Json -Depth 3)
            if ($eventErrors) { Add-DiagnosticText 'event-query-status.txt' 'Event query returned errors; absence of matching events is not proof of no crash.' }
        } else {
            Stop-Job $eventJob
            Add-DiagnosticText 'event-query-status.txt' 'Event query timed out after 20 seconds.'
        }
    } finally { Remove-Job $eventJob -Force }
    $dumpSummary = @()
    if ($IncludeDumps) {
        $dumps = Join-Path $diagnostics 'dumps'
        if (Test-Path -LiteralPath $dumps) {
            foreach ($file in @(Get-ChildItem -LiteralPath $dumps -File -Filter 'voxely*.dmp' | Sort-Object LastWriteTime -Descending | Select-Object -First 3)) {
                if ($file.Length -gt 33554432) { $dumpSummary += @{name=$file.Name;status='skipped-over-32MiB'}; continue }
                try {
                    # Don't copy a dump WER is still writing. Denying write sharing keeps
                    # the opened file stable; check size again using this exact handle.
                    $inputStream = [IO.File]::Open($file.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
                } catch [IO.IOException] { $dumpSummary += @{name=$file.Name;status='skipped-unavailable-or-writing'}; continue }
                try {
                    $length = $inputStream.Length
                    if ($length -gt 33554432) { $dumpSummary += @{name=$file.Name;status='skipped-over-32MiB'}; continue }
                    $entry = $archive.CreateEntry('dumps/' + $file.Name)
                    $entryStream = $entry.Open()
                    try { $copied = Copy-DiagnosticBytes $inputStream $entryStream $length } finally { $entryStream.Dispose() }
                    if ($copied -ne $length) { throw 'Dump changed while collecting diagnostics.' }
                    $dumpSummary += @{name=$file.Name;status='included';bytes=$copied}
                } finally { $inputStream.Dispose() }
            }
        }
    }
    Add-DiagnosticText 'collection.json' (@{timeUtc=[DateTime]::UtcNow.ToString('o');includeDumps=[bool]$IncludeDumps;dumps=$dumpSummary;historicalCauseProven=$false} | ConvertTo-Json -Depth 3)
} finally { $archive.Dispose(); $stream.Dispose() }
# Written directly to ZIP, so no temporary extracted directory needs cleanup.
$check = [IO.Compression.ZipFile]::OpenRead($archivePath)
try { if ($check.Entries.Count -lt 3) { throw 'Incomplete diagnostic archive.' }; Write-Output "Diagnostics saved: $archivePath ($($check.Entries.Count) entries)" } finally { $check.Dispose() }
