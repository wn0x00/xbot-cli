"""Fault and concurrency tests with a native mock provider; no UI interaction."""
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
    with tempfile.TemporaryDirectory(prefix="xbot-win32-faults-") as temporary:
        directory = Path(temporary)
        provider = directory / "provider.exe"
        source = ROOT / "tests/fixtures/win32-fault-provider.cs"
        quote = lambda path: "'" + str(path).replace("'", "''") + "'"
        subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
                        "Add-Type -Path " + quote(source) + " -OutputAssembly " + quote(provider)
                        + " -OutputType ConsoleApplication -ReferencedAssemblies System.Web.Extensions"],
                       check=True, capture_output=True, timeout=30)
        state = directory / "state"
        state.mkdir()
        count = directory / "count.txt"
        env = {**os.environ, "XBOT_UIA_PYTHON": str(provider), "XBOT_WIN32_STATE_DIR": str(state),
               "XBOT_TEST_COUNT": str(count)}

        def snapshot():
            identifier = str(uuid.uuid4())
            data = {"snapshot_id": identifier, "backend": "uiautomation", "process_instance": "123",
                    "refs_resolvable_for_actions": True, "scope": {"hwnd": "0x1", "process_id": os.getpid()},
                    "limits": {"max_nodes": 1000, "max_depth": 12}}
            (state / (identifier + ".json")).write_text(json.dumps(data), encoding="utf-8")
            return identifier

        def args(identifier, timeout="2"):
            return [str(EXE), "win32", "click", "w4", "--snapshot", identifier,
                    "--timeout", timeout, "--json"]

        checks = []
        for mode, code in (("timeout", "TIMEOUT"), ("error", "WIN32_PROVIDER_ERROR")):
            identifier = snapshot()
            before = count.read_text().count("issued") if count.exists() else 0
            started = time.monotonic()
            result = subprocess.run(args(identifier), env={**env, "XBOT_TEST_MODE": mode}, capture_output=True, timeout=10)
            assert result.returncode != 0 and not result.stdout
            payload = json.loads(result.stderr)
            assert payload["error"]["code"] == code, payload
            assert payload["error"]["retryable"] is False
            assert payload["data"]["applied"] is None and payload["data"]["outcome"] == "unknown", payload
            assert count.read_text().count("issued") == before + 1
            if mode == "timeout":
                assert time.monotonic() - started < 4
            result = subprocess.run(args(identifier), env=env, capture_output=True, timeout=5)
            assert json.loads(result.stderr)["error"]["code"] == "WIN32_SNAPSHOT_USED"
            assert count.read_text().count("issued") == before + 1
            checks.append(mode + " after issue is unknown and never replayed")
        identifier = snapshot()
        before = count.read_text().count("issued")
        result = subprocess.run(args(identifier), env={**env, "XBOT_TEST_MODE": "success"}, capture_output=True, timeout=10)
        assert result.returncode == 0 and not result.stderr
        data = json.loads(result.stdout)["data"]
        assert data["applied"] is True and data["warning"]["code"] == "WIN32_PROVIDER_ERROR", data
        assert "snapshot" not in data
        assert count.read_text().count("issued") == before + 1
        checks.append("followup snapshot failure keeps success without replay")
        first, second = snapshot(), snapshot()
        running = subprocess.Popen(args(first, "3"), env={**env, "XBOT_TEST_MODE": "timeout"},
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 3
            while not (state / (first + ".used")).exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            result = subprocess.run(args(second), env=env, capture_output=True, timeout=5)
            assert json.loads(result.stderr)["error"]["code"] == "WIN32_ACTION_BUSY", result.stderr
            assert not (state / (second + ".used")).exists()
            checks.append("different snapshots cannot act concurrently on same process")
        finally:
            running.communicate(timeout=8)
        print(json.dumps({"ok": True, "checks": checks, "ui_accessed": False}, indent=2))


if __name__ == "__main__":
    main()
