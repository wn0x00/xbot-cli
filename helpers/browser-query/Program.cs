using System.IO;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Loader;
using System.Text.Json;
using ShadowBot.Common;
using ShadowBot.Common.Utilities;
using ShadowBot.Runtime;
using ShadowBot.UIAutomation.Utilities;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            if (args.Length != 1)
                throw new ArgumentException("Expected the ShadowBot version directory.");
            string directory = Path.GetFullPath(args[0]);
            foreach (string name in new[] { "ShadowBot.Common", "ShadowBot.Runtime", "ShadowBot.UIAutomation" })
                if (!File.Exists(Path.Combine(directory, name + ".dll")))
                    throw new FileNotFoundException("Missing installed assembly: " + name, directory);

            AssemblyLoadContext.Default.Resolving += (context, name) =>
            {
                string file = Path.Combine(directory, name.Name + ".dll");
                return File.Exists(file) ? context.LoadFromAssemblyPath(file) : null;
            };
            AssemblyLoadContext.Default.ResolvingUnmanagedDll += (_, name) =>
            {
                string file = Path.Combine(directory, name);
                if (!File.Exists(file) && !name.EndsWith(".dll", StringComparison.OrdinalIgnoreCase))
                    file += ".dll";
                return File.Exists(file) ? NativeLibrary.Load(file) : IntPtr.Zero;
            };
            Directory.SetCurrentDirectory(directory);
            Environment.SetEnvironmentVariable("PATH", directory + ";" + Environment.GetEnvironmentVariable("PATH"));
            Write(new { ok = true, data = Query() });
            return 0;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error);
            Write(new { ok = false, error = error.Message });
            return 1;
        }
    }

    private static void Write(object value) =>
        Console.WriteLine(JsonSerializer.Serialize(value, new JsonSerializerOptions
        {
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase
        }));

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static object Query()
    {
        XbotEnv.Initialize(false);
        var browsers = new List<object>();
        foreach (var (type, name, info) in new (PluginType, string, ChromiumBrowserInfo?)[]
        {
            (PluginType.chrome, "Google Chrome", BuildInBrowserInfoHelper.Chrome),
            (PluginType.edge, "Microsoft Edge", BuildInBrowserInfoHelper.Edge),
            (PluginType.bro360, "360 Browser", BuildInBrowserInfoHelper.Bro360),
            (PluginType.firefox, "Mozilla Firefox", null)
        })
        {
            bool installed = PluginManager.IsInstalledExtension(type);
            browsers.Add(new
            {
                id = type.ToString(),
                type = type.ToString(),
                name,
                installed = (bool?)installed,
                registered = false,
                status = installed ? "installed" : "not-installed",
                exePath = info?.ExePath,
                pipeName = info?.PipeName,
                webSupported = info != null && !string.IsNullOrWhiteSpace(info.PipeName),
                source = "PluginManager.IsInstalledExtension"
            });
        }
        foreach (ChromiumBrowserInfo info in ChromiumConfigHelper.CustomBrowserInfos)
        {
            browsers.Add(new
            {
                id = info.ProductName,
                type = "customBrowser",
                name = string.IsNullOrWhiteSpace(info.AliasName) ? info.ProductName : info.AliasName,
                productName = info.ProductName,
                installed = (bool?)null,
                registered = true,
                status = "registered",
                exePath = info.ExePath,
                executableExists = File.Exists(info.ExePath),
                pipeName = info.PipeName,
                webSupported = !string.IsNullOrWhiteSpace(info.PipeName),
                source = "ChromiumConfigHelper.CustomBrowserInfos"
            });
        }
        return new { browsers };
    }
}
