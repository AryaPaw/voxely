# Shared local-launch primitive. Does not elevate or alter Windows Job policy.
function Test-SameExecutablePath([string]$Candidate, [string]$Expected) {
    if ([string]::IsNullOrWhiteSpace($Candidate) -or [string]::IsNullOrWhiteSpace($Expected)) { return $false }
    try {
        return [string]::Equals([IO.Path]::GetFullPath($Candidate), [IO.Path]::GetFullPath($Expected), [StringComparison]::OrdinalIgnoreCase)
    } catch { return $false }
}

function Initialize-VoxelyProcessProbe {
    if ('Voxely.LocalLaunch.ProcessJob' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace Voxely.LocalLaunch {
    public static class ProcessJob {
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool IsProcessInJob(IntPtr process, IntPtr job,
            [MarshalAs(UnmanagedType.Bool)] out bool result);
        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool IsWindowVisible(IntPtr window);
    }
}
'@
}

function Start-IndependentWindowsProcess {
    param([string]$CommandLine, [string]$WorkingDirectory, [switch]$Hidden)
    $startup = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{
        CreateFlags = [uint32]0x01000000 # CREATE_BREAKAWAY_FROM_JOB
        ShowWindow = [uint16]$(if ($Hidden) { 0 } else { 1 })
    }
    $result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -OperationTimeoutSec 15 -Arguments @{
        CommandLine = $CommandLine
        CurrentDirectory = $WorkingDirectory
        ProcessStartupInformation = $startup
    }
    if ($result.ReturnValue -ne 0) {
        throw "Independent launch failed (Win32_Process.Create: $($result.ReturnValue))."
    }
    return [int]$result.ProcessId
}

function Assert-IndependentWindowsProcess {
    param([System.Diagnostics.Process]$Process)
    Initialize-VoxelyProcessProbe
    $inJob = $false
    if (-not [Voxely.LocalLaunch.ProcessJob]::IsProcessInJob($Process.Handle, [IntPtr]::Zero, [ref]$inJob)) {
        throw "Cannot query Windows Job: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())."
    }
    if ($inJob) { throw 'Process still belongs to a Windows Job.' }
    if ($Process.SessionId -ne [System.Diagnostics.Process]::GetCurrentProcess().SessionId) {
        throw 'Process started in a different Windows session.'
    }
}
