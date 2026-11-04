using System;
using System.Reflection;
using System.Linq;

namespace XbotInjectee;

internal static class ActivityInstallGuard
{
    public static void RequireConsistent(string code, string version, string[] before, string[] memory, string[] disk, string installedVersion)
    {
        var prefix = code + "==";
        var expected = new[] {prefix + version};
        if (memory == null || disk == null || installedVersion != version ||
            !memory.Where(x=>x.StartsWith(prefix, StringComparison.Ordinal)).SequenceEqual(expected) ||
            !disk.Where(x=>x.StartsWith(prefix, StringComparison.Ordinal)).SequenceEqual(expected))
            throw new InvalidOperationException("installed files and memory/disk dependency versions are not consistent; inspect before retrying");
        var others = (before ?? Array.Empty<string>()).Where(x=>!x.StartsWith(prefix, StringComparison.Ordinal)).OrderBy(x=>x).ToArray();
        if (!others.SequenceEqual(memory.Where(x=>!x.StartsWith(prefix, StringComparison.Ordinal)).OrderBy(x=>x)) ||
            !others.SequenceEqual(disk.Where(x=>!x.StartsWith(prefix, StringComparison.Ordinal)).OrderBy(x=>x)))
            throw new InvalidOperationException("unrelated dependencies changed during activity installation");
    }
    private static object Read(object manager, string name) => manager?.GetType()
        .GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)?.GetValue(manager);
    public static object Snapshot(object manager) => new {
        engineStatus=Read(manager,"EngineStatus")?.ToString(),
        isRunning=Read(manager,"IsRunning"), processIsPaused=Read(manager,"ProcessIsPaused") };
    public static void RequireIdle(object engineManager)
    {
        var status = Read(engineManager,"EngineStatus")?.ToString();
        var running = Read(engineManager,"IsRunning");
        var paused = Read(engineManager,"ProcessIsPaused");
        if (!(running is bool isRunning) || !(paused is bool isPaused) ||
            isRunning || isPaused || !(string.Equals(status, "none", StringComparison.OrdinalIgnoreCase) ||
                                      string.Equals(status, "idle", StringComparison.OrdinalIgnoreCase)))
            throw new InvalidOperationException("activity installation requires a confirmed idle engine; status=" +
                (status ?? "unknown") + ", running=" + (running?.ToString() ?? "unknown") +
                ", paused=" + (paused?.ToString() ?? "unknown"));
    }
}
