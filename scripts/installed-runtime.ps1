# Shared acceptance for disposable Windows installer environments only.
function Test-VoxelyReadyRecord($Record, [int]$ProcessId, [string]$Version, [DateTime]$StartedAt, [string]$EventName = 'ready') {
  try {
    $time = $Record.time_utc
    if ($time -is [DateTimeOffset]) { $utc = $time.UtcDateTime }
    elseif ($time -is [DateTime]) { $utc = $time.ToUniversalTime() }
    else { $utc = [DateTimeOffset]::Parse($time, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AssumeUniversal).UtcDateTime }
    return $Record.schema_version -eq 1 -and $Record.process_id -eq $ProcessId -and
      $Record.version -ceq $Version -and $Record.local_build -eq $false -and
      $Record.run_id -match '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$' -and $Record.event.name -ceq $EventName -and
      $utc -ge $StartedAt.ToUniversalTime()
  } catch { return $false }
}

function Test-VoxelyMainWindow($Window, [int]$ProcessId) {
  return $Window.ProcessId -eq $ProcessId -and $Window.Handle -gt 0 -and
    $Window.Visible -eq $true -and $Window.Title -ceq 'Voxely' -and
    $Window.ClassName -cne 'Tao Thread Event Target'
}

function Initialize-VoxelyInstallerWindows {
  if (-not ('VoxelyInstallerWindows' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class VoxelyInstallerWindows {
  public sealed class Window {
    public long Handle; public int ProcessId; public bool Visible;
    public string Title; public string ClassName; public string[] DialogText;
  }
  delegate bool Callback(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumWindows(Callback c, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr h, Callback c, IntPtr p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, StringBuilder s, uint f, uint t, out IntPtr r);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  public static Window[] ForProcess(int pid, bool includeDialog) {
    var result = new List<Window>();
    EnumWindows((h,p) => {
      uint owner; GetWindowThreadProcessId(h, out owner);
      if (owner == pid) {
        var title = new StringBuilder(512); var cls = new StringBuilder(256);
        GetWindowText(h,title,title.Capacity); GetClassName(h,cls,cls.Capacity);
        var text = new List<string>();
        if (includeDialog && cls.ToString() == "#32770") {
          int visited = 0;
          EnumChildWindows(h,(child,unused) => {
            if (visited++ >= 16) return false;
            uint childOwner; GetWindowThreadProcessId(child, out childOwner);
            if (childOwner != pid) return true;
            var value = new StringBuilder(512); IntPtr status;
            SendMessageTimeout(child,13,(IntPtr)value.Capacity,value,2,200,out status);
            text.Add(value.ToString());
            return true;
          },IntPtr.Zero);
        }
        result.Add(new Window { Handle=h.ToInt64(), ProcessId=pid, Visible=IsWindowVisible(h), Title=title.ToString(), ClassName=cls.ToString(), DialogText=text.ToArray() });
      }
      return true;
    }, IntPtr.Zero);
    return result.ToArray();
  }
}
'@
  }
}

function Wait-VoxelyInstalledRuntime($Process, [string]$ExpectedPath, [string]$Version, [string]$DataDirectory, [int]$TimeoutSeconds = 60) {
  Initialize-VoxelyInstallerWindows
  $started = $Process.StartTime.ToUniversalTime()
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  $windows = @()
  do {
    $Process.Refresh()
    if ($Process.HasExited) { throw "Installed Voxely exited before readiness: $($Process.ExitCode)" }
    if ($Process.Path -ine $ExpectedPath -or $Process.StartTime.ToUniversalTime() -ne $started) { throw 'Installed process identity changed' }
    $windows = @([VoxelyInstallerWindows]::ForProcess($Process.Id, $false))
    $main = @($windows | Where-Object { Test-VoxelyMainWindow $_ $Process.Id })
    $ready = $null
    $startedRuns = @{}
    for ($index = 0; $index -lt 8; $index++) {
      $journal = Join-Path $DataDirectory ('diagnostics\runtime-journal-{0:00}.jsonl' -f $index)
      if (-not (Test-Path -LiteralPath $journal -PathType Leaf)) { continue }
      if ((Get-Item -LiteralPath $journal).Length -gt 262144) { throw 'Runtime journal exceeds its configured bound' }
      foreach ($line in @(Get-Content -LiteralPath $journal -ErrorAction SilentlyContinue)) {
        try { $record = $line | ConvertFrom-Json -ErrorAction Stop } catch { continue }
        if (Test-VoxelyReadyRecord $record $Process.Id $Version $started) { $ready = $record }
        if (Test-VoxelyReadyRecord $record $Process.Id $Version $started 'process_start') { $startedRuns[$record.run_id] = $true }
      }
    }
    if ($main.Count -eq 1 -and $null -ne $ready -and $startedRuns.ContainsKey($ready.run_id)) {
      return [pscustomobject]@{ Handle = $main[0].Handle; Title = $main[0].Title; ClassName = $main[0].ClassName; RunId = $ready.run_id }
    }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)
  $windows = @([VoxelyInstallerWindows]::ForProcess($Process.Id, $true))
  throw ('Installed Voxely never reached main-window/ready acceptance. Windows: ' + ($windows | ConvertTo-Json -Compress -Depth 4))
}

function Test-VoxelyInstalledRuntimeHelpers {
  Initialize-VoxelyInstallerWindows
  $null = [VoxelyInstallerWindows]::ForProcess([Diagnostics.Process]::GetCurrentProcess().Id, $false)
  $start = [DateTime]::UtcNow.AddSeconds(-1)
  $record = [pscustomobject]@{ schema_version=1; process_id=123; version='0.3.0'; local_build=$false; run_id='12345678-1234-1234-1234-123456789abc'; event=@{name='ready'}; time_utc=[DateTime]::UtcNow.ToString('o') }
  if (-not (Test-VoxelyReadyRecord $record 123 '0.3.0' $start)) { throw 'Valid ready rejected' }
  $serialized = $record | ConvertTo-Json | ConvertFrom-Json
  if (-not (Test-VoxelyReadyRecord $serialized 123 '0.3.0' $start)) { throw 'Serialized valid ready rejected' }
  foreach ($field in @('process_id','version','local_build','event','time_utc','run_id','schema_version')) {
    $bad = $record | ConvertTo-Json | ConvertFrom-Json
    switch ($field) {
      'process_id' { $bad.$field=456 }
      'local_build' { $bad.$field=$true }
      'event' { $bad.$field=@{name='setup_started'} }
      'time_utc' { $bad.$field=$start.AddDays(-1).ToString('o') }
      'schema_version' { $bad.$field=2 }
      default { $bad.$field='invalid' }
    }
    if (Test-VoxelyReadyRecord $bad 123 '0.3.0' $start) { throw "Invalid ready accepted: $field" }
  }
  $window = [pscustomobject]@{ProcessId=123;Handle=456;Visible=$true;Title='Voxely';ClassName='Tao window'}
  if (-not (Test-VoxelyMainWindow $window 123)) { throw 'Valid main window rejected' }
  foreach ($field in @('ProcessId','Handle','Visible','Title','ClassName')) {
    $bad=$window | ConvertTo-Json | ConvertFrom-Json
    switch ($field) {
      'ProcessId' {$bad.$field=789}
      'Handle' {$bad.$field=0}
      'Visible' {$bad.$field=$false}
      'Title' {$bad.$field=''}
      'ClassName' {$bad.$field='Tao Thread Event Target'}
    }
    if (Test-VoxelyMainWindow $bad 123) { throw "Invalid main window accepted: $field" }
  }
}
