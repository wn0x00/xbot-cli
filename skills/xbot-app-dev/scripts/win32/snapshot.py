"""Experimental, read-only UIA snapshot worker. No input or selector writes."""
import argparse
import contextlib
import ctypes
from ctypes import wintypes
import datetime
import json
import sys
import time
import uuid


class SnapshotError(Exception):
    def __init__(self, code):
        self.code = code


def collect(root, max_nodes, max_depth, timeout, scope_check):
    started = time.monotonic()
    snapshot_id = str(uuid.uuid4())
    nodes = []
    issues = []
    reasons = set()
    seen = set()

    def issue(ref, field, kind):
        reasons.add("read_error")
        if len(issues) < 32:
            issues.append({"ref": ref, "field": field, "kind": kind})

    def read(control, name, ref):
        try:
            return getattr(control, name)
        except Exception as error:
            issue(ref, name, type(error).__name__)
            return None

    def text(value, ref, field):
        if value is None:
            return None
        value = str(value)
        if len(value) > 512:
            reasons.add("text_truncated")
            return value[:512]
        return value

    def relation(control, method, ref):
        try:
            return getattr(control, method)()
        except Exception as error:
            issue(ref, method, type(error).__name__)
            return None

    def walk(control, parent, depth):
        if time.monotonic() - started >= timeout:
            reasons.add("time_budget")
            return
        if len(nodes) >= max_nodes:
            reasons.add("node_budget")
            return
        ref = "w" + str(len(nodes) + 1)
        runtime_id = relation(control, "GetRuntimeId", ref)
        if runtime_id is not None:
            runtime_id = list(runtime_id)
            key = tuple(runtime_id)
            if key in seen:
                reasons.add("repeated_runtime_id")
                return
            seen.add(key)
        password = read(control, "IsPassword", ref)
        node = {
            "ref": ref, "parent_ref": parent, "depth": depth,
            "runtime_id": runtime_id, "is_password": password,
            "name": "[redacted]" if password is not False else text(read(control, "Name", ref), ref, "Name"),
        }
        for field, prop in (
            ("control_type", "ControlTypeName"), ("automation_id", "AutomationId"),
            ("class_name", "ClassName"), ("framework_id", "FrameworkId"),
            ("process_id", "ProcessId"), ("hwnd", "NativeWindowHandle"),
            ("enabled", "IsEnabled"), ("offscreen", "IsOffscreen"),
        ):
            value = read(control, prop, ref)
            node[field] = text(value, ref, prop) if isinstance(value, str) else value
        # Bounds are observations, never a fallback instruction to click.
        rect = read(control, "BoundingRectangle", ref)
        node["bounds"] = None if rect is None else [rect.left, rect.top, rect.right, rect.bottom]
        nodes.append(node)
        child = relation(control, "GetFirstChildControl", ref)
        if child is not None and depth >= max_depth:
            reasons.add("depth_budget")
            return
        while child is not None:
            if time.monotonic() - started >= timeout:
                reasons.add("time_budget")
                return
            if len(nodes) >= max_nodes:
                reasons.add("node_budget")
                return
            walk(child, ref, depth + 1)
            child = relation(child, "GetNextSiblingControl", ref)

    if not scope_check():
        raise SnapshotError("WIN32_SCOPE_MISMATCH")
    root_before = relation(root, "GetRuntimeId", None)
    walk(root, None, 0)
    root_after = relation(root, "GetRuntimeId", None)
    if time.monotonic() - started >= timeout:
        reasons.add("time_budget")
    if not scope_check() or root_before != root_after:
        raise SnapshotError("WIN32_SCOPE_CHANGED")
    return {
        "schema_version": 1, "backend": "uiautomation", "experimental": True,
        "snapshot_id": snapshot_id, "captured_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "duration_ms": round((time.monotonic() - started) * 1000),
        "ref_scope": "snapshot_only_evidence", "refs_resolvable_for_actions": False,
        "atomic": False, "business_completeness": "unknown",
        "observation_warnings": ["ROOT_ONLY"] if len(nodes) == 1 else [],
        "traversal_complete": not reasons, "partial_reasons": sorted(reasons),
        "node_count": len(nodes), "nodes": nodes, "issues": issues,
        "limits": {"max_nodes": max_nodes, "max_depth": max_depth, "timeout_seconds": timeout},
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--hwnd", type=lambda value: int(value, 0), required=True)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--max-nodes", type=int, default=1000)
    parser.add_argument("--max-depth", type=int, default=12)
    parser.add_argument("--timeout", type=int, default=10)
    args = parser.parse_args()
    if not (0 < args.hwnd <= sys.maxsize and 0 < args.pid < 2 ** 32
            and 1 <= args.max_nodes <= 5000 and 0 <= args.max_depth <= 64
            and 1 <= args.timeout <= 60):
        raise SnapshotError("BAD_INPUT")
    if sys.platform != "win32":
        raise SnapshotError("WIN32_UNSUPPORTED_PLATFORM")
    try:
        import uiautomation as auto
    except ImportError:
        raise SnapshotError("WIN32_DEPENDENCY_MISSING")
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    user32.IsWindow.argtypes = [ctypes.c_void_p]
    user32.IsWindow.restype = ctypes.c_int
    user32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
    user32.GetWindowThreadProcessId.restype = ctypes.c_ulong

    def matches_scope():
        pid = ctypes.c_ulong()
        if not user32.IsWindow(args.hwnd):
            return False
        user32.GetWindowThreadProcessId(args.hwnd, ctypes.byref(pid))
        return pid.value == args.pid

    if not matches_scope():
        raise SnapshotError("WIN32_SCOPE_MISMATCH")
    control = auto.ControlFromHandle(args.hwnd)
    if control is None or control.ProcessId != args.pid:
        raise SnapshotError("WIN32_SCOPE_MISMATCH")
    result = collect(control, args.max_nodes, args.max_depth, args.timeout, matches_scope)
    result["scope"] = {"hwnd": hex(args.hwnd), "process_id": args.pid}
    try:
        from importlib.metadata import version
        result["backend_version"] = version("uiautomation")
    except Exception:
        result["backend_version"] = "unknown"
    user32.IsWindowVisible.argtypes = [ctypes.c_void_p]
    user32.IsIconic.argtypes = [ctypes.c_void_p]
    user32.GetWindowRect.argtypes = [ctypes.c_void_p, ctypes.POINTER(wintypes.RECT)]
    rect = wintypes.RECT()
    got_rect = user32.GetWindowRect(args.hwnd, ctypes.byref(rect))
    result["window_observation"] = {
        "visible": bool(user32.IsWindowVisible(args.hwnd)),
        "minimized": bool(user32.IsIconic(args.hwnd)),
        "bounds": [rect.left, rect.top, rect.right, rect.bottom] if got_rect else None,
    }
    if result["window_observation"]["minimized"]:
        result["observation_warnings"].append("WINDOW_MINIMIZED")
    return result


if __name__ == "__main__":
    try:
        # Third-party diagnostics must not corrupt the stdout protocol.
        with contextlib.redirect_stdout(sys.stderr):
            data = main()
        print(json.dumps({"ok": True, "data": data}, ensure_ascii=True))
    except Exception as error:
        code = error.code if isinstance(error, SnapshotError) else "WIN32_PROVIDER_ERROR"
        print(json.dumps({"ok": False, "code": code, "kind": type(error).__name__}))
        sys.exit(1)
