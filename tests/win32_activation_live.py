"""Opt-in activation test using locally installed ShadowBot and owned hidden windows.

Usage: python -X utf8 tests/win32_activation_live.py [path/to/xbot-cli.exe]
Never selects or interacts with a business app. Requires Windows x64, both DLLs,
and existing compatible Qt 5 runtimes via XBOT_TEST_QT_X86 and XBOT_TEST_QT_X64.
Also requires the Visual Studio C++ tools (discovered using vswhere).
"""
import base64
import ctypes
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
EXE = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "target/debug/xbot-cli.exe"
WORKER = (ROOT / "src/win32_activate.ps1").read_text(encoding="utf-8")


def worker(powershell, request):
    encoded = base64.b64encode(json.dumps(request).encode()).decode()
    script = f"$requestBase64 = '{encoded}'\n{WORKER}"
    result = subprocess.run([
        str(powershell), "-NoProfile", "-NonInteractive", "-Mta", "-EncodedCommand",
        base64.b64encode(script.encode("utf-16le")).decode()],
        capture_output=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
    for line in reversed(result.stdout.decode("utf-8-sig").splitlines()):
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if "ok" in value:
            return value
    raise AssertionError(result.stderr.decode("utf-8", "replace"))


def fixture_state(fixture, receipt):
    deadline = time.monotonic() + 15
    while True:
        try:
            return json.loads(receipt.read_text(encoding="utf-8-sig"))
        except (OSError, ValueError):
            if fixture.poll() is not None:
                raise AssertionError(f"Fixture exited {fixture.returncode}: " + fixture.stderr.read().decode("utf-8", "replace"))
            if time.monotonic() > deadline:
                fixture.terminate()
                _, diagnostics = fixture.communicate(timeout=5)
                raise AssertionError("Fixture did not become ready: " + diagnostics.decode("utf-8", "replace"))
            time.sleep(0.05)


def main():
    passed = []
    observations = []
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_wchar_p]
    kernel.CreateMutexW.restype = ctypes.c_void_p
    kernel.ReleaseMutex.argtypes = [ctypes.c_void_p]
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    vswhere = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = subprocess.check_output([
        str(vswhere), "-latest", "-products", "*", "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
        text=True).strip()
    assert installation, "Visual Studio C++ tools are required"
    with tempfile.TemporaryDirectory(prefix="xbot-activation-") as temporary:
        for architecture, directory, other in [("x86", "SysWOW64", "System32"),
                                                ("x64", "System32", "SysWOW64")]:
            powershell = Path(os.environ["SystemRoot"]) / directory / "WindowsPowerShell/v1.0/powershell.exe"
            other_shell = Path(os.environ["SystemRoot"]) / other / "WindowsPowerShell/v1.0/powershell.exe"
            receipt = Path(temporary) / f"{architecture}.json"
            fixture_directory = Path(temporary) / architecture
            fixture_directory.mkdir()
            # The vendor installer chooses an adapter by the real EXE basename.
            # Use its recognized name only in this private, owned test directory.
            fixture_exe = fixture_directory / "jdm_dd_workbench.exe"
            setup = Path(installation) / "Common7/Tools/VsDevCmd.bat"
            variables = subprocess.check_output(
                f'"{os.environ["COMSPEC"]}" /d /s /c ""{setup}" -no_logo -host_arch=x64 -arch={architecture} >nul && set"',
                text=True, encoding="mbcs")
            build_env = dict(line.split("=", 1) for line in variables.splitlines() if "=" in line and not line.startswith("="))
            build_path = next(value for name, value in build_env.items() if name.lower() == "path")
            compiler = shutil.which("cl.exe", path=build_path)
            assert compiler, "C++ compiler missing from Visual Studio environment"
            compiled = subprocess.run([
                compiler, "/nologo", "/std:c++17", "/EHsc", "/MT", "/W4",
                f"/Fe:{fixture_exe}", f"/Fo:{fixture_directory / 'fixture.obj'}",
                str(ROOT / "tests/fixtures/win32-activation.cpp"), "user32.lib"],
                env=build_env, capture_output=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
            assert compiled.returncode == 0, compiled.stdout.decode("utf-8", "replace")
            runtime = Path(os.environ[f"XBOT_TEST_QT_{architecture.upper()}"])
            assert any((directory / "platforms/qwindows.dll").is_file()
                       for directory in (runtime, runtime.parent)), runtime
            fixture = subprocess.Popen([
                str(fixture_exe), "xbot-activation-" + str(uuid.uuid4()), str(receipt), str(runtime)],
                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                state = fixture_state(fixture, receipt)
                assert state["pid"] == fixture.pid and state["architecture"] == architecture, state
                request = {"pid": fixture.pid, "hwnd": state["hwnd"],
                           "process_creation_time": state["creation_time"],
                           "target_architecture": architecture, "dll_path": "must-not-load.dll"}
                for shell, override, code in [
                    (powershell, {"process_creation_time": "1"}, "WIN32_TARGET_MISMATCH"),
                    (other_shell, {}, "WIN32_TARGET_ARCH_MISMATCH"),
                    (powershell, {"target_architecture": "invalid"}, "WIN32_TARGET_ARCH_MISMATCH"),
                    (powershell, {"hwnd": 1}, "WIN32_TARGET_MISMATCH"),
                    (powershell, {"hwnd": 0xf1234567}, "WIN32_TARGET_MISMATCH"),
                ]:
                    result = worker(shell, {**request, **override})
                    assert result["ok"] is False and result["code"] == code, result
                    assert result["data"]["status"] == "not_started", result
                    assert result["data"]["install_called"] is False, result
                passed.append(f"{architecture}: process/window/worker architecture guards")

                # Same mutex identity as v0.11.1, including across worker bitness.
                ticks = int(state["creation_time"]) + 504911232000000000
                mutex = kernel.CreateMutexW(None, True, f"Local\\xbot-cli-tree-{fixture.pid}-{ticks}")
                assert mutex, ctypes.get_last_error()
                try:
                    busy = worker(powershell, request)
                    assert busy["ok"] is False and busy["code"] == "WIN32_ACTIVATION_BUSY", busy
                    assert busy["data"]["install_called"] is False, busy
                finally:
                    kernel.ReleaseMutex(mutex)
                    kernel.CloseHandle(mutex)
                passed.append(f"{architecture}: concurrent activation rejected before install")

                def activate():
                    result = subprocess.run([
                        str(EXE), "win32", "activate", "--pid", str(fixture.pid),
                        "--hwnd", str(state["hwnd"]), "--timeout", "30", "--json"],
                        capture_output=True, timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
                    assert result.returncode == 0, result.stderr.decode("utf-8", "replace")
                    data = json.loads(result.stdout)["data"]
                    assert data["pid"] == fixture.pid, data
                    assert data["target_architecture"] == data["worker_architecture"] == architecture, data
                    assert f"Native/{architecture}/RpaBuilder.dll" in data["dll_path"].replace("\\", "/"), data
                    assert data["loaded_dll"] and data["restore"] is False, data
                    assert directory in data["installation"]["worker"], data
                    return data

                first = activate()
                assert first["status"] == "activated" and first["install_called"] is True, first
                second = activate()
                assert second["status"] == "already_loaded" and second["install_called"] is False, second
                assert first["loaded_dll"] == second["loaded_dll"], (first, second)
                assert fixture.poll() is None
                passed.append(f"{architecture}: first activation and idempotent second invocation")
                observations.append({"architecture": architecture, "first": first, "second": second})
            finally:
                if fixture.poll() is None:
                    fixture.terminate()
                fixture.communicate(timeout=10)
            unsupported_exe = fixture_directory / "xbot_cli_activation_unsupported_fixture.exe"
            shutil.copyfile(fixture_exe, unsupported_exe)
            receipt = Path(temporary) / f"{architecture}-unsupported.json"
            fixture = subprocess.Popen([
                str(unsupported_exe), "xbot-unsupported-" + str(uuid.uuid4()), str(receipt), str(runtime)],
                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                state = fixture_state(fixture, receipt)
                result = subprocess.run([
                    str(EXE), "win32", "activate", "--pid", str(fixture.pid),
                    "--hwnd", str(state["hwnd"]), "--timeout", "30", "--json"],
                    capture_output=True, timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
                assert result.returncode != 0, result.stdout
                failure = json.loads(result.stderr)
                assert failure["error"]["code"] == "WIN32_ACTIVATION_PROVIDER_UNSUPPORTED", failure
                assert failure["error"]["retryable"] is False, failure
                assert failure["data"]["hresult"] == "0x80004001", failure
                assert failure["data"]["target_architecture"] == failure["data"]["worker_architecture"] == architecture, failure
                assert fixture.poll() is None
                observations.append({"architecture": architecture, "unsupported": failure})
                passed.append(f"{architecture}: vendor E_NOTIMPL remains an explicit nonretryable failure")
            finally:
                if fixture.poll() is None:
                    fixture.terminate()
                fixture.communicate(timeout=10)
    print(json.dumps({"ok": True, "passed": passed, "observations": observations}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
