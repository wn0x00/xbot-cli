# Enterprise WeChat has a separate vendor entry: Native\x86\misc.exe wxwork <HWND>.
# Its exit code and RpaBugFixFlags marker alone do not establish activation.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$request = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($requestBase64)) | ConvertFrom-Json
$state = [ordered]@{
    pid = [uint32]$request.pid
    hwnd = ('0x{0:x}' -f [long]$request.hwnd)
    backend = 'shadowbot_misc_wxwork'
    status = 'not_started'
    install_called = $false
    helper_called = $false
    helper_path = [string]$request.helper_path
    target_architecture = [string]$request.target_architecture
    worker_architecture = $(if ([Environment]::Is64BitProcess) { 'x64' } else { 'x86' })
    hook_verification = 'not_checked'
    tree_verification = 'not_checked'
    restore = $false
}
$code = 'WIN32_ACTIVATION_PROVIDER_ERROR'
$target = $null; $mutex = $null; $locked = $false; $helper = $null
try {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class XbotWxWorkActivation {
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Ansi)] public static extern IntPtr GetPropA(IntPtr hwnd, string name);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeoutW(IntPtr hwnd, uint msg, UIntPtr wp, IntPtr lp, uint flags, uint timeout, out UIntPtr result);
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool GetProcessTimes(IntPtr process, out long created, out long exited, out long kernel, out long user);
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool IsWow64Process2(IntPtr process, out ushort processMachine, out ushort nativeMachine);
    [DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    public static IntPtr WindowHandle(ulong value) {
        if (value > UInt32.MaxValue) throw new ArgumentOutOfRangeException("value");
        return new IntPtr(unchecked((int)value));
    }
    // 0x2b66 is the vendor hook's echo probe, recovered from misc.exe.
    public static bool Probe(IntPtr hwnd, uint nonce) {
        UIntPtr reply;
        return SendMessageTimeoutW(hwnd, 0x2b66, UIntPtr.Zero, new IntPtr(nonce), 2, 1000, out reply) != IntPtr.Zero
            && reply.ToUInt64() == nonce;
    }
}
'@
    $code = 'WIN32_TARGET_ARCH_MISMATCH'
    if ([Environment]::Is64BitProcess -or [string]$request.target_architecture -cne 'x86') { throw 'misc wxwork requires an x86 target and worker' }
    $code = 'WIN32_TARGET_MISMATCH'
    $handle = [XbotWxWorkActivation]::WindowHandle([uint64]$request.hwnd)
    function Assert-Target {
        [uint32]$owner = 0
        if (-not [XbotWxWorkActivation]::IsWindow($handle)) { throw 'Window does not exist' }
        $null = [XbotWxWorkActivation]::GetWindowThreadProcessId($handle, [ref]$owner)
        $class = New-Object Text.StringBuilder(256)
        $null = [XbotWxWorkActivation]::GetClassNameW($handle, $class, $class.Capacity)
        if ($owner -ne [uint32]$request.pid -or $class.ToString() -cne 'WeWorkWindow') { throw 'Window identity changed' }
        if ($null -ne $target -and [XbotWxWorkActivation]::WaitForSingleObject($target.Handle, 0) -ne 258) { throw 'Selected process instance exited' }
    }
    Assert-Target
    $code = 'WIN32_TARGET_ACCESS_FAILED'
    $target = [Diagnostics.Process]::GetProcessById([int]$request.pid)
    $null = $target.Handle
    [long]$created = 0; [long]$exited = 0; [long]$kernel = 0; [long]$userTime = 0
    if (-not [XbotWxWorkActivation]::GetProcessTimes($target.Handle, [ref]$created, [ref]$exited, [ref]$kernel, [ref]$userTime)) {
        throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error()))
    }
    $code = 'WIN32_TARGET_MISMATCH'
    $instance = $created.ToString([Globalization.CultureInfo]::InvariantCulture)
    if ($instance -cne [string]$request.process_creation_time) { throw 'Selected process instance changed' }
    $state.process_creation_time = $instance
    $start = $target.StartTime.ToUniversalTime()
    $state.process_started_at = $start.ToString('o')
    $state.exe_path = $target.MainModule.FileName
    if ([IO.Path]::GetFileName($state.exe_path) -ine 'WXWork.exe') { throw 'misc wxwork is restricted to WXWork.exe' }
    $code = 'WIN32_TARGET_ACCESS_FAILED'
    [uint16]$processMachine = 0; [uint16]$nativeMachine = 0
    if (-not [XbotWxWorkActivation]::IsWow64Process2($target.Handle, [ref]$processMachine, [ref]$nativeMachine)) {
        throw (New-Object ComponentModel.Win32Exception([Runtime.InteropServices.Marshal]::GetLastWin32Error()))
    }
    $code = 'WIN32_TARGET_ARCH_MISMATCH'
    if ($processMachine -ne 0x014c -or $nativeMachine -ne 0x8664) { throw 'misc wxwork only supports an x86 target on Windows x64' }
    $mutex = New-Object Threading.Mutex($false, ('Local\xbot-cli-tree-{0}-{1}' -f $request.pid, $start.Ticks))
    try { $locked = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $locked = $true }
    if (-not $locked) { $code = 'WIN32_ACTIVATION_BUSY'; throw 'Activation of this process is already in progress' }
    $code = 'WIN32_TARGET_MISMATCH'
    Assert-Target
    function Test-Hook {
        for ($i = 0; $i -lt 2; $i++) {
            $nonce = [uint32](Get-Random -Minimum 10000000 -Maximum 2000000000)
            if (-not [XbotWxWorkActivation]::Probe($handle, $nonce)) { return $false }
        }
        return $true
    }
    $state.vendor_marker = [XbotWxWorkActivation]::GetPropA($handle, 'RpaBugFixFlags').ToInt64()
    $code = 'WIN32_ACTIVATION_STATE_FAILED'
    $stateDirectory = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'xbot-cli\win32-activation'
    $receipt = Join-Path $stateDirectory ('wxwork-{0}-{1}.json' -f $request.pid, $instance)
    if ($state.vendor_marker -ne 0 -and (Test-Hook)) {
        $state.status = 'already_active'
        $state.hook_verification = 'confirmed_by_message'
    } elseif ($state.vendor_marker -ne 0 -or [IO.File]::Exists($receipt)) {
        $state.status = 'outcome_unknown'
        $state.hook_verification = 'not_confirmed'
        $code = 'WIN32_ACTIVATION_UNVERIFIED'
        throw 'Previous activation was recorded but the vendor hook cannot be confirmed; installation was not repeated'
    } else {
        $code = 'WIN32_ACTIVATION_HELPER_MISSING'
        if (-not [IO.File]::Exists($state.helper_path)) { throw 'Native\x86\misc.exe is missing' }
        $code = 'WIN32_ACTIVATION_STATE_FAILED'
        $null = [IO.Directory]::CreateDirectory($stateDirectory)
        # Record before starting, so a worker timeout cannot permit blind reinjection.
        [IO.File]::WriteAllText($receipt, (@{pid=$state.pid; hwnd=$state.hwnd; process_creation_time=$instance; status='outcome_unknown'} | ConvertTo-Json -Compress))
        $info = New-Object Diagnostics.ProcessStartInfo
        $info.FileName = $state.helper_path
        # Never use HWND 0, which makes the helper search for another window.
        # %i parses a signed 32-bit HWND; preserve its bits for high-bit handles.
        $info.Arguments = 'wxwork ' + $handle.ToInt32().ToString([Globalization.CultureInfo]::InvariantCulture)
        $info.UseShellExecute = $false
        $info.CreateNoWindow = $true
        $code = 'WIN32_ACTIVATION_HELPER_FAILED'
        try { $helper = [Diagnostics.Process]::Start($info) } catch { [IO.File]::Delete($receipt); throw }
        $state.helper_pid = $helper.Id
        $state.install_called = $true
        $state.helper_called = $true
        $state.status = 'outcome_unknown'
        if (-not $helper.WaitForExit(10000)) {
            # The vendor briefly suspends its target: never kill it mid-operation.
            $state.helper_still_running = $true
            throw 'misc.exe has not exited; outcome unknown, do not repeat activation'
        }
        $state.return_code = $helper.ExitCode
        $code = 'WIN32_TARGET_MISMATCH'
        Assert-Target
        $state.vendor_marker = [XbotWxWorkActivation]::GetPropA($handle, 'RpaBugFixFlags').ToInt64()
        $state.hook_verification = 'not_confirmed'
        $code = 'WIN32_ACTIVATION_UNVERIFIED'
        if ($helper.ExitCode -ne 0 -or $state.vendor_marker -eq 0 -or -not (Test-Hook)) {
            throw 'misc.exe did not establish a verifiable vendor hook; exit code 0 alone does not prove activation'
        }
        $state.status = 'activated'
        $state.hook_verification = 'confirmed_by_message'
        [IO.File]::WriteAllText($receipt, ($state | ConvertTo-Json -Depth 6 -Compress))
    }
    @{ok=$true; data=$state} | ConvertTo-Json -Depth 6 -Compress
} catch {
    @{ok=$false; code=$code; message=$_.Exception.Message; data=$state} | ConvertTo-Json -Depth 6 -Compress
    exit 1
} finally {
    if ($null -ne $helper) { $helper.Dispose() }
    if ($locked) { $mutex.ReleaseMutex() }
    if ($null -ne $mutex) { $mutex.Dispose() }
    if ($null -ne $target) { $target.Dispose() }
}
