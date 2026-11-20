"""Opt-in native adapter test using owned hidden PE32 / DuiLib ABI fixtures."""
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
CLI = Path(sys.argv[1]).resolve() if len(sys.argv)>1 else ROOT / 'target/debug/xbot-cli.exe'
user = c.WinDLL('user32', use_last_error=True)
user.SendMessageTimeoutW.argtypes = [w.HWND,w.UINT,w.WPARAM,w.LPARAM,w.UINT,w.UINT,c.POINTER(c.c_size_t)]
user.SendMessageTimeoutW.restype = w.LPARAM
kernel = c.WinDLL('kernel32',use_last_error=True)
kernel.CreateMutexW.argtypes=[c.c_void_p,w.BOOL,w.LPCWSTR]
kernel.CreateMutexW.restype=w.HANDLE
kernel.ReleaseMutex.argtypes=[w.HANDLE]
kernel.CloseHandle.argtypes=[w.HANDLE]

def message(hwnd, msg, wp, lp):
    result = c.c_size_t()
    assert user.SendMessageTimeoutW(hwnd,msg,wp,lp,2,2000,c.byref(result)), c.get_last_error()
    return result.value

def main():
    vswhere = Path(os.environ['ProgramFiles(x86)']) / 'Microsoft Visual Studio/Installer/vswhere.exe'
    vs = subprocess.check_output([str(vswhere),'-latest','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.x86.x64','-property','installationPath'],text=True).strip()
    setup = Path(vs) / 'Common7/Tools/VsDevCmd.bat'
    variables = subprocess.check_output(f'"{os.environ["COMSPEC"]}" /d /s /c ""{setup}" -no_logo -host_arch=x64 -arch=x86 >nul && set"',text=True,encoding='mbcs')
    env = dict(line.split('=',1) for line in variables.splitlines() if '=' in line and not line.startswith('='))
    compiler = shutil.which('cl.exe',path=next(v for k,v in env.items() if k.lower()=='path'))
    records = []
    with tempfile.TemporaryDirectory(prefix='xbot-wework-builtin-') as tmp:
        directory = Path(tmp)
        definition = directory / 'exports.def'
        definition.write_text('EXPORTS\n?OnGetObject@CWindowWnd@DuiLib@@MAEJIIJAA_N@Z=_FixtureOnGetObject@16\n')
        source = str(ROOT / 'tests/fixtures/win32-wework-builtin.cpp')
        def build(dll,gated=False,location=None):
            location=location or directory
            command = [compiler,'/nologo','/EHsc','/MT','/W4', '/Od',f'/Fo:{location / ("dll.obj" if dll else "exe.obj")}',f'/Fe:{location / ("DuiLib.dll" if dll else "WXWork.exe")}',source,'user32.lib']
            if dll: command += ['/DBUILD_DLL','/LD','/link',f'/DEF:{definition}']
            if gated: command.insert(1,'/DGATED')
            built = subprocess.run(command,env=env,cwd=directory,capture_output=True,creationflags=subprocess.CREATE_NO_WINDOW,timeout=30)
            assert built.returncode==0,built.stdout.decode('mbcs','replace')+built.stderr.decode('mbcs','replace')
        build(False)
        for index,(mode,gated) in enumerate((('normal',False),('unverified',False),('busy',False),('foreign-jump',False),('no-object',False),('normal',True))):
            location=directory/str(index)
            location.mkdir()
            shutil.copy2(directory/'WXWork.exe',location/'WXWork.exe')
            build(True,gated,location)
            receipt = location / (mode+'.json')
            process = subprocess.Popen([str(location/'WXWork.exe'),str(receipt),mode],cwd=location,creationflags=subprocess.CREATE_NO_WINDOW)
            mutex=None
            try:
                deadline=time.monotonic()+10
                while not receipt.exists():
                    assert process.poll() is None and time.monotonic()<deadline,f'fixture startup failed: exit={process.poll()}, files={[p.name for p in directory.iterdir()]}'
                    time.sleep(0.03)
                state=json.loads(receipt.read_text())
                if mode=='busy':
                    mutex=kernel.CreateMutexW(None,True,f"Local\\xbot-cli-tree-{process.pid}-{int(state['creation_time'])+504911232000000000}")
                    assert mutex,c.get_last_error()
                if mode=='normal': assert message(state['hwnd'],0x3d,1,-4)==0
                attempts=[]
                for attempt in range(2):
                    result=subprocess.run([str(CLI),'win32','activate','--pid',str(process.pid),'--hwnd',str(state['hwnd']),'--provider','wework','--json'],capture_output=True,timeout=15,creationflags=subprocess.CREATE_NO_WINDOW)
                    payload=json.loads(result.stdout if result.returncode==0 else result.stderr)
                    data=payload['data']
                    if mode=='normal':
                        assert payload['ok'] and data['status']==('activated' if attempt==0 else 'already_active'),payload
                        assert data['install_called'] is (attempt==0),payload
                        assert data['accessibility_verification']=='object_returned',payload
                        assert message(state['hwnd'],0x3d,1,-4)==0x44332211,'handled branch failed'
                        assert message(state['hwnd'],0x3d,2,-4)==0x13579,'unhandled branch lost ECX'
                        assert message(state['hwnd'],0x8001,2,0)==0x13579,'normal messages changed'
                        assert len(data['gate_rvas'])==(3 if gated else 0),payload
                    elif mode=='unverified':
                        assert not payload['ok'] and payload['error']['code']=='WIN32_ACTIVATION_UNVERIFIED',payload
                        assert data['status']=='installed_unverified' and data['install_called'] is (attempt==0),payload
                    elif mode=='busy':
                        assert not payload['ok'] and payload['error']['code']=='WIN32_ACTIVATION_BUSY',payload
                        assert not data['install_called'],payload
                    else:
                        assert not payload['ok'] and payload['error']['code']=='WIN32_ACTIVATION_PROVIDER_UNSUPPORTED',payload
                        assert data['status']=='not_started' and not data['install_called'],payload
                    attempts.append(payload)
                records.append({'mode':mode,'gated':gated,'attempts':attempts})
            finally:
                if mutex:
                    kernel.ReleaseMutex(mutex);kernel.CloseHandle(mutex)
                process.terminate();process.wait(timeout=5)
    print(json.dumps({'ok':True,'records':records},ensure_ascii=False,indent=2))

if __name__=='__main__': main()
