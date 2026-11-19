"""Snapshot-scoped UIA actions. No selector relaxation or automatic replay."""
import contextlib
import ctypes
from ctypes import wintypes
import json
import os
import sys
import time

from xbot_win32_snapshot import SnapshotError, open_scope


PATTERNS = ("Invoke", "Value", "SelectionItem", "Toggle", "ExpandCollapse", "Scroll")
PROPERTIES = (("control_type", "ControlTypeName"), ("automation_id", "AutomationId"),
              ("class_name", "ClassName"), ("framework_id", "FrameworkId"),
              ("process_id", "ProcessId"))


def fail(code):
    raise SnapshotError(code)


def runtime_id(control):
    value = control.GetRuntimeId()
    if not value:
        fail("WIN32_REF_UNRESOLVABLE")
    return list(value)


def identity(control, expected):
    if runtime_id(control) != expected.get("runtime_id"):
        fail("WIN32_REF_STALE")
    for key, attribute in PROPERTIES:
        if expected.get(key) is None or getattr(control, attribute) != expected[key]:
            fail("WIN32_REF_STALE")
    if control.IsPassword != expected.get("is_password"):
        fail("WIN32_REF_STALE")
    if expected.get("is_password") is False and control.Name != expected.get("name"):
        fail("WIN32_REF_STALE")


def resolve(root, snapshot, reference, deadline):
    nodes = snapshot.get("nodes", [])
    if not isinstance(nodes, list) or len(nodes) > 5000:
        fail("WIN32_SNAPSHOT_INVALID")
    refs = {}
    for node in nodes:
        key = node.get("ref")
        if not isinstance(key, str) or key in refs:
            fail("WIN32_SNAPSHOT_INVALID")
        refs[key] = node
    if reference not in refs:
        fail("WIN32_REF_MISSING")
    chain = []
    node = refs[reference]
    while True:
        if node in chain or len(chain) > 64:
            fail("WIN32_SNAPSHOT_INVALID")
        if not node.get("runtime_id"):
            fail("WIN32_REF_UNRESOLVABLE")
        chain.append(node)
        parent = node.get("parent_ref")
        if parent is None:
            break
        if parent not in refs:
            fail("WIN32_SNAPSHOT_INVALID")
        node = refs[parent]
    chain.reverse()
    identity(root, chain[0])
    control = root
    reads = 0
    for expected in chain[1:]:
        child = control.GetFirstChildControl()
        matches = []
        seen = set()
        while child is not None:
            reads += 1
            if reads > 5000 or time.monotonic() >= deadline:
                fail("WIN32_RESOLVE_BUDGET")
            key = tuple(runtime_id(child))
            if key in seen:
                fail("WIN32_REF_AMBIGUOUS")
            seen.add(key)
            if list(key) == expected["runtime_id"]:
                matches.append(child)
            child = child.GetNextSiblingControl()
        if len(matches) != 1:
            fail("WIN32_REF_STALE" if not matches else "WIN32_REF_AMBIGUOUS")
        control = matches[0]
        identity(control, expected)
    return control, chain[-1]


def supported(auto, control):
    return {name: control.GetPattern(getattr(auto.PatternId, name + "Pattern"))
            for name in PATTERNS}


def inspect_control(auto, control, expected):
    patterns = supported(auto, control)
    result = {"ref": expected["ref"], "runtime_id": runtime_id(control),
              "name": "[redacted]" if control.IsPassword is not False else control.Name,
              "is_password": control.IsPassword, "enabled": control.IsEnabled,
              "offscreen": control.IsOffscreen,
              "patterns": [name for name, value in patterns.items() if value is not None]}
    for key, attribute in PROPERTIES:
        result[key] = getattr(control, attribute)
    rect = control.BoundingRectangle
    result["bounds"] = [rect.left, rect.top, rect.right, rect.bottom]
    if patterns["Value"] is not None:
        result["read_only"] = patterns["Value"].IsReadOnly
        if control.IsPassword is False:
            value = patterns["Value"].Value
            result["value"] = value[:4096]
            result["value_truncated"] = len(value) > 4096
    for pattern, key, attribute in (
            ("Toggle", "toggle_state", "ToggleState"),
            ("SelectionItem", "selected", "IsSelected"),
            ("ExpandCollapse", "expand_state", "ExpandCollapseState"),
            ("Scroll", "horizontal_percent", "HorizontalScrollPercent"),
            ("Scroll", "vertical_percent", "VerticalScrollPercent")):
        if patterns[pattern] is not None:
            result[key] = getattr(patterns[pattern], attribute)
    return result


def issue(path, action, snapshot_id, reference):
    # Exclusive creation also prevents two CLI processes acting on the same tree.
    try:
        with open(path, "x", encoding="utf-8") as stream:
            json.dump({"state": "issued", "action": action,
                       "snapshot_id": snapshot_id, "ref": reference}, stream)
            stream.flush()
            os.fsync(stream.fileno())
    except FileExistsError:
        fail("WIN32_SNAPSHOT_USED")


def physical_guard(auto, user32, hwnd, control):
    user32.GetForegroundWindow.restype = wintypes.HWND
    if user32.GetForegroundWindow() != hwnd or control.IsOffscreen:
        fail("WIN32_PHYSICAL_TARGET_NOT_READY")
    rect = control.BoundingRectangle
    if rect.right <= rect.left or rect.bottom <= rect.top:
        fail("WIN32_PHYSICAL_TARGET_NOT_READY")
    x, y = (rect.left + rect.right) // 2, (rect.top + rect.bottom) // 2
    hit = auto.ControlFromPoint(x, y)
    for _ in range(65):
        if hit is None:
            break
        if auto.ControlsAreSame(hit, control):
            return x, y
        hit = hit.GetParentControl()
    fail("WIN32_PHYSICAL_HIT_MISMATCH")


def execute(request, auto):
    snapshot = request["snapshot"]
    action = request["action"]
    reference = request["ref"]
    options = request.get("options", {})
    hwnd = int(snapshot["scope"]["hwnd"], 0)
    pid = snapshot["scope"]["process_id"]
    user32, _, scope_check = open_scope(hwnd, pid, snapshot["process_instance"])
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.IsIconic.argtypes = [wintypes.HWND]
    user32.SetForegroundWindow.argtypes = [wintypes.HWND]
    root = auto.ControlFromHandle(hwnd)
    deadline = time.monotonic() + request["timeout"]
    control, expected = resolve(root, snapshot, reference, deadline)
    result = {"command": "win32." + action, "backend": "uiautomation",
              "source_snapshot_id": snapshot["snapshot_id"], "ref": reference,
              "scope": snapshot["scope"], "applied": False}
    if action == "inspect":
        result["control"] = inspect_control(auto, control, expected)
        if not scope_check():
            fail("WIN32_SCOPE_CHANGED")
        return result
    if not user32.IsWindowVisible(hwnd) or user32.IsIconic(hwnd):
        fail("WIN32_WINDOW_NOT_READY")
    if not control.IsEnabled:
        fail("WIN32_CONTROL_DISABLED")
    patterns = supported(auto, control)

    def require(name):
        value = patterns.get(name)
        if value is None:
            fail("WIN32_PATTERN_UNSUPPORTED")
        return value

    def validate():
        if not scope_check():
            fail("WIN32_SCOPE_CHANGED")
        if not user32.IsWindowVisible(hwnd) or user32.IsIconic(hwnd):
            fail("WIN32_WINDOW_NOT_READY")
        # Recheck ancestry immediately before issuing an operation.
        current, _ = resolve(root, snapshot, reference, deadline)
        if not current.IsEnabled:
            fail("WIN32_CONTROL_DISABLED")

    def begin():
        validate()
        issue(request["receipt_path"], action, snapshot["snapshot_id"], reference)

    operation = None
    if action == "click" and options["method"] == "invoke":
        operation = require("Invoke").Invoke
    elif action == "fill":
        pattern = require("Value")
        if pattern.IsReadOnly:
            fail("WIN32_CONTROL_READ_ONLY")
        if control.IsPassword is False and pattern.Value == options["text"]:
            result["outcome"] = "no_change"
            return result
        operation = lambda: pattern.SetValue(options["text"], waitTime=0)
    elif action in ("check", "uncheck"):
        pattern = require("Toggle")
        desired = 1 if action == "check" else 0
        if pattern.ToggleState == desired:
            result["outcome"] = "no_change"
            return result
        if pattern.ToggleState not in (0, 1):
            fail("WIN32_TOGGLE_INDETERMINATE")
        operation = pattern.Toggle
    elif action == "select":
        pattern = require("SelectionItem")
        if pattern.IsSelected:
            result["outcome"] = "no_change"
            return result
        operation = pattern.Select
    elif action in ("expand", "collapse"):
        pattern = require("ExpandCollapse")
        desired = 1 if action == "expand" else 0
        if pattern.ExpandCollapseState == desired:
            result["outcome"] = "no_change"
            return result
        if pattern.ExpandCollapseState == 3:
            fail("WIN32_PATTERN_UNSUPPORTED")
        operation = pattern.Expand if action == "expand" else pattern.Collapse
    elif action == "scroll":
        pattern = require("Scroll")
        direction = options["direction"]
        horizontal = direction in ("left", "right")
        if not (pattern.HorizontallyScrollable if horizontal else pattern.VerticallyScrollable):
            fail("WIN32_PATTERN_UNSUPPORTED")
        amount = (0 if options["large"] else 1) if direction in ("up", "left") else (3 if options["large"] else 4)
        operation = lambda: pattern.Scroll(amount if horizontal else 2, 2 if horizontal else amount, waitTime=0)
    elif action == "focus":
        operation = control.SetFocus
    elif action not in ("click", "dblclick", "hover", "type", "press"):
        fail("BAD_INPUT")

    if operation is not None:
        begin()
        if operation() is False:
            fail("WIN32_ACTION_OUTCOME_UNKNOWN")
    else:
        if control.IsOffscreen:
            fail("WIN32_PHYSICAL_TARGET_NOT_READY")
        begin()
        # Foreground activation is itself a mutation, after the durable marker.
        user32.SetForegroundWindow(hwnd)
        if action in ("type", "press"):
            if not control.SetFocus():
                fail("WIN32_FOCUS_FAILED")
            user32.GetForegroundWindow.restype = wintypes.HWND
            focused = auto.GetFocusedControl()
            if user32.GetForegroundWindow() != hwnd or focused is None or not auto.ControlsAreSame(focused, control):
                fail("WIN32_FOCUS_FAILED")
            validate()
            if action == "type":
                keys = "".join("{{}" if char == "{" else "{}}" if char == "}" else char
                               for char in options["text"])
            else:
                keys = options["key_sequence"]
            if keys:
                auto.SendKeys(keys, interval=0.002, waitTime=0)
        else:
            validate()
            x, y = physical_guard(auto, user32, hwnd, control)
            if action == "hover":
                auto.MoveTo(x, y)
            elif action == "dblclick":
                auto.Click(x, y, waitTime=auto.GetDoubleClickTime() / 2000.0)
                auto.Click(x, y, waitTime=0)
            else:
                auto.Click(x, y, waitTime=0)
    result.update(applied=True, outcome="action_returned", receipt_file=request["receipt_path"])
    return result


def main():
    request = json.load(sys.stdin)
    try:
        import uiautomation as auto
    except ImportError:
        fail("WIN32_DEPENDENCY_MISSING")
    return execute(request, auto)


if __name__ == "__main__":
    try:
        with contextlib.redirect_stdout(sys.stderr):
            data = main()
        print(json.dumps({"ok": True, "data": data}, ensure_ascii=True))
    except Exception as error:
        code = error.code if isinstance(error, SnapshotError) else "WIN32_PROVIDER_ERROR"
        # Provider exceptions may contain typed secrets; keep only their class.
        print(json.dumps({"ok": False, "code": code, "kind": type(error).__name__}))
        sys.exit(1)
