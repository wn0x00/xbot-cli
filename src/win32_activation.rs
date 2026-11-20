//! Explicit, isolated activation using the locally installed ShadowBot DLL.
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{run_capture, BackendError, Res};

const WORKER: &str = include_str!("win32_activate.ps1");
const WXWORK_WORKER: &str = include_str!("win32_activate_wxwork.ps1");
const REGISTRY: &str = r"HKLM64\SOFTWARE\ShadowBot x64\InstallPath";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Architecture {
    X86,
    X64,
}

impl Architecture {
    fn from_machines(process: u16, native: u16) -> Res<Self> {
        match (process, native) {
            (0x014c, 0x8664) => Ok(Self::X86),
            (0, 0x8664) => Ok(Self::X64),
            _ => Err(BackendError::new(
                "WIN32_TARGET_ARCH_UNSUPPORTED",
                "仅支持 Windows x64 系统上的 x86 或 x64 目标进程",
            )
            .with_data(json!({"process_machine":process,"native_machine":native}))),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::X64 => "x64",
        }
    }

    fn worker(self, system: &Path) -> PathBuf {
        system
            .join(match self {
                Self::X86 => "SysWOW64",
                Self::X64 => "System32",
            })
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe")
    }
}

struct Target {
    architecture: Architecture,
    creation_time: String,
    process_name: String,
    class_name: String,
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Drop for Target {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
fn target(hwnd: u64, pid: u32) -> Res<Target> {
    use windows::Win32::Foundation::{FILETIME, HWND, WAIT_TIMEOUT};
    use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE;
    use windows::Win32::System::Threading::{
        GetProcessTimes, IsWow64Process2, OpenProcess, QueryFullProcessImageNameW,
        WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowThreadProcessId, IsWindow,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
    }
    .map_err(|e| {
        BackendError::new(
            "WIN32_TARGET_ACCESS_FAILED",
            format!("无法查询目标进程: {e}"),
        )
    })?;
    // Keep this handle alive until the worker exits; independently bind its request
    // to the creation FILETIME so PID reuse cannot select a different instance.
    let mut target = Target {
        architecture: Architecture::X64,
        creation_time: String::new(),
        process_name: String::new(),
        class_name: String::new(),
        handle,
    };
    let window = HWND(hwnd as usize as *mut std::ffi::c_void);
    let validate = || unsafe {
        let mut owner = 0;
        let _ = GetWindowThreadProcessId(window, Some(&mut owner));
        if !IsWindow(Some(window)).as_bool()
            || owner != pid
            || WaitForSingleObject(handle, 0) != WAIT_TIMEOUT
        {
            return Err(BackendError::new(
                "WIN32_TARGET_MISMATCH",
                "窗口不属于指定 PID，或目标进程已经退出",
            ));
        }
        Ok(())
    };
    validate()?;
    let mut process = IMAGE_FILE_MACHINE::default();
    let mut native = IMAGE_FILE_MACHINE::default();
    unsafe { IsWow64Process2(handle, &mut process, Some(&mut native)) }.map_err(|e| {
        BackendError::new(
            "WIN32_TARGET_ACCESS_FAILED",
            format!("读取进程架构失败: {e}"),
        )
    })?;
    target.architecture = Architecture::from_machines(process.0, native.0)?;
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }.map_err(
        |e| {
            BackendError::new(
                "WIN32_TARGET_ACCESS_FAILED",
                format!("读取进程实例失败: {e}"),
            )
        },
    )?;
    target.creation_time =
        ((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)).to_string();
    let mut path = vec![0u16; 32768];
    let mut size = path.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(path.as_mut_ptr()),
            &mut size,
        )
    }
    .map_err(|e| {
        BackendError::new(
            "WIN32_TARGET_ACCESS_FAILED",
            format!("读取进程路径失败: {e}"),
        )
    })?;
    let path = String::from_utf16_lossy(&path[..size as usize]);
    target.process_name = path.rsplit(['\\', '/']).next().unwrap_or("").to_owned();
    let mut class = [0u16; 256];
    let size = unsafe { GetClassNameW(window, &mut class) };
    if size == 0 {
        return Err(BackendError::new(
            "WIN32_TARGET_ACCESS_FAILED",
            "读取窗口类名失败",
        ));
    }
    target.class_name = String::from_utf16_lossy(&class[..size as usize]);
    validate()?;
    Ok(target)
}

#[cfg(not(windows))]
fn target(_hwnd: u64, _pid: u32) -> Res<Target> {
    Err(BackendError::new(
        "WIN32_UNSUPPORTED_PLATFORM",
        "win32 activate 仅支持 Windows x64 系统",
    ))
}

fn numeric_version(name: &str) -> Option<Vec<u64>> {
    let version = name.strip_prefix("shadowbot-")?;
    let mut parts = Vec::new();
    for part in version.split('.') {
        if part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        parts.push(part.parse().ok()?);
    }
    while parts.len() > 1 && parts.last() == Some(&0) {
        parts.pop();
    }
    Some(parts)
}

fn latest_directory(root: &Path) -> Res<PathBuf> {
    let entries = std::fs::read_dir(root).map_err(|e| {
        BackendError::new(
            "WIN32_INSTALLATION_MISSING",
            format!("无法读取影刀安装目录: {e}"),
        )
    })?;
    let mut versions = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            BackendError::new(
                "WIN32_INSTALLATION_MISSING",
                format!("无法枚举影刀目录: {e}"),
            )
        })?;
        if entry.path().is_dir() {
            if let Some(version) = entry.file_name().to_str().and_then(numeric_version) {
                versions.push((version, entry.path()));
            }
        }
    }
    versions.sort();
    versions.pop().map(|(_, path)| path).ok_or_else(|| {
        BackendError::new(
            "WIN32_INSTALLATION_MISSING",
            "InstallPath 下没有数字版本 shadowbot-* 目录",
        )
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Provider {
    RpaBuilder,
    WxWorkMisc,
}

impl Provider {
    fn for_target(architecture: Architecture, process_name: &str, class_name: &str) -> Self {
        if architecture == Architecture::X86
            && process_name.eq_ignore_ascii_case("WXWork.exe")
            && class_name == "WeWorkWindow"
        {
            Self::WxWorkMisc
        } else {
            Self::RpaBuilder
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::RpaBuilder => "RpaBuilder.dll",
            Self::WxWorkMisc => "misc.exe",
        }
    }

    fn worker(self) -> &'static str {
        match self {
            Self::RpaBuilder => WORKER,
            Self::WxWorkMisc => WXWORK_WORKER,
        }
    }
}

fn select_native(root: &Path, architecture: Architecture, provider: Provider) -> Res<PathBuf> {
    let directory = latest_directory(root)?;
    let dll = directory
        .join("Native")
        .join(architecture.name())
        .join(provider.file_name());
    if !dll.is_file() {
        return Err(BackendError::new(
            if provider == Provider::RpaBuilder { "WIN32_ACTIVATION_DLL_MISSING" } else { "WIN32_ACTIVATION_HELPER_MISSING" },
            format!("最大版本目录中没有 Native\\{}\\{}", architecture.name(), provider.file_name()),
        )
        .with_data(json!({"version_directory":directory,"dll_path":dll,"target_architecture":architecture.name()})));
    }
    Ok(dll)
}

#[cfg(windows)]
fn install_root() -> Res<PathBuf> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::*;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_SUBKEY_WOW6464KEY;
    let mut size = 0;
    let read = |buffer, size: &mut u32| unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\ShadowBot x64"),
            PCWSTR(w!("InstallPath").as_ptr()),
            flags,
            None,
            buffer,
            Some(size),
        )
        .ok()
    };
    read(None, &mut size).map_err(|e| {
        BackendError::new(
            "WIN32_INSTALLATION_MISSING",
            format!("读取 {REGISTRY} 失败: {e}"),
        )
    })?;
    if size == 0 || size > 65536 {
        return Err(BackendError::new(
            "WIN32_INSTALLATION_MISSING",
            "InstallPath 长度无效",
        ));
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    read(Some(buffer.as_mut_ptr().cast()), &mut size).map_err(|e| {
        BackendError::new(
            "WIN32_INSTALLATION_MISSING",
            format!("读取 InstallPath 失败: {e}"),
        )
    })?;
    let raw = String::from_utf16_lossy(&buffer);
    let path = PathBuf::from(raw.trim_end_matches('\0').trim().trim_matches('"'));
    if !path.is_absolute() || !path.is_dir() {
        return Err(BackendError::new(
            "WIN32_INSTALLATION_MISSING",
            "InstallPath 不是有效的绝对安装目录",
        ));
    }
    Ok(path)
}

#[cfg(not(windows))]
fn install_root() -> Res<PathBuf> {
    Err(BackendError::new(
        "WIN32_UNSUPPORTED_PLATFORM",
        "win32 activate 仅支持 Windows x64",
    ))
}

pub(crate) fn execute(hwnd: u64, pid: u32, requested: &str, timeout: u64) -> Res<CmdOut> {
    let selected = target(hwnd, pid).map_err(|error| not_started(error, hwnd, pid))?;
    let is_wework = Provider::for_target(
        selected.architecture,
        &selected.process_name,
        &selected.class_name,
    ) == Provider::WxWorkMisc;
    if requested == "wework" && !is_wework {
        return Err(not_started(
            BackendError::new(
                "WIN32_ACTIVATION_PROVIDER_UNSUPPORTED",
                "内置 wework 适配仅支持 x86 WXWork.exe 的 WeWorkWindow 窗口",
            ),
            hwnd,
            pid,
        ));
    }
    if requested == "wework" || (requested == "auto" && is_wework) {
        return crate::win32_wework::execute(hwnd, pid, &selected.creation_time, timeout);
    }
    let (target, root, provider, native, worker) = (|| {
        let target = selected;
        let root = install_root()?;
        let provider = Provider::for_target(
            target.architecture,
            &target.process_name,
            &target.class_name,
        );
        let native = select_native(&root, target.architecture, provider)?;
        let system = std::env::var_os("SystemRoot").ok_or_else(|| {
            BackendError::new(
                "WIN32_DEPENDENCY_MISSING",
                "未找到 Windows PowerShell 系统目录",
            )
        })?;
        let worker = target.architecture.worker(Path::new(&system));
        Ok((target, root, provider, native, worker))
    })()
    .map_err(|mut error: BackendError| {
        let data = error.data.get_or_insert_with(|| json!({}));
        data["pid"] = json!(pid);
        data["hwnd"] = json!(format!("0x{hwnd:x}"));
        data["status"] = json!("not_started");
        data["install_called"] = json!(false);
        error.retryable = Some(false);
        error
    })?;
    let input = json!({"hwnd":hwnd,"pid":pid,"dll_path":native,"helper_path":native,
        "target_architecture":target.architecture.name(),"process_creation_time":target.creation_time});
    // Encode data separately, never interpolate a registry path as executable script.
    let input = STANDARD.encode(input.to_string());
    let script = format!("$requestBase64 = '{input}'\n{}", provider.worker());
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut command = Command::new(&worker);
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-Mta",
        "-EncodedCommand",
        &STANDARD.encode(utf16),
    ]);
    let output = run_capture(command, timeout).map_err(|mut error| {
        if error.code == "SHELL_CLI_MISSING" || error.code == "SPAWN_FAILED" {
            error.code = "WIN32_DEPENDENCY_MISSING".into();
            error.message = "无法启动 Windows PowerShell 激活子进程".into();
        }
        error.retryable = Some(false);
        error
    })?;
    // Vendor DLLs can write diagnostics to stdout; the worker owns the final JSON line.
    let payload: Value = output
        .stdout
        .lines()
        .rev()
        .find_map(|line| {
            serde_json::from_str::<Value>(line.trim_start_matches('\u{feff}'))
                .ok()
                .filter(|value| value.get("ok").is_some())
        })
        .ok_or_else(|| {
            BackendError::new(
                "WIN32_ACTIVATION_PROVIDER_ERROR",
                "激活子进程未返回有效结果；目标状态未知",
            )
            .with_retryable(false)
        })?;
    if payload["ok"] != true || !output.status.success() {
        return Err(BackendError::new(
            payload["code"]
                .as_str()
                .unwrap_or("WIN32_ACTIVATION_PROVIDER_ERROR"),
            payload["message"].as_str().unwrap_or("激活未完成"),
        )
        .with_data(payload["data"].clone())
        .with_hint(if payload["code"] == "WIN32_ACTIVATION_PROVIDER_UNSUPPORTED" {
            "影刀 DLL 对当前应用返回未实现；更换位数不能解决此返回值。请核对其应用适配，不要强制重复安装"
        } else {
            "核对 HWND/PID、目标权限及返回状态；已加载或结果未知时不要强制重复安装"
        })
        .with_retryable(false));
    }
    let mut data = payload["data"]
        .as_object()
        .cloned()
        .ok_or_else(|| BackendError::new("WIN32_ACTIVATION_PROVIDER_ERROR", "激活结果缺少对象"))?;
    data.insert(
        "installation".into(),
        if provider == Provider::RpaBuilder {
            json!({"registry":REGISTRY,"root":root,"selected_dll":native,"worker":worker})
        } else {
            json!({"registry":REGISTRY,"root":root,"selected_helper":native,"worker":worker})
        },
    );
    Ok((Value::Object(data), vec![]))
}

fn not_started(mut error: BackendError, hwnd: u64, pid: u32) -> BackendError {
    let data = error.data.get_or_insert_with(|| json!({}));
    data["pid"] = json!(pid);
    data["hwnd"] = json!(format!("0x{hwnd:x}"));
    data["status"] = json!("not_started");
    data["install_called"] = json!(false);
    error.retryable = Some(false);
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select_dll(root: &Path, architecture: Architecture) -> Res<PathBuf> {
        select_native(root, architecture, Provider::RpaBuilder)
    }

    #[test]
    fn wxwork_uses_its_own_provider_only_for_matching_process_window_and_architecture() {
        assert_eq!(
            Provider::for_target(Architecture::X86, "WXWork.exe", "WeWorkWindow"),
            Provider::WxWorkMisc
        );
        assert_eq!(
            Provider::for_target(Architecture::X86, "wxwork.EXE", "WeWorkWindow"),
            Provider::WxWorkMisc
        );
        for (architecture, process, class) in [
            (Architecture::X64, "WXWork.exe", "WeWorkWindow"),
            (Architecture::X86, "WXWork.exe", "OtherWindow"),
            (Architecture::X86, "Other.exe", "WeWorkWindow"),
        ] {
            assert_eq!(
                Provider::for_target(architecture, process, class),
                Provider::RpaBuilder
            );
        }
        assert_eq!(Provider::WxWorkMisc.file_name(), "misc.exe");
    }
    #[test]
    fn numeric_versions_and_latest_missing_dll_do_not_fall_back() {
        assert!(
            numeric_version("shadowbot-6.10").unwrap()
                > numeric_version("shadowbot-6.9.999").unwrap()
        );
        assert_eq!(
            numeric_version("shadowbot-7.0.0"),
            numeric_version("shadowbot-7")
        );
        for invalid in [
            "shadowbot-",
            "shadowbot-6..3",
            "shadowbot-+6.3",
            "shadowbot-6.3-beta",
            "other-6.3",
        ] {
            assert!(numeric_version(invalid).is_none(), "{invalid}");
        }
        let root = std::env::temp_dir().join(format!(
            "xbot-activate-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(root.join("shadowbot-6.9.999/Native/x64")).unwrap();
        std::fs::write(root.join("shadowbot-6.9.999/Native/x64/RpaBuilder.dll"), []).unwrap();
        std::fs::create_dir_all(root.join("shadowbot-6.10.0")).unwrap();
        std::fs::write(root.join("shadowbot-99.0"), []).unwrap();
        assert_eq!(
            latest_directory(&root).unwrap(),
            root.join("shadowbot-6.10.0")
        );
        assert_eq!(
            select_dll(&root, Architecture::X64).unwrap_err().code,
            "WIN32_ACTIVATION_DLL_MISSING"
        );
        let x86 = root.join("shadowbot-6.10.0/Native/x86/RpaBuilder.dll");
        std::fs::create_dir_all(x86.parent().unwrap()).unwrap();
        std::fs::write(&x86, []).unwrap();
        assert_eq!(select_dll(&root, Architecture::X86).unwrap(), x86);
        assert_eq!(
            select_native(&root, Architecture::X86, Provider::WxWorkMisc)
                .unwrap_err()
                .code,
            "WIN32_ACTIVATION_HELPER_MISSING"
        );
        let misc = root.join("shadowbot-6.10.0/Native/x86/misc.exe");
        std::fs::write(&misc, []).unwrap();
        assert_eq!(
            select_native(&root, Architecture::X86, Provider::WxWorkMisc).unwrap(),
            misc
        );
        // Neither an old x64 DLL nor the newest x86 DLL can satisfy x64.
        assert_eq!(
            select_dll(&root, Architecture::X64).unwrap_err().code,
            "WIN32_ACTIVATION_DLL_MISSING"
        );
        let x64 = root.join("shadowbot-6.10.0/Native/x64/RpaBuilder.dll");
        std::fs::create_dir_all(x64.parent().unwrap()).unwrap();
        std::fs::write(&x64, []).unwrap();
        assert_eq!(select_dll(&root, Architecture::X64).unwrap(), x64);
        assert_eq!(select_dll(&root, Architecture::X86).unwrap(), x86);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn architecture_routes_to_matching_worker_and_rejects_other_machines() {
        for (process, architecture, directory) in [
            (0x014c, Architecture::X86, "SysWOW64"),
            (0, Architecture::X64, "System32"),
        ] {
            assert_eq!(
                Architecture::from_machines(process, 0x8664).unwrap(),
                architecture
            );
            assert_eq!(
                architecture.worker(Path::new("Windows")),
                Path::new("Windows")
                    .join(directory)
                    .join("WindowsPowerShell/v1.0/powershell.exe")
            );
        }
        for (process, native) in [(0, 0xaa64), (0x014c, 0xaa64), (0x01c4, 0x8664), (0, 0x014c)] {
            assert_eq!(
                Architecture::from_machines(process, native)
                    .unwrap_err()
                    .code,
                "WIN32_TARGET_ARCH_UNSUPPORTED"
            );
        }
    }
}
