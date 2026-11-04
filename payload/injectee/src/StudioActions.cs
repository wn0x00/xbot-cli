using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using ShadowBot.Components.Protocol.Engine;

namespace XbotInjectee;

/// <summary>
/// Reflectively invokes the toolbar VM that Studio's F5 / Pause / Stop buttons
/// are bound to. Calling these (rather than IEngineComponent directly) keeps
/// Studio's UI state in sync — tab status, log panel, button enablement etc.
/// </summary>
internal static class StudioActions
{
    private const string ToolBarType =
        "ShadowBot.Shell.Development.ViewModels.DevelopToolBarViewModel";
    private const int AutoReloadIntervalMs = 3000;
    private static Timer _autoReloadTimer;
    private static int _autoReloadStarted;
    private static int _autoReloadRunning;

    public record Result(bool ok, string detail, object data = null);

    public static void StartAutoReload()
    {
        if (Interlocked.CompareExchange(ref _autoReloadStarted, 1, 0) != 0)
            return;
        _autoReloadTimer = new Timer(_ => AutoReloadAllCodeFlows(),
            null,
            AutoReloadIntervalMs,
            AutoReloadIntervalMs);
        BootstrapLog.Info($"auto reload all CodeFlow every {AutoReloadIntervalMs}ms started");
    }

    private static void AutoReloadAllCodeFlows()
    {
        if (Interlocked.CompareExchange(ref _autoReloadRunning, 1, 0) != 0)
            return;
        try
        {
            var result = ReloadCodeFlows(null, true);
            if (!result.ok && !IsBenignAutoReloadMiss(result.detail))
                BootstrapLog.Error($"auto reload CodeFlow failed: {result.detail}");
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"auto reload CodeFlow: {ex}");
        }
        finally
        {
            Interlocked.Exchange(ref _autoReloadRunning, 0);
        }
    }

    private static bool IsBenignAutoReloadMiss(string detail)
    {
        if (string.IsNullOrEmpty(detail)) return true;
        return detail.Contains("no live DevelopToolBarViewModel")
            || detail.Contains("_workspace is null")
            || detail.Contains("Workspace.Flows missing")
            || detail.Contains("Workspace.RobotPath empty");
    }

    /// <summary>
    /// F5 path. If <paramref name="flowName"/> is null, just invokes the
    /// toolbar's <c>Run(DebugMode.Normal)</c> (which runs the currently active
    /// flow / Workspace.Startup, matching what the user gets clicking the
    /// toolbar button). If a flow name is supplied, we reuse Studio's own
    /// <c>Options(DebugMode.Normal)</c> builder (so PythonVersion /
    /// DebuggerOptions / RobotFolder / Source are filled the same way as F5)
    /// and only patch <c>Module</c>, then hand the options straight to
    /// <c>_engineManager.Run(options)</c>. Going through EngineManager
    /// (rather than IEngineComponent.Create + Start) keeps Studio's UI
    /// observers — tab status, log panel, button state — wired up.
    /// </summary>
    public static Result Run(string flowName = null)
    {
        if (string.IsNullOrEmpty(flowName))
            return InvokeToolBar("Run", new object[] { DebugMode.Normal });
        return RunFlow(flowName);
    }

    private static Result RunFlow(string flowName)
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no live DevelopToolBarViewModel");
                var vmType = vm.GetType();
                // Validate the flow exists before doing anything — invalid names
                // would otherwise produce confusing engine startup errors.
                var workspace = GetField(vm, "_workspace");
                var flows = workspace?.GetType().GetProperty("Flows")?.GetValue(workspace) as IEnumerable;
                if (flows == null) return new Result(false, "workspace has no Flows");
                var known = new List<string>();
                object selectedFlow = null;
                string selectedModule = null;
                foreach (var f in flows)
                {
                    var n = f?.GetType().GetProperty("Name")?.GetValue(f) as string;
                    if (n == null) continue;
                    var moduleName = FlowModuleName(f);
                    known.Add(string.IsNullOrEmpty(moduleName) || moduleName == n
                        ? n
                        : $"{n} -> {moduleName}");
                    if (n == flowName || moduleName == flowName || ModuleTail(flowName) == moduleName)
                    {
                        selectedFlow = f;
                        selectedModule = moduleName;
                    }
                }
                if (selectedFlow == null && !flowName.Contains('.'))
                    return new Result(false, $"flow '{flowName}' not found. Available: [{string.Join(", ", known)}]");

                var optionsMethod = vmType.GetMethod("Options",
                    BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
                if (optionsMethod == null)
                    return new Result(false, "DevelopToolBarViewModel.Options(DebugMode) missing");
                var options = optionsMethod.Invoke(vm, new object[] { DebugMode.Normal });
                if (options == null) return new Result(false, "Options(Normal) returned null");
                var moduleProp = options.GetType().GetProperty("Module");
                if (moduleProp == null || !moduleProp.CanWrite)
                    return new Result(false, "EngineCreateOptions.Module not writable");
                // Module is a Python import path, not just the flow name —
                // for the standard layout it looks like "xbot_robot.main".
                // Keep the package prefix that Options() filled in and only
                // swap the trailing segment, so "main" → "xbot_robot.main"
                // becomes "module1" → "xbot_robot.module1" without us having
                // to know the package convention.
                var currentModule = moduleProp.GetValue(options) as string ?? "";
                var moduleLeaf = !string.IsNullOrEmpty(selectedModule) ? selectedModule : flowName;
                string newModule;
                if (flowName.Contains('.'))
                {
                    newModule = flowName; // caller passed a fully-qualified name
                }
                else
                {
                    var dotIdx = currentModule.LastIndexOf('.');
                    newModule = dotIdx >= 0
                        ? currentModule.Substring(0, dotIdx + 1) + moduleLeaf
                        : moduleLeaf;
                }
                moduleProp.SetValue(options, newModule);
                BootstrapLog.Info($"Module rewritten: '{currentModule}' -> '{newModule}'");

                var engineManager = GetField(vm, "_engineManager");
                if (engineManager == null) return new Result(false, "_engineManager is null");
                var runMethod = engineManager.GetType().GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                    .FirstOrDefault(m => m.Name == "Run" && m.GetParameters().Length == 1
                        && m.GetParameters()[0].ParameterType.Name == "EngineCreateOptions");
                if (runMethod == null) return new Result(false, "EngineManager.Run(EngineCreateOptions) missing");
                BootstrapLog.Info($"running flow '{flowName}' via EngineManager.Run");
                var ret = runMethod.Invoke(engineManager, new[] { options });
                if (ret is Task task)
                {
                    _ = task.ContinueWith(t =>
                    {
                        if (t.IsFaulted) BootstrapLog.Error($"Run(flow) faulted: {t.Exception}");
                    });
                }
                return new Result(true, $"running flow {flowName}");
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"RunFlow({flowName}): {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    private static string FlowModuleName(object flow)
    {
        foreach (var propName in new[] { "PythonFileName", "FileName" })
        {
            var raw = flow?.GetType().GetProperty(propName)?.GetValue(flow) as string;
            var normalized = NormalizeModuleLeaf(raw);
            if (!string.IsNullOrEmpty(normalized)) return normalized;
        }
        return null;
    }

    private static string NormalizeModuleLeaf(string value)
    {
        if (string.IsNullOrWhiteSpace(value)) return null;
        var s = value.Trim();
        if (s.IndexOfAny(new[] { '\\', '/' }) >= 0)
            s = Path.GetFileName(s);
        foreach (var ext in new[] { ".flow.json", ".py", ".json", ".pybx" })
        {
            if (s.EndsWith(ext, StringComparison.OrdinalIgnoreCase))
                return s.Substring(0, s.Length - ext.Length);
        }
        return s;
    }

    private static string ModuleTail(string module)
    {
        if (string.IsNullOrWhiteSpace(module)) return null;
        var i = module.LastIndexOf('.');
        return i >= 0 ? module.Substring(i + 1) : module;
    }

    /// <summary>
    /// List flows in the open project. Returns each flow's name plus flags for
    /// startup (the one F5 runs by default if nothing else is active) and
    /// "active" (the flow currently focused in the editor tab).
    /// </summary>
    public static Result ListFlows()
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no live DevelopToolBarViewModel");
                var workspace = GetField(vm, "_workspace");
                if (workspace == null) return new Result(false, "_workspace is null");
                var wsType = workspace.GetType();
                var flows = wsType.GetProperty("Flows")?.GetValue(workspace) as IEnumerable;
                if (flows == null) return new Result(false, "Workspace.Flows missing");
                var startup = wsType.GetProperty("Startup")?.GetValue(workspace);
                string activeName = null;
                try
                {
                    var m = vm.GetType().GetMethod("GetActiveFlowName",
                        BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
                    if (m != null) activeName = m.Invoke(vm, null) as string;
                }
                catch { }
                var list = new List<object>();
                foreach (var flow in flows)
                {
                    if (flow == null) continue;
                    var name = flow.GetType().GetProperty("Name")?.GetValue(flow) as string;
                    if (string.IsNullOrEmpty(name)) continue;
                    list.Add(new {
                        name,
                        type = flow.GetType().Name,
                        isStartup = ReferenceEquals(flow, startup),
                        isActive = name == activeName,
                    });
                }
                return new Result(true, "ok", list);
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error("ListFlows: " + ex);
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    /// <summary>
    /// Create a new Studio module through ShadowBot's runtime model, not by
    /// simulating clicks. The FlowCollection raises Studio's collection-changed
    /// events, and the document manager opens the newly created module.
    /// </summary>
    public static Result CreateModule(string name)
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no live DevelopToolBarViewModel");
                var workspace = GetField(vm, "_workspace");
                if (workspace == null) return new Result(false, "_workspace is null");

                var wsType = workspace.GetType();
                var flows = wsType.GetProperty("Flows")?.GetValue(workspace);
                if (flows == null) return new Result(false, "Workspace.Flows missing");
                var robotPath = wsType.GetProperty("RobotPath")?.GetValue(workspace) as string;
                if (string.IsNullOrWhiteSpace(robotPath))
                    return new Result(false, "Workspace.RobotPath empty");

                var finalName = string.IsNullOrWhiteSpace(name)
                    ? NextFlowName(flows, "module")
                    : name.Trim();
                if (string.IsNullOrWhiteSpace(finalName))
                    return new Result(false, "name is empty");
                if (FindFlowByName(flows, finalName) != null)
                    return new Result(false, $"flow '{finalName}' already exists");

                object created;
                if (ModuleFilesExist(robotPath, finalName))
                {
                    created = BuildCodeFlow(workspace, finalName);
                    if (created == null) return new Result(false, $"module files exist but CodeFlow construction failed: {finalName}");
                }
                else
                {
                    var create = flows.GetType().GetMethod("CreateCodeFlow",
                        BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance,
                        null,
                        new[] { typeof(string) },
                        null);
                    if (create == null)
                        return new Result(false, "FlowCollection.CreateCodeFlow(string) missing");

                    created = create.Invoke(flows, new object[] { finalName });
                    if (created == null) return new Result(false, "CreateCodeFlow returned null");
                    var createdName = created.GetType().GetProperty("Name")?.GetValue(created) as string;
                    if (!string.IsNullOrWhiteSpace(name) && createdName != finalName)
                    {
                        var rename = TryRenameFlow(created, finalName);
                        if (!rename.ok) return rename;
                    }
                }

                var register = EnsurePackageFlowInfo(workspace, finalName);
                if (!register.ok) return register;
                if (FindFlowByName(flows, finalName) == null)
                {
                    var add = ForceAddFlowItem(flows, finalName, created);
                    if (!add.ok) return add;
                }
                if (FindFlowByName(flows, finalName) == null)
                    return new Result(false, $"created module '{finalName}' was not found in Workspace.Flows");

                try
                {
                    flows.GetType().GetMethod("Save", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                        ?.Invoke(flows, null);
                }
                catch (Exception ex)
                {
                    BootstrapLog.Error($"CreateModule flows save failed: {ex.Message}");
                }

                try
                {
                    wsType.GetMethod("Save", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                        ?.Invoke(workspace, null);
                }
                catch (Exception ex)
                {
                    BootstrapLog.Error($"CreateModule save failed: {ex.Message}");
                }

                bool opened = false;
                var docMgr = GetField(vm, "_documentManager");
                var open = docMgr?.GetType().GetMethod("OpenFlowDocument",
                    BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
                if (open != null)
                {
                    open.Invoke(docMgr, new object[] { created });
                    opened = true;
                }

                BootstrapLog.Info($"create-module name={finalName} opened={opened}");
                return new Result(true, "ok", new
                {
                    kind = "module",
                    name = finalName,
                    type = created.GetType().Name,
                    opened,
                    fileName = created.GetType().GetProperty("FileName")?.GetValue(created) as string,
                    uri = created.GetType().GetProperty("Uri")?.GetValue(created) as string,
                });
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"CreateModule: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    private static bool ModuleFilesExist(string robotPath, string name)
    {
        return File.Exists(Path.Combine(robotPath, name + ".py"))
            && File.Exists(Path.Combine(robotPath, ".dev", name + ".flow.json"));
    }

    private static object BuildCodeFlow(object workspace, string name)
    {
        var codeFlowType = ResolveType("ShadowBot.Runtime.Development.CodeFlow");
        if (codeFlowType == null) return null;
        var ctor = codeFlowType.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .FirstOrDefault(c =>
            {
                var ps = c.GetParameters();
                return ps.Length == 4
                    && ps[0].ParameterType.IsInstanceOfType(workspace)
                    && ps[1].ParameterType == typeof(string)
                    && ps[2].ParameterType == typeof(string);
            });
        return ctor?.Invoke(new object[] { workspace, name, null, null });
    }

    private static Result EnsurePackageFlowInfo(object workspace, string name)
    {
        var document = GetPackageDocument(workspace);
        if (document == null) return new Result(false, "PackageDocument not found");
        var docType = document.GetType();
        var flowsProp = docType.GetProperty("Flows", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
        var current = flowsProp?.GetValue(document) as Array;
        if (flowsProp == null || current == null) return new Result(false, "PackageDocument.Flows missing");

        foreach (var info in current)
        {
            var n = info?.GetType().GetProperty("Name")?.GetValue(info) as string;
            if (string.Equals(n, name, StringComparison.OrdinalIgnoreCase))
                return new Result(true, "already registered");
        }

        var flowInfoType = current.GetType().GetElementType() ?? ResolveType("ShadowBot.Runtime.Packages.FlowInfo");
        if (flowInfoType == null) return new Result(false, "FlowInfo type not found");
        var next = Array.CreateInstance(flowInfoType, current.Length + 1);
        Array.Copy(current, next, current.Length);
        var infoObj = Activator.CreateInstance(flowInfoType);
        SetProperty(infoObj, "Name", name);
        SetProperty(infoObj, "Filename", name);
        var kindProp = flowInfoType.GetProperty("Kind");
        if (kindProp != null && kindProp.CanWrite)
            kindProp.SetValue(infoObj, Enum.Parse(kindProp.PropertyType, "Code"));
        SetProperty(infoObj, "Opened", false);
        SetProperty(infoObj, "GroupName", null);
        SetProperty(infoObj, "EnableCopilot", false);
        next.SetValue(infoObj, current.Length);
        flowsProp.SetValue(document, next);
        docType.GetMethod("Save", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            ?.Invoke(document, null);
        return new Result(true, "registered");
    }

    private static object GetPackageDocument(object workspace)
    {
        var package = GetField(workspace, "_package");
        var doc = package?.GetType().GetProperty("Document", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            ?.GetValue(package);
        return doc ?? GetField(workspace, "_document");
    }

    private static void SetProperty(object obj, string name, object value)
    {
        var prop = obj.GetType().GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
        if (prop != null && prop.CanWrite) prop.SetValue(obj, value);
    }

    private static Type ResolveType(string name)
    {
        foreach (var asm in AppDomain.CurrentDomain.GetAssemblies())
        {
            try
            {
                var t = asm.GetType(name, false);
                if (t != null) return t;
            }
            catch { }
        }
        return null;
    }

    private static HashSet<string> FlowNames(object flows)
    {
        var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (flows is IEnumerable items)
        {
            foreach (var item in items)
            {
                var n = item?.GetType().GetProperty("Name")?.GetValue(item) as string;
                if (!string.IsNullOrWhiteSpace(n)) names.Add(n);
            }
        }
        return names;
    }

    private static object FindNewFlow(object flows, HashSet<string> before, object preferred)
    {
        object fallback = null;
        if (flows is IEnumerable items)
        {
            foreach (var item in items)
            {
                if (ReferenceEquals(item, preferred)) return item;
                var n = item?.GetType().GetProperty("Name")?.GetValue(item) as string;
                if (!string.IsNullOrWhiteSpace(n) && !before.Contains(n)) fallback = item;
            }
        }
        return fallback;
    }

    private static Result TryRenameFlow(object flow, string name)
    {
        var prop = flow.GetType().GetProperty("Name", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
        if (prop == null || !prop.CanWrite) return new Result(false, "created module Name property is not writable");
        try
        {
            prop.SetValue(flow, name);
            flow.GetType().GetMethod("Save", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                ?.Invoke(flow, null);
            return new Result(true, "renamed");
        }
        catch (Exception ex)
        {
            return new Result(false, $"rename failed: {ex.GetType().Name}: {ex.Message}");
        }
    }

    private static Result AddFlowToCollection(object flows, string name, object flow)
    {
        var flowType = flow.GetType();
        var setByName = flows.GetType()
            .GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .FirstOrDefault(m =>
            {
                if (m.Name != "set_Item") return false;
                var ps = m.GetParameters();
                return ps.Length == 2
                    && ps[0].ParameterType == typeof(string)
                    && ps[1].ParameterType.IsAssignableFrom(flowType);
            });
        if (setByName != null)
        {
            setByName.Invoke(flows, new object[] { name, flow });
            if (FindFlowByName(flows, name) != null)
                return new Result(true, "added via string indexer");
        }

        var add = flows.GetType()
            .GetMethods(BindingFlags.Public | BindingFlags.Instance)
            .FirstOrDefault(m =>
            {
                if (m.Name != "Add") return false;
                var ps = m.GetParameters();
                return ps.Length == 1 && ps[0].ParameterType.IsAssignableFrom(flowType);
            });
        if (add != null)
        {
            add.Invoke(flows, new[] { flow });
            if (FindFlowByName(flows, name) != null)
                return new Result(true, "added via Add");
        }

        var count = (int)(flows.GetType().GetProperty("Count")?.GetValue(flows) ?? 0);
        var insert = FindInstanceMethod(flows.GetType(), "InsertItem", m =>
            {
                var ps = m.GetParameters();
                return ps.Length == 2
                    && ps[0].ParameterType == typeof(int)
                    && ps[1].ParameterType.IsAssignableFrom(flowType);
            });
        if (insert != null)
        {
            insert.Invoke(flows, new object[] { count, flow });
            if (FindFlowByName(flows, name) != null)
                return new Result(true, "added via InsertItem");
        }

        var forced = ForceAddFlowItem(flows, name, flow);
        if (forced.ok) return forced;

        return new Result(false, "FlowCollection add methods did not update Workspace.Flows");
    }

    private static Result ForceAddFlowItem(object flows, string name, object flow)
    {
        var itemsField = FindInstanceField(flows.GetType(), "items");
        var items = itemsField?.GetValue(flows);
        if (items == null) return new Result(false, "FlowCollection backing items unavailable");

        var countProp = items.GetType().GetProperty("Count");
        var index = (int)(countProp?.GetValue(items) ?? 0);
        if (items is IList list)
        {
            list.Add(flow);
        }
        else
        {
            var add = items.GetType().GetMethod("Add", BindingFlags.Public | BindingFlags.Instance);
            if (add == null) return new Result(false, "FlowCollection backing items Add missing");
            add.Invoke(items, new[] { flow });
        }

        var flowInterface = flow.GetType().GetInterfaces()
            .FirstOrDefault(i => i.FullName == "ShadowBot.Runtime.Development.IFlow")
            ?? flow.GetType();
        var arr = Array.CreateInstance(flowInterface, 1);
        arr.SetValue(flow, 0);

        var onAdded = FindInstanceMethod(flows.GetType(), "OnItemAdded", m =>
        {
            var ps = m.GetParameters();
            return ps.Length == 2 && ps[0].ParameterType == typeof(int) && ps[1].ParameterType.IsArray;
        });
        onAdded?.Invoke(flows, new object[] { index, arr });

        return FindFlowByName(flows, name) != null
            ? new Result(true, "added via Items")
            : new Result(false, "FlowCollection.Items add did not update enumeration");
    }

    private static MethodInfo FindInstanceMethod(Type type, string name, Func<MethodInfo, bool> predicate)
    {
        for (var t = type; t != null; t = t.BaseType)
        {
            var m = t.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly)
                .FirstOrDefault(x => x.Name == name && predicate(x));
            if (m != null) return m;
        }
        return null;
    }

    private static FieldInfo FindInstanceField(Type type, string name)
    {
        for (var t = type; t != null; t = t.BaseType)
        {
            var f = t.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (f != null) return f;
        }
        return null;
    }

    private static object FindFlowByName(object flows, string name)
    {
        if (flows is IEnumerable items)
        {
            foreach (var item in items)
            {
                var n = item?.GetType().GetProperty("Name")?.GetValue(item) as string;
                if (string.Equals(n, name, StringComparison.OrdinalIgnoreCase)) return item;
            }
        }
        return null;
    }

    private static string NextFlowName(object flows, string prefix)
    {
        var used = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (flows is IEnumerable items)
        {
            foreach (var item in items)
            {
                var n = item?.GetType().GetProperty("Name")?.GetValue(item) as string;
                if (!string.IsNullOrWhiteSpace(n)) used.Add(n);
            }
        }
        for (var i = 1; i < 10000; i++)
        {
            var candidate = prefix + i;
            if (!used.Contains(candidate)) return candidate;
        }
        return prefix + DateTimeOffset.Now.ToUnixTimeSeconds();
    }

    private static object GetField(object obj, string name)
    {
        var f = obj.GetType().GetField(name,
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
        return f?.GetValue(obj);
    }

    /// <summary>
    /// Snapshot of what Studio currently has open: app name + UUID + the
    /// xbot_robot/ source folder + the startup flow name. Returns ok=false
    /// when no project is loaded (toolbar VM not bound yet), so the caller
    /// can distinguish "Studio idle" from a real error.
    /// </summary>
    public static Result Current()
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no project loaded (DevelopToolBarViewModel not bound)");
                var workspace = GetField(vm, "_workspace");
                if (workspace == null) return new Result(false, "_workspace is null");
                var wsType = workspace.GetType();
                string Get(string p) => wsType.GetProperty(p)?.GetValue(workspace) as string;
                var startup = wsType.GetProperty("Startup")?.GetValue(workspace);
                var startupName = startup?.GetType().GetProperty("Name")?.GetValue(startup) as string;
                return new Result(true, "ok", new
                {
                    name = Get("Name"),
                    uuid = Get("UUID"),
                    robotPath = Get("RobotPath"),
                    appVersionId = Get("AppVersionId"),
                    version = Get("Version"),
                    startup = startupName,
                });
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"Current: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    internal static Result ConfigureNewActivity(string uuid, string name, string code, string description)
    {
        return OnUi(() =>
        {
            var workspace = GetField(FindToolBarVm(), "_workspace")
                ?? throw new InvalidOperationException("created activity workspace unavailable; inspect before retrying");
            var type = workspace.GetType();
            object Read(string key) => type.GetProperty(key)?.GetValue(workspace);
            if ((string)Read("UUID") != uuid || !string.Equals(Read("RobotType")?.ToString(), "Activity", StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("new workspace identity/type does not match the created activity");
            if (string.IsNullOrWhiteSpace(name) || !System.Text.RegularExpressions.Regex.IsMatch(code ?? "", "^activity_[a-z][a-z0-9_]+$") || System.Text.RegularExpressions.Regex.IsMatch(code ?? "", "^activity_[0-9a-f]{8}$"))
                throw new ArgumentException("name and semantic activity_code are required");
            var oldName = Read("Name") as string;
            var oldCode = Read("ActivityCode") as string;
            if ((!string.IsNullOrWhiteSpace(oldName) && oldName != name) ||
                (!string.IsNullOrEmpty(oldCode) && !System.Text.RegularExpressions.Regex.IsMatch(oldCode, "^activity_[0-9a-f]{8}$") && oldCode != code))
                throw new InvalidOperationException("finish-create refuses to rename an established activity");
            foreach (var value in new[] { ("Name", (object)name), ("Description", description), ("ActivityCode", code), ("EncryptBot", true) })
                (type.GetProperty(value.Item1) ?? throw new NotSupportedException("workspace property missing: " + value.Item1)).SetValue(workspace, value.Item2);
            type.GetMethod("Save", Type.EmptyTypes).Invoke(workspace, null);
            return new Result(true, "created through activity template and IStudioManager", new {
                uuid, name = Read("Name"), activityCode = Read("ActivityCode"), robotType = Read("RobotType")?.ToString(), robotPath = Read("RobotPath") });
        });
    }

    internal static Result ConfigureNewConsumer(string uuid, string name, string description)
    {
        return OnUi(() =>
        {
            var workspace = GetField(FindToolBarVm(), "_workspace") ?? throw new InvalidOperationException("consumer workspace unavailable");
            var type = workspace.GetType();
            object Read(string key) => type.GetProperty(key)?.GetValue(workspace);
            if ((string)Read("UUID") != uuid || string.Equals(Read("RobotType")?.ToString(), "Activity", StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("consumer identity/type mismatch");
            if (!string.IsNullOrWhiteSpace(Read("Name") as string) && (string)Read("Name") != name)
                throw new InvalidOperationException("refusing to rename an existing consumer");
            type.GetProperty("Name").SetValue(workspace, name);
            type.GetProperty("Description").SetValue(workspace, description);
            type.GetMethod("Save", Type.EmptyTypes).Invoke(workspace, null);
            return new Result(true, "created consumer through blank template", new {uuid,name,robotType=Read("RobotType")?.ToString(),robotPath=Read("RobotPath")});
        });
    }

    internal static Result ProbeInstallState()
    {
        return OnUi(() =>
        {
            var toolbar = FindToolBarVm() ?? throw new InvalidOperationException("no Studio toolbar loaded");
            var workspace = GetField(toolbar,"_workspace") ?? throw new InvalidOperationException("no workspace loaded");
            var engine = GetField(toolbar,"_engineManager");
            var state = ActivityInstallGuard.Snapshot(engine);
            try { ActivityInstallGuard.RequireIdle(engine); return new Result(true,"confirmed idle",new {
                uuid=workspace.GetType().GetProperty("UUID")?.GetValue(workspace),state,
                dependencies=workspace.GetType().GetProperty("InternalDependencies")?.GetValue(workspace),
                hasChanged=workspace.GetType().GetProperty("HasChanged")?.GetValue(workspace),
                documentChanged=GetField(workspace,"_documentChanged"),
                documentDependencies=GetField(workspace,"_document")?.GetType().GetProperty("InternalDependencies")?.GetValue(GetField(workspace,"_document")),
                repositoryDocumentDependencies=GetField(workspace.GetType().GetProperty("ActivityRepository")?.GetValue(workspace),"_document")?.GetType().GetProperty("InternalDependencies")?.GetValue(GetField(workspace.GetType().GetProperty("ActivityRepository")?.GetValue(workspace),"_document")),
                dependencyMethods=workspace.GetType().GetMethods().Where(m=>m.Name.Contains("Activity")||m.Name.Contains("Depend"))
                    .Select(m=>m.Name+"("+string.Join(",",m.GetParameters().Select(p=>p.ParameterType.Name+" "+p.Name))+")").ToArray(),
                dependencyProperties=workspace.GetType().GetProperties().Where(p=>p.Name.Contains("Activit")||p.Name.Contains("Depend"))
                    .Select(p=>new {p.Name,type=p.PropertyType.FullName,p.CanWrite}).ToArray()
            }); }
            catch(InvalidOperationException ex) { return new Result(false,ex.Message,state); }
        });
    }

    internal static Result InstallActivity(string appId, string code, string version, string name, bool upgrade = false)
    {
        if (string.IsNullOrWhiteSpace(code) || string.IsNullOrWhiteSpace(version) || string.IsNullOrWhiteSpace(name))
            throw new ArgumentException("activity code, version and name are required");
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(150));
        var token = deadline.Token;
        object originalWorkspace = null;
        string[] originalDependencies = null;
        string stage = "download";
        object Workspace()
        {
            var toolbar = FindToolBarVm();
            var workspace = GetField(toolbar, "_workspace") ?? throw new InvalidOperationException("no consumer workspace loaded");
            if (workspace.GetType().GetProperty("UUID")?.GetValue(workspace) as string != appId ||
                originalWorkspace != null && !ReferenceEquals(workspace, originalWorkspace))
                throw new InvalidOperationException("current consumer UUID mismatch");
            ActivityInstallGuard.RequireIdle(GetField(toolbar, "_engineManager"));
            token.ThrowIfCancellationRequested();
            return workspace;
        }
        try
        {
            var pending = OnUi(() =>
            {
                originalWorkspace = Workspace();
                var type = originalWorkspace.GetType();
                originalDependencies = (string[])type.GetProperty("InternalDependencies").GetValue(originalWorkspace);
                if (upgrade)
                    return type.GetMethod("UpgradeActivity", new[] {typeof(string), typeof(string)})
                        .Invoke(originalWorkspace, new object[] {code, version}) as Task
                        ?? throw new InvalidOperationException("UpgradeActivity did not return task");
                var method = type.GetMethods().Single(m => m.Name == "InstallActivity" && m.GetParameters().Length == 6);
                var mode = Enum.Parse(method.GetParameters()[3].ParameterType, "Manual");
                return method.Invoke(originalWorkspace, new object[] {code,version,name,mode,token,false}) as Task
                    ?? throw new InvalidOperationException("InstallActivity did not return task");
            });
            pending.WaitAsync(token).GetAwaiter().GetResult();
            stage = "dependency tasks";
            var settle = OnUi(() =>
            {
                var workspace = Workspace();
                var type = workspace.GetType();
                var repository = type.GetProperty("ActivityRepository").GetValue(workspace);
                if (!(bool)repository.GetType().GetMethod("VerifyInstalled").Invoke(repository, new object[] {code,version}))
                    throw new InvalidOperationException("installed activity version verification failed");
                type.GetMethod("NotifyActivityAdd", new[] {typeof(string),typeof(string)}).Invoke(workspace, new object[] {code,version});
                // Download completion alone does not dirty the package document. Without the
                // native change notification, Save can retain the previous dependency pin.
                var changed = type.GetMethod("RaiseWorkspaceChanged");
                changed.Invoke(workspace, new object[] {
                    Enum.Parse(changed.GetParameters()[0].ParameterType, upgrade ? "ActivityUpdated" : "ActivityInstalled"), true});
                type.GetMethod("Save", Type.EmptyTypes).Invoke(workspace, null);
                return type.GetMethod("WaitPythonTaskCenterIdle", BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance)
                    .Invoke(workspace, new object[] {token}) as Task
                    ?? throw new InvalidOperationException("dependency completion task unavailable");
            });
            settle.WaitAsync(token).GetAwaiter().GetResult();
            if (settle.GetType().GetProperty("Result")?.GetValue(settle) is bool complete && !complete)
                throw new InvalidOperationException("dependency tasks did not finish successfully");
            stage = "save and verify";
            // Run after the client's queued completion callbacks, not ahead of them at Send priority.
            return Application.Current.Dispatcher.InvokeAsync(() =>
            {
                var workspace = Workspace();
                var type = workspace.GetType();
                type.GetMethod("Save", Type.EmptyTypes).Invoke(workspace, null);
                var dependencies = (string[])type.GetProperty("InternalDependencies").GetValue(workspace);
                var root = (string)type.GetProperty("RobotPath").GetValue(workspace);
                using var document = System.Text.Json.JsonDocument.Parse(File.ReadAllText(Path.Combine(root,"package.json")));
                var disk = document.RootElement.GetProperty("internaldependencies").EnumerateArray().Select(x=>x.GetString()).ToArray();
                var library = Path.GetFullPath(Path.Combine(root,"..","xbot_extensions",code,"package.json"));
                var extensions = Path.GetFullPath(Path.Combine(root,"..","xbot_extensions")) + Path.DirectorySeparatorChar;
                if (!library.StartsWith(extensions, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("activity path outside consumer");
                using var installed = System.Text.Json.JsonDocument.Parse(File.ReadAllText(library));
                var installedVersion = installed.RootElement.GetProperty("version").GetString();
                ActivityInstallGuard.RequireConsistent(code, version, originalDependencies, dependencies, disk, installedVersion);
                return new Result(true,"installed, dependency tasks completed and saved",new {appId,activityCode=code,version,dependencies,installedVersion});
            }, System.Windows.Threading.DispatcherPriority.ContextIdle, token).Task.WaitAsync(token).GetAwaiter().GetResult();
        }
        catch (Exception error)
        {
            return new Result(false, "activity installation incomplete at " + stage + ": " + error.GetBaseException().Message,
                new {appId,activityCode=code,version,stage});
        }
    }

    /// <summary>
    /// Reload one (or every) CodeFlow's in-memory source from the .py file on
    /// disk. Studio doesn't watch the filesystem — when the user edits a
    /// module .py externally, Studio's <c>CodeFlow._sourceCode</c> stays stale,
    /// and the next F5 saves the stale copy back, silently clobbering the
    /// external edits.
    ///
    /// We use the public <c>CodeFlow.SetSourceCode(string)</c> setter, which
    /// updates the StringBuilder backing store AND raises PropertyChanged so
    /// the Monaco editor in <c>CodeFlowViewModel</c> auto-refreshes.
    ///
    /// Guards:
    /// - VisualFlow is rejected: its truth is the block tree, not the .py
    ///   (the .py is just a serialized output). Reloading would desync UI.
    /// - If the file's tab is open AND has unsaved edits in the editor,
    ///   skip — we don't want to silently throw away in-editor work.
    /// - MD5 short-circuit: if memory already matches disk, no-op (keeps UI
    ///   from flickering when caller is paranoid and reloads everything).
    /// </summary>
    internal static Result ReloadCodeFlows(string name, bool all)
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no live DevelopToolBarViewModel");
                var workspace = GetField(vm, "_workspace");
                if (workspace == null) return new Result(false, "_workspace is null");
                var wsType = workspace.GetType();
                var flows = wsType.GetProperty("Flows")?.GetValue(workspace) as IEnumerable;
                if (flows == null) return new Result(false, "Workspace.Flows missing");
                var robotPath = wsType.GetProperty("RobotPath")?.GetValue(workspace) as string;
                if (string.IsNullOrEmpty(robotPath))
                    return new Result(false, "Workspace.RobotPath empty (no project open?)");

                // Open editor tabs — used to detect "user has unsaved edits in
                // the Monaco editor for this flow".
                var docMgr = GetField(vm, "_documentManager");
                var openDocs = docMgr?.GetType().GetProperty("Items")?.GetValue(docMgr) as IEnumerable;

                var results = new List<object>();
                var matched = false;
                foreach (var flow in flows.Cast<object>())
                {
                    if (flow == null) continue;
                    var ft = flow.GetType();
                    var flowName = ft.GetProperty("Name")?.GetValue(flow) as string;
                    if (string.IsNullOrEmpty(flowName)) continue;
                    if (!all && flowName != name) continue;
                    matched = true;

                    if (ft.Name != "CodeFlow")
                    {
                        results.Add(new { name = flowName, reloaded = false, reason = $"skip {ft.Name}: only CodeFlow can be reloaded from .py" });
                        continue;
                    }

                    object openVm = null;
                    if (openDocs != null)
                    {
                        openVm = openDocs.Cast<object>().FirstOrDefault(d =>
                            d.GetType().Name == "CodeFlowViewModel" &&
                            ReferenceEquals(GetField(d, "_flow"), flow));
                        if (openVm != null)
                        {
                            var dirty = GetField(openVm, "_sourceCodeChanged") as bool? ?? false;
                            if (dirty)
                            {
                                results.Add(new { name = flowName, reloaded = false, reason = "editor tab has unsaved changes — save/discard first" });
                                continue;
                            }
                        }
                    }

                    // Empirically: Workspace.RobotPath already points at the
                    // xbot_robot/ folder (not the app root), so the file is
                    // just RobotPath + Name + ".py". CodeFlow's own filename
                    // properties are unreliable on live instances —
                    // PythonFileName comes back empty, FileName returns
                    // "xbot_robot/<name>" with no extension.
                    var fullPath = Path.Combine(robotPath, flowName + ".py");
                    if (!File.Exists(fullPath))
                    {
                        results.Add(new { name = flowName, reloaded = false, reason = $"file not found: {fullPath}" });
                        continue;
                    }
                    string diskText;
                    try { diskText = File.ReadAllText(fullPath); }
                    catch (Exception ex)
                    {
                        results.Add(new { name = flowName, reloaded = false, reason = $"read failed: {ex.Message}" });
                        continue;
                    }

                    // MD5 short-circuit.
                    var md5Method = ft.GetMethod("Md5", BindingFlags.Public | BindingFlags.Instance);
                    var memMd5 = md5Method?.Invoke(flow, null) as string;
                    if (string.IsNullOrEmpty(memMd5))
                    {
                        var memText = ft.GetProperty("SourceCode", BindingFlags.Public | BindingFlags.Instance)
                            ?.GetValue(flow) as string;
                        if (memText != null)
                            memMd5 = Md5Hex(memText);
                    }
                    var diskMd5 = Md5Hex(diskText);
                    if (!string.IsNullOrEmpty(memMd5) && memMd5.Equals(diskMd5, StringComparison.OrdinalIgnoreCase))
                    {
                        results.Add(new { name = flowName, reloaded = false, reason = "already in sync", bytes = diskText.Length });
                        continue;
                    }

                    var setter = ft.GetMethod("SetSourceCode", BindingFlags.Public | BindingFlags.Instance);
                    if (setter == null)
                    {
                        results.Add(new { name = flowName, reloaded = false, reason = "SetSourceCode not found on CodeFlow" });
                        continue;
                    }
                    setter.Invoke(flow, new object[] { diskText });
                    BootstrapLog.Info($"auto reload CodeFlow {flowName}: {memMd5}->{diskMd5} ({diskText.Length} bytes)");

                    // SetSourceCode only updates CodeFlow._sourceCode — it does
                    // NOT push the new text into the Monaco editor (ICodeEditor
                    // has no SetText API). To make the UI catch up we
                    // close + reopen the tab; Studio's OpenFlowDocument
                    // rebuilds a fresh CodeFlowViewModel that reads SourceCode
                    // afresh. Cost: caret position / scroll / undo history
                    // for that tab are reset (acceptable — without this the
                    // editor silently shows stale content).
                    bool uiRefreshed = false;
                    if (openVm != null && docMgr != null)
                    {
                        try
                        {
                            var dmType = docMgr.GetType();
                            var closeM = dmType.GetMethod("CloseFlowDocument", BindingFlags.Public | BindingFlags.Instance);
                            var openM = dmType.GetMethod("OpenFlowDocument", BindingFlags.Public | BindingFlags.Instance);
                            if (closeM != null && openM != null)
                            {
                                closeM.Invoke(docMgr, new object[] { flow });
                                openM.Invoke(docMgr, new object[] { flow });
                                uiRefreshed = true;
                            }
                        }
                        catch (Exception ex)
                        {
                            BootstrapLog.Error($"auto reload CodeFlow tab refresh failed: {ex.Message}");
                        }
                    }
                    results.Add(new { name = flowName, reloaded = true, bytes = diskText.Length, uiRefreshed });
                }

                if (!all && !matched)
                    return new Result(false, $"flow '{name}' not found");
                return new Result(true, "ok", results);
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"ReloadCodeFlows: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    /// <summary>
    /// Replace one CodeFlow's source from caller-supplied text: writes the .py
    /// on disk AND updates the live CodeFlow via SetSourceCode — parameters,
    /// forms and caller bindings are never rebuilt. Mirrors the ReloadCodeFlows
    /// guards: CodeFlow only, no clobbering an editor tab with unsaved edits,
    /// optional expectMd5 must match the current in-memory source.
    /// </summary>
    internal static Result SetCodeFlowSource(string flowName, string source, string expectMd5)
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no live DevelopToolBarViewModel");
                var workspace = GetField(vm, "_workspace");
                if (workspace == null) return new Result(false, "_workspace is null");
                var wsType = workspace.GetType();
                var flows = wsType.GetProperty("Flows")?.GetValue(workspace) as IEnumerable;
                if (flows == null) return new Result(false, "Workspace.Flows missing");
                var robotPath = wsType.GetProperty("RobotPath")?.GetValue(workspace) as string;
                if (string.IsNullOrEmpty(robotPath))
                    return new Result(false, "Workspace.RobotPath empty (no project open?)");

                var docMgr = GetField(vm, "_documentManager");
                var openDocs = docMgr?.GetType().GetProperty("Items")?.GetValue(docMgr) as IEnumerable;

                object flow = null;
                foreach (var f in flows.Cast<object>())
                {
                    if (f == null) continue;
                    var n = f.GetType().GetProperty("Name")?.GetValue(f) as string;
                    if (n == flowName) { flow = f; break; }
                }
                if (flow == null) return new Result(false, $"flow '{flowName}' not found");
                var ft = flow.GetType();
                if (ft.Name != "CodeFlow")
                    return new Result(false, $"flow '{flowName}' is {ft.Name}; only CodeFlow source can be replaced");

                object openVm = null;
                if (openDocs != null)
                {
                    openVm = openDocs.Cast<object>().FirstOrDefault(d =>
                        d.GetType().Name == "CodeFlowViewModel" &&
                        ReferenceEquals(GetField(d, "_flow"), flow));
                    if (openVm != null)
                    {
                        var dirty = GetField(openVm, "_sourceCodeChanged") as bool? ?? false;
                        if (dirty)
                            return new Result(false, $"editor tab for '{flowName}' has unsaved changes — save or discard them first");
                    }
                }

                var md5Method = ft.GetMethod("Md5", BindingFlags.Public | BindingFlags.Instance);
                var memMd5 = md5Method?.Invoke(flow, null) as string;
                if (string.IsNullOrEmpty(memMd5))
                {
                    var memText = ft.GetProperty("SourceCode", BindingFlags.Public | BindingFlags.Instance)
                        ?.GetValue(flow) as string;
                    if (memText != null) memMd5 = Md5Hex(memText);
                }
                if (!string.IsNullOrEmpty(expectMd5) &&
                    !string.Equals(memMd5, expectMd5, StringComparison.OrdinalIgnoreCase))
                    return new Result(false, $"source changed since it was read (expectMd5 {expectMd5} != current {memMd5}); re-read and retry");

                var fullPath = Path.Combine(robotPath, flowName + ".py");
                var encoding = new System.Text.UTF8Encoding(false);
                if (File.Exists(fullPath))
                {
                    var head = File.ReadAllBytes(fullPath);
                    if (head.Length >= 3 && head[0] == 0xEF && head[1] == 0xBB && head[2] == 0xBF)
                        encoding = new System.Text.UTF8Encoding(true);
                }
                File.WriteAllText(fullPath, source ?? "", encoding);

                var setter = ft.GetMethod("SetSourceCode", BindingFlags.Public | BindingFlags.Instance);
                if (setter == null)
                    return new Result(false, "SetSourceCode not found on CodeFlow");
                setter.Invoke(flow, new object[] { source ?? "" });
                BootstrapLog.Info($"set CodeFlow source {flowName}: {memMd5}->{Md5Hex(source)} ({(source ?? "").Length} chars)");

                bool uiRefreshed = false;
                if (openVm != null && docMgr != null)
                {
                    try
                    {
                        var dmType = docMgr.GetType();
                        var closeM = dmType.GetMethod("CloseFlowDocument", BindingFlags.Public | BindingFlags.Instance);
                        var openM = dmType.GetMethod("OpenFlowDocument", BindingFlags.Public | BindingFlags.Instance);
                        if (closeM != null && openM != null)
                        {
                            closeM.Invoke(docMgr, new object[] { flow });
                            openM.Invoke(docMgr, new object[] { flow });
                            uiRefreshed = true;
                        }
                    }
                    catch (Exception ex)
                    {
                        BootstrapLog.Error($"codeflow source tab refresh failed: {ex.Message}");
                    }
                }
                return new Result(true, "ok", new { name = flowName, md5 = Md5Hex(source ?? ""), chars = (source ?? "").Length, uiRefreshed });
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"SetCodeFlowSource: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    private static string Md5Hex(string text)
    {
        using var md5 = System.Security.Cryptography.MD5.Create();
        var bytes = md5.ComputeHash(System.Text.Encoding.UTF8.GetBytes(text ?? ""));
        var sb = new System.Text.StringBuilder(bytes.Length * 2);
        foreach (var b in bytes) sb.Append(b.ToString("x2"));
        return sb.ToString();
    }

    /// <summary>
    /// Pause/resume toggle — matches the F2/Ctrl-F2 hotkey in Studio. The
    /// toolbar VM has no Pause method; the logic lives on EngineManager
    /// (the toolbar's `_engineManager` field) as `PausedSwitch()`.
    /// </summary>
    public static Result Pause() => InvokeOnToolBarField("_engineManager", "PausedSwitch", null);

    /// <summary>
    /// Studio's Stop button. Routes through the toolbar's EngineManager
    /// (`_engineManager.Stop()`) which is the exact same call Studio's UI
    /// dispatches when Stop is clicked. No nuclear kill needed — Studio's
    /// internal logic handles the actual engine teardown.
    /// </summary>
    public static Result Stop() => InvokeOnToolBarField("_engineManager", "Stop", null);

    /// <summary>
    /// Reflectively invoke a method on a field of the toolbar VM. We use this
    /// to reach `_engineManager.Stop()` / `.PausedSwitch()` which is where
    /// Studio's actual run-control logic lives.
    /// </summary>
    private static Result InvokeOnToolBarField(string fieldName, string methodName, object[] args)
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no live DevelopToolBarViewModel");
                var f = vm.GetType().GetField(fieldName,
                    BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
                if (f == null) return new Result(false, $"field {fieldName} not on toolbar VM");
                var target = f.GetValue(vm);
                if (target == null) return new Result(false, $"{fieldName} is null");
                var argCount = args?.Length ?? 0;
                var m = target.GetType().GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                    .FirstOrDefault(x => x.Name == methodName && x.GetParameters().Length == argCount);
                if (m == null)
                    return new Result(false, $"{target.GetType().Name}.{methodName}({argCount}) not found");
                BootstrapLog.Info($"invoking {target.GetType().Name}.{methodName}");
                var ret = m.Invoke(target, args);
                if (ret is Task task)
                {
                    _ = task.ContinueWith(t =>
                    {
                        if (t.IsFaulted) BootstrapLog.Error($"{methodName} faulted: {t.Exception}");
                    });
                }
                return new Result(true, $"invoked {target.GetType().Name}.{methodName}");
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"{fieldName}.{methodName}: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    /// <summary>Enumerate the toolbar VM's methods + fields. Diagnostic only.</summary>
    public static Result DescribeToolBar()
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no live DevelopToolBarViewModel");
                return new Result(true, "ok", DescribeObject(vm));
            });
        }
        catch (Exception ex) { return new Result(false, ex.Message); }
    }

    /// <summary>Drill into a named field of the toolbar VM, e.g.
    /// `?field=_engineManager` to inspect what type/methods it has.</summary>
    public static Result DescribeToolBarField(string fieldName)
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null) return new Result(false, "no live DevelopToolBarViewModel");
                var f = vm.GetType().GetField(fieldName,
                    BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
                if (f == null) return new Result(false, $"field {fieldName} not found");
                object v;
                try { v = f.GetValue(vm); } catch (Exception ex) { return new Result(false, ex.Message); }
                if (v == null) return new Result(false, $"{fieldName} is null");
                return new Result(true, "ok", DescribeObject(v));
            });
        }
        catch (Exception ex) { return new Result(false, ex.Message); }
    }

    private static object DescribeObject(object obj)
    {
        var t = obj.GetType();
        var methods = t.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly)
            .Concat(t.GetInterfaces().SelectMany(i => i.GetMethods()))
            .Where(m => !m.IsSpecialName)
            .Select(m => $"{m.DeclaringType?.Name}.{m.Name}({string.Join(",", m.GetParameters().Select(p => p.ParameterType.Name))})")
            .Distinct()
            .OrderBy(s => s)
            .ToArray();
        var fields = t.GetFields(BindingFlags.NonPublic | BindingFlags.Instance)
            .Select(f => $"{f.FieldType.Name} {f.Name}")
            .ToArray();
        return new { type = t.FullName, methods, fields };
    }

    /// <summary>
    /// Count how many engine slots currently hold a live XbotEngine. The CLI
    /// uses the 1→0 transition as the "run finished" signal — far more
    /// reliable than a log-idle heuristic, which mistakes user `sleep()` for
    /// completion.
    /// </summary>
    public static int CountActiveEngines()
    {
        var eng = Resolver.Engine();
        if (eng == null) return 0;
        try
        {
            return OnUi(() =>
            {
                var enginesField = eng.GetType().GetField("_engines",
                    BindingFlags.NonPublic | BindingFlags.Instance);
                if (enginesField == null) return 0;
                var arr = enginesField.GetValue(eng) as Array;
                if (arr == null) return 0;
                int n = 0;
                foreach (var slot in arr) if (slot != null) n++;
                return n;
            });
        }
        catch { return 0; }
    }


    private static Result InvokeToolBar(string methodName, object[] args)
    {
        try
        {
            return OnUi(() =>
            {
                var vm = FindToolBarVm();
                if (vm == null)
                    return new Result(false, "no live DevelopToolBarViewModel");
                var argCount = args?.Length ?? 0;
                var m = vm.GetType().GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
                    .FirstOrDefault(x => x.Name == methodName && x.GetParameters().Length == argCount);
                if (m == null)
                    return new Result(false, $"method {methodName}({argCount}) not found on {vm.GetType().FullName}");
                BootstrapLog.Info($"invoking {vm.GetType().Name}.{methodName}");
                var ret = m.Invoke(vm, args);
                if (ret is Task task)
                {
                    _ = task.ContinueWith(t =>
                    {
                        if (t.IsFaulted)
                            BootstrapLog.Error($"{methodName} task faulted: {t.Exception}");
                    });
                }
                return new Result(true, $"invoked {methodName}");
            });
        }
        catch (Exception ex)
        {
            BootstrapLog.Error($"{methodName}: {ex}");
            return new Result(false, $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    /// <summary>
    /// Locate the live, view-bound DevelopToolBarViewModel by walking
    /// Application.Current.Windows' data contexts. Returns null if Studio's
    /// editor isn't open (Console mode, app picker, splash, etc).
    ///
    /// We restrict the walk to "Develop*" windows because:
    ///  - It runs on the WPF dispatcher (UI thread). Walking the full tree
    ///    of Application.Current.Windows in Console mode descends into
    ///    MyApps2ViewModel and the subscribed/developed app trees, where
    ///    several property getters lazy-load from network or DB. The
    ///    blocking get is unobservable to us (try/catch sees no exception)
    ///    and just freezes the UI thread for seconds — Windows then marks
    ///    Shell as "not responding".
    ///  - DevelopToolBarViewModel can only live under a DevelopView. If no
    ///    such window exists, no amount of deeper walking will find it.
    ///
    /// We also deliberately do NOT fall back to <c>IoC.Get</c>: in non-editor
    /// modes that constructs a fresh phantom VM with a null Workspace, and
    /// invoking <c>Run(DebugMode)</c> on it deadlocks Shell's UI thread.
    /// </summary>
    private static object FindToolBarVm()
    {
        if (Application.Current == null) return null;
        foreach (Window w in Application.Current.Windows)
        {
            var typeName = w.GetType().FullName ?? "";
            if (!typeName.Contains("Develop")) continue;
            var hit = Walk(w.DataContext, ToolBarType, 0);
            if (hit != null) return hit;
        }
        return null;
    }

    private static object Walk(object node, string typeName, int depth)
    {
        if (node == null || depth > 14) return null;
        var t = node.GetType();
        if (t.FullName == typeName) return node;
        var ns = t.Namespace ?? "";
        if (depth > 0 && !ns.StartsWith("ShadowBot.") && !ns.StartsWith("Caliburn.")) return null;
        foreach (var p in t.GetProperties(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance))
        {
            if (p.GetIndexParameters().Length > 0) continue;
            object v;
            try { v = p.GetValue(node); } catch { continue; }
            var found = Walk(v, typeName, depth + 1);
            if (found != null) return found;
        }
        foreach (var f in t.GetFields(BindingFlags.NonPublic | BindingFlags.Instance))
        {
            object v;
            try { v = f.GetValue(node); } catch { continue; }
            var found = Walk(v, typeName, depth + 1);
            if (found != null) return found;
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
