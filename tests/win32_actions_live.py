"""Opt-in end-to-end test: operates only its own WPF fixture, never business apps."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
EXE = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "target/debug/xbot-cli.exe"


def main():
    with tempfile.TemporaryDirectory(prefix="xbot-win32-actions-") as temporary:
        directory = Path(temporary)
        receipt = directory / "fixture.json"
        env = {**os.environ, "XBOT_UIA_PYTHON": sys.executable,
               "XBOT_WIN32_STATE_DIR": str(directory / "snapshots")}
        title = "xbot-cli-actions-" + str(uuid.uuid4())
        fixture = subprocess.Popen([
            "powershell.exe", "-NoProfile", "-STA", "-WindowStyle", "Hidden",
            "-ExecutionPolicy", "Bypass", "-File", str(ROOT / "tests/fixtures/win32-actions.ps1"),
            "-Title", title, "-Receipt", str(receipt)],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        passed = []

        def cli(*args, error=None):
            result = subprocess.run([str(EXE), "win32", *args, "--json"],
                                    capture_output=True, timeout=35, env=env)
            if error:
                data = json.loads(result.stderr)
                assert not result.returncode == 0, data
                assert data["error"]["code"] == error, data
                return data
            assert result.returncode == 0, result.stderr.decode("utf-8", "replace")
            return json.loads(result.stdout)["data"]

        def fixture_state(predicate):
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                try:
                    value = json.loads(receipt.read_text(encoding="utf-8-sig"))
                    if predicate(value):
                        return value
                except (OSError, ValueError):
                    pass
                time.sleep(0.05)
            raise AssertionError("Fixture readback did not match expected state")

        try:
            fixture_state(lambda value: True)
            window = cli("list", "--title", title, "--exact")["windows"]
            assert len(window) == 1
            window = window[0]
            assert window["pid"] == fixture.pid

            def capture():
                return cli("snapshot", "--pid", str(window["pid"]), "--hwnd", str(window["hwnd"]),
                           "--max-depth", "30", "--max-nodes", "1000", "--timeout", "20")

            def ref(snapshot, automation_id):
                matches = [n["ref"] for n in snapshot["nodes"] if n["automation_id"] == automation_id]
                assert len(matches) == 1, (automation_id, matches)
                return matches[0]

            def act(action, snapshot, automation_id, *args, error=None):
                return cli(action, ref(snapshot, automation_id), *args,
                           "--snapshot", snapshot["snapshot_id"], "--timeout", "20", error=error)

            snapshot = capture()
            assert snapshot["refs_resolvable_for_actions"] is True
            info = act("inspect", snapshot, "Input")
            assert "Value" in info["control"]["patterns"]
            passed.append("snapshot + inspect capabilities")
            act("click", snapshot, "Input", error="WIN32_PATTERN_UNSUPPORTED")
            act("fill", snapshot, "ReadOnly", "attempt", error="WIN32_CONTROL_READ_ONLY")
            act("click", snapshot, "Disabled", error="WIN32_CONTROL_DISABLED")
            passed.append("unsupported/read-only/disabled reject without consuming snapshot")
            literal = "hello 中文 {Enter} & $()"
            result = act("fill", snapshot, "Input", literal)
            fixture_state(lambda state: state["text"] == literal)
            old = snapshot
            snapshot = result["snapshot"]
            act("fill", old, "Input", "repeat", error="WIN32_SNAPSHOT_USED")
            passed.append("literal fill + source snapshot replay rejected")
            text_file = directory / "input.txt"
            text_file.write_text("file 中文 {Enter}\nsecond line", encoding="utf-8")
            result = act("fill", snapshot, "Input", "--file", str(text_file))
            fixture_state(lambda state: state["text"].replace("\r\n", "\n") == text_file.read_text(encoding="utf-8"))
            snapshot = result["snapshot"]
            passed.append("UTF-8 file input")
            result = act("fill", snapshot, "Input", literal)
            snapshot = result["snapshot"]
            result = act("fill", snapshot, "Input", literal)
            assert result["applied"] is False and result["outcome"] == "no_change"
            snapshot = result["snapshot"]
            result = act("click", snapshot, "Invoke")
            fixture_state(lambda state: state["count"] == 1)
            snapshot = result["snapshot"]
            passed.append("Invoke executes once")
            for action, desired in (("check", True), ("uncheck", False)):
                result = act(action, snapshot, "Check")
                fixture_state(lambda state: state["checked"] is desired)
                snapshot = result["snapshot"]
            passed.append("check + uncheck")
            result = act("select", snapshot, "Beta")
            fixture_state(lambda state: state["selected"] == 1)
            snapshot = result["snapshot"]
            passed.append("selection")
            for action, desired in (("expand", True), ("collapse", False)):
                result = act(action, snapshot, "Branch")
                fixture_state(lambda state: state["expanded"] is desired)
                snapshot = result["snapshot"]
            passed.append("expand + collapse")
            result = act("scroll", snapshot, "Scroller", "down", "--large")
            fixture_state(lambda state: state["scroll"] > 0)
            snapshot = result["snapshot"]
            passed.append("scroll")
            result = act("focus", snapshot, "Input")
            snapshot = result["snapshot"]
            result = act("press", snapshot, "Input", "Ctrl+A")
            snapshot = result["snapshot"]
            literal = "typed {Enter} 中文"
            result = act("type", snapshot, "Input", literal)
            fixture_state(lambda state: state["text"] == literal)
            snapshot = result["snapshot"]
            passed.append("focus + keyboard chord + literal type")
            result = act("hover", snapshot, "Invoke")
            snapshot = result["snapshot"]
            result = act("click", snapshot, "Invoke", "--method", "physical")
            fixture_state(lambda state: state["count"] == 2)
            snapshot = result["snapshot"]
            result = act("dblclick", snapshot, "Invoke")
            fixture_state(lambda state: state["count"] >= 3)
            snapshot = result["snapshot"]
            passed.append("physical hover/click/double click")
            old = snapshot
            result = act("click", snapshot, "Replace")
            fixture_state(lambda state: state["replaced"] == 1)
            # Another untouched snapshot proves RuntimeId matching rejects a
            # replacement with the exact same name, class and AutomationId.
            stale_id = str(uuid.uuid4())
            old["snapshot_id"] = stale_id
            (directory / "snapshots" / (stale_id + ".json")).write_text(json.dumps(old), encoding="utf-8")
            act("inspect", old, "Victim", error="WIN32_REF_STALE")
            passed.append("replaced same-name/same-id control is stale")
            stale_process = result["snapshot"]
            fake_id = str(uuid.uuid4())
            stale_process["snapshot_id"] = fake_id
            stale_process["process_instance"] = "1"
            (directory / "snapshots" / (fake_id + ".json")).write_text(json.dumps(stale_process), encoding="utf-8")
            act("inspect", stale_process, "Input", error="WIN32_SCOPE_CHANGED")
            passed.append("process instance mismatch rejected")
            print(json.dumps({"ok": True, "checks": passed, "business_apps_touched": False}, ensure_ascii=False, indent=2))
        finally:
            fixture.terminate()
            try:
                fixture.wait(timeout=5)
            except subprocess.TimeoutExpired:
                fixture.kill()
                fixture.wait(timeout=5)
            error = fixture.stderr.read().decode("utf-8", "replace")
            if error:
                print(error, file=sys.stderr)


if __name__ == "__main__":
    main()
