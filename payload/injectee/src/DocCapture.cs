using System;
using System.Collections;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Threading.Tasks;
using System.Threading;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Threading;
using System.Windows.Controls;
using System.Windows.Media;
using PixelFormat = System.Drawing.Imaging.PixelFormat;

namespace XbotInjectee;

/// <summary>Captures real client windows; contains no instruction-library business rules.</summary>
internal static class DocCapture
{
    private const BindingFlags Flags = BindingFlags.Instance | BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic;
    private static object Get(object value, string name) => value?.GetType().GetProperty(name, Flags)?.GetValue(value);
    private static object Field(object value, string name) => value?.GetType().GetField(name, Flags)?.GetValue(value);
    private static Type TypeOf(string name) => AppDomain.CurrentDomain.GetAssemblies().Select(a => a.GetType(name, false))
        .FirstOrDefault(t => t != null) ?? throw new NotSupportedException("Client type unavailable: " + name);
    private static object Service(Type type)
    {
        var get = TypeOf("Caliburn.Micro.IoC").GetField("GetInstance", Flags)?.GetValue(null) as Func<Type, string, object>;
        return get?.Invoke(type, null) ?? throw new NotSupportedException("Client service unavailable: " + type.FullName);
    }
    private static T OnUi<T>(Func<T> action, DateTime? deadline = null)
    {
        var dispatcher = Application.Current?.Dispatcher ?? throw new InvalidOperationException("Client is not running");
        void CheckDeadline()
        {
            if (deadline.HasValue && DateTime.UtcNow >= deadline.Value)
                throw new TimeoutException("Capture deadline reached; queued UI work is cancelled");
        }
        CheckDeadline();
        if (dispatcher.CheckAccess()) return action();
        var pending = dispatcher.InvokeAsync(() => { CheckDeadline(); return action(); });
        var remaining = deadline.HasValue ? Math.Min(15, Math.Max(.001, (deadline.Value - DateTime.UtcNow).TotalSeconds)) : 15;
        if (!pending.Task.Wait(TimeSpan.FromSeconds(remaining)))
        {
            pending.Abort();
            throw new TimeoutException("Client UI did not respond; inspect before retrying");
        }
        return pending.Task.GetAwaiter().GetResult();
    }
    private static object Workspace(string appId)
    {
        if (!Guid.TryParse(appId, out _)) throw new ArgumentException("A valid target app_id is required");
        var toolbar = typeof(StudioActions).GetMethod("FindToolBarVm", Flags)?.Invoke(null, null)
            ?? throw new InvalidOperationException("Studio is not open");
        var workspace = Field(toolbar, "_workspace") ?? throw new InvalidOperationException("Studio workspace unavailable");
        if (!string.Equals(Get(workspace, "UUID")?.ToString(), appId, StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException("Target app_id does not match the current Studio");
        ActivityInstallGuard.RequireIdle(Field(toolbar, "_engineManager"));
        return workspace;
    }
    private static object Block(object workspace, string flowId, string blockId)
    {
        var flows = ((IEnumerable)Get(workspace, "Flows")).Cast<object>();
        var flow = flows.SingleOrDefault(f => Get(f, "FileName")?.ToString() == flowId)
            ?? flows.SingleOrDefault(f => Get(f, "Name")?.ToString() == flowId)
            ?? throw new ArgumentException("Target flow was not found");
        return ((IEnumerable)Get(flow, "Blocks")).Cast<object>().SingleOrDefault(b => Get(b, "Id")?.ToString() == blockId)
            ?? throw new ArgumentException("Target block_id was not found in the target flow");
    }
    private static object[] Dependencies(ConstructorInfo constructor, object workspace, object block)
        => constructor.GetParameters().Select(p => p.ParameterType.IsInstanceOfType(block) ? block :
            p.ParameterType.IsInstanceOfType(workspace) ? workspace : Service(p.ParameterType)).ToArray();
    private static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        yield return root;
        for (var index = 0; index < VisualTreeHelper.GetChildrenCount(root); index++)
            foreach (var child in Descendants(VisualTreeHelper.GetChild(root, index))) yield return child;
    }

    public static object Probe(string appId) => OnUi(() =>
    {
        var workspace = Workspace(appId);
        var constructor = TypeOf("ShadowBot.Shell.Development.BlockSettings.BlockSettingsViewModel").GetConstructors().Single();
        var services = constructor.GetParameters().Select(p =>
        {
            if (p.Name == "block" || p.ParameterType.IsInstanceOfType(workspace))
                return new { type = p.ParameterType.FullName, available = true, error = (string)null };
            try { Service(p.ParameterType); return new { type = p.ParameterType.FullName, available = true, error = (string)null }; }
            catch (Exception ex) { return new { type = p.ParameterType.FullName, available = false, error = ex.GetBaseException().Message }; }
        }).ToArray();
        return (object)new { ok = true, appId, services,
            visibleWindows = Application.Current.Windows.Cast<Window>().Where(w => w.IsVisible)
                .Select(w => new { type = w.GetType().FullName, modelType = w.DataContext?.GetType().FullName }).ToArray(),
            capture = "native window pixels; no mouse cursor compositing" };
    });

    private static string Output(string path)
    {
        if (!Path.IsPathFullyQualified(path) || !string.Equals(Path.GetExtension(path), ".png", StringComparison.OrdinalIgnoreCase))
            throw new ArgumentException("An absolute PNG output path is required");
        path = Path.GetFullPath(path);
        if (File.Exists(path)) throw new IOException("Output exists; capture to a new path");
        Directory.CreateDirectory(Path.GetDirectoryName(path));
        return path;
    }
    [StructLayout(LayoutKind.Sequential)] private struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr handle, out Rect rectangle);
    [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
    private static object CaptureWindow(Window window, string path, string appId, string flowId, string blockId)
    {
        window.Activate();
        window.UpdateLayout();
        var handle = new WindowInteropHelper(window).Handle;
        if (handle == IntPtr.Zero || !GetWindowRect(handle, out var rect)) throw new InvalidOperationException("Native window bounds unavailable");
        if (GetForegroundWindow() != handle) throw new InvalidOperationException("Target window is not foreground; no screenshot claimed");
        var width = rect.Right - rect.Left;
        var height = rect.Bottom - rect.Top;
        if (width < 100 || height < 100 || width > 10000 || height > 10000) throw new InvalidOperationException("Invalid window dimensions");
        // CopyFromScreen does not draw the hardware cursor; it also captures Chromium hosted HWNDs.
        using (var bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb))
        {
            using var graphics = Graphics.FromImage(bitmap);
            graphics.CopyFromScreen(rect.Left, rect.Top, 0, 0, new System.Drawing.Size(width, height), CopyPixelOperation.SourceCopy);
            bitmap.Save(path, ImageFormat.Png);
        }
        var result = new { ok = true, path, appId, flowId, blockId, width, height,
            windowType = window.GetType().FullName, captureMethod = "native-window-pixels-without-cursor",
            sha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(path))).ToLowerInvariant() };
        File.WriteAllText(Path.ChangeExtension(path, ".capture.json"), System.Text.Json.JsonSerializer.Serialize(result));
        return result;
    }

    public static object Config(string appId, string flowId, string blockId, string output, double? heightLimit = null)
    {
        if (heightLimit.HasValue && (double.IsNaN(heightLimit.Value) || double.IsInfinity(heightLimit.Value) || heightLimit.Value < 350 || heightLimit.Value > 3000))
            throw new ArgumentException("height_limit must be between 350 and 3000 logical pixels");
        output = Output(output);
        var completion = new TaskCompletionSource<object>(TaskCreationOptions.RunContinuationsAsynchronously);
        var cancelled = new CancellationTokenSource();
        var dispatcher = Application.Current?.Dispatcher ?? throw new InvalidOperationException("Client is not running");
        var operation = dispatcher.InvokeAsync(new Action(() =>
        {
            if (cancelled.IsCancellationRequested) { completion.TrySetCanceled(); return; }
            object model = null;
            object block = null;
            string before = null;
            Window ownedWindow = null;
            DispatcherTimer timer = null;
            var started = DateTime.UtcNow;
            TabItem[] tabs = null;
            var tabIndex = 0;
            var pageIndex = 0;
            var initialized = false;
            ScrollViewer scroller = null;
            var captures = new List<object>();
            var capturePaths = new List<string>();
            void Finish(Exception error = null)
            {
                timer?.Stop();
                try { model?.GetType().GetMethod("Cancel", Flags)?.Invoke(model, null); ownedWindow?.Close(); }
                catch (Exception ex) { error ??= ex.GetBaseException(); }
                if (block != null && block.GetType().GetMethod("ToJson", Flags)?.Invoke(block, null)?.ToString() != before)
                    error = new InvalidOperationException("Configuration capture changed block state; inspect before proceeding");
                if (error != null) completion.TrySetException(error);
            }
            try
            {
                var workspace = Workspace(appId);
                if (Application.Current.Windows.Cast<Window>().Any(w => w.IsVisible && w.GetType().Name.Contains("BlockSettings")))
                    throw new InvalidOperationException("An existing parameter window is open; preserve it before capturing");
                block = Block(workspace, flowId, blockId);
                before = block.GetType().GetMethod("ToJson", Flags)?.Invoke(block, null)?.ToString();
                var type = TypeOf("ShadowBot.Shell.Development.BlockSettings.BlockSettingsViewModel");
                var constructor = type.GetConstructors().Single();
                model = constructor.Invoke(Dependencies(constructor, workspace, block));
                var manager = Service(TypeOf("ShadowBot.Shell.Development.Common.IBlockSettingsViewManager"));
                timer = new DispatcherTimer(DispatcherPriority.ApplicationIdle, dispatcher) { Interval = TimeSpan.FromMilliseconds(300) };
                timer.Tick += (_, _) =>
                {
                    try
                    {
                        if (cancelled.IsCancellationRequested)
                        {
                            Finish(new OperationCanceledException("Capture was cancelled before completion"));
                            return;
                        }
                        ownedWindow ??= Application.Current.Windows.Cast<Window>().FirstOrDefault(w => w.IsVisible && ReferenceEquals(w.DataContext, model));
                        if (DateTime.UtcNow - started > TimeSpan.FromSeconds(25))
                        {
                            Finish(new TimeoutException("Capture page limit reached; no complete form capture claimed"));
                            return;
                        }
                        if (ownedWindow != null && DateTime.UtcNow - started > TimeSpan.FromSeconds(2))
                        {
                            if (!initialized)
                            {
                                ToolTipService.SetIsEnabled(ownedWindow, false);
                                ownedWindow.MaxHeight = Math.Min(heightLimit ?? SystemParameters.WorkArea.Height - 30, SystemParameters.WorkArea.Height - 30);
                                ownedWindow.UpdateLayout();
                                tabs = Descendants(ownedWindow).OfType<TabItem>().Where(tab =>
                                {
                                    var header = tab.Header as string ?? Get(tab.Header, "Text")?.ToString();
                                    return tab.IsVisible && tab.IsEnabled && (header == "常规" || header == "高级");
                                }).ToArray();
                                if (tabs.Length == 0)
                                    throw new NotSupportedException("Native parameter tabs unavailable; this form requires a dedicated capture adapter");
                                if (tabs.Length > 0) tabs[0].IsSelected = true;
                                initialized = true;
                                return;
                            }
                            ownedWindow.UpdateLayout();
                            scroller ??= Descendants(ownedWindow).OfType<ScrollViewer>()
                                .Where(viewer => viewer.IsVisible && viewer.ActualHeight > 120 && viewer.ScrollableHeight > 1 &&
                                    viewer.TemplatedParent is not System.Windows.Controls.Primitives.TextBoxBase &&
                                    viewer.TemplatedParent is not ComboBox)
                                .OrderByDescending(viewer => viewer.ActualHeight).FirstOrDefault();
                            var path = captures.Count == 0 ? output : Path.Combine(Path.GetDirectoryName(output),
                                Path.GetFileNameWithoutExtension(output) + "-tab" + (tabIndex + 1) + "-page" + (pageIndex + 1) + ".png");
                            if (File.Exists(path)) throw new IOException("Capture page already exists: " + path);
                            captures.Add(CaptureWindow(ownedWindow, path, appId, flowId, blockId));
                            capturePaths.Add(path);
                            if (scroller != null && scroller.ScrollableHeight - scroller.VerticalOffset > 1)
                            {
                                var offset = scroller.VerticalOffset;
                                scroller.ScrollToVerticalOffset(Math.Min(scroller.ScrollableHeight, offset + Math.Max(1, scroller.ViewportHeight * .8)));
                                scroller.UpdateLayout();
                                if (scroller.VerticalOffset <= offset)
                                    throw new InvalidOperationException("Parameter form scrolling did not advance; no complete capture claimed");
                                pageIndex++;
                                return;
                            }
                            if (tabs.Length > 0 && tabIndex + 1 < tabs.Length)
                            {
                                tabIndex++;
                                pageIndex = 0;
                                scroller = null;
                                tabs[tabIndex].IsSelected = true;
                                ownedWindow.UpdateLayout();
                                return;
                            }
                            var result = new { ok = true, path = output, appId, flowId, blockId,
                                images = capturePaths.ToArray(), captures = captures.ToArray(),
                                tabs = tabs.Select(tab => tab.Header as string ?? Get(tab.Header, "Text")?.ToString()).ToArray(),
                                blockUnchanged = true };
                            Finish();
                            if (!completion.Task.IsFaulted)
                            {
                                File.WriteAllText(Path.ChangeExtension(output, ".batch.json"), System.Text.Json.JsonSerializer.Serialize(result));
                                completion.TrySetResult(result);
                            }
                        }
                        else if (DateTime.UtcNow - started > TimeSpan.FromSeconds(25))
                            Finish(new TimeoutException("Native parameter window was not ready; no screenshot claimed"));
                    }
                    catch (Exception ex) { Finish(ex.GetBaseException()); }
                };
                timer.Start();
                // Use the client's normal settings window manager, including native/Chromium forms.
                var task = manager.GetType().GetMethod("ShowWindow", Flags)?.Invoke(manager, new[] { model }) as Task;
                if (task != null) _ = task.ContinueWith(t =>
                {
                    if (t.IsFaulted) dispatcher.BeginInvoke(new Action(() => Finish(t.Exception.GetBaseException())));
                });
            }
            catch (Exception ex) { Finish(ex.GetBaseException()); }
        }));
        if (!completion.Task.Wait(TimeSpan.FromSeconds(30)))
        {
            cancelled.Cancel();
            operation.Abort();
            throw new TimeoutException("Capture timed out and queued work was cancelled; inspect any partial evidence before retrying");
        }
        return completion.Task.GetAwaiter().GetResult();
    }

    public static object Logs(string appId, string output)
    {
        var deadline = DateTime.UtcNow.AddSeconds(20);
        T Step<T>(Func<T> action) => OnUi(action, deadline);
        output = Output(output);
        // Activate the client's existing log panel; never create or repopulate a replacement log view.
        Step(() =>
        {
            Workspace(appId);
            var window = Application.Current.Windows.Cast<Window>().Single(w => w.IsVisible &&
                w.DataContext?.GetType().FullName == "ShadowBot.Shell.Development.ViewModels.DevelopViewModel");
            var bottom = Get(window.DataContext, "BottomDocking");
            var model = Field(bottom, "_logViewModel") ?? throw new InvalidOperationException("Native log model unavailable");
            bottom.GetType().GetMethod("ExpandAndActiveScreen", Flags)?.Invoke(bottom, new[] { model, null });
            var layout = Get(Get(window.DataContext, "Layout"), "Bottom");
            var size = layout?.GetType().GetProperty("Size", Flags);
            if (size != null) size.SetValue(layout, Math.Min(640d, window.ActualHeight * .7));
            window.Activate();
            return true;
        });
        Task.Delay(400).GetAwaiter().GetResult();
        FrameworkElement view = null;
        Window window = null;
        ScrollViewer scroller = null;
        string text = null;
        double originalOffset = 0;
        Step(() =>
        {
            Workspace(appId);
            window = Application.Current.Windows.Cast<Window>().Single(w => w.IsVisible &&
                w.DataContext?.GetType().FullName == "ShadowBot.Shell.Development.ViewModels.DevelopViewModel");
            var bottom = Get(window.DataContext, "BottomDocking");
            var model = Field(bottom, "_logViewModel");
            view = Field(model, "_logView") as FrameworkElement
                ?? throw new InvalidOperationException("Live native log control is not loaded");
            view.GetType().GetMethod("UpdataLogsFromQueue", Flags)?.Invoke(view, null);
            view.UpdateLayout();
            text = model.GetType().GetMethod("BuildLogs", Flags)?.Invoke(model, null)?.ToString();
            if (string.IsNullOrWhiteSpace(text)) throw new InvalidOperationException("No native run logs to capture");
            scroller = Descendants(view).OfType<ScrollViewer>().Where(item => item.IsVisible && item.ActualHeight > 80)
                .OrderByDescending(item => item.ActualHeight).FirstOrDefault();
            if (scroller != null)
            {
                originalOffset = scroller.VerticalOffset;
                scroller.ScrollToTop();
                scroller.ScrollToLeftEnd();
            }
            ToolTipService.SetIsEnabled(view, false);
            return true;
        });
        var images = new List<string>();
        var captures = new List<object>();
        double requestedOffset = 0;
        try
        {
            for (var page = 0; page < 50; page++)
            {
                var positioned = false;
                for (var attempt = 0; attempt < 5 && !positioned; attempt++)
                {
                    Step(() => { scroller?.ScrollToVerticalOffset(requestedOffset); view.UpdateLayout(); return true; });
                    Task.Delay(150).GetAwaiter().GetResult();
                    positioned = Step(() => scroller == null || Math.Abs(scroller.VerticalOffset - requestedOffset) < 1);
                }
                if (!positioned) throw new InvalidOperationException("Native log view moved during capture; no complete log capture claimed");
                var done = Step(() =>
                {
                    Workspace(appId);
                    view.UpdateLayout();
                    if (scroller != null && Math.Abs(scroller.VerticalOffset - requestedOffset) >= 1)
                        throw new InvalidOperationException("Native log viewport changed; no complete log capture claimed");
                    var dpi = System.Windows.Media.VisualTreeHelper.GetDpi(view);
                    var width = (int)Math.Ceiling(view.ActualWidth * dpi.DpiScaleX);
                    // Measure visible native text, including wrapped rows, before copying pixels.
                    // No content is redrawn; only the unused bottom of the panel is excluded.
                    var contentBottom = Descendants(view).OfType<TextBlock>()
                        .Where(item => item.IsVisible && item.ActualHeight > 0 && !string.IsNullOrWhiteSpace(item.Text))
                        .Select(item => item.TransformToAncestor(view).TransformBounds(new System.Windows.Rect(0, 0, item.ActualWidth, item.ActualHeight)))
                        .Where(bounds => bounds.Bottom > 0 && bounds.Top < view.ActualHeight)
                        .Select(bounds => Math.Min(view.ActualHeight, bounds.Bottom)).DefaultIfEmpty(view.ActualHeight).Max();
                    var height = (int)Math.Ceiling(Math.Min(view.ActualHeight, Math.Max(80, contentBottom + 12)) * dpi.DpiScaleY);
                    if (width < 100 || height < 80 || !view.IsVisible) throw new InvalidOperationException("Native log panel is not visible");
                    var point = view.PointToScreen(new System.Windows.Point(0, 0));
                    if (GetForegroundWindow() != new WindowInteropHelper(window).Handle)
                        throw new InvalidOperationException("Studio is not foreground; no log screenshot claimed");
                    var target = page == 0 ? output : Path.Combine(Path.GetDirectoryName(output), Path.GetFileNameWithoutExtension(output) + "-page" + (page + 1) + ".png");
                    if (File.Exists(target)) throw new IOException("Log capture page already exists: " + target);
                    using (var bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb))
                    {
                        using var graphics = Graphics.FromImage(bitmap);
                        graphics.CopyFromScreen((int)point.X, (int)point.Y, 0, 0, new System.Drawing.Size(width, height), CopyPixelOperation.SourceCopy);
                        bitmap.Save(target, ImageFormat.Png);
                    }
                    var result = new { ok = true, appId, path = target, width, height,
                        originalPanelHeight = (int)Math.Ceiling(view.ActualHeight * dpi.DpiScaleY), adaptiveHeight = true,
                        controlType = view.GetType().FullName, captureMethod = "existing-native-log-panel-without-cursor",
                        verticalOffset = scroller?.VerticalOffset, viewportHeight = scroller?.ViewportHeight,
                        scrollableHeight = scroller?.ScrollableHeight,
                        sha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(target))).ToLowerInvariant() };
                    File.WriteAllText(Path.ChangeExtension(target, ".capture.json"), System.Text.Json.JsonSerializer.Serialize(result));
                    captures.Add(result);
                    images.Add(target);
                    if (scroller == null || scroller.ScrollableHeight - scroller.VerticalOffset <= .01) return true;
                    var offset = scroller.VerticalOffset;
                    requestedOffset = Math.Min(scroller.ScrollableHeight, offset + Math.Max(1, Math.Floor(scroller.ViewportHeight * .8)));
                    scroller.ScrollToVerticalOffset(requestedOffset);
                    scroller.UpdateLayout();
                    if (scroller.VerticalOffset <= offset)
                        throw new InvalidOperationException("Log scrolling did not advance; no complete log capture claimed");
                    return false;
                });
                if (done)
                {
                    if (DateTime.UtcNow >= deadline) throw new TimeoutException("Log capture expired; no complete batch receipt written");
                    File.WriteAllText(Path.ChangeExtension(output, ".log.txt"), text);
                    var result = new { ok = true, appId, path = output, images = images.ToArray(), captures = captures.ToArray(), complete = true };
                    File.WriteAllText(Path.ChangeExtension(output, ".batch.json"), System.Text.Json.JsonSerializer.Serialize(result));
                    return result;
                }
            }
            throw new TimeoutException("Log page limit reached; no complete log capture claimed");
        }
        finally
        {
            if (DateTime.UtcNow < deadline)
                Step(() => { scroller?.ScrollToVerticalOffset(originalOffset); return true; });
        }
    }
}
