//! Bounded WeWork WM_GETOBJECT adapter, derived from the user-supplied script.
//! Only validated x86 DuiLib/vtable layouts are admitted. No watch/uninstall loop.
use crate::app_cmds::CmdOut;
use crate::core::{BackendError, Res};

#[cfg(windows)]
pub(crate) use native::execute;

#[cfg(not(windows))]
pub(crate) fn execute(_: u64, _: u32, _: &str, _: u64) -> Res<CmdOut> {
    Err(BackendError::new(
        "WIN32_UNSUPPORTED_PLATFORM",
        "wework 适配仅支持 Windows x64",
    ))
}

#[cfg(any(windows, test))]
fn stub(entry: u32, address: u32, slot: u8) -> Vec<u8> {
    let mut code = Vec::from(&b"\x83\x7c\x24\x04\x3d\x75\x00\x83\x7c\x24\x0c\xfc\x75\x00"[..]);
    // Keep handled and the original this pointer in two local words. OnGetObject
    // is thiscall: ECX may be destroyed even when it declines the message.
    code.extend_from_slice(b"\x83\xec\x08\x89\x4c\x24\x04\xc6\x04\x24\x00\x8d\x04\x24\x50\x8b\x44\x24\x18\x50\x8b\x44\x24\x18\x50\x6a\x00\x8b\x01\xff\x50");
    code.push(slot * 4);
    code.extend_from_slice(b"\x80\x3c\x24\x00\x75\x00");
    let handled_jump = code.len() - 1;
    code.extend_from_slice(b"\x8b\x4c\x24\x04\x83\xc4\x08");
    let fallback = code.len();
    // The only admitted entry is exactly 66 90 55 8b ec: five full instructions'
    // bytes. Never skip two unrelocated bytes after a three-byte prologue.
    code.extend_from_slice(b"\x66\x90\x55\x8b\xec\xe9");
    let next = address.wrapping_add(code.len() as u32 + 4);
    code.extend_from_slice(&entry.wrapping_add(5).wrapping_sub(next).to_le_bytes());
    let handled = code.len();
    code.extend_from_slice(b"\x83\xc4\x08\xc2\x0c\x00");
    code[6] = (fallback - 7) as u8;
    code[13] = (fallback - 14) as u8;
    code[handled_jump] = (handled - handled_jump - 1) as u8;
    code
}

#[cfg(windows)]
mod native {
    use super::*;
    use serde_json::{json, Value};
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::mem::size_of;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Diagnostics::Debug::*;
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    use windows::Win32::System::Memory::*;
    use windows::Win32::System::Threading::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const MAGIC: &[u8; 8] = b"XBOTWW01";
    const HEADER: u32 = 32;
    const ORIGINAL: &[u8] = b"\x66\x90\x55\x8b\xec";

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSuspendProcess(process: HANDLE) -> i32;
        fn NtResumeProcess(process: HANDLE) -> i32;
    }

    fn unsupported(message: impl Into<String>) -> BackendError {
        BackendError::new("WIN32_ACTIVATION_PROVIDER_UNSUPPORTED", message)
    }
    fn api(error: windows::core::Error) -> BackendError {
        BackendError::new("WIN32_ACTIVATION_ACCESS_FAILED", error.to_string())
    }
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    struct Lock(Handle);
    impl Drop for Lock {
        fn drop(&mut self) {
            unsafe {
                let _ = ReleaseMutex(self.0 .0);
            }
        }
    }
    struct Suspended {
        process: HANDLE,
        active: bool,
    }
    impl Suspended {
        fn new(process: HANDLE) -> Res<Self> {
            let status = unsafe { NtSuspendProcess(process) };
            if status < 0 {
                return Err(BackendError::new(
                    "WIN32_ACTIVATION_SUSPEND_FAILED",
                    format!("NtSuspendProcess: 0x{:08x}", status as u32),
                ));
            }
            Ok(Self {
                process,
                active: true,
            })
        }
        fn resume(&mut self) -> Res<()> {
            let status = unsafe { NtResumeProcess(self.process) };
            if status < 0 {
                return Err(BackendError::new(
                    "WIN32_ACTIVATION_RESUME_FAILED",
                    format!("NtResumeProcess: 0x{:08x}", status as u32),
                ));
            }
            self.active = false;
            Ok(())
        }
    }
    impl Drop for Suspended {
        fn drop(&mut self) {
            if self.active {
                unsafe {
                    NtResumeProcess(self.process);
                }
            }
        }
    }
    struct Remote(Handle);
    impl Remote {
        fn read(&self, address: u32, size: usize) -> Res<Vec<u8>> {
            let mut data = vec![0; size];
            let mut count = 0;
            unsafe {
                ReadProcessMemory(
                    self.0 .0,
                    address as usize as _,
                    data.as_mut_ptr().cast(),
                    size,
                    Some(&mut count),
                )
            }
            .map_err(api)?;
            if count != size {
                return Err(unsupported("读取目标内存不完整"));
            }
            Ok(data)
        }
        fn word(&self, address: u32) -> Res<u32> {
            Ok(u32::from_le_bytes(
                self.read(address, 4)?.try_into().unwrap(),
            ))
        }
        fn write(&self, address: u32, data: &[u8]) -> Res<()> {
            let mut count = 0;
            unsafe {
                WriteProcessMemory(
                    self.0 .0,
                    address as usize as _,
                    data.as_ptr().cast(),
                    data.len(),
                    Some(&mut count),
                )
            }
            .map_err(api)?;
            if count != data.len() {
                return Err(unsupported("写入目标内存不完整"));
            }
            Ok(())
        }
        fn protect(
            &self,
            address: u32,
            size: usize,
            protection: PAGE_PROTECTION_FLAGS,
        ) -> Res<PAGE_PROTECTION_FLAGS> {
            let mut previous = PAGE_PROTECTION_FLAGS::default();
            unsafe {
                VirtualProtectEx(
                    self.0 .0,
                    address as usize as _,
                    size,
                    protection,
                    &mut previous,
                )
            }
            .map_err(api)?;
            Ok(previous)
        }
        fn flush(&self, address: u32, size: usize) -> Res<()> {
            unsafe { FlushInstructionCache(self.0 .0, Some(address as usize as _), size) }
                .map_err(api)
        }
    }

    #[derive(Clone)]
    struct Section {
        rva: u32,
        size: u32,
        raw: u32,
        raw_size: u32,
        flags: u32,
    }
    struct Pe {
        file: File,
        length: u64,
        base: u32,
        size: u32,
        stamp: u32,
        sections: Vec<Section>,
        export: u32,
    }
    fn u16at(data: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
    }
    fn u32at(data: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
    }
    impl Pe {
        fn bytes(&mut self, offset: u64, length: usize) -> Res<Vec<u8>> {
            if length > 4 * 1024 * 1024
                || offset
                    .checked_add(length as u64)
                    .is_none_or(|end| end > self.length)
            {
                return Err(unsupported("PE 文件范围无效"));
            }
            let mut data = vec![0; length];
            self.file
                .seek(SeekFrom::Start(offset))
                .and_then(|_| self.file.read_exact(&mut data))
                .map_err(|e| unsupported(format!("读取 PE 失败: {e}")))?;
            Ok(data)
        }
        fn open(path: &Path) -> Res<Self> {
            let file = File::open(path).map_err(|e| unsupported(format!("无法读取模块: {e}")))?;
            let length = file
                .metadata()
                .map_err(|e| unsupported(e.to_string()))?
                .len();
            let mut pe = Self {
                file,
                length,
                base: 0,
                size: 0,
                stamp: 0,
                sections: vec![],
                export: 0,
            };
            let dos = pe.bytes(0, 64)?;
            if &dos[..2] != b"MZ" {
                return Err(unsupported("模块不是 PE"));
            }
            let nt = u32at(&dos, 60) as u64;
            let header = pe.bytes(nt, 24)?;
            let count = u16at(&header, 6) as usize;
            let optional_size = u16at(&header, 20) as usize;
            if &header[..4] != b"PE\0\0"
                || u16at(&header, 4) != 0x14c
                || count == 0
                || count > 96
                || !(104..=4096).contains(&optional_size)
            {
                return Err(unsupported("仅支持有效的 x86 PE32 模块"));
            }
            let optional = pe.bytes(nt + 24, optional_size)?;
            if u16at(&optional, 0) != 0x10b {
                return Err(unsupported("模块不是 PE32"));
            }
            pe.base = u32at(&optional, 28);
            pe.size = u32at(&optional, 56);
            pe.export = u32at(&optional, 96);
            pe.stamp = u32at(&header, 8);
            let sections = pe.bytes(nt + 24 + optional_size as u64, count * 40)?;
            for item in sections.as_chunks::<40>().0 {
                pe.sections.push(Section {
                    rva: u32at(item, 12),
                    size: u32at(item, 8),
                    raw: u32at(item, 20),
                    raw_size: u32at(item, 16),
                    flags: u32at(item, 36),
                });
            }
            Ok(pe)
        }
        fn section(&self, rva: u32, length: usize) -> Option<&Section> {
            self.sections.iter().find(|s| {
                rva >= s.rva
                    && u64::from(rva) + length as u64 <= u64::from(s.rva) + u64::from(s.size)
            })
        }
        fn data(&mut self, rva: u32, length: usize) -> Res<Vec<u8>> {
            let s = self
                .section(rva, length)
                .ok_or_else(|| unsupported("RVA 不在模块节内"))?;
            let delta = rva - s.rva;
            if u64::from(delta) + length as u64 > u64::from(s.raw_size) {
                return Err(unsupported("RVA 没有文件数据"));
            }
            self.bytes(u64::from(s.raw) + u64::from(delta), length)
        }
        fn executable(&self, rva: u32, length: usize) -> bool {
            self.section(rva, length)
                .is_some_and(|s| s.flags & 0x20000000 != 0)
        }
        fn onget(&mut self) -> Res<u32> {
            let directory = self.data(self.export, 40)?;
            let functions = u32at(&directory, 20) as usize;
            let count = u32at(&directory, 24) as usize;
            if count > 20000 || functions > 20000 {
                return Err(unsupported("导出表过大"));
            }
            let names = self.data(u32at(&directory, 32), count * 4)?;
            let ordinals = self.data(u32at(&directory, 36), count * 2)?;
            let addresses = self.data(u32at(&directory, 28), functions * 4)?;
            let mut matches = vec![];
            for i in 0..count {
                let rva = u32at(&names, i * 4);
                let section = self
                    .section(rva, 1)
                    .ok_or_else(|| unsupported("导出名称范围无效"))?;
                let length = (section.raw_size.saturating_sub(rva - section.rva) as usize).min(256);
                let name = self.data(rva, length)?;
                let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
                if name == b"?OnGetObject@CWindowWnd@DuiLib@@MAEJIIJAA_N@Z" {
                    let ordinal = u16at(&ordinals, i * 2) as usize;
                    if ordinal >= functions {
                        return Err(unsupported("导出序号无效"));
                    }
                    matches.push(u32at(&addresses, ordinal * 4));
                }
            }
            if matches.len() != 1 || !self.executable(matches[0], 128) {
                return Err(unsupported("没有唯一的预期 OnGetObject x86 导出"));
            }
            Ok(matches[0])
        }
        fn validate_loaded(&mut self, remote: &Remote, base: u32) -> Res<()> {
            let dos = remote.read(base, 64)?;
            if &dos[..2] != b"MZ" {
                return Err(unsupported("远程模块头无效"));
            }
            let nt = u32at(&dos, 60);
            if nt > 1024 * 1024 {
                return Err(unsupported("远程 PE 头范围无效"));
            }
            let header = remote.read(
                base.checked_add(nt)
                    .ok_or_else(|| unsupported("模块地址溢出"))?,
                84,
            )?;
            if &header[..4] != b"PE\0\0"
                || u32at(&header, 8) != self.stamp
                || u32at(&header, 80) != self.size
            {
                return Err(unsupported("模块文件与已加载映像身份不同"));
            }
            Ok(())
        }
    }

    fn modules(pid: u32) -> Res<Vec<(String, u32, PathBuf)>> {
        let snapshot = Handle(
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) }
                .map_err(api)?,
        );
        let mut item = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        unsafe { Module32FirstW(snapshot.0, &mut item) }.map_err(api)?;
        let mut output = vec![];
        loop {
            let name = String::from_utf16_lossy(
                &item.szModule[..item
                    .szModule
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(item.szModule.len())],
            );
            let path = String::from_utf16_lossy(
                &item.szExePath[..item
                    .szExePath
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(item.szExePath.len())],
            );
            // WOW64 also exposes its native ntdll/loader modules. They are not
            // members of the x86 application address space we adapt.
            if let Ok(base) = u32::try_from(item.modBaseAddr as usize) {
                output.push((name, base, PathBuf::from(path)));
            }
            if unsafe { Module32NextW(snapshot.0, &mut item) }.is_err() {
                break;
            }
        }
        output.sort_by_key(|(_, base, _)| *base);
        output.dedup_by(|a, b| a.1 == b.1 && a.0.eq_ignore_ascii_case(&b.0));
        Ok(output)
    }
    fn identity(remote: &Remote, hwnd: HWND, pid: u32, instance: &str) -> Res<()> {
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut owner));
        }
        if owner != pid
            || !unsafe { IsWindow(Some(hwnd)) }.as_bool()
            || unsafe { WaitForSingleObject(remote.0 .0, 0) } != WAIT_TIMEOUT
        {
            return Err(BackendError::new(
                "WIN32_TARGET_MISMATCH",
                "目标窗口或进程实例已变化",
            ));
        }
        let mut created = FILETIME::default();
        let mut other = [FILETIME::default(); 3];
        let [exit, kernel, user] = &mut other;
        unsafe { GetProcessTimes(remote.0 .0, &mut created, exit, kernel, user) }.map_err(api)?;
        let current = ((u64::from(created.dwHighDateTime) << 32)
            | u64::from(created.dwLowDateTime))
        .to_string();
        if current != instance {
            return Err(BackendError::new(
                "WIN32_TARGET_MISMATCH",
                "目标 PID 已被重用",
            ));
        }
        Ok(())
    }
    fn thread_positions(pid: u32, entry: u32) -> Res<()> {
        let snapshot =
            Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(api)?);
        let mut item = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        unsafe { Thread32First(snapshot.0, &mut item) }.map_err(api)?;
        let mut count = 0;
        loop {
            if item.th32OwnerProcessID == pid {
                let thread = Handle(
                    unsafe {
                        OpenThread(
                            THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                            false,
                            item.th32ThreadID,
                        )
                    }
                    .map_err(api)?,
                );
                let mut context = WOW64_CONTEXT {
                    ContextFlags: WOW64_CONTEXT_CONTROL,
                    ..Default::default()
                };
                unsafe { Wow64GetThreadContext(thread.0, &mut context) }.map_err(api)?;
                if (entry..entry + 5).contains(&context.Eip) {
                    return Err(BackendError::new(
                        "WIN32_ACTIVATION_BUSY",
                        "目标线程正在待修改入口执行，本次没有写入",
                    ));
                }
                count += 1;
            }
            if unsafe { Thread32Next(snapshot.0, &mut item) }.is_err() {
                break;
            }
        }
        if count == 0 {
            return Err(unsupported("没有可核验的目标线程"));
        }
        Ok(())
    }
    fn resolves(remote: &Remote, address: u32, target: u32, base: u32, pe: &Pe) -> bool {
        if address == target {
            return true;
        }
        if address < base || address - base >= pe.size {
            return false;
        }
        let Ok(code) = remote.read(address, 6) else {
            return false;
        };
        if code[..2] == [0xff, 0x25] {
            return remote.word(u32at(&code, 2)).is_ok_and(|a| a == target);
        }
        if code[0] == 0xe9 {
            return address.wrapping_add(5).wrapping_add(u32at(&code, 1)) == target;
        }
        // An IAT slot must be in non-executable data, rather than arbitrary code.
        !pe.executable(address - base, 4) && u32at(&code, 0) == target
    }
    fn old_ungated(code: &[u8]) -> bool {
        code.starts_with(b"\x55\x8b\xec")
            && code[..48].windows(4).any(|s| s == b"\x83\x7d\x10\xfc")
            && code.windows(3).any(|s| s == b"\xc2\x10\x00")
    }
    fn flags(pe: &mut Pe, onget: u32) -> Res<Vec<u32>> {
        let code = pe.data(onget, 128)?;
        // New layouts are admitted only when all three gates occur inside the
        // exported handler itself, with conditional branches and writable data.
        let mut found = vec![];
        for i in 0..code.len() - 9 {
            if code[i..i + 2] != [0x80, 0x3d]
                || code[i + 6] != 0
                || !matches!(code[i + 7], 0x74 | 0x75)
            {
                continue;
            }
            let Some(rva) = u32at(&code, i + 2).checked_sub(pe.base) else {
                continue;
            };
            if pe
                .section(rva, 1)
                .is_some_and(|s| s.flags & 0x80000000 != 0 && s.flags & 0x20000000 == 0)
            {
                found.push(rva);
            }
        }
        found.sort();
        found.dedup();
        if found.len() == 3 && found[2] - found[0] <= 4 {
            return Ok(found);
        }
        if found.is_empty() && old_ungated(&code) {
            return Ok(vec![]);
        }
        Err(unsupported(
            "DuiLib 门控布局未经验证；不会使用全模块的模糊三字节匹配",
        ))
    }

    struct Patch<'a> {
        hwnd: HWND,
        pid: u32,
        instance: &'a str,
        entry: u32,
        onget: u32,
        slot: u8,
        gates: &'a [u32],
    }

    fn install(remote: &Remote, patch: Patch<'_>, deadline: Instant, state: &mut Value) -> Res<()> {
        let Patch {
            hwnd,
            pid,
            instance,
            entry,
            onget,
            slot,
            gates,
        } = patch;
        identity(remote, hwnd, pid, instance)?;
        if Instant::now() >= deadline {
            return Err(BackendError::new(
                "WIN32_ACTIVATION_TIMEOUT",
                "解析超时，本次没有写入",
            ));
        }
        let allocation = unsafe {
            VirtualAllocEx(
                remote.0 .0,
                None,
                4096,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if allocation.is_null() {
            return Err(api(windows::core::Error::from_win32()));
        }
        let base = match u32::try_from(allocation as usize) {
            Ok(value) => value,
            Err(_) => {
                unsafe {
                    let _ = VirtualFreeEx(remote.0 .0, allocation, 0, MEM_RELEASE);
                }
                return Err(unsupported("无法分配 x86 地址空间"));
            }
        };
        let address = base + HEADER;
        let code = stub(entry, address, slot);
        let mut image = vec![0; HEADER as usize];
        image[..8].copy_from_slice(MAGIC);
        image[8..12].copy_from_slice(&entry.to_le_bytes());
        image[12..16].copy_from_slice(&onget.to_le_bytes());
        image[16] = slot;
        image.extend_from_slice(&code);
        let mut referenced = false;
        let result = (|| {
            remote.write(base, &image)?;
            remote.protect(base, 4096, PAGE_EXECUTE_READ)?;
            remote.flush(base, image.len())?;
            // No UI messages, allocations or loader calls while the target is suspended.
            let mut suspended = Suspended::new(remote.0 .0)?;
            let transaction = (|| {
                identity(remote, hwnd, pid, instance)?;
                thread_positions(pid, entry)?;
                if remote.read(entry, 5)? != ORIGINAL {
                    return Err(unsupported("入口在解析后发生变化，本次没有安装"));
                }
                let old: Vec<u8> = gates
                    .iter()
                    .map(|g| remote.read(*g, 1).map(|b| b[0]))
                    .collect::<Res<_>>()?;
                if old.iter().any(|b| *b > 1) {
                    return Err(unsupported("门控当前值不是布尔值"));
                }
                let protection = remote.protect(entry, 5, PAGE_EXECUTE_READWRITE)?;
                // Mark before the first write: a partial jump must also be rolled back.
                referenced = true;
                state["status"] = json!("outcome_unknown");
                state["install_called"] = json!(true);
                let write = (|| {
                    for gate in gates {
                        remote.write(*gate, &[1])?;
                    }
                    let mut jump = vec![0xe9];
                    jump.extend_from_slice(
                        &address.wrapping_sub(entry.wrapping_add(5)).to_le_bytes(),
                    );
                    remote.write(entry, &jump)?;
                    remote.flush(entry, 5)?;
                    if remote.read(entry, 5)? != jump {
                        return Err(unsupported("入口写入后校验失败"));
                    }
                    remote.protect(entry, 5, protection)?;
                    Ok(())
                })();
                if let Err(error) = write {
                    let rollback = (|| {
                        remote.protect(entry, 5, PAGE_EXECUTE_READWRITE)?;
                        remote.write(entry, ORIGINAL)?;
                        for (gate, value) in gates.iter().zip(old) {
                            remote.write(*gate, &[value])?;
                        }
                        remote.flush(entry, 5)?;
                        remote.protect(entry, 5, protection)?;
                        Ok::<_, BackendError>(())
                    })();
                    if rollback.is_ok() {
                        referenced = false;
                        state["status"] = json!("rolled_back");
                    } else {
                        return Err(BackendError::new(
                            "WIN32_ACTIVATION_ROLLBACK_FAILED",
                            "写入及回滚未能完整确认；状态未知，不要重复安装",
                        ));
                    }
                    return Err(error);
                }
                Ok(())
            })();
            let resumed = suspended.resume();
            resumed?;
            transaction
        })();
        if !referenced {
            unsafe {
                let _ = VirtualFreeEx(remote.0 .0, allocation, 0, MEM_RELEASE);
            }
        }
        result?;
        state["hook_address"] = json!(format!("0x{address:x}"));
        state["hook_verification"] = json!("confirmed_by_bytes");
        Ok(())
    }

    pub(crate) fn execute(hwnd: u64, pid: u32, instance: &str, timeout: u64) -> Res<CmdOut> {
        let deadline = Instant::now() + Duration::from_secs(timeout);
        let mut state = json!({"backend":"wework_builtin","pid":pid,"hwnd":format!("0x{hwnd:x}"),"process_creation_time":instance,"target_architecture":"x86","status":"not_started","install_called":false,"restore":false,"hook_verification":"not_checked","tree_verification":"not_checked"});
        let result = (|| {
            let remote = Remote(Handle(
                unsafe {
                    OpenProcess(
                        PROCESS_QUERY_INFORMATION
                            | PROCESS_VM_READ
                            | PROCESS_VM_WRITE
                            | PROCESS_VM_OPERATION
                            | PROCESS_SUSPEND_RESUME
                            | PROCESS_SYNCHRONIZE,
                        false,
                        pid,
                    )
                }
                .map_err(api)?,
            ));
            let hwnd = HWND(hwnd as usize as _);
            identity(&remote, hwnd, pid, instance)?;
            let ticks = instance
                .parse::<u64>()
                .map_err(|_| unsupported("进程创建时间无效"))?
                .checked_add(504911232000000000)
                .ok_or_else(|| unsupported("进程创建时间溢出"))?;
            let name: Vec<u16> = format!("Local\\xbot-cli-tree-{pid}-{ticks}\0")
                .encode_utf16()
                .collect();
            let mutex =
                Handle(unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }.map_err(api)?);
            let wait = unsafe { WaitForSingleObject(mutex.0, 0) };
            if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                return Err(BackendError::new(
                    "WIN32_ACTIVATION_BUSY",
                    "同一进程正在激活",
                ));
            }
            let _lock = Lock(mutex);
            let loaded = modules(pid)?;
            let find = |name: &str| -> Res<(u32, PathBuf)> {
                let matches: Vec<_> = loaded
                    .iter()
                    .filter(|(n, _, _)| n.eq_ignore_ascii_case(name))
                    .collect();
                if matches.len() != 1 {
                    return Err(unsupported(format!("没有唯一的 {name} 模块")));
                }
                Ok((matches[0].1, matches[0].2.clone()))
            };
            let (dui, dui_path) = find("DuiLib.dll")?;
            let (exe, exe_path) = find("WXWork.exe")?;
            let mut dp = Pe::open(&dui_path)?;
            let mut ep = Pe::open(&exe_path)?;
            dp.validate_loaded(&remote, dui)?;
            ep.validate_loaded(&remote, exe)?;
            let onget_rva = dp.onget()?;
            let onget = dui
                .checked_add(onget_rva)
                .ok_or_else(|| unsupported("模块地址溢出"))?;
            let gate_rvas = flags(&mut dp, onget_rva)?;
            let gates: Vec<u32> = gate_rvas
                .iter()
                .map(|r| {
                    dui.checked_add(*r)
                        .ok_or_else(|| unsupported("门控地址溢出"))
                })
                .collect::<Res<_>>()?;
            let live_onget = remote.read(onget, 128)?;
            if gates.is_empty() && !old_ungated(&live_onget) {
                return Err(unsupported("实时 OnGetObject 实现与文件 ABI 不符"));
            }
            let this = unsafe { GetWindowLongW(hwnd, GWLP_USERDATA) } as u32;
            if this == 0 {
                return Err(unsupported("目标窗口没有 DuiLib 活对象"));
            }
            let vtable = remote.word(this)?;
            let entries = remote.read(vtable, 80)?;
            let slots: Vec<_> = (0..20)
                .filter(|i| resolves(&remote, u32at(&entries, i * 4), onget, exe, &ep))
                .collect();
            if slots.len() != 1 || slots[0] < 5 {
                return Err(unsupported(
                    "无法唯一验证 OnGetObject/HandleMessage 虚表关系",
                ));
            }
            let slot = slots[0] as u8;
            let entry = u32at(&entries, (slot as usize - 5) * 4);
            if entry < exe || !ep.executable(entry - exe, 5) {
                return Err(unsupported("HandleMessage 不在 WXWork 可执行节内"));
            }
            if ep.data(entry - exe, 5)? != ORIGINAL {
                return Err(unsupported(
                    "HandleMessage 文件入口不是支持的完整五字节序言",
                ));
            }
            state["duilib_path"] = json!(dui_path);
            state["exe_path"] = json!(exe_path);
            state["onget_rva"] = json!(format!("0x{onget_rva:x}"));
            state["handle_message_rva"] = json!(format!("0x{:x}", entry - exe));
            state["onget_slot"] = json!(slot);
            state["gate_rvas"] = json!(gate_rvas);
            let head = remote.read(entry, 5)?;
            let already = head[0] == 0xe9;
            if already {
                let address = entry.wrapping_add(5).wrapping_add(u32at(&head, 1));
                let base = address
                    .checked_sub(HEADER)
                    .ok_or_else(|| unsupported("已有跳转不属于本适配器"))?;
                let expected = stub(entry, address, slot);
                let image = remote
                    .read(base, HEADER as usize + expected.len())
                    .map_err(|_| unsupported("已有跳转无法验证，不会覆盖"))?;
                if &image[..8] != MAGIC
                    || u32at(&image, 8) != entry
                    || u32at(&image, 12) != onget
                    || image[16] != slot
                    || image[HEADER as usize..] != expected
                    || gates
                        .iter()
                        .any(|g| remote.read(*g, 1).map_or(true, |v| v != [1]))
                {
                    return Err(unsupported("已有跳转不是完整的已知激活实现，不会覆盖"));
                }
                state["hook_address"] = json!(format!("0x{address:x}"));
                state["hook_verification"] = json!("confirmed_by_bytes");
                state["status"] = json!("already_active");
            } else {
                if head != ORIGINAL {
                    return Err(unsupported("HandleMessage 实时入口不符，本次没有写入"));
                }
                install(
                    &remote,
                    Patch {
                        hwnd,
                        pid,
                        instance,
                        entry,
                        onget,
                        slot,
                        gates: &gates,
                    },
                    deadline,
                    &mut state,
                )?;
                state["status"] = json!("activated");
            }
            identity(&remote, hwnd, pid, instance)?;
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(2000) as u32;
            let mut response = 0;
            let sent = remaining > 0
                && unsafe {
                    SendMessageTimeoutW(
                        hwnd,
                        WM_GETOBJECT,
                        WPARAM(1),
                        LPARAM(-4),
                        SMTO_ABORTIFHUNG | SMTO_BLOCK,
                        remaining,
                        Some(&mut response),
                    )
                }
                .0 != 0;
            state["wm_getobject_result"] = json!(format!("0x{response:x}"));
            state["accessibility_verification"] = json!(if sent && response != 0 {
                "object_returned"
            } else if !sent {
                "timeout_or_unavailable"
            } else {
                "no_object_returned"
            });
            // An LRESULT proves neither interface validity nor child count. The
            // existing bounded snapshot command performs actual UIA traversal.
            if !sent || response == 0 {
                state["status"] = json!("installed_unverified");
                return Err(BackendError::new(
                    "WIN32_ACTIVATION_UNVERIFIED",
                    "激活代码已安装，但 WM_GETOBJECT 未返回对象；未重复安装，请用 snapshot 核对",
                ));
            }
            Ok(())
        })();
        if let Err(mut error) = result {
            error.data = Some(state);
            error.retryable = Some(false);
            return Err(error);
        }
        Ok((state, vec![]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stub_preserves_this_and_replays_all_displaced_bytes() {
        let code = stub(0x12340000, 0x23450020, 14);
        assert!(code.windows(4).any(|w| w == b"\x89\x4c\x24\x04"));
        assert!(code.windows(4).any(|w| w == b"\x8b\x4c\x24\x04"));
        let fallback = 7 + code[6] as usize;
        assert_eq!(fallback, 14 + code[13] as usize);
        assert_eq!(&code[fallback..fallback + 5], b"\x66\x90\x55\x8b\xec");
        let next = 0x23450020u32 + fallback as u32 + 10;
        let rel = u32::from_le_bytes(code[fallback + 6..fallback + 10].try_into().unwrap());
        assert_eq!(next.wrapping_add(rel), 0x12340005);
        assert!(code.windows(3).any(|w| w == [0xff, 0x50, 56]));
    }
}
