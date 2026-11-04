using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Windows;
using System.Windows.Media;
using Caliburn.Micro;

namespace XbotInjectee;

/// <summary>
/// Console-mode driver: enqueues runs through ShadowBot's <c>ITaskManager</c>
/// rather than the Studio toolbar VM. Studio runs a single open project via
/// EngineManager + EngineCreateOptions; Console schedules any *published*
/// app (by UUID) onto a task queue, which then dispatches to the same
/// IEngineComponent. The two paths coexist in one Shell process.
///
/// All work goes through reflection — we deliberately avoid compile-time
/// references to ShadowBot.Runtime.Console types so this assembly stays
/// loadable when those internals shift between Studio versions.
/// </summary>
internal static class ConsoleActions
{
    public record Result(bool ok, string detail, object data = null);

    /// <summary>
    /// Enqueue an app for execution. <paramref name="appId"/> is the app's
    /// published UUID (the same one shown in the Console "我的应用" panel).
    /// We tag SourceKind=Assistant + SourceName/SourceId="xbot-cli" so the
    /// task is identifiable in storage — TaskSourceKind has no "Cli" member
    /// to add, and Assistant is the closest semantically-external bucket.
    /// </summary>
    public static Result EnqueueTask(
        string appId,
        string appName,
        Dictionary<string, object> inputs,
        Dictionary<string, string> appParams)
    {
        if (string.IsNullOrEmpty(appId))
            return new Result(false, "appId is required");
        try
        {
            return OnUi(() =>
            {
                var tm = ResolveTaskManager();
                if (tm == null) return new Result(false, "ITaskManager not found via IoC");

                var settingsType = ResolveType("ShadowBot.Runtime.Console.Tasks.TaskSettings");
                if (settingsType == null) return new Result(false, "TaskSettings type missing");
                var settings = Activator.CreateInstance(settingsType);

                Set(settings, "AppId", appId);
                if (!string.IsNullOrEmpty(appName)) Set(settings, "AppName", appName);

                var skType = ResolveType("ShadowBot.Runtime.Console.Tasks.TaskSourceKind");
                if (skType != null)
                {
                    try { Set(settings, "SourceKind", Enum.Parse(skType, "Assistant")); }
                    catch (Exception ex) { BootstrapLog.Error("set SourceKind Assistant failed: " + ex.Message); }
                }
                Set(settings, "SourceId", "xbot-cli");
                Set(settings, "SourceName", "xbot-cli");
                // QueueWhenBusy = true so the call doesn't get rejected if
                // there's a task already running — the CLI user expects to
                // wait, not be told "busy try again".
                Set(settings, "QueueWhenBusy", true);
                if (inputs != null && inputs.Count > 0) Set(settings, "Inputs", inputs);
                if (appParams != null && appParams.Count > 0) Set(settings, "AppParams", appParams);

                var enqueue = tm.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance)
                    .FirstOrDefault(m => m.Name == "EnqueueTask" && m.GetParameters().Length == 2);
                if (enqueue == null) return new Result(false, "ITaskManager.EnqueueTask(settings,bool) missing");

                BootstrapLog.Info($"console.enqueue app={appId} name={appName ?? ""} inputs={inputs?.Count ?? 0}");
                var ret = enqueue.Invoke(tm, new object[] { settings, false });
                if (ret == null) return new Result(false, "EnqueueTask returned null");

                var rt = ret.GetType();
                var accepted = (bool)(rt.GetProperty("Accepted")?.GetValue(ret) ?? false);
                var reasonCode = rt.GetProperty("ReasonCode")?.GetValue(ret) as string;
                var reasonMsg = rt.GetProperty("ReasonMessage")?.GetValue(ret) as string;
                var taskId = rt.GetProperty("TaskId")?.GetValue(ret) as string;
                if (!accepted)
                    return new Result(false, $"rejected ({reasonCode}): {reasonMsg}");
                BootstrapLog.Info($"console.enqueue OK taskId={taskId}");
                return new Result(true, "queued", new { taskId });
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"ConsoleActions.EnqueueTask: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    public static Result RunWithCliSource(string appId, string appName, Dictionary<string, object> inputs)
    {
        var r = RunByConsoleProtocol(appId, inputs);
        if (r.ok) ScheduleCliSourceTag(appId);
        return r;
    }

    private static Result RunByConsoleProtocol(string appId, Dictionary<string, object> inputs)
    {
        if (string.IsNullOrEmpty(appId))
            return new Result(false, "appId is required");
        try
        {
            return OnUi(() =>
            {
                var main = FindConsoleMainVm();
                if (main == null) return new Result(false, "Console MainViewModel not found");
                var startKindType = ResolveType("ShadowBot.Runtime.StartKind");
                if (startKindType == null) return new Result(false, "ShadowBot.Runtime.StartKind type missing");
                var runKind = Enum.Parse(startKindType, "Run");
                var method = main.GetType().GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                    .FirstOrDefault(m => m.Name == "ConsoleUrlProtocal" && m.GetParameters().Length == 4);
                if (method == null) return new Result(false, "MainViewModel.ConsoleUrlProtocal missing");
                BootstrapLog.Info($"console.protocol-run app={appId} inputs={inputs?.Count ?? 0}");
                method.Invoke(main, new object[] { appId, null, runKind, inputs });
                return new Result(true, "started", new { taskId = "" });
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"ConsoleActions.RunByConsoleProtocol: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    private static void ScheduleCliSourceTag(string appId)
    {
        System.Threading.ThreadPool.QueueUserWorkItem(_ =>
        {
            for (var i = 0; i < 30; i++)
            {
                try
                {
                    if (OnUi(() => TryTagCliSource(appId)))
                    {
                        BootstrapLog.Info($"console.source tagged xbot-cli app={appId}");
                        return;
                    }
                }
                catch (Exception ex)
                {
                    BootstrapLog.Error("TagCliSource: " + ex.Message);
                }
                System.Threading.Thread.Sleep(100);
            }
            BootstrapLog.Error($"console.source tag missed app={appId}");
        });
    }

    private static bool TryTagCliSource(string appId)
    {
        var tm = ResolveTaskManager();
        if (tm == null) return false;

        var current = tm.GetType().GetProperty("CurrentTask")?.GetValue(tm);
        var info = current?.GetType().GetProperty("TaskInfo")?.GetValue(current);
        if (TaskInfoMatchesApp(info, appId))
            return TagTaskInfo(info);

        return false;
    }

    private static bool TagTaskInfo(object info)
    {
        if (info == null) return false;
        Set(info, "SourceId", "xbot-cli");
        Set(info, "SourceName", "xbot-cli");

        var skType = ResolveType("ShadowBot.Runtime.Console.Tasks.TaskSourceKind");
        if (skType != null)
        {
            try { Set(info, "SourceKind", Enum.Parse(skType, "Assistant")); }
            catch (Exception ex) { BootstrapLog.Error("set TaskInfo.SourceKind Assistant failed: " + ex.Message); }
        }

        var settings = info.GetType().GetProperty("TaskSettings", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            ?.GetValue(info);
        if (settings != null)
        {
            Set(settings, "SourceId", "xbot-cli");
            Set(settings, "SourceName", "xbot-cli");
            if (skType != null)
            {
                try { Set(settings, "SourceKind", Enum.Parse(skType, "Assistant")); }
                catch (Exception ex) { BootstrapLog.Error("set TaskSettings.SourceKind Assistant failed: " + ex.Message); }
            }
        }
        return true;
    }

    public static Result OpenDevelopedAppInStudio(string appId)
    {
        if (string.IsNullOrEmpty(appId))
            return new Result(false, "appId is required");
        try
        {
            return OnUi(() =>
            {
                var main = FindConsoleMainVm();
                if (main == null) return new Result(false, "Console MainViewModel not found");
                var openType = ResolveType("ShadowBot.Shell.Console.Services.ApiGateway.OperatorAppOpenRequest");
                var actionType = ResolveType("ShadowBot.Shell.Console.ViewModels.Message.ApiOperatorAction");
                var messageType = ResolveType("ShadowBot.Shell.Console.ViewModels.Message.ApiOperatorMessage");
                if (openType == null || actionType == null || messageType == null)
                    return new Result(false, "ApiOperator open types missing");

                var openReq = Activator.CreateInstance(openType);
                Set(openReq, "AppId", appId);
                var action = Enum.Parse(actionType, "OpenDevelopedAppInStudio");
                var message = OperatorMessages.Create(messageType, action, false, "appOpen", openReq);
                var handle = main.GetType().GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                    .FirstOrDefault(m => m.Name == "Handle" && m.GetParameters().Length == 1
                        && m.GetParameters()[0].ParameterType == messageType);
                if (handle == null) return new Result(false, "MainViewModel.Handle(ApiOperatorMessage) missing");
                BootstrapLog.Info($"studio.open-app app={appId}");
                handle.Invoke(main, new object[] { message });
                return new Result(true, "open requested", new { appId });
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"ConsoleActions.OpenDevelopedAppInStudio: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    public static Result PublishDevelopedApp(string appId, string updateLog, TimeSpan timeout)
    {
        object message = null;
        try
        {
            var setup = OnUi(() =>
            {
                var main = FindConsoleMainVm();
                if (main == null) return new Result(false, "Console MainViewModel not found");
                var publishType = ResolveType("ShadowBot.Shell.Console.Services.ApiGateway.OperatorAppPublishRequest");
                var actionType = ResolveType("ShadowBot.Shell.Console.ViewModels.Message.ApiOperatorAction");
                var messageType = ResolveType("ShadowBot.Shell.Console.ViewModels.Message.ApiOperatorMessage");
                if (publishType == null || actionType == null || messageType == null)
                    return new Result(false, "ApiOperator publish types missing");

                if (string.IsNullOrEmpty(appId))
                    return new Result(false, "appId is required");

                var publishReq = Activator.CreateInstance(publishType);
                Set(publishReq, "AppId", appId);
                Set(publishReq, "UpdateLog", updateLog ?? "");
                var action = Enum.Parse(actionType, "PublishDevelopedApp");
                message = OperatorMessages.Create(messageType, action, false, "appPublish", publishReq);
                var handle = main.GetType().GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                    .FirstOrDefault(m => m.Name == "Handle" && m.GetParameters().Length == 1
                        && m.GetParameters()[0].ParameterType == messageType);
                if (handle == null) return new Result(false, "MainViewModel.Handle(ApiOperatorMessage) missing");
                BootstrapLog.Info($"studio.publish app={appId} updateLogLen={updateLog?.Length ?? 0}");
                handle.Invoke(main, new object[] { message });
                return new Result(true, "publish requested", new { appId });
            });
            if (!setup.ok) return setup;

            var result = WaitForOperatorResult(message, timeout);
            if (result == null) return new Result(false, "publish timed out");
            var success = (bool)(result.GetType().GetProperty("Success")?.GetValue(result) ?? true);
            var code = result.GetType().GetProperty("Code")?.GetValue(result)?.ToString();
            var msg = result.GetType().GetProperty("Message")?.GetValue(result)?.ToString();
            if (!success) return new Result(false, $"{code}: {msg}".Trim(':', ' '), new { code, message = msg });
            return new Result(true, string.IsNullOrEmpty(msg) ? "published" : msg, new { code, message = msg });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"ConsoleActions.PublishDevelopedApp: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    public static Result ReturnToConsole(bool force)
    {
        object message = null;
        try
        {
            var setup = OnUi(() =>
            {
                var main = FindConsoleMainVm();
                if (main == null) return new Result(false, "Console MainViewModel not found");
                var actionType = ResolveType("ShadowBot.Shell.Console.ViewModels.Message.ApiOperatorAction");
                var messageType = ResolveType("ShadowBot.Shell.Console.ViewModels.Message.ApiOperatorMessage");
                if (actionType == null || messageType == null)
                    return new Result(false, "ApiOperator message types missing");

                var action = Enum.Parse(actionType, "SyncCloseCurrentStudioApp");
                message = OperatorMessages.Create(messageType, action, force);
                var handle = main.GetType().GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                    .FirstOrDefault(m => m.Name == "Handle" && m.GetParameters().Length == 1
                        && m.GetParameters()[0].ParameterType == messageType);
                if (handle == null) return new Result(false, "MainViewModel.Handle(ApiOperatorMessage) missing");
                BootstrapLog.Info($"console.return force={force}");
                handle.Invoke(main, new object[] { message });
                return new Result(true, "return requested");
            });
            if (!setup.ok) return setup;

            var result = WaitForOperatorResult(message, TimeSpan.FromSeconds(30));
            if (result == null) return new Result(true, "return requested");
            var success = (bool)(result.GetType().GetProperty("Success")?.GetValue(result) ?? true);
            var code = result.GetType().GetProperty("Code")?.GetValue(result)?.ToString();
            var msg = result.GetType().GetProperty("Message")?.GetValue(result)?.ToString();
            if (!success && code == "studio_not_open")
                return new Result(true, "already in console", new { code, message = msg });
            if (!success) return new Result(false, $"{code}: {msg}".Trim(':', ' '), new { code, message = msg });
            return new Result(true, string.IsNullOrEmpty(msg) ? "returned" : msg, new { code, message = msg });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"ConsoleActions.ReturnToConsole: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    /// <summary>Stop a running task by id (the value we return from
    /// EnqueueTask). Reason is recorded against the task for audit.</summary>
    public static Result StopTask(string taskId, string reason)
    {
        if (string.IsNullOrEmpty(taskId))
            return new Result(false, "taskId is required");
        try
        {
            return OnUi(() =>
            {
                var tm = ResolveTaskManager();
                if (tm == null) return new Result(false, "ITaskManager not found via IoC");
                var m = tm.GetType().GetMethod("StopTaskById", BindingFlags.Public | BindingFlags.Instance);
                if (m == null) return new Result(false, "StopTaskById missing");
                var ret = m.Invoke(tm, new object[] { taskId, reason ?? "xbot-cli stop" });
                BootstrapLog.Info($"console.stop taskId={taskId} -> {ret}");
                return new Result(true, "stop requested", new { result = ret?.ToString() });
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"ConsoleActions.StopTask: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    public static Result CurrentTaskStatus(string taskId = null)
    {
        try
        {
            return OnUi(() =>
            {
                var tm = ResolveTaskManager();
                if (tm == null) return new Result(false, "ITaskManager not found via IoC");
                var isRunning = (bool)(tm.GetType().GetProperty("IsRunning")?.GetValue(tm) ?? false);
                var current = tm.GetType().GetProperty("CurrentTask")?.GetValue(tm);
                object snapshot = null;
                object info = null;
                if (current == null)
                {
                    info = FindQueuedTaskInfo(tm, taskId) ?? FindStoredTaskInfo(tm, taskId);
                }
                else
                {
                    info = current.GetType().GetProperty("TaskInfo")?.GetValue(current);
                    try
                    {
                        var m = current.GetType().GetMethod("GetRunningInstructionSnapshot",
                            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
                        snapshot = m?.Invoke(current, null);
                    }
                    catch { }
                    if (!TaskInfoMatches(info, taskId))
                        info = FindQueuedTaskInfo(tm, taskId) ?? FindStoredTaskInfo(tm, taskId) ?? info;
                }

                if (info == null)
                    return new Result(true, "idle", new { isRunning, matches = false, current = (object)null });

                var id = GetString(info, "Id");
                var status = GetValueString(info, "Status");
                var error = GetString(info, "Error");
                var appId = GetString(info, "AppId");
                var appName = GetString(info, "AppName");
                var description = GetString(info, "Description");

                var matches = string.IsNullOrEmpty(taskId) || string.IsNullOrEmpty(id) || id == taskId;
                return new Result(true, "ok", new
                {
                    isRunning,
                    matches,
                    current = new
                    {
                        id,
                        status,
                        error,
                        appId,
                        appName,
                        description,
                        snapshot = SnapshotData(snapshot),
                    }
                });
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"ConsoleActions.CurrentTaskStatus: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    private static bool TaskInfoMatches(object info, string taskId)
    {
        if (string.IsNullOrEmpty(taskId)) return true;
        var id = GetString(info, "Id");
        return string.IsNullOrEmpty(id) || id == taskId;
    }

    private static bool TaskInfoMatchesApp(object info, string appId)
    {
        if (info == null || string.IsNullOrEmpty(appId)) return false;
        return GetString(info, "AppId") == appId;
    }

    private static object FindStoredTaskInfo(object taskManager, string taskId)
    {
        if (string.IsNullOrEmpty(taskId)) return null;
        try
        {
            var storage = taskManager.GetType().GetProperty("Storage",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)?.GetValue(taskManager);
            var m = storage?.GetType().GetMethod("QueryTaskInfoById",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            return m?.Invoke(storage, new object[] { taskId });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error("FindStoredTaskInfo: " + ex.Message);
            return null;
        }
    }

    private static object FindQueuedTaskInfo(object taskManager, string taskId)
    {
        if (string.IsNullOrEmpty(taskId)) return null;
        try
        {
            var queue = GetField(taskManager, "_taskQueue") as IEnumerable;
            if (queue == null) return null;
            foreach (var item in queue)
            {
                if (TaskInfoMatches(item, taskId)) return item;
            }
        }
        catch (Exception ex)
        {
            BootstrapLog.Error("FindQueuedTaskInfo: " + ex.Message);
        }
        return null;
    }

    private static object ResolveTaskManager()
    {
        // IoC.Get<ITaskManager>() — the same registration the UI uses to
        // route "click Run" on the my-apps page.
        try
        {
            var iTaskMgr = ResolveType("ShadowBot.Shell.Console.Common.ITaskManager");
            if (iTaskMgr == null) return null;
            // IoC.Get is a Func<Type,string,object> static field at runtime.
            var iocType = Type.GetType("Caliburn.Micro.IoC, Caliburn.Micro");
            var fn = iocType?.GetField("GetInstance", BindingFlags.Public | BindingFlags.Static)
                ?.GetValue(null) as Func<Type, string, object>;
            return fn?.Invoke(iTaskMgr, null);
        }
        catch (Exception ex)
        {
            BootstrapLog.Error("ResolveTaskManager: " + ex.Message);
            return null;
        }
    }

    private static object FindConsoleMainVm()
    {
        var app = Application.Current;
        if (app == null) return null;
        var seen = new HashSet<object>();
        foreach (Window w in app.Windows)
        {
            if (IsConsoleMainVm(w.DataContext)) return w.DataContext;
            var found = FindConsoleMainVmInTree(w, seen);
            if (found != null) return found;
        }
        return null;
    }

    private static bool IsConsoleMainVm(object obj) =>
        obj?.GetType().FullName == "ShadowBot.Shell.Console.ViewModels.MainViewModel";

    private static object FindConsoleMainVmInTree(object node, HashSet<object> seen)
    {
        if (node == null || !seen.Add(node)) return null;

        if (node is FrameworkElement fe && IsConsoleMainVm(fe.DataContext))
            return fe.DataContext;
        if (node is FrameworkContentElement fce && IsConsoleMainVm(fce.DataContext))
            return fce.DataContext;

        if (node is DependencyObject dep)
        {
            try
            {
                var count = VisualTreeHelper.GetChildrenCount(dep);
                for (var i = 0; i < count; i++)
                {
                    var found = FindConsoleMainVmInTree(VisualTreeHelper.GetChild(dep, i), seen);
                    if (found != null) return found;
                }
            }
            catch { }

            try
            {
                foreach (var child in LogicalTreeHelper.GetChildren(dep))
                {
                    var found = FindConsoleMainVmInTree(child, seen);
                    if (found != null) return found;
                }
            }
            catch { }
        }

        return null;
    }

    private static object WaitForOperatorResult(object message, TimeSpan timeout)
    {
        if (message == null) return null;
        try
        {
            var wait = message.GetType().GetMethod("WaitForResult",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            if (wait == null) return null;
            var args = new object[] { timeout, null };
            var ok = (bool)(wait.Invoke(message, args) ?? false);
            return ok ? args[1] : null;
        }
        catch (Exception ex)
        {
            BootstrapLog.Error("WaitForOperatorResult: " + ex.Message);
            return null;
        }
    }

    private static void Set(object obj, string propName, object value)
    {
        var p = obj.GetType().GetProperty(propName, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
        if (p == null || !p.CanWrite) return;
        p.SetValue(obj, value);
    }

    private static string GetString(object obj, string propName) =>
        obj?.GetType().GetProperty(propName, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            ?.GetValue(obj)?.ToString();

    private static string GetValueString(object obj, string propName) =>
        GetString(obj, propName);

    private static object GetField(object obj, string name)
    {
        var f = obj?.GetType().GetField(name,
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
        return f?.GetValue(obj);
    }

    private static object SnapshotData(object snapshot)
    {
        if (snapshot == null) return null;
        var t = snapshot.GetType();
        object Get(string name) => t.GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            ?.GetValue(snapshot);
        return new
        {
            lineNo = Get("LineNo"),
            blockTitle = Get("BlockTitle")?.ToString(),
            flowName = Get("FlowName")?.ToString(),
            message = Get("Message")?.ToString(),
            updatedAtUtc = Get("UpdatedAtUtc")?.ToString(),
        };
    }

    private static Type ResolveType(string fullName)
    {
        foreach (var asm in AppDomain.CurrentDomain.GetAssemblies())
        {
            try
            {
                var t = asm.GetType(fullName, throwOnError: false);
                if (t != null) return t;
            }
            catch { }
        }
        return null;
    }

    private static T OnUi<T>(Func<T> work)
    {
        var app = Application.Current;
        if (app == null || app.Dispatcher.CheckAccess()) return work();
        return app.Dispatcher.Invoke(work);
    }
}
