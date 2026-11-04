using System;
using System.Collections.Generic;
using System.Reflection;
using System.Text;
using System.Windows;

namespace XbotInjectee;

/// <summary>
/// Mirror Studio's log panel content. Hooks LogViewModel.LogAdded (raised for
/// every line that would appear in Studio's 运行日志 panel, including
/// xbot.logging.info/warn/error from the Python side) and buffers entries for
/// the CLI to poll. We also subscribe to EngineComponent.OutputReceived as a
/// fallback for Console runs where the Studio log panel is not mounted.
/// </summary>
internal static class LogTap
{
    public record Entry(string text, string level);

    private static readonly object _gate = new();
    private static readonly List<Entry> _all = new();
    // Strong refs to the *current* hooked sources, used to detect when Studio
    // swapped them out under us (happens on project switch — LogViewModel is
    // a child of DevelopView and gets reconstructed). When the live instance
    // no longer matches what we've hooked, we unhook the stale one and bind
    // the new one. Pre-fix, a single _logVmHooked bool meant the first
    // project's LogVm got hooked and subsequent project switches lost logs.
    private static object _hookedLogVm;
    private static Delegate _logVmHandler;
    private static object _hookedEngineEvent;
    private static Delegate _engineHandler;
    private static MethodInfo _engineRemoveMethod;
    private static readonly Dictionary<int, object> _hookedRuntimeLogManagers = new();
    private static readonly Dictionary<int, Delegate> _runtimeLogHandlers = new();
    /// <summary>Bumped each time `Reset()` is called, so callers can detect
    /// that the buffer was cleared between polls.</summary>
    private static long _generation;

    /// <summary>Hook both sources. Safe to call repeatedly — each is wired at
    /// most once. Returns true if anything is hooked at all.</summary>
    public static bool EnsureHooked()
    {
        bool any = false;
        try { any |= TryHookLogVm(); } catch (Exception ex) { BootstrapLog.Error("hook LogVm: " + ex.Message); }
        try { any |= TryHookEngineOutput(); } catch (Exception ex) { BootstrapLog.Error("hook Engine: " + ex.Message); }
        try { any |= TryHookRuntimeLogManagers(); } catch (Exception ex) { BootstrapLog.Error("hook RuntimeLog: " + ex.Message); }
        return any;
    }

    /// <summary>Drop all buffered entries. Called before each `xbot-cli run`
    /// so the CLI only sees the new run's output.</summary>
    public static void Reset()
    {
        lock (_gate)
        {
            _all.Clear();
            _generation++;
            foreach (var manager in _hookedRuntimeLogManagers.Values)
            {
                try { (manager as IDisposable)?.Dispose(); } catch { }
            }
            _hookedRuntimeLogManagers.Clear();
            _runtimeLogHandlers.Clear();
        }
    }

    public record Slice(long generation, int total, int since, Entry[] entries);

    public static Slice Get(int since)
    {
        lock (_gate)
        {
            var total = _all.Count;
            if (since < 0) since = 0;
            if (since >= total) return new Slice(_generation, total, since, Array.Empty<Entry>());
            var slice = new Entry[total - since];
            _all.CopyTo(since, slice, 0, total - since);
            return new Slice(_generation, total, since, slice);
        }
    }

    // ─────────────────────────── source: LogViewModel.LogAdded ──────────────

    private static bool TryHookLogVm()
    {
        if (Application.Current == null) return _hookedLogVm != null;
        // Walking WPF DataContexts must happen on the dispatcher thread.
        return Application.Current.Dispatcher.Invoke(() =>
        {
            lock (_gate)
            {
                object current = FindVm("ShadowBot.Shell.Development.ViewModels.LogViewModel");
                if (current == null)
                {
                    // No Develop window in the tree — user returned to Console
                    // mode (or the editor's still loading). If we had a hook to
                    // a now-orphaned VM, explicitly detach it: holding a
                    // strong ref to a disposing-but-alive LogVm has been seen
                    // to make Studio's project-close routine hang waiting on
                    // its own teardown, and Shell sits at "Not Responding"
                    // until Windows kills it.
                    if (_hookedLogVm != null)
                    {
                        TryDetachLogVm(_hookedLogVm, _logVmHandler);
                        _hookedLogVm = null;
                        _logVmHandler = null;
                    }
                    return false;
                }
                if (ReferenceEquals(current, _hookedLogVm)) return true;

                // Project switched (or first hook). Unbind stale handler if any.
                TryDetachLogVm(_hookedLogVm, _logVmHandler);

                var ev = current.GetType().GetEvent("LogAdded",
                    BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
                if (ev == null) { BootstrapLog.Error("LogTap: no LogAdded event"); return false; }
                var m = typeof(LogTap).GetMethod(nameof(OnLogAdded),
                    BindingFlags.NonPublic | BindingFlags.Static);
                Delegate handler;
                try { handler = Delegate.CreateDelegate(ev.EventHandlerType, m); }
                catch (Exception ex) { BootstrapLog.Error("LogTap CreateDelegate(LogAdded): " + ex.Message); return false; }
                ev.AddEventHandler(current, handler);
                _hookedLogVm = current;
                _logVmHandler = handler;
                BootstrapLog.Info("LogTap: (re)hooked LogViewModel.LogAdded");
                return true;
            }
        });
    }

    // EventHandler<LogAddedEventArgs> — args has Display (pre-formatted Studio
    // panel line), Level (Info/Warn/Error), Content, Time, FlowName,
    // LineNumber, LevelDisplay (Chinese label). Display is exactly what
    // Studio's log panel shows, so use it verbatim.
    private static int _logAddedDumps;
    private static void OnLogAdded(object sender, object e)
    {
        try
        {
            if (e == null) return;
            DumpFirstFew(e, ref _logAddedDumps, "LogAdded");

            var t = e.GetType();
            string display = Str(t, e, "Display");
            string level = Str(t, e, "Level") ?? "Info";
            if (string.IsNullOrEmpty(display))
            {
                // Fallback if Display isn't populated (older Studio versions).
                var content = Str(t, e, "Content", "Message");
                if (string.IsNullOrEmpty(content)) return;
                var time = Str(t, e, "Time") ?? DateTime.Now.ToString("HH:mm:ss.fff");
                var levelDisplay = Str(t, e, "LevelDisplay") ?? TranslateLevel(level);
                display = $"[{levelDisplay}] [{time}] {content}";
            }
            // LogAddedEventArgs.Display sometimes includes a trailing newline;
            // strip it so we don't insert blank lines.
            Append(display.TrimEnd('\r', '\n'), level);
        }
        catch (Exception ex)
        {
            BootstrapLog.Error("OnLogAdded: " + ex.Message);
        }
    }

    private static object UnwrapInner(object e)
    {
        var t = e.GetType();
        foreach (var name in new[] { "Item", "Log", "LogItem", "Entry" })
        {
            var p = t.GetProperty(name);
            if (p == null) continue;
            try { var v = p.GetValue(e); if (v != null) return v; } catch { }
        }
        return null;
    }

    // ────────────────────── source: EngineComponent.OutputReceived ──────────
    // ShadowBot uses a custom ShadowBot.Common.Event<T> (not standard CLR
    // event); the subscribe method is `AddListener`.

    private static bool TryHookEngineOutput()
    {
        lock (_gate)
        {
            var eng = Resolver.Engine();
            if (eng == null) return _hookedEngineEvent != null;
            var prop = eng.GetType().GetProperty("OutputReceived",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            if (prop == null) return false;
            object evObj;
            try { evObj = prop.GetValue(eng); } catch { return false; }
            if (evObj == null) return _hookedEngineEvent != null;
            // EngineComponent is an IoC singleton so this ref should be stable
            // across project switches, but track it anyway in case ShadowBot
            // ever swaps the event-object out behind the scenes.
            if (ReferenceEquals(evObj, _hookedEngineEvent)) return true;

            // New (or replaced) event obj. Best-effort detach from the prior one.
            if (_hookedEngineEvent != null && _engineHandler != null && _engineRemoveMethod != null)
            {
                try { _engineRemoveMethod.Invoke(_hookedEngineEvent, new object[] { _engineHandler }); }
                catch (Exception ex) { BootstrapLog.Info("LogTap: unhook stale engine event threw: " + ex.Message); }
            }

            MethodInfo add = null, remove = null;
            foreach (var m in evObj.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance))
            {
                if (m.GetParameters().Length != 1) continue;
                if (add == null && (m.Name == "AddListener" || m.Name == "Register" || m.Name == "Subscribe" || m.Name == "Add")) add = m;
                else if (remove == null && (m.Name == "RemoveListener" || m.Name == "Unregister" || m.Name == "Unsubscribe" || m.Name == "Remove")) remove = m;
            }
            if (add == null) return false;
            var delType = add.GetParameters()[0].ParameterType;
            var handlerMethod = typeof(LogTap).GetMethod(nameof(OnEngineOutput),
                BindingFlags.NonPublic | BindingFlags.Static);
            Delegate handler;
            try { handler = Delegate.CreateDelegate(delType, handlerMethod); }
            catch (Exception ex) { BootstrapLog.Error("LogTap CreateDelegate(Engine): " + ex.Message); return false; }
            add.Invoke(evObj, new object[] { handler });
            _hookedEngineEvent = evObj;
            _engineHandler = handler;
            _engineRemoveMethod = remove;
            BootstrapLog.Info("LogTap: (re)hooked Engine.OutputReceived via " + add.Name);
            return true;
        }
    }

    // Action<EngineOutputReceivedEventArgs> — args has EngineId and Data
    // (the raw line from the engine pipe / pdb / stderr).
    private static int _engineDumps;
    private static void OnEngineOutput(object args)
    {
        try
        {
            if (args == null) return;
            DumpFirstFew(args, ref _engineDumps, "EngineOutput");

            var t = args.GetType();
            string data = Str(t, args, "Data", "Message", "Content");
            if (string.IsNullOrWhiteSpace(data)) return;
            data = data.TrimEnd();
            if (IsInternalXbotOutput(data)) return;

            var level = LooksLikeError(data) ? "Error" : "Info";
            var levelDisplay = TranslateLevel(level);
            var time = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff");
            Append($"[{levelDisplay}] [{time}] {data}", level);
        }
        catch (Exception ex)
        {
            BootstrapLog.Error("OnEngineOutput: " + ex.Message);
        }
    }

    private static bool IsInternalXbotOutput(string data) =>
        data.StartsWith("xbot_out >>", StringComparison.Ordinal)
        || data.StartsWith("xbot_out>>", StringComparison.Ordinal);

    // ───────────────────── source: LogDataManager for Console runs ─────────
    // Studio's LogViewModel implements IEngineLogService and internally creates
    // a LogDataManager, then calls SetEngine(engine, engineId). Console mode
    // does not mount LogViewModel, so xbot_visual.programing.log /
    // xbot.logging.info never reaches LogAdded unless we create a manager
    // ourselves for each active engine slot.

    private static bool TryHookRuntimeLogManagers()
    {
        lock (_gate)
        {
            var eng = Resolver.Engine();
            if (eng == null) return _hookedRuntimeLogManagers.Count > 0;
            var ids = ActiveEngineIds(eng);
            bool any = _hookedRuntimeLogManagers.Count > 0;
            for (var i = 0; i < ids.Length; i++)
            {
                var id = ids[i];
                if (_hookedRuntimeLogManagers.ContainsKey(id))
                {
                    any = true;
                    continue;
                }
                if (TryHookRuntimeLogManager(eng, id))
                    any = true;
            }
            return any;
        }
    }

    private static bool TryHookRuntimeLogManager(object eng, int engineId)
    {
        var managerType = ResolveType("ShadowBot.Shell.ShadowBotToolkit.LogDataManager");
        if (managerType == null) return false;
        object manager;
        try { manager = Activator.CreateInstance(managerType); }
        catch (Exception ex) { BootstrapLog.Error("LogDataManager create: " + ex.Message); return false; }

        var ev = managerType.GetEvent("LogAdded",
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
        if (ev == null) { BootstrapLog.Error("LogDataManager: no LogAdded event"); return false; }
        var m = typeof(LogTap).GetMethod(nameof(OnLogAdded),
            BindingFlags.NonPublic | BindingFlags.Static);
        Delegate handler;
        try { handler = Delegate.CreateDelegate(ev.EventHandlerType, m); }
        catch (Exception ex) { BootstrapLog.Error("LogDataManager CreateDelegate(LogAdded): " + ex.Message); return false; }
        try { ev.AddEventHandler(manager, handler); }
        catch (Exception ex) { BootstrapLog.Error("LogDataManager add LogAdded: " + ex.Message); return false; }

        var setEngine = managerType.GetMethod("SetEngine",
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
        if (setEngine == null) { BootstrapLog.Error("LogDataManager.SetEngine missing"); return false; }
        try { setEngine.Invoke(manager, new object[] { eng, engineId }); }
        catch (Exception ex) { BootstrapLog.Error($"LogDataManager.SetEngine({engineId}): " + ex.Message); return false; }

        _hookedRuntimeLogManagers[engineId] = manager;
        _runtimeLogHandlers[engineId] = handler;
        BootstrapLog.Info($"LogTap: hooked LogDataManager for engineId={engineId}");
        return true;
    }

    private static int[] ActiveEngineIds(object eng)
    {
        try
        {
            var enginesField = eng.GetType().GetField("_engines",
                BindingFlags.NonPublic | BindingFlags.Instance);
            var arr = enginesField?.GetValue(eng) as Array;
            if (arr == null) return Array.Empty<int>();
            var ids = new List<int>();
            for (var i = 0; i < arr.Length; i++)
            {
                if (arr.GetValue(i) != null) ids.Add(i);
            }
            return ids.ToArray();
        }
        catch
        {
            return Array.Empty<int>();
        }
    }

    private static bool LooksLikeError(string data)
    {
        if (string.IsNullOrWhiteSpace(data)) return false;
        return data.Contains("Traceback (most recent call last)")
            || data.Contains("Exception:")
            || data.Contains("Error:")
            || data.Contains("错误")
            || data.Contains("异常");
    }

    private static string TranslateLevel(string l) => l switch
    {
        "Warn" or "Warning" => "警告",
        "Error" => "错误",
        "Debug" => "调试",
        _ => "信息",
    };

    // ────────────────────────────── helpers ─────────────────────────────────

    private static void Append(string text, string level)
    {
        lock (_gate)
        {
            _all.Add(new Entry(text, level));
        }
    }

    private static string Str(Type t, object obj, params string[] names)
    {
        foreach (var n in names)
        {
            var p = t.GetProperty(n);
            if (p == null) continue;
            try
            {
                var v = p.GetValue(obj);
                if (v != null) return v.ToString();
            }
            catch { }
        }
        return null;
    }

    /// <summary>Build a log message that includes the primary body plus any
    /// populated detail/stack fields, joined with newlines. Avoids dropping
    /// crucial Python traceback info that lives in fields like ErrorMessage.</summary>
    private static string AssembleMessage(Type t, object obj, string[] bodyKeys, string[] detailKeys)
    {
        string body = null;
        foreach (var k in bodyKeys)
        {
            body = Str(t, obj, k);
            if (!string.IsNullOrWhiteSpace(body)) break;
        }
        var details = new List<string>();
        foreach (var k in detailKeys)
        {
            var v = Str(t, obj, k);
            if (string.IsNullOrWhiteSpace(v)) continue;
            // Avoid duplicating the body if a detail field happens to mirror it.
            if (body != null && body.Contains(v)) continue;
            details.Add(v);
        }
        if (string.IsNullOrWhiteSpace(body) && details.Count == 0) return null;
        if (details.Count == 0) return body;
        if (string.IsNullOrWhiteSpace(body)) return string.Join("\n", details);
        return body + "\n" + string.Join("\n", details);
    }

    /// <summary>Diagnostic: dump every non-null property of the first few
    /// events of each kind to the bootstrap log, so we can see what fields
    /// ShadowBot actually populates for log lines vs crashes vs warnings.</summary>
    private static void DumpFirstFew(object e, ref int counter, string tag)
    {
        if (counter >= 3) return;
        counter++;
        try
        {
            var sb = new System.Text.StringBuilder();
            sb.Append(tag).Append('[').Append(counter).Append("] ").Append(e.GetType().FullName);
            foreach (var p in e.GetType().GetProperties(BindingFlags.Public | BindingFlags.Instance))
            {
                if (p.GetIndexParameters().Length > 0) continue;
                object v;
                try { v = p.GetValue(e); } catch { continue; }
                if (v == null) continue;
                var s = v.ToString();
                if (string.IsNullOrEmpty(s)) continue;
                if (s.Length > 200) s = s.Substring(0, 200) + "...";
                sb.Append("\n  ").Append(p.Name).Append('=').Append(s);
            }
            BootstrapLog.Info(sb.ToString());
        }
        catch { /* diagnostic — never fail */ }
    }

    private static void TryDetachLogVm(object vm, Delegate handler)
    {
        if (vm == null || handler == null) return;
        try
        {
            var ev = vm.GetType().GetEvent("LogAdded",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            ev?.RemoveEventHandler(vm, handler);
            BootstrapLog.Info("LogTap: detached stale LogVm handler");
        }
        catch (Exception ex)
        {
            BootstrapLog.Info("LogTap: detach LogVm handler threw: " + ex.Message);
        }
    }

    private static object FindVm(string typeName)
    {
        // Scope to Develop* windows for the same reason StudioActions does:
        // walking MainView's Console subtree on the UI thread blocks on
        // lazy-loading property getters and freezes Shell. LogViewModel is
        // a Studio bottom-docking VM, so it lives under DevelopView anyway.
        foreach (Window w in Application.Current.Windows)
        {
            var wTypeName = w.GetType().FullName ?? "";
            if (!wTypeName.Contains("Develop")) continue;
            var hit = Walk(w.DataContext, typeName, 0);
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
            object v; try { v = p.GetValue(node); } catch { continue; }
            var found = Walk(v, typeName, depth + 1);
            if (found != null) return found;
        }
        foreach (var f in t.GetFields(BindingFlags.NonPublic | BindingFlags.Instance))
        {
            object v; try { v = f.GetValue(node); } catch { continue; }
            var found = Walk(v, typeName, depth + 1);
            if (found != null) return found;
        }
        return null;
    }

    private static Type ResolveType(string name)
    {
        foreach (var asm in AppDomain.CurrentDomain.GetAssemblies())
        {
            try
            {
                var t = asm.GetType(name, throwOnError: false);
                if (t != null) return t;
            }
            catch { }
        }
        return null;
    }
}
