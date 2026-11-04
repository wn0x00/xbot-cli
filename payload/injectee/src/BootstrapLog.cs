using System;
using System.IO;

namespace XbotInjectee;

internal static class BootstrapLog
{
    private static readonly string Path =
        System.IO.Path.Combine(Environment.GetEnvironmentVariable("TEMP") ?? ".",
            "xbot_injectee.log");
    private static readonly object Lock = new();

    public static void Info(string msg) => Write("INFO ", msg);
    public static void Error(string msg) => Write("ERROR", msg);

    private static void Write(string lvl, string msg)
    {
        try
        {
            lock (Lock)
            {
                File.AppendAllText(Path, $"{DateTime.Now:HH:mm:ss.fff} [{lvl}] {msg}\r\n");
            }
        }
        catch { /* swallow — logging must never throw */ }
    }
}
