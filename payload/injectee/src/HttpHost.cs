using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Windows;

namespace XbotInjectee;

/// <summary>
/// Minimal HttpListener-based control surface. Five endpoints only:
///   GET  /health                — injectee liveness
///   POST /studio/run            — Studio's toolbar Run (DebugMode.Normal)
///   POST /studio/pause          — toggles pause/resume on the running engine
///   POST /studio/stop           — Studio's toolbar Stop (used by Ctrl+C path)
///   GET  /studio/output?since=N — log entries from LogViewModel.LogAdded + engine output
/// </summary>
internal static class HttpHost
{
    private static HttpListener _listener;
    private static int _activeRequests;
    private static readonly object Admission=new();
    private static bool _stopping;
    private static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static void Run(int port)
    {
        var listener = new HttpListener();
        listener.Prefixes.Add($"http://127.0.0.1:{port}/");
        try { listener.Start(); }
        catch (Exception ex)
        {
            BootstrapLog.Error($"HttpListener.Start on {port}: {ex.Message}");
            return;
        }
        _listener=listener;
        lock(Admission){_stopping=false;}
        BootstrapLog.Info($"http listening on {port}");

        // NOTE: we intentionally do NOT call LogTap.EnsureHooked() here.
        // EnsureHooked goes through Dispatcher.Invoke, which blocks the
        // listener thread for as long as the WPF dispatcher is busy (Studio
        // splash, project load, modal login dialog, etc). If we block here,
        // the `while (true) GetContext` loop below never runs, every incoming
        // request hangs in HTTP.sys, and even /health stops responding.
        // /studio/run and /studio/output each call EnsureHooked() inline, so
        // logs still get tapped by the time anyone cares.

        // Dispatch each request on the threadpool. Some handlers (anything
        // touching VMs) call Dispatcher.Invoke under the hood, which blocks
        // the calling thread while WPF's UI thread is busy (login dialog,
        // splash, project load, modal). If we ran Handle() synchronously on
        // the listener thread, one slow request would stall the whole
        // listener — even /health would stop responding while a /studio/run
        // sits waiting for Dispatcher. Off-loading lets the accept loop keep
        // serving while in-flight handlers wait their turn on the UI thread.
        while (true)
        {
            HttpListenerContext ctx;
            try { ctx = listener.GetContext(); }
            catch (Exception ex) { BootstrapLog.Error("GetContext: " + ex.Message); break; }
            bool admitted;
            lock(Admission){admitted=!_stopping;if(admitted)_activeRequests++;}
            if(!admitted){try{WriteJson(ctx,503,new{ok=false,detail="control listener is upgrading"});}catch{}continue;}
            System.Threading.ThreadPool.QueueUserWorkItem(_ =>
            {
                try { Handle(ctx); }
                catch (Exception ex)
                {
                    try { WriteJson(ctx, 500, new { ok = false, error = ex.GetType().Name, message = ex.Message }); }
                    catch { /* client gone */ }
                }
                finally {System.Threading.Interlocked.Decrement(ref _activeRequests);}
            });
        }
    }

    private static void Handle(HttpListenerContext ctx)
    {
        var req = ctx.Request;
        var path = req.Url?.AbsolutePath?.TrimEnd('/') ?? "";
        var method = req.HttpMethod.ToUpperInvariant();
        if(method=="POST" && path=="/control/stop") {
            bool accepted;
            lock(Admission){accepted=string.IsNullOrEmpty(req.Headers["Origin"])&&_activeRequests==1&&!_stopping;if(accepted)_stopping=true;}
            if(!accepted){WriteJson(ctx,409,new{ok=false,detail="control host busy or browser-origin request"});return;}
            WriteJson(ctx,200,new{ok=true,detail="stopping only this CLI control listener"});
            System.Threading.ThreadPool.QueueUserWorkItem(_=>_listener.Stop());return;
        }

        if(method=="POST" && (path=="/client/api/invoke" || path=="/client/settings"))
        {
            if(!string.IsNullOrEmpty(req.Headers["Origin"])) {WriteJson(ctx,403,new{ok=false,detail="browser-origin requests are not accepted"});return;}
            if(req.ContentLength64>1048576 || !(req.ContentType??"").StartsWith("application/json",StringComparison.OrdinalIgnoreCase)) {WriteJson(ctx,400,new{ok=false,detail="application/json body up to 1 MiB required"});return;}
            try {
                using var reader=new StreamReader(req.InputStream,req.ContentEncoding);
                var chars=new char[1048577];var length=0;
                while(length<chars.Length){var read=reader.Read(chars,length,chars.Length-length);if(read==0)break;length+=read;}
                if(length>1048576)throw new ArgumentException("body exceeds limit");
                using var document=JsonDocument.Parse(new string(chars,0,length));
                var result=path=="/client/api/invoke"?ClientActions.Invoke(document.RootElement):ClientActions.SettingsOperation(document.RootElement);
                WriteJson(ctx,200,result);
            } catch(Exception ex){WriteJson(ctx,ex is ArgumentException||ex is JsonException?400:500,new{ok=false,detail=ex.GetBaseException().Message});}
            return;
        }

        if (method == "GET" && path == "/health")
        {
            var app = Application.Current;
            var windowCount = 0;
            if (app != null)
            {
                try
                {
                    windowCount = app.Dispatcher.CheckAccess()?app.Windows.Count:-1;
                }
                catch { /* health must never fail because WPF is busy */ }
            }
            WriteJson(ctx, 200, new
            {
                ok = true,
                pid = System.Diagnostics.Process.GetCurrentProcess().Id,
                hasApplication = app != null,
                windowCount,
                controlProtocol=2,
                buildId=typeof(HttpHost).Assembly.GetName().Name,
            });
            return;
        }

        if (method == "GET" && path == "/activity/probe")
        {
            WriteJson(ctx, 200, ActivityActions.Probe()); return;
        }
        if (method == "POST" && (path == "/activity/capture-probe" || path == "/activity/capture-config" || path == "/activity/capture-logs"))
        {
            if (!string.IsNullOrEmpty(req.Headers["Origin"])) { WriteJson(ctx,403,new {ok=false,detail="browser-origin request rejected"}); return; }
            using var reader = new StreamReader(req.InputStream, req.ContentEncoding);
            using var doc = JsonDocument.Parse(reader.ReadToEnd());
            var root = doc.RootElement;
            var result = path == "/activity/capture-probe" ? DocCapture.Probe(root.GetProperty("appId").GetString()) :
                path == "/activity/capture-logs" ? DocCapture.Logs(root.GetProperty("appId").GetString(), root.GetProperty("output").GetString()) :
                DocCapture.Config(root.GetProperty("appId").GetString(), root.GetProperty("flowId").GetString(), root.GetProperty("blockId").GetString(), root.GetProperty("output").GetString(),
                    root.TryGetProperty("heightLimit", out var height) && height.ValueKind == JsonValueKind.Number ? height.GetDouble() : null);
            WriteJson(ctx, 200, result); return;
        }
        if (method == "GET" && path == "/activity/install-state")
        {
            WriteJson(ctx,200,StudioActions.ProbeInstallState()); return;
        }
        if (method == "POST" && path == "/activity/create")
        {
            if (!string.IsNullOrEmpty(req.Headers["Origin"])) { WriteJson(ctx,403,new {ok=false,detail="browser-origin request rejected"}); return; }
            using var reader = new StreamReader(req.InputStream, req.ContentEncoding);
            using var doc = JsonDocument.Parse(reader.ReadToEnd());
            var root = doc.RootElement;
            WriteJson(ctx, 200, ActivityActions.Create(root.GetProperty("name").GetString(), root.GetProperty("code").GetString(), root.GetProperty("description").GetString())); return;
        }
        if (method == "POST" && path == "/activity/finish-create")
        {
            if (!string.IsNullOrEmpty(req.Headers["Origin"])) { WriteJson(ctx,403,new {ok=false,detail="browser-origin request rejected"}); return; }
            using var reader = new StreamReader(req.InputStream, req.ContentEncoding);
            using var doc = JsonDocument.Parse(reader.ReadToEnd());
            var root = doc.RootElement;
            WriteJson(ctx, 200, StudioActions.ConfigureNewActivity(root.GetProperty("appId").GetString(), root.GetProperty("name").GetString(), root.GetProperty("code").GetString(), root.GetProperty("description").GetString())); return;
        }
        if (method == "POST" && (path == "/activity/create-consumer" || path == "/activity/install"))
        {
            if (!string.IsNullOrEmpty(req.Headers["Origin"])) { WriteJson(ctx,403,new {ok=false,detail="browser-origin request rejected"}); return; }
            using var reader = new StreamReader(req.InputStream, req.ContentEncoding);
            using var doc = JsonDocument.Parse(reader.ReadToEnd());
            var root = doc.RootElement;
            var value = path == "/activity/create-consumer"
                ? ActivityActions.CreateConsumer(root.GetProperty("name").GetString(), root.GetProperty("description").GetString())
                : StudioActions.InstallActivity(root.GetProperty("appId").GetString(), root.GetProperty("code").GetString(), root.GetProperty("version").GetString(), root.GetProperty("name").GetString(), root.TryGetProperty("upgrade", out var upgrade) && upgrade.GetBoolean());
            WriteJson(ctx, 200, value); return;
        }

        if (method == "POST" && path == "/studio/run")
        {
            // Optional body {"flow":"<name>"} runs a specific flow instead of
            // whatever F5 would pick. Empty body = original F5 behaviour.
            string flow = null;
            try
            {
                using var sr = new StreamReader(req.InputStream, req.ContentEncoding);
                var body = sr.ReadToEnd();
                if (!string.IsNullOrWhiteSpace(body))
                {
                    using var doc = JsonDocument.Parse(body);
                    if (doc.RootElement.TryGetProperty("flow", out var f) && f.ValueKind == JsonValueKind.String)
                        flow = f.GetString();
                }
            }
            catch { /* malformed body — treat as no flow, original behaviour */ }
            // Drop previous run's buffer so /studio/output only returns this run.
            LogTap.Reset();
            LogTap.EnsureHooked();
            var r = StudioActions.Run(flow);
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "GET" && path == "/studio/flows")
        {
            var r = StudioActions.ListFlows();
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "GET" && path == "/studio/current")
        {
            var r = StudioActions.Current();
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if(method=="POST" && path=="/studio/reload-flow") {
            using var reader=new StreamReader(req.InputStream,req.ContentEncoding);
            using var document=JsonDocument.Parse(reader.ReadToEnd());
            string name=document.RootElement.TryGetProperty("name",out var n)&&n.ValueKind==JsonValueKind.String?n.GetString():null;
            var result=StudioActions.ReloadCodeFlows(name,string.IsNullOrWhiteSpace(name));
            WriteJson(ctx,result.ok?200:500,result);return;
        }

        if (method == "POST" && path == "/studio/codeflow/source")
        {
            // Replace one CodeFlow's in-memory source AND its .py on disk in a
            // single UI-thread step — the source-only write path that the
            // official codeflow.write/edit cannot provide without rebuilding
            // flow parameters and forms.
            if (!string.IsNullOrEmpty(req.Headers["Origin"])) { WriteJson(ctx,403,new {ok=false,detail="browser-origin request rejected"}); return; }
            try
            {
                using var reader = new StreamReader(req.InputStream, req.ContentEncoding);
                using var document = JsonDocument.Parse(reader.ReadToEnd());
                var root = document.RootElement;
                var flow = root.GetProperty("flow").GetString();
                var source = root.GetProperty("source").GetString();
                var expectMd5 = root.TryGetProperty("expectMd5", out var e) && e.ValueKind == JsonValueKind.String ? e.GetString() : null;
                var result = StudioActions.SetCodeFlowSource(flow, source, expectMd5);
                WriteJson(ctx, result.ok ? 200 : 500, result);
            }
            catch (Exception ex)
            {
                WriteJson(ctx, ex is ArgumentException || ex is JsonException ? 400 : 500,
                    new { ok = false, detail = ex.GetBaseException().Message });
            }
            return;
        }

        if (method == "POST" && path == "/studio/create-module")
        {
            string name = null;
            try
            {
                using var sr = new StreamReader(req.InputStream, req.ContentEncoding);
                var body = sr.ReadToEnd();
                if (!string.IsNullOrWhiteSpace(body))
                {
                    using var doc = JsonDocument.Parse(body);
                    var root = doc.RootElement;
                    if (root.TryGetProperty("name", out var n) && n.ValueKind == JsonValueKind.String)
                        name = n.GetString();
                }
            }
            catch (Exception ex)
            {
                WriteJson(ctx, 400, new { ok = false, detail = "bad body: " + ex.Message });
                return;
            }
            var r = StudioActions.CreateModule(name);
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "POST" && path == "/console/run")
        {
            // Body: { "appId":"<UUID>", "inputs":{...} }
            // Use Console's protocol run for the current-user execution
            // context, then tag the created task's SourceName as xbot-cli.
            string appId = null, appName = null;
            Dictionary<string, object> inputs = null;
            bool hasAppParams = false;
            try
            {
                using var sr = new StreamReader(req.InputStream, req.ContentEncoding);
                var body = sr.ReadToEnd();
                using var doc = JsonDocument.Parse(body);
                var root = doc.RootElement;
                if (root.TryGetProperty("appId", out var a)) appId = a.GetString();
                if (root.TryGetProperty("appName", out var n) && n.ValueKind == JsonValueKind.String)
                    appName = n.GetString();
                if (root.TryGetProperty("inputs", out var i) && i.ValueKind == JsonValueKind.Object)
                    inputs = JsonObjectToDict(i);
                hasAppParams = root.TryGetProperty("appParams", out var p)
                    && p.ValueKind == JsonValueKind.Object
                    && p.EnumerateObject().Any();
            }
            catch (Exception ex)
            {
                WriteJson(ctx, 400, new { ok = false, detail = "bad body: " + ex.Message });
                return;
            }
            if (hasAppParams)
            {
                WriteJson(ctx, 400, new { ok = false, detail = "--app-param is not supported by Console protocol run; use --input KEY=VALUE" });
                return;
            }
            // Drop previous run's log buffer the same way /studio/run does —
            // console runs flow through the same IEngineComponent so LogTap
            // already picks them up; we just want a clean window per run.
            LogTap.Reset();
            LogTap.EnsureHooked();
            var r = ConsoleActions.RunWithCliSource(appId, appName, inputs);
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "POST" && path == "/studio/open-app")
        {
            string appId = null;
            try
            {
                using var sr = new StreamReader(req.InputStream, req.ContentEncoding);
                var body = sr.ReadToEnd();
                using var doc = JsonDocument.Parse(body);
                if (doc.RootElement.TryGetProperty("appId", out var a) && a.ValueKind == JsonValueKind.String)
                    appId = a.GetString();
            }
            catch (Exception ex)
            {
                WriteJson(ctx, 400, new { ok = false, detail = "bad body: " + ex.Message });
                return;
            }
            var r = ConsoleActions.OpenDevelopedAppInStudio(appId);
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "POST" && path == "/console/publish")
        {
            string appId = null, updateLog = "";
            int timeoutSeconds = 180;
            try
            {
                using var sr = new StreamReader(req.InputStream, req.ContentEncoding);
                var body = sr.ReadToEnd();
                if (!string.IsNullOrWhiteSpace(body))
                {
                    using var doc = JsonDocument.Parse(body);
                    var root = doc.RootElement;
                    if (root.TryGetProperty("appId", out var a) && a.ValueKind == JsonValueKind.String)
                        appId = a.GetString();
                    if (root.TryGetProperty("updateLog", out var u) && u.ValueKind == JsonValueKind.String)
                        updateLog = u.GetString() ?? "";
                    if (root.TryGetProperty("timeoutSeconds", out var t) && t.ValueKind == JsonValueKind.Number
                        && t.TryGetInt32(out var seconds) && seconds > 0)
                        timeoutSeconds = seconds;
                }
            }
            catch (Exception ex)
            {
                WriteJson(ctx, 400, new { ok = false, detail = "bad body: " + ex.Message });
                return;
            }
            var r = ConsoleActions.PublishDevelopedApp(appId, updateLog, TimeSpan.FromSeconds(timeoutSeconds));
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "POST" && path == "/console/stop")
        {
            string taskId = null, reason = null;
            try
            {
                using var sr = new StreamReader(req.InputStream, req.ContentEncoding);
                var body = sr.ReadToEnd();
                using var doc = JsonDocument.Parse(body);
                if (doc.RootElement.TryGetProperty("taskId", out var t) && t.ValueKind == JsonValueKind.String) taskId = t.GetString();
                if (doc.RootElement.TryGetProperty("reason", out var rs) && rs.ValueKind == JsonValueKind.String) reason = rs.GetString();
            }
            catch { }
            var r = ConsoleActions.StopTask(taskId, reason);
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "POST" && path == "/console/show")
        {
            bool force = false;
            try
            {
                using var sr = new StreamReader(req.InputStream, req.ContentEncoding);
                var body = sr.ReadToEnd();
                if (!string.IsNullOrWhiteSpace(body))
                {
                    using var doc = JsonDocument.Parse(body);
                    if (doc.RootElement.TryGetProperty("force", out var f)
                        && (f.ValueKind == JsonValueKind.True || f.ValueKind == JsonValueKind.False))
                        force = f.GetBoolean();
                }
            }
            catch (Exception ex)
            {
                WriteJson(ctx, 400, new { ok = false, detail = "bad body: " + ex.Message });
                return;
            }
            var r = ConsoleActions.ReturnToConsole(force);
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "GET" && path == "/console/status")
        {
            var taskId = req.QueryString["taskId"];
            var r = ConsoleActions.CurrentTaskStatus(taskId);
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "POST" && path == "/studio/pause")
        {
            // EngineManager.PausedSwitch() is Studio's own toggle — pauses
            // if running, resumes if paused. No state tracking on our side.
            var r = StudioActions.Pause();
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "POST" && path == "/studio/stop")
        {
            var r = StudioActions.Stop();
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "GET" && path == "/studio/describe-toolbar")
        {
            var f = req.QueryString["field"];
            var r = string.IsNullOrEmpty(f)
                ? StudioActions.DescribeToolBar()
                : StudioActions.DescribeToolBarField(f);
            WriteJson(ctx, r.ok ? 200 : 500, r);
            return;
        }

        if (method == "GET" && path == "/studio/discover/vms")
        {
            // Default keyword set targets the right-side flow panel + related
            // process/block/project VMs. Override with ?q=Foo,Bar.
            var q = req.QueryString["q"];
            var keywords = (string.IsNullOrEmpty(q) ? "Flow,Process,Block,Project,Sidebar,Navigation,Tab"
                : q).Split(',', StringSplitOptions.RemoveEmptyEntries);
            int.TryParse(req.QueryString["depth"] ?? "8", out var depth);
            WriteJson(ctx, 200, Discover.FindVms(keywords, depth));
            return;
        }

        if (method == "GET" && path == "/studio/discover/engine")
        {
            WriteJson(ctx, 200, Discover.EngineMethods());
            return;
        }

        if (method == "GET" && path == "/studio/discover/type")
        {
            var name = req.QueryString["name"] ?? "";
            WriteJson(ctx, 200, Discover.DescribeType(name));
            return;
        }

        if (method == "GET" && path == "/studio/output")
        {
            LogTap.EnsureHooked(); // retry if LogVm wasn't there at startup
            int.TryParse(req.QueryString["since"] ?? "0", out var since);
            var slice = LogTap.Get(since);
            var active = StudioActions.CountActiveEngines();
            WriteJson(ctx, 200, new {
                ok = true,
                generation = slice.generation,
                total = slice.total,
                since = slice.since,
                entries = slice.entries,
                activeEngines = active,
            });
            return;
        }

        WriteJson(ctx, 404, new { ok = false, error = "not_found", path });
    }

    /// <summary>Convert a JSON object element into a Dictionary&lt;string,object&gt;
    /// preserving primitive types — TaskSettings.Inputs is typed object so
    /// the receiving Python sees int/bool/string/null faithfully instead of
    /// everything-as-string.</summary>
    private static Dictionary<string, object> JsonObjectToDict(JsonElement obj)
    {
        var dict = new Dictionary<string, object>();
        foreach (var kv in obj.EnumerateObject())
            dict[kv.Name] = JsonElementToObject(kv.Value);
        return dict;
    }

    private static object JsonElementToObject(JsonElement el) => el.ValueKind switch
    {
        JsonValueKind.String => el.GetString(),
        JsonValueKind.Number => el.TryGetInt64(out var i) ? (object)i : el.GetDouble(),
        JsonValueKind.True => true,
        JsonValueKind.False => false,
        JsonValueKind.Null => null,
        JsonValueKind.Object => JsonObjectToDict(el),
        JsonValueKind.Array => el.EnumerateArray().Select(JsonElementToObject).ToArray(),
        _ => el.ToString(),
    };

    private static void WriteJson(HttpListenerContext ctx, int status, object obj)
    {
        ctx.Response.StatusCode = status;
        ctx.Response.ContentType = "application/json; charset=utf-8";
        var bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(obj, Json));
        ctx.Response.OutputStream.Write(bytes, 0, bytes.Length);
        ctx.Response.OutputStream.Close();
    }
}
