//! 经典 Win32 注入:定位 ShadowBot.Shell.exe,在其地址空间写入 shim DLL 路径,
//! CreateRemoteThread(LoadLibraryW, path) 加载。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetProcessTimes, OpenProcess, WaitForSingleObject, INFINITE,
    LPTHREAD_START_ROUTINE, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

use crate::core::{BackendError, Res};

pub fn inject_into_shell(shim_path: &Path) -> Res<u32> {
    let shim_path = strip_unc(
        std::fs::canonicalize(shim_path).map_err(|e| {
            BackendError::new(
                "INPROC_INJECT",
                format!("canonicalize {}: {e}", shim_path.display()),
            )
        })?,
    );
    if !shim_path.is_file() {
        return Err(BackendError::new(
            "INPROC_INJECT",
            format!("shim DLL not found: {}", shim_path.display()),
        ));
    }
    let pid = find_shell_pid()
        .ok_or_else(|| BackendError::new("INPROC_NO_SHELL", "ShadowBot.Shell.exe not running"))?;
    let access = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_WRITE
        | PROCESS_VM_READ;
    let proc = unsafe { OpenProcess(access, false, pid) }.map_err(|e| {
        BackendError::new("INPROC_INJECT", format!("OpenProcess({pid}): {e}"))
    })?;
    let r = do_inject(proc, &shim_path);
    unsafe {
        let _ = CloseHandle(proc);
    }
    r.map(|_| pid)
}

fn do_inject(proc: HANDLE, shim_path: &Path) -> Res<()> {
    let path_wide: Vec<u16> = OsStr::new(shim_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let bytes_len = path_wide.len() * 2;

    let remote = unsafe {
        VirtualAllocEx(
            proc,
            None,
            bytes_len,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote.is_null() {
        return Err(BackendError::new(
            "INPROC_INJECT",
            format!("VirtualAllocEx failed: {:?}", unsafe { GetLastError() }),
        ));
    }
    let mut written: usize = 0;
    let ok = unsafe {
        WriteProcessMemory(
            proc,
            remote,
            path_wide.as_ptr() as *const _,
            bytes_len,
            Some(&mut written),
        )
    };
    if ok.is_err() || written != bytes_len {
        unsafe {
            let _ = VirtualFreeEx(proc, remote, 0, MEM_RELEASE);
        }
        return Err(BackendError::new(
            "INPROC_INJECT",
            format!("WriteProcessMemory wrote {written}/{bytes_len}"),
        ));
    }

    let kernel32 = unsafe { GetModuleHandleW(PCWSTR(wide("kernel32.dll").as_ptr())) }
        .map_err(|e| BackendError::new("INPROC_INJECT", format!("GetModuleHandle kernel32: {e}")))?;
    let load_library_w = unsafe { GetProcAddress(kernel32, PCSTR(b"LoadLibraryW\0".as_ptr())) }
        .ok_or_else(|| BackendError::new("INPROC_INJECT", "GetProcAddress LoadLibraryW"))?;
    let load_library_fn: LPTHREAD_START_ROUTINE = unsafe { std::mem::transmute(load_library_w) };

    let thread = unsafe { CreateRemoteThread(proc, None, 0, load_library_fn, Some(remote), 0, None) }
        .map_err(|e| {
            unsafe {
                let _ = VirtualFreeEx(proc, remote, 0, MEM_RELEASE);
            }
            BackendError::new("INPROC_INJECT", format!("CreateRemoteThread: {e}"))
        })?;
    unsafe {
        let _ = WaitForSingleObject(thread, INFINITE);
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(proc, remote, 0, MEM_RELEASE);
    }
    Ok(())
}

/// 最新的 ShadowBot.Shell.exe 进程(按创建时间)。
pub fn find_shell_pid() -> Option<u32> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut best: Option<(u32, u64)> = None;
        let mut valid = Process32FirstW(snapshot, &mut entry).is_ok();
        while valid {
            let length = entry
                .szExeFile
                .iter()
                .position(|v| *v == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
            if name.eq_ignore_ascii_case("ShadowBot.Shell.exe") {
                let created = creation_time(entry.th32ProcessID).unwrap_or(0);
                if best.map(|(_, start)| created > start).unwrap_or(true) {
                    best = Some((entry.th32ProcessID, created));
                }
            }
            valid = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        best.map(|(pid, _)| pid)
    }
}

fn creation_time(pid: u32) -> Option<u64> {
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut create = Default::default();
        let mut exit = Default::default();
        let mut kernel = Default::default();
        let mut user = Default::default();
        let ok = GetProcessTimes(
            proc,
            &mut create,
            &mut exit,
            &mut kernel,
            &mut user,
        )
        .is_ok();
        let _ = CloseHandle(proc);
        if !ok {
            return None;
        }
        Some(((create.dwHighDateTime as u64) << 32) | create.dwLowDateTime as u64)
    }
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn strip_unc(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p
    }
}
