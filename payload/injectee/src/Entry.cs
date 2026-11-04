using System;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.Loader;
using System.Threading;

namespace XbotInjectee;

/// <summary>
/// Two-stage entry. hostfxr loads this assembly into a *fresh, isolated*
/// AssemblyLoadContext — which cannot see Shell's already-resolved
/// Caliburn.Micro IoC or any ShadowBot.* singletons. Stage 1 (called via
/// UnmanagedCallersOnly) reflectively reloads this assembly into
/// AssemblyLoadContext.Default where Shell's live services live, then invokes
/// Stage 2 inside that ALC. Stage 2 does the real work.
/// </summary>
public static class Entry
{
    private static int _started;

    [UnmanagedCallersOnly]
    public static int Start(IntPtr arg, int argLength)
    {
        try
        {
            var asmPath = typeof(Entry).Assembly.Location;
            BootstrapLog.Info($"stage1: in isolated ALC, asm={asmPath}");

            Assembly defaultAsm = null;
            var asmName = typeof(Entry).Assembly.GetName().Name;
            foreach (var a in AssemblyLoadContext.Default.Assemblies)
            {
                if (a.GetName().Name == asmName) { defaultAsm = a; break; }
            }
            defaultAsm ??= AssemblyLoadContext.Default.LoadFromAssemblyPath(asmPath);

            var realType = defaultAsm.GetType("XbotInjectee.Entry", throwOnError: true);
            var realMethod = realType.GetMethod(nameof(StartReal),
                BindingFlags.Public | BindingFlags.Static)
                ?? throw new MissingMethodException("StartReal");

            BootstrapLog.Info("stage1: invoking StartReal in Default ALC");
            var rc = (int)realMethod.Invoke(null, new object[] { arg.ToInt64(), argLength });
            BootstrapLog.Info($"stage1: StartReal returned {rc}");
            return rc;
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"stage1 failed: {ex}");
            return 100;
        }
    }

    /// <summary>Runs in Default ALC. Boots the HTTP listener on a background thread.</summary>
    public static int StartReal(long argPtrAsLong, int argLength)
    {
        if (Interlocked.CompareExchange(ref _started, 1, 0) != 0)
        {
            BootstrapLog.Info("stage2: already started, no-op");
            return 2;
        }
        try
        {
            int port = 5057;
            if (argPtrAsLong != 0 && argLength > 0)
            {
                var bytes = new byte[argLength];
                Marshal.Copy(new IntPtr(argPtrAsLong), bytes, 0, argLength);
                if (int.TryParse(System.Text.Encoding.ASCII.GetString(bytes), out var p))
                    port = p;
            }
            var t = new Thread(() => {try{HttpHost.Run(port);}finally{Interlocked.Exchange(ref _started,0);}})
            {
                IsBackground = true,
                Name = "XbotInjectee-Http",
            };
            t.Start();
            // Connecting management commands must not start background edits.
            // Flow reload remains available through the explicit CLI command.
            BootstrapLog.Info($"stage2: http listener thread spawned on port {port}");
            return 0;
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"stage2 failed: {ex}");
            return 101;
        }
    }
}
