"""Opt-in: exercise the installed vendor misc.exe using owned hidden x86 windows.

Requires Windows x64, ShadowBot, and existing Visual Studio C++ tools. Does not
select a business window. Usage: python -X utf8 tests/win32_wxwork_activation_live.py [CLI]
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
EXE = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/debug/xbot-cli.exe"


def main():
    vswhere = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = subprocess.check_output([
        str(vswhere), "-latest", "-products", "*", "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], text=True).strip()
    assert installation, "Existing Visual Studio C++ tools required"
    setup = Path(installation) / "Common7/Tools/VsDevCmd.bat"
    variables = subprocess.check_output(
        f'"{os.environ["COMSPEC"]}" /d /s /c ""{setup}" -no_logo -host_arch=x64 -arch=x86 >nul && set"',
        text=True, encoding="mbcs")
    environment = dict(line.split("=", 1) for line in variables.splitlines() if "=" in line and not line.startswith("="))
    compiler = shutil.which("cl.exe", path=next(v for k, v in environment.items() if k.lower() == "path"))
    assert compiler, "C++ compiler missing"
    observations = []
    with tempfile.TemporaryDirectory(prefix="xbot-wxwork-") as temporary:
        directory = Path(temporary)
        # Use the recognized basename only in this owned temporary directory.
        fixture_exe = directory / "WXWork.exe"
        build = subprocess.run([
            compiler, "/nologo", "/EHsc", "/MT", "/W4", f"/Fe:{fixture_exe}",
            f"/Fo:{directory / 'fixture.obj'}", str(ROOT / "tests/fixtures/win32-wxwork.cpp"), "user32.lib"],
            env=environment, capture_output=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
        assert build.returncode == 0, build.stdout.decode("mbcs", "replace")
        for mode in ("normal", "unverified", "marker-only"):
            receipt = directory / (mode + ".json")
            process = subprocess.Popen([str(fixture_exe), str(receipt), mode], creationflags=subprocess.CREATE_NO_WINDOW)
            activation_receipt = None
            try:
                deadline = time.monotonic() + 10
                while not receipt.is_file():
                    assert time.monotonic() < deadline and process.poll() is None, "Fixture did not become ready"
                    time.sleep(0.05)
                state = json.loads(receipt.read_text())
                assert state["pid"] == process.pid
                activation_receipt = Path(os.environ["LOCALAPPDATA"]) / "xbot-cli/win32-activation" / f"wxwork-{process.pid}-{state['creation_time']}.json"
                results = []
                for attempt in range(2):
                    result = subprocess.run([
                        str(EXE), "win32", "activate", "--pid", str(state["pid"]),
                        "--hwnd", str(state["hwnd"]), "--provider", "shadowbot", "--json"], capture_output=True,
                        timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
                    payload = json.loads(result.stdout if result.returncode == 0 else result.stderr)
                    assert "data" in payload, payload
                    data = payload["data"]
                    assert data["backend"] == "shadowbot_misc_wxwork", payload
                    assert data["worker_architecture"] == data["target_architecture"] == "x86", payload
                    assert data["restore"] is False and data["tree_verification"] == "not_checked", payload
                    assert "Native/x86/misc.exe" in data["helper_path"].replace("\\", "/"), payload
                    if mode == "normal":
                        assert payload["ok"] is True and data["status"] == ("activated" if attempt == 0 else "already_active"), payload
                        assert data["helper_called"] is (attempt == 0), payload
                        assert data["hook_verification"] == "confirmed_by_message", payload
                    else:
                        assert payload["ok"] is False and payload["error"]["code"] == "WIN32_ACTIVATION_UNVERIFIED", payload
                        assert data["status"] == "outcome_unknown", payload
                        assert data["helper_called"] is (mode == "unverified" and attempt == 0), payload
                        if mode == "unverified" and attempt == 0:
                            assert data["return_code"] == 0, payload
                    results.append(data)
                observations.append({"mode": mode, "results": results})
            finally:
                process.terminate()
                process.wait(timeout=10)
                if activation_receipt is not None:
                    activation_receipt.unlink(missing_ok=True)
    print(json.dumps({"ok": True, "observations": observations}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
