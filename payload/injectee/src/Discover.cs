using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Windows;

namespace XbotInjectee;

/// <summary>
/// Reflective discovery helpers. The flow-list feature needs us to locate
/// Studio's right-panel flow VM (unknown type) and a method on
/// IEngineComponent that runs a specific flow by identifier. Rather than
/// guess names, we dump candidates and let the human pick which one to wire.
/// </summary>
internal static class Discover
{
    /// <summary>
    /// Walk every Window's DataContext tree, return any VM whose type's
    /// FullName contains one of <paramref name="keywords"/>. Each hit
    /// includes its dot-path from the Window root, its type, and a brief
    /// summary of properties (with sampled values) and declared methods.
    /// </summary>
    public static object FindVms(string[] keywords, int maxDepth)
    {
        return OnUi<object>(() =>
        {
            if (Application.Current == null)
                return new { ok = false, detail = "no Application" };
            var hits = new List<object>();
            var seen = new HashSet<object>(ReferenceEqualityComparer.Instance);
            foreach (Window w in Application.Current.Windows)
            {
                var root = w.DataContext;
                Walk(root, $"${w.GetType().Name}.DataContext", 0, maxDepth, keywords, seen, hits);
            }
            return new { ok = true, count = hits.Count, hits };
        });
    }

    private static void Walk(object node, string path, int depth, int maxDepth,
        string[] keywords, HashSet<object> seen, List<object> hits)
    {
        if (node == null || depth > maxDepth) return;
        if (node is string) return;
        if (!seen.Add(node)) return;
        var t = node.GetType();
        var full = t.FullName ?? "";
        var ns = t.Namespace ?? "";
        // Restrict deep traversal to ShadowBot/Caliburn namespaces — WPF
        // internals (Windows.Markup, etc.) are noise.
        bool isOurs = ns.StartsWith("ShadowBot.") || ns.StartsWith("Caliburn.");
        if (depth > 1 && !isOurs) return;
        if (isOurs && keywords.Any(k => full.IndexOf(k, StringComparison.OrdinalIgnoreCase) >= 0))
        {
            hits.Add(SummarizeVm(node, path));
            // Don't recurse INTO the hit — its summary already covers props.
            return;
        }
        // Recurse properties.
        foreach (var p in t.GetProperties(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance))
        {
            if (p.GetIndexParameters().Length > 0) continue;
            object v;
            try { v = p.GetValue(node); } catch { continue; }
            if (v == null) continue;
            // Collections: peek at a few items (they often hold the VMs we want).
            if (v is IEnumerable col && !(v is string))
            {
                int i = 0;
                foreach (var item in col)
                {
                    if (i >= 8) break;
                    Walk(item, $"{path}.{p.Name}[{i}]", depth + 1, maxDepth, keywords, seen, hits);
                    i++;
                }
                continue;
            }
            Walk(v, $"{path}.{p.Name}", depth + 1, maxDepth, keywords, seen, hits);
        }
        // Recurse private fields too — Caliburn screens often hold child VMs
        // in _foo fields rather than public properties.
        foreach (var f in t.GetFields(BindingFlags.NonPublic | BindingFlags.Instance))
        {
            object v;
            try { v = f.GetValue(node); } catch { continue; }
            if (v == null) continue;
            Walk(v, $"{path}.{f.Name}", depth + 1, maxDepth, keywords, seen, hits);
        }
    }

    private static object SummarizeVm(object obj, string path)
    {
        var t = obj.GetType();
        var props = t.GetProperties(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .Where(p => p.GetIndexParameters().Length == 0)
            .Select(p =>
            {
                string sample;
                try
                {
                    var v = p.GetValue(obj);
                    if (v == null) sample = "null";
                    else if (v is string s) sample = Truncate(s, 120);
                    else if (v is IEnumerable e && !(v is string))
                    {
                        int c = 0; var firstNames = new List<string>();
                        foreach (var item in e)
                        {
                            if (item != null && firstNames.Count < 5)
                                firstNames.Add(NameOf(item));
                            c++;
                        }
                        sample = $"<{p.PropertyType.Name} count={c}> [{string.Join(", ", firstNames)}]";
                    }
                    else sample = Truncate(v.ToString(), 120);
                }
                catch (Exception ex) { sample = $"<err: {ex.GetType().Name}>"; }
                return $"{p.PropertyType.Name} {p.Name} = {sample}";
            })
            .Take(60)
            .ToArray();
        var methods = t.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly)
            .Where(m => !m.IsSpecialName)
            .Select(m => $"{m.ReturnType.Name} {m.Name}({string.Join(",", m.GetParameters().Select(p => $"{p.ParameterType.Name} {p.Name}"))})")
            .Distinct()
            .OrderBy(s => s)
            .Take(60)
            .ToArray();
        var fields = t.GetFields(BindingFlags.NonPublic | BindingFlags.Instance)
            .Select(f => $"{f.FieldType.Name} {f.Name}")
            .Take(40)
            .ToArray();
        return new { path, type = t.FullName, properties = props, fields, methods };
    }

    /// <summary>Best-effort "what's a sensible label for this object" used
    /// when previewing collection contents. Tries Name/DisplayName/Title and
    /// falls back to the type name.</summary>
    private static string NameOf(object item)
    {
        var t = item.GetType();
        foreach (var n in new[] { "Name", "DisplayName", "Title", "Key", "Id" })
        {
            var p = t.GetProperty(n);
            if (p == null) continue;
            try
            {
                var v = p.GetValue(item);
                if (v != null) return $"{n}={Truncate(v.ToString(), 40)}";
            }
            catch { }
        }
        return t.Name;
    }

    private static string Truncate(string s, int max) =>
        string.IsNullOrEmpty(s) ? s : (s.Length > max ? s.Substring(0, max) + "..." : s);

    /// <summary>Dump IEngineComponent's concrete type, interfaces it
    /// implements, all instance methods and non-public fields. The CLI side
    /// will read this to pick the right "run a flow by id" entry point.</summary>
    public static object EngineMethods()
    {
        var eng = Resolver.Engine();
        if (eng == null) return new { ok = false, detail = "no IEngineComponent (editor not loaded?)" };
        var t = eng.GetType();
        var ifaces = t.GetInterfaces().Select(i => i.FullName).OrderBy(s => s).ToArray();
        var methods = t.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .Where(m => !m.IsSpecialName)
            .Select(m => $"{m.DeclaringType.Name}.{m.Name}({string.Join(",", m.GetParameters().Select(p => $"{p.ParameterType.Name} {p.Name}"))}) -> {m.ReturnType.Name}")
            .Distinct()
            .OrderBy(s => s)
            .ToArray();
        var fields = t.GetFields(BindingFlags.NonPublic | BindingFlags.Instance)
            .Select(f => $"{f.FieldType.Name} {f.Name}")
            .OrderBy(s => s)
            .ToArray();
        return new { ok = true, type = t.FullName, interfaces = ifaces, methods, fields };
    }

    /// <summary>Load a type by name (FQN or simple — we scan all loaded
    /// assemblies) and dump its constructors, properties, fields. Used to
    /// figure out the shape of parameter types like EngineCreateOptions that
    /// don't appear as a live instance in the VM graph.</summary>
    public static object DescribeType(string name)
    {
        var t = ResolveType(name);
        if (t == null) return new { ok = false, detail = $"type not found: {name}" };
        var ctors = t.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .Select(c => $".ctor({string.Join(",", c.GetParameters().Select(p => $"{p.ParameterType.Name} {p.Name}"))})")
            .ToArray();
        var props = t.GetProperties(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .Select(p => $"{(p.CanWrite ? "rw" : "ro")} {p.PropertyType.FullName} {p.Name}")
            .OrderBy(s => s).ToArray();
        var fields = t.GetFields(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .Select(f => $"{f.FieldType.FullName} {f.Name}")
            .OrderBy(s => s).ToArray();
        var methods = t.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly)
            .Where(m => !m.IsSpecialName)
            .Select(m => $"{m.ReturnType.Name} {m.Name}({string.Join(",", m.GetParameters().Select(p => $"{p.ParameterType.Name} {p.Name}"))})")
            .OrderBy(s => s).ToArray();
        return new {
            ok = true,
            type = t.FullName,
            assembly = t.Assembly.GetName().Name,
            baseType = t.BaseType?.FullName,
            interfaces = t.GetInterfaces().Select(i => i.FullName).ToArray(),
            isEnum = t.IsEnum,
            enumValues = t.IsEnum ? Enum.GetNames(t) : null,
            constructors = ctors,
            properties = props,
            fields,
            methods,
        };
    }

    private static Type ResolveType(string name)
    {
        foreach (var asm in AppDomain.CurrentDomain.GetAssemblies())
        {
            try
            {
                var t = asm.GetType(name, throwOnError: false);
                if (t != null) return t;
                // Also try simple-name match if user didn't pass FQN.
                if (!name.Contains("."))
                {
                    foreach (var tt in asm.GetTypes())
                        if (tt.Name == name) return tt;
                }
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
