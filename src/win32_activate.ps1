# Runs in a disposable x64 Windows PowerShell process. No window interaction.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$request = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($requestBase64)) | ConvertFrom-Json
$state = [ordered]@{
    pid = [uint32]$request.pid
    hwnd = ('0x{0:x}' -f [long]$request.hwnd)
    backend = 'shadowbot_rpabuilder'
    status = 'not_started'
    install_called = $false
    dll_path = [string]$request.dll_path
    loaded_dll = $null
    tree_verification = 'not_checked'
    restore = $false
}
$code = 'WIN32_ACTIVATION_PROVIDER_ERROR'
$locked = $false
$mutex = $null
$target = $null
try {
    if (-not [Environment]::Is64BitProcess) { throw 'Activation requires a 64-bit worker' }
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class XbotTreeActivation {
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool IsWow64Process(IntPtr process, out bool wow64);
    [DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);
    [DllImport("kernel32.dll", CharSet=CharSet.Ansi, ExactSpelling=true, SetLastError=true)]
    public static extern IntPtr GetProcAddress(IntPtr module, string name);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate int Install(uint pid, [MarshalAs(UnmanagedType.LPStr)] string info);
    public static int Activate(string path, uint pid, string info) {
        IntPtr module = LoadLibraryExW(path, IntPtr.Zero, 0x1100);
        if (module == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        IntPtr address = GetProcAddress(module, "InstallRpaDllSimple");
        if (address == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        var install = (Install)Marshal.GetDelegateForFunctionPointer(address, typeof(Install));
        // Keep the vendor module loaded until the worker exits.
        return install(pid, info);
    }
}
'@
    $code = 'WIN32_TARGET_MISMATCH'
    $handle = [IntPtr][long]$request.hwnd
    function Assert-Target {
        [uint32]$owner = 0
        if (-not [XbotTreeActivation]::IsWindow($handle)) { throw 'Window does not exist' }
        $null = [XbotTreeActivation]::GetWindowThreadProcessId($handle, [ref]$owner)
        if ($owner -ne [uint32]$request.pid) { throw 'HWND does not belong to the specified PID' }
        if ($null -ne $target -and [XbotTreeActivation]::WaitForSingleObject($target.Handle, 0) -ne 258) {
            throw 'The selected process instance has exited'
        }
    }
    Assert-Target
    $code = 'WIN32_TARGET_ACCESS_FAILED'
    $target = [Diagnostics.Process]::GetProcessById([int]$request.pid)
    # Holding the handle pins this process instance, including across PID reuse.
    $null = $target.Handle
    $start = $target.StartTime.ToUniversalTime()
    $state.process_started_at = $start.ToString('o')
    $state.exe_path = $target.MainModule.FileName
    [bool]$wow64 = $false
    if (-not [XbotTreeActivation]::IsWow64Process($target.Handle, [ref]$wow64)) {
        throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error()))
    }
    if ($wow64) { $code = 'WIN32_TARGET_ARCH_UNSUPPORTED'; throw 'Only x64 target processes are supported' }
    $mutex = New-Object Threading.Mutex($false, ('Local\xbot-cli-tree-{0}-{1}' -f $request.pid, $start.Ticks))
    try { $locked = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $locked = $true }
    if (-not $locked) { $code = 'WIN32_ACTIVATION_BUSY'; throw 'Activation of this process is already in progress' }
    function Get-LoadedBuilder {
        $target.Refresh()
        foreach ($module in $target.Modules) {
            if ($module.ModuleName -ieq 'RpaBuilder.dll') { return $module.FileName }
        }
        return $null
    }
    $code = 'WIN32_TARGET_ACCESS_FAILED'
    $state.loaded_dll = Get-LoadedBuilder
    $code = 'WIN32_TARGET_MISMATCH'
    Assert-Target
    if ($null -ne $state.loaded_dll) {
        $state.status = 'already_loaded'
    } else {
        $code = 'WIN32_ACTIVATION_FAILED'
        $state.status = 'outcome_unknown'
        $state.install_called = $true
        $result = [XbotTreeActivation]::Activate([string]$request.dll_path, [uint32]$request.pid, [IO.Path]::GetFileNameWithoutExtension($state.exe_path))
        $state.return_code = $result
        $state.hresult = ('0x{0:X8}' -f ([long]$result -band 0xffffffffL))
        if ($result -ne 0) { throw ('InstallRpaDllSimple returned ' + $state.hresult) }
        $code = 'WIN32_TARGET_MISMATCH'
        Assert-Target
        $code = 'WIN32_TARGET_ACCESS_FAILED'
        $state.loaded_dll = Get-LoadedBuilder
        if ($null -eq $state.loaded_dll) {
            $code = 'WIN32_ACTIVATION_UNVERIFIED'
            throw 'DLL returned success but its presence in the target could not be verified'
        }
        $state.status = 'activated'
    }
    @{ok=$true; data=$state} | ConvertTo-Json -Depth 6 -Compress
} catch {
    @{ok=$false; code=$code; message=$_.Exception.Message; data=$state} | ConvertTo-Json -Depth 6 -Compress
    exit 1
} finally {
    if ($locked) { $mutex.ReleaseMutex() }
    if ($null -ne $mutex) { $mutex.Dispose() }
    if ($null -ne $target) { $target.Dispose() }
}
