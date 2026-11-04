//! XbotShim — native shim injected into ShadowBot.Shell.exe.
//!
//! DllMain auto-runs bootstrap on first load. The bootstrap is also exposed as
//! exported function `xbot_bootstrap(arg: *const u8) -> u32` so the injector
//! can re-trigger it without restarting Shell (useful for iteration).
//!
//! Bootstrap uses `hostfxr` (already loaded in the target since it's a .NET 6
//! app) to load `XbotInjectee.dll` and invoke `XbotInjectee.Entry.Start`.

#![allow(unsafe_code)]

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HINSTANCE, HMODULE};
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

const HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER: i32 = 5;
type HostfxrHandle = *mut std::ffi::c_void;

static SHIM_HINST: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
static BOOTSTRAP_DONE: AtomicBool = AtomicBool::new(false);

#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        // Stash our hinst so xbot_bootstrap can find sibling DLLs.
        let _ = SHIM_HINST.set(hinst.0 as usize);
        // Auto-bootstrap on first load. DllMain must return quickly — hand off
        // to a worker thread.
        std::thread::spawn(|| {
            if let Err(e) = bootstrap() {
                log(&format!("auto-bootstrap error: {e}"));
            }
        });
    }
    BOOL(1)
}

/// Re-invokable bootstrap. Safe to call multiple times — second+ calls return
/// 2 (already done). Exported with C linkage so injector can call it via
/// CreateRemoteThread.
#[no_mangle]
pub extern "system" fn xbot_bootstrap(_arg: *const u8) -> u32 {
    match bootstrap() {
        Ok(()) => 0,
        Err(e) => {
            log(&format!("xbot_bootstrap error: {e}"));
            1
        }
    }
}

fn log(msg: &str) {
    if let Ok(temp) = std::env::var("TEMP") {
        let path = Path::new(&temp).join("xbot_shim.log");
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{}", msg);
        }
    }
}

fn bootstrap() -> Result<(), String> {
    if BOOTSTRAP_DONE.swap(true, Ordering::SeqCst) {
        log("bootstrap already done, skipping");
        return Ok(());
    }
    log("shim bootstrap start");

    let hinst = SHIM_HINST.get().copied().ok_or("shim hinst not captured")?;
    let self_path = current_module_path(hinst)?;
    log(&format!("shim path = {}", self_path.display()));
    let dir = self_path.parent().ok_or("no parent dir")?;
    let identity=std::fs::read_to_string(dir.join("XbotInjectee.identity")).unwrap_or_else(|_|"XbotInjectee".to_string());
    let injectee = dir.join(format!("{}.dll",identity.trim()));
    log(&format!("injectee identity={} path={}",identity.trim(),injectee.display()));
    let runtimeconfig = dir.join("XbotInjectee.runtimeconfig.json");
    if !injectee.is_file() {
        return Err(format!(
            "XbotInjectee.dll not found at {}",
            injectee.display()
        ));
    }
    if !runtimeconfig.is_file() {
        return Err(format!(
            "runtimeconfig not found at {}",
            runtimeconfig.display()
        ));
    }

    let hostfxr = unsafe { LoadLibraryW(PCWSTR(wide("hostfxr.dll").as_ptr())) }
        .map_err(|e| format!("LoadLibrary hostfxr.dll failed: {e}"))?;
    let init = unsafe {
        GetProcAddress(
            hostfxr,
            win_pcstr("hostfxr_initialize_for_runtime_config\0"),
        )
    }
    .ok_or("missing hostfxr_initialize_for_runtime_config")?;
    let get_delegate =
        unsafe { GetProcAddress(hostfxr, win_pcstr("hostfxr_get_runtime_delegate\0")) }
            .ok_or("missing hostfxr_get_runtime_delegate")?;
    let close = unsafe { GetProcAddress(hostfxr, win_pcstr("hostfxr_close\0")) }
        .ok_or("missing hostfxr_close")?;
    type InitFn =
        unsafe extern "system" fn(PCWSTR, *const std::ffi::c_void, *mut HostfxrHandle) -> i32;
    type GetDelegateFn =
        unsafe extern "system" fn(HostfxrHandle, i32, *mut *mut std::ffi::c_void) -> i32;
    type CloseFn = unsafe extern "system" fn(HostfxrHandle) -> i32;
    let init: InitFn = unsafe { std::mem::transmute(init) };
    let get_delegate: GetDelegateFn = unsafe { std::mem::transmute(get_delegate) };
    let close: CloseFn = unsafe { std::mem::transmute(close) };

    let rt_cfg_w = wide(&runtimeconfig.to_string_lossy());
    let mut ctx: HostfxrHandle = ptr::null_mut();
    let hr = unsafe { init(PCWSTR(rt_cfg_w.as_ptr()), ptr::null(), &mut ctx) };
    if hr != 0 && hr != 1 {
        return Err(format!(
            "hostfxr_initialize_for_runtime_config hr=0x{:x}",
            hr
        ));
    }
    log(&format!("hostfxr init hr={hr}"));

    let mut delegate: *mut std::ffi::c_void = ptr::null_mut();
    let hr = unsafe {
        get_delegate(
            ctx,
            HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER,
            &mut delegate,
        )
    };
    let _ = unsafe { close(ctx) };
    if hr != 0 || delegate.is_null() {
        return Err(format!("get_runtime_delegate hr=0x{:x}", hr));
    }
    type LoadAssemblyAndGetFunctionPointerFn = unsafe extern "system" fn(
        PCWSTR,
        PCWSTR,
        PCWSTR,
        PCWSTR,
        *const std::ffi::c_void,
        *mut *mut std::ffi::c_void,
    ) -> i32;
    let load_assembly: LoadAssemblyAndGetFunctionPointerFn =
        unsafe { std::mem::transmute(delegate) };

    let assembly_path_w = wide(&injectee.to_string_lossy());
    let type_name_w = wide(&format!("XbotInjectee.Entry, {}",identity.trim()));
    let method_name_w = wide("Start");
    let unmanaged_marker = -1isize as *const u16; // UNMANAGEDCALLERSONLY_METHOD

    let mut entry: *mut std::ffi::c_void = ptr::null_mut();
    let hr = unsafe {
        load_assembly(
            PCWSTR(assembly_path_w.as_ptr()),
            PCWSTR(type_name_w.as_ptr()),
            PCWSTR(method_name_w.as_ptr()),
            PCWSTR(unmanaged_marker),
            ptr::null(),
            &mut entry,
        )
    };
    if hr != 0 || entry.is_null() {
        return Err(format!(
            "load_assembly_and_get_function_pointer hr=0x{:x}",
            hr
        ));
    }
    log("invoking Entry.Start");

    type EntryFn = unsafe extern "system" fn(*const u8, i32) -> i32;
    let entry_fn: EntryFn = unsafe { std::mem::transmute(entry) };
    let rc = unsafe { entry_fn(ptr::null(), 0) };
    log(&format!("Entry.Start returned {rc}"));
    Ok(())
}

fn current_module_path(hinst_raw: usize) -> Result<PathBuf, String> {
    let mut buf = vec![0u16; 1024];
    let hmod = HMODULE(hinst_raw as _);
    let len = unsafe { GetModuleFileNameW(Some(hmod), &mut buf) };
    if len == 0 {
        return Err("GetModuleFileNameW returned 0".into());
    }
    buf.truncate(len as usize);
    Ok(PathBuf::from(OsString::from_wide(&buf)))
}

fn wide(s: &str) -> Vec<u16> {
    OsString::from(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn win_pcstr(s: &'static str) -> windows::core::PCSTR {
    windows::core::PCSTR(s.as_ptr())
}
