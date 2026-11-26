"""Bounded UIA single-control repair and an optional WeChat test adapter.

UIA is a third-party backend, independent of ShadowBot selectors. Repair only
restores a previously recorded identity; it never relaxes that identity.
"""
import copy
import ctypes
from ctypes import wintypes
import datetime
import json
import os
import time
import uuid

import uiautomation as auto


class RepairError(RuntimeError):
    def __init__(self, code, detail=None):
        self.code = code
        self.detail = detail or {}
        super().__init__(code + ": " + json.dumps(self.detail, ensure_ascii=True))


PROPERTIES = {"control_type", "name", "automation_id", "class_name"}
USER32 = ctypes.WinDLL("user32", use_last_error=True)
KERNEL32 = ctypes.WinDLL("kernel32", use_last_error=True)
USER32.GetForegroundWindow.restype = wintypes.HWND
USER32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
USER32.IsIconic.argtypes = [wintypes.HWND]
USER32.IsWindowVisible.argtypes = [wintypes.HWND]
KERNEL32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
KERNEL32.OpenProcess.restype = wintypes.HANDLE
KERNEL32.CloseHandle.argtypes = [wintypes.HANDLE]
KERNEL32.QueryFullProcessImageNameW.argtypes = [
    wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)
]
KERNEL32.GetProcessTimes.argtypes = [
    wintypes.HANDLE, ctypes.POINTER(wintypes.FILETIME), ctypes.POINTER(wintypes.FILETIME),
    ctypes.POINTER(wintypes.FILETIME), ctypes.POINTER(wintypes.FILETIME)
]


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def process_identity(pid):
    handle = KERNEL32.OpenProcess(0x1000, False, pid)
    if not handle:
        raise RepairError("PROCESS_UNREADABLE", {"pid": pid})
    try:
        size = wintypes.DWORD(32768)
        path = ctypes.create_unicode_buffer(size.value)
        stamps = [wintypes.FILETIME() for _ in range(4)]
        if not KERNEL32.QueryFullProcessImageNameW(handle, 0, path, ctypes.byref(size)):
            raise RepairError("PROCESS_UNREADABLE", {"pid": pid})
        if not KERNEL32.GetProcessTimes(handle, *[ctypes.byref(s) for s in stamps]):
            raise RepairError("PROCESS_TIME_UNREADABLE", {"pid": pid})
        return (os.path.normcase(path.value),
                stamps[0].dwHighDateTime << 32 | stamps[0].dwLowDateTime)
    finally:
        KERNEL32.CloseHandle(handle)


def save_new(path, value):
    """Exclusive creation avoids replacing either an old asset or a ledger."""
    with open(path, "x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.flush()
        os.fsync(stream.fileno())


def matches(properties, query):
    if not isinstance(query, dict) or not query or set(query) - PROPERTIES:
        raise RepairError("QUERY_INVALID")
    return all(isinstance(value, str) and properties.get(key) == value
               for key, value in query.items())


def select_window(scope):
    windows = []
    for control in auto.GetRootControl().GetChildren():
        if control.ClassName != scope["window_class"]:
            continue
        hwnd = control.NativeWindowHandle
        title = ctypes.create_unicode_buffer(2048)
        USER32.GetWindowTextW(hwnd, title, len(title))
        if title.value != scope["window_title"]:
            continue
        identity = process_identity(control.ProcessId)
        if identity[0] == os.path.normcase(scope["exe_path"]):
            windows.append((control, identity))
    if len(windows) != 1:
        raise RepairError("WINDOW_NOT_UNIQUE", {"count": len(windows)})
    return windows[0]


def validate_asset(asset):
    if asset.get("backend") != "uiautomation" or asset.get("usage") != "single":
        raise RepairError("UNSUPPORTED_ASSET")
    identity = asset.get("identity", {})
    props = identity.get("properties", {})
    if (not props.get("control_type")
            or not (props.get("name") or props.get("automation_id"))
            or not identity.get("ancestors")):
        raise RepairError("IDENTITY_INSUFFICIENT")
    matches(props, props)
    for ancestor in identity["ancestors"]:
        matches(ancestor, ancestor)
    scope = asset.get("scope", {})
    if not all(scope.get(key) for key in ("exe_path", "window_class", "window_title")):
        raise RepairError("SCOPE_INSUFFICIENT")
    if not asset.get("variants") or len(asset["variants"]) > 16:
        raise RepairError("VARIANTS_INVALID")
    ids = [v["id"] for v in asset["variants"]]
    if len(ids) != len(set(ids)):
        raise RepairError("VARIANT_ID_DUPLICATE")
    for variant in asset["variants"]:
        query = variant["query"]
        matches(props, query)
        # A variant must retain the identity and its ordered ancestor contract.
        if variant.get("ancestors") != identity["ancestors"]:
            raise RepairError("VARIANT_ANCESTORS_MISMATCH")


class Observation:
    def __init__(self, scope, limits=None):
        limits = limits or {}
        self.max_nodes = limits.get("max_nodes", 2000)
        self.max_depth = limits.get("max_depth", 48)
        self.seconds = limits.get("seconds", 12)
        if not (1 <= self.max_nodes <= 5000 and 1 <= self.max_depth <= 64
                and 0.01 <= self.seconds <= 60):
            raise RepairError("BUDGET_INVALID")
        self.started = time.monotonic()
        self.root, self.process = select_window(scope)
        self.pid = self.root.ProcessId
        self.hwnd = self.root.NativeWindowHandle
        if USER32.IsIconic(self.hwnd) or not USER32.IsWindowVisible(self.hwnd):
            raise RepairError("WINDOW_NOT_READY", {"minimized": bool(USER32.IsIconic(self.hwnd))})
        self.root_runtime_id = tuple(self.root.GetRuntimeId())
        self.nodes = []
        self.walk(self.root, [])
        self.check_scope()

    def check_scope(self):
        if (self.root.ProcessId != self.pid
                or self.root.NativeWindowHandle != self.hwnd
                or tuple(self.root.GetRuntimeId()) != self.root_runtime_id
                or process_identity(self.pid) != self.process):
            raise RepairError("SCOPE_CHANGED")

    def walk(self, control, parents):
        if (len(self.nodes) >= self.max_nodes
                or time.monotonic() - self.started > self.seconds):
            raise RepairError("OBSERVATION_INCOMPLETE", {"nodes": len(self.nodes)})
        password = control.IsPassword
        if password:
            return
        props = {"control_type": control.ControlTypeName, "name": control.Name,
                 "automation_id": control.AutomationId, "class_name": control.ClassName}
        if control.ProcessId != self.pid:
            raise RepairError("CHILD_PROCESS_MISMATCH")
        node = {"properties": props, "control": control, "parents": parents,
                "runtime_id": tuple(control.GetRuntimeId()),
                "enabled": control.IsEnabled, "offscreen": control.IsOffscreen}
        self.nodes.append(node)
        children = control.GetChildren()
        if children and len(parents) >= self.max_depth:
            raise RepairError("OBSERVATION_INCOMPLETE", {"reason": "depth"})
        for child in children:
            self.walk(child, parents + [props])

    def find(self, query, ancestors):
        result = []
        for node in self.nodes:
            if not matches(node["properties"], query):
                continue
            position = 0
            for parent in node["parents"]:
                if position < len(ancestors) and matches(parent, ancestors[position]):
                    position += 1
            if position == len(ancestors):
                result.append(node)
        return result


def resolve(asset, allow_repair=False, limits=None):
    validate_asset(asset)
    observation = Observation(asset["scope"], limits)
    identity = asset["identity"]
    candidates = observation.find(identity["properties"], identity["ancestors"])
    compatible = []
    counts = {}
    for variant in asset["variants"]:
        found = observation.find(variant["query"], variant["ancestors"])
        counts[variant["id"]] = len(found)
        if len(found) > 1:
            raise RepairError("LOCATOR_AMBIGUOUS", {"variant": variant["id"], "count": len(found)})
        if found and not matches(found[0]["properties"], identity["properties"]):
            raise RepairError("IDENTITY_MISMATCH", {"variant": variant["id"]})
        if found:
            compatible.append(found[0])
    distinct = []
    for node in compatible:
        if not any(auto.ControlsAreSame(node["control"], other["control"]) for other in distinct):
            distinct.append(node)
    if len(candidates) > 1 or len(distinct) > 1:
        raise RepairError("TARGET_AMBIGUOUS", {"identity_count": len(candidates)})
    updated = copy.deepcopy(asset)
    repaired = False
    if not distinct:
        if not allow_repair:
            raise RepairError("LOCATOR_NOT_FOUND", {"variant_counts": counts})
        if len(candidates) != 1:
            raise RepairError("REPAIR_UNPROVEN", {"identity_count": len(candidates)})
        node = candidates[0]
        query = dict(identity["properties"])
        # Never replace old queries; create an observed, identity-preserving one.
        if node["properties"]["automation_id"]:
            query["automation_id"] = node["properties"]["automation_id"]
        updated["variants"].append({
            "id": "repair-" + uuid.uuid4().hex[:12], "query": query,
            "ancestors": copy.deepcopy(identity["ancestors"]), "captured_at": now()
        })
        if len(observation.find(query, identity["ancestors"])) != 1:
            raise RepairError("REPAIR_QUERY_AMBIGUOUS")
        distinct = [node]
        repaired = True
    node = distinct[0]
    return observation, node, updated, {
        "asset_id": asset["asset_id"], "node_count": len(observation.nodes),
        "variant_counts": counts, "repaired": repaired,
        "variant_count": len(updated["variants"]), "properties": node["properties"],
        "enabled": node["enabled"], "offscreen": node["offscreen"],
        "backend": "uiautomation", "observed_at": now()
    }


def read_value(control):
    pattern = control.GetPattern(auto.PatternId.ValuePattern)
    if pattern is None:
        raise RepairError("VALUE_PATTERN_UNAVAILABLE")
    return pattern.Value


def enabled(node):
    if not node["enabled"] or node["offscreen"]:
        raise RepairError("TARGET_NOT_ACTIONABLE")


def foreground(observation):
    observation.check_scope()
    if int(USER32.GetForegroundWindow() or 0) != observation.hwnd:
        raise RepairError("FOREGROUND_MISMATCH")


def physical_click(observation, node):
    """A separately selected method; never an automatic Invoke fallback."""
    enabled(node)
    foreground(observation)
    control = node["control"]
    rect = control.BoundingRectangle
    x, y = int((rect.left + rect.right) / 2), int((rect.top + rect.bottom) / 2)
    if rect.right <= rect.left or rect.bottom <= rect.top:
        raise RepairError("BOUNDS_INVALID")
    hit = auto.ControlFromPoint(x, y)
    for _ in range(8):
        if hit is None:
            break
        if auto.ControlsAreSame(hit, control):
            control.Click(simulateMove=False, waitTime=0.3)
            return
        hit = hit.GetParentControl()
    raise RepairError("TARGET_OCCLUDED_OR_COORDINATE_MISMATCH")


def execute(request):
    mode = request["mode"]
    if mode not in ("query", "prepare_search", "open_group", "inspect_chat",
                    "send", "reconcile", "clear_own_test_draft"):
        raise RepairError("MODE_INVALID")
    with auto.UIAutomationInitializerInThread():
        return _execute(request, mode)


def _execute(request, mode):
    asset = request["asset"]
    if mode == "send" and os.path.exists(request["ledger_path"]):
        raise RepairError("ACTION_ALREADY_RECORDED")
    if request.get("activate_window"):
        validate_asset(asset)
        window, identity = select_window(asset["scope"])
        if not window.SetActive(waitTime=0.3):
            raise RepairError("WINDOW_ACTIVATION_FAILED")
        if process_identity(window.ProcessId) != identity:
            raise RepairError("SCOPE_CHANGED")
    if mode in ("inspect_chat", "send", "reconcile", "clear_own_test_draft"):
        related = [request["chat_asset"]]
        if mode == "send":
            related.append(request["input_asset"])
        if any(item["scope"] != asset["scope"] for item in related):
            raise RepairError("GUARD_SCOPE_MISMATCH")
        if request["chat_asset"]["identity"]["properties"].get("name") != request["group"]:
            raise RepairError("GUARD_GROUP_MISMATCH")
    observation, node, updated, result = resolve(
        asset, request.get("allow_repair", False), request.get("limits"))
    result["window_activation_requested"] = bool(request.get("activate_window"))
    if request.get("require_enabled"):
        enabled(node)
    if result["repaired"]:
        save_new(request["repaired_asset_path"], updated)
    if mode == "query":
        result["patterns"] = {
            name: node["control"].GetPattern(pattern_id) is not None
            for name, pattern_id in (("value", auto.PatternId.ValuePattern),
                                     ("invoke", auto.PatternId.InvokePattern),
                                     ("text", auto.PatternId.TextPattern))
        }
        return result
    if mode == "prepare_search":
        enabled(node)
        observation.root.SetActive(waitTime=0.3)
        foreground(observation)
        if request.get("search_physical_click"):
            physical_click(observation, node)
            if not auto.ControlsAreSame(auto.GetFocusedControl(), node["control"]):
                raise RepairError("SEARCH_FOCUS_UNCONFIRMED")
        pattern = node["control"].GetPattern(auto.PatternId.ValuePattern)
        if pattern is None or pattern.IsReadOnly:
            raise RepairError("VALUE_PATTERN_UNAVAILABLE")
        if read_value(node["control"]) not in ("", request["group"]):
            raise RepairError("EXISTING_SEARCH_TEXT")
        if not pattern.SetValue(request["group"], waitTime=0.5):
            raise RepairError("SEARCH_WRITE_UNKNOWN")
        if read_value(node["control"]) != request["group"]:
            raise RepairError("SEARCH_WRITE_UNCONFIRMED")
        result["search_value_confirmed"] = True
        # Observe again; never infer exact result identity from the search text.
        state = Observation(asset["scope"])
        result["search_candidates"] = [
            {"properties": n["properties"], "ancestors": n["parents"],
             "invoke": n["control"].GetPattern(auto.PatternId.InvokePattern) is not None,
             "selection_item": n["control"].GetPattern(auto.PatternId.SelectionItemPattern) is not None}
            for n in state.nodes
            if n["properties"]["name"] == request["group"]
            and n["properties"]["control_type"] == "ListItemControl"
        ]
        if request.get("open_result_asset"):
            _, result_node, _, result_info = resolve(request["open_result_asset"])
            enabled(result_node)
            foreground(state)
            if request.get("open_result_method", "invoke") == "physical_click":
                physical_click(state, result_node)
            else:
                invoke = result_node["control"].GetPattern(auto.PatternId.InvokePattern)
                if invoke is None:
                    raise RepairError("INVOKE_PATTERN_UNAVAILABLE")
                if not invoke.Invoke(waitTime=0.5):
                    raise RepairError("GROUP_OPEN_UNKNOWN")
            result["open_result"] = result_info
            after, _, _, verification = resolve(request["chat_asset"])
            result["conversation_verified"] = True
            result["chat_verification"] = verification
            result["search_overlay_present"] = bool(after.find(
                {"class_name": "mmui::SearchContentPopover"}, []))
        return result
    if mode == "open_group":
        enabled(node)
        observation.root.SetActive(waitTime=0.3)
        foreground(observation)
        pattern = node["control"].GetPattern(auto.PatternId.InvokePattern)
        if pattern is None:
            raise RepairError("INVOKE_PATTERN_UNAVAILABLE")
        if not pattern.Invoke(waitTime=0.5):
            raise RepairError("GROUP_OPEN_UNKNOWN")
        return {"open_invoked": True, "target": result}
    # Chat identity is verified independently of the input or send control.
    _, _, _, chat = resolve(request["chat_asset"])
    if chat["properties"]["name"] != request["group"]:
        raise RepairError("CONVERSATION_MISMATCH")
    if mode == "inspect_chat":
        result["conversation_verified"] = True
        result["input_empty"] = read_value(node["control"]) == ""
        return result
    if mode in ("reconcile", "clear_own_test_draft"):
        with open(request["ledger_path"], "r", encoding="utf-8") as stream:
            intent = json.load(stream)
        if intent["group"] != request["group"]:
            raise RepairError("LEDGER_GROUP_MISMATCH")
        count = _message_count(asset["scope"], request["history_query"], intent["message"])
        draft = read_value(node["control"])
        empty = draft == ""
        if mode == "clear_own_test_draft":
            # Test cleanup never retries the old send or marks it successful.
            if count != 0 or draft != intent["message"]:
                raise RepairError("TEST_DRAFT_CLEANUP_UNPROVEN")
            observation.root.SetActive(waitTime=0.3)
            foreground(observation)
            save_new(request["ledger_path"] + ".test-draft-cleanup.json", {
                "operation_id": intent["operation_id"], "state": "cleanup_intent",
                "original_send_state": "unknown", "created_at": now()})
            if not node["control"].GetPattern(auto.PatternId.ValuePattern).SetValue("", waitTime=0.3):
                raise RepairError("TEST_DRAFT_CLEANUP_UNKNOWN")
            return {"own_test_draft_cleared": read_value(node["control"]) == "",
                    "original_send_state": "unknown", "send_calls": 0}
        return {"operation_id": intent["operation_id"], "message": intent["message"],
                "state": "client_confirmed" if count == 1 and empty else "unknown",
                "exact_history_matches": count, "input_empty": empty,
                "draft_matches_intent": draft == intent["message"],
                "read_only": True, "retry_allowed": False, "observed_at": now()}
    if not request.get("send_authorized"):
        raise RepairError("SEND_NOT_AUTHORIZED")
    if observation.find({"class_name": "mmui::SearchContentPopover"}, []):
        raise RepairError("SEARCH_OVERLAY_OPEN")
    send_method = request.get("send_method", "invoke")
    if send_method not in ("invoke", "physical_click"):
        raise RepairError("SEND_METHOD_INVALID")
    message = request["message"]
    if not message.startswith(request["message_prefix"] + " ") or "\n" in message:
        raise RepairError("MESSAGE_OUT_OF_SCOPE")
    ledger_path = request["ledger_path"]
    # A prior intent, including an uncertain one, blocks another send attempt.
    if os.path.exists(ledger_path):
        raise RepairError("ACTION_ALREADY_RECORDED")
    input_asset = request["input_asset"]
    _, input_node, _, _ = resolve(input_asset)
    enabled(input_node)
    if read_value(input_node["control"]) != "":
        raise RepairError("EXISTING_DRAFT")
    if _message_count(asset["scope"], request["history_query"], message):
        raise RepairError("MESSAGE_ALREADY_VISIBLE")
    observation.root.SetActive(waitTime=0.3)
    foreground(observation)
    value = input_node["control"].GetPattern(auto.PatternId.ValuePattern)
    if value is None or value.IsReadOnly:
        raise RepairError("VALUE_PATTERN_UNAVAILABLE")
    intent = {"operation_id": request["operation_id"], "message": message,
              "group": request["group"], "state": "prepared",
              "created_at": now(), "send_calls": 0}
    save_new(ledger_path, intent)
    if not value.SetValue(message, waitTime=0.5):
        raise RepairError("DRAFT_WRITE_UNKNOWN")
    _, draft_node, _, _ = resolve(input_asset)
    if read_value(draft_node["control"]) != message:
        raise RepairError("DRAFT_WRITE_UNCONFIRMED")
    # Re-resolve all business guards after drafting; only then issue one invoke.
    _, _, _, _ = resolve(request["chat_asset"])
    current, send_node, _, send_result = resolve(updated)
    enabled(send_node)
    if current.find({"class_name": "mmui::SearchContentPopover"}, []):
        raise RepairError("SEARCH_OVERLAY_OPEN")
    chat_identity = request["chat_asset"]["identity"]
    titles = current.find(chat_identity["properties"], chat_identity["ancestors"])
    input_identity = input_asset["identity"]
    inputs = current.find(input_identity["properties"], input_identity["ancestors"])
    if (len(titles) != 1 or titles[0]["control"].Name != request["group"]
            or len(inputs) != 1 or inputs[0]["control"].Name != request["group"]
            or read_value(inputs[0]["control"]) != message):
        raise RepairError("FINAL_CHAT_GUARD_FAILED")
    foreground(current)
    pattern = send_node["control"].GetPattern(auto.PatternId.InvokePattern)
    if send_method == "invoke" and pattern is None:
        raise RepairError("INVOKE_PATTERN_UNAVAILABLE")
    save_new(ledger_path + ".issued.json", {
        "operation_id": request["operation_id"], "state": "action_about_to_run",
        "send_method": send_method,
        "created_at": now(), "send_call_upper_bound": 1
    })
    # An exception or lost acknowledgement here must never trigger a retry.
    if send_method == "physical_click":
        physical_click(current, send_node)
        invoked = None
    else:
        invoked = pattern.Invoke(waitTime=0.3)
    deadline = time.monotonic() + 12
    while True:
        try:
            _, after_input, _, _ = resolve(input_asset)
            empty = read_value(after_input["control"]) == ""
            _, _, _, _ = resolve(request["chat_asset"])
            count = _message_count(asset["scope"], request["history_query"], message)
        except RepairError as error:
            raise RepairError("SEND_OUTCOME_UNKNOWN", {
                "observation_error": error.code, "retry_allowed": False,
                "send_method": send_method
            }) from error
        if count == 1 and empty:
            receipt = {"operation_id": request["operation_id"],
                       "state": "client_confirmed", "message": message,
                       "group": request["group"], "send_calls": 1,
                       "send_method": send_method,
                       "invoke_returned": invoked, "exact_history_matches": count,
                       "input_empty": empty, "confirmed_at": now(),
                       "server_delivery": "not_independently_verified",
                       "repair": result, "selected": send_result}
            save_new(ledger_path + ".receipt.json", receipt)
            return receipt
        if count > 1 or time.monotonic() >= deadline:
            raise RepairError("SEND_OUTCOME_UNKNOWN", {
                "exact_history_matches": count, "input_empty": empty,
                "invoke_returned": invoked, "send_method": send_method,
                "retry_allowed": False
            })
        time.sleep(0.4)


def _message_count(scope, history_query, message):
    state = Observation(scope)
    lists = state.find(history_query, [])
    if len(lists) != 1:
        raise RepairError("HISTORY_NOT_UNIQUE")
    history = lists[0]["control"]
    # The receipt is a visible-client observation, not a server acknowledgement.
    return sum(1 for control in history.GetChildren()
               if control.ControlTypeName == "ListItemControl" and control.Name == message)
