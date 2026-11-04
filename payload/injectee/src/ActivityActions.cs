using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text.RegularExpressions;
using System.Threading.Tasks;
using System.Windows;

namespace XbotInjectee;

internal static class ActivityActions
{
    private static Type TypeOf(string name) => AppDomain.CurrentDomain.GetAssemblies()
        .Select(a => a.GetType(name, false)).FirstOrDefault(t => t != null)
        ?? throw new NotSupportedException("client type missing: " + name);
    private static object Get(object value, string name) => value?.GetType().GetProperty(name)?.GetValue(value);
    private static object Service(string name)
    {
        var factory = TypeOf("Caliburn.Micro.IoC").GetField("GetInstance", BindingFlags.Public | BindingFlags.Static)
            ?.GetValue(null) as Func<Type, string, object>;
        return factory?.Invoke(TypeOf(name), null) ?? throw new NotSupportedException("client service unavailable: " + name);
    }
    private static T OnUi<T>(Func<T> work)
    {
        var app = Application.Current ?? throw new InvalidOperationException("no live client application");
        if (app.Dispatcher.CheckAccess()) return work();
        var op = app.Dispatcher.InvokeAsync(work);
        if (!op.Task.Wait(TimeSpan.FromSeconds(10)))
        {
            if (op.Abort()) throw new TimeoutException("UI busy; queued operation cancelled");
            throw new TimeoutException("operation started; outcome unknown, inspect before retrying");
        }
        return op.Task.GetAwaiter().GetResult();
    }
    public static object Probe() => OnUi(() =>
    {
        var manager = Service("ShadowBot.Shell.Console.Common.IStudioManager");
        var studio = Get(manager, "Studio");
        var template = Path.Combine(AppContext.BaseDirectory, "FlowTemplates", "activity.zip");
        var factory = TypeOf("ShadowBot.Runtime.Packages.TemplatePackageFactory");
        var apiType = TypeOf("ShadowBot.Components.Protocol.Swagger.IApiClient");
        var api = Service(apiType.FullName);
        if (!File.Exists(template)) throw new FileNotFoundException("activity template not found", template);
        if (factory.GetConstructor(new[] { typeof(string), apiType }) == null)
            throw new NotSupportedException("TemplatePackageFactory signature changed");
        return (object)new { ok = true, template, factory = factory.FullName, manager = manager.GetType().FullName,
            apiClient = api.GetType().FullName, isBusy = Get(manager, "IsBusy"), hasStudio = studio != null,
            studioId = Get(Get(studio, "Info"), "UUID") };
    });
    public static object Create(string name, string code, string description)
    {
        if (string.IsNullOrWhiteSpace(name) || !Regex.IsMatch(code ?? "", "^activity_[a-z][a-z0-9_]+$") ||
            Regex.IsMatch(code ?? "", "^activity_[0-9a-f]{8}$"))
            throw new ArgumentException("name and semantic activity_code are required");
        var uuid = CreateWorkspace("activity.zip");
        return StudioActions.ConfigureNewActivity(uuid, name.Trim(), code, description ?? "");
    }
    public static object CreateConsumer(string name, string description)
    {
        if (string.IsNullOrWhiteSpace(name)) throw new ArgumentException("consumer name required");
        var uuid = CreateWorkspace("blank.zip");
        return StudioActions.ConfigureNewConsumer(uuid, name.Trim(), description ?? "");
    }
    private static string CreateWorkspace(string templateName)
    {
        var pending = OnUi(() =>
        {
            var manager = Service("ShadowBot.Shell.Console.Common.IStudioManager");
            if (Get(manager, "Studio") != null || (Get(manager, "IsBusy") is bool busy && busy))
                throw new InvalidOperationException("close the current Studio before creating an activity");
            var api = Service("ShadowBot.Components.Protocol.Swagger.IApiClient");
            var template = Path.Combine(AppContext.BaseDirectory, "FlowTemplates", templateName);
            if (!File.Exists(template)) throw new FileNotFoundException("activity template not found", template);
            var factory = Activator.CreateInstance(TypeOf("ShadowBot.Runtime.Packages.TemplatePackageFactory"), template, api);
            var options = Activator.CreateInstance(TypeOf("ShadowBot.Shell.Console.Common.CreateStudioOptions"));
            var method = manager.GetType().GetMethods().Single(m => m.Name == "CreateStudioAsync" && m.GetParameters().Length == 2);
            return method.Invoke(manager, new[] { factory, options }) as Task
                ?? throw new InvalidOperationException("CreateStudioAsync did not return a task");
        });
        // Never wait for an async UI operation from the dispatcher itself.
        pending.WaitAsync(TimeSpan.FromSeconds(150)).GetAwaiter().GetResult();
        var studioResult = pending.GetType().GetProperty("Result")?.GetValue(pending)
            ?? throw new InvalidOperationException("activity creation returned no Studio; inspect state before retrying");
        var uuid = Get(Get(studioResult, "Workspace"), "UUID") as string
            ?? Get(Get(studioResult, "Info"), "UUID") as string;
        if (string.IsNullOrEmpty(uuid)) throw new InvalidOperationException("created Studio has no UUID");
        return uuid;
    }
}
