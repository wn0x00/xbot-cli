//! Explicit, isolated activation using the locally installed ShadowBot DLL.
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{run_capture, BackendError, Res};

const WORKER: &str = include_str!("win32_activate.ps1");
const REGISTRY: &str = r"HKLM64\SOFTWARE\ShadowBot x64\InstallPath";

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

fn select_dll(root: &Path) -> Res<PathBuf> {
    let directory = latest_directory(root)?;
    let dll = directory.join("Native").join("x64").join("RpaBuilder.dll");
    if !dll.is_file() {
        return Err(BackendError::new(
            "WIN32_ACTIVATION_DLL_MISSING",
            "最大版本目录中没有 Native\\x64\\RpaBuilder.dll",
        )
        .with_data(json!({"version_directory":directory,"dll_path":dll})));
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

pub(crate) fn execute(hwnd: u64, pid: u32, timeout: u64) -> Res<CmdOut> {
    let root = install_root()?;
    let dll = select_dll(&root)?;
    let input = json!({"hwnd":hwnd,"pid":pid,"dll_path":dll});
    // Encode data separately, never interpolate a registry path as executable script.
    let input = STANDARD.encode(input.to_string());
    let script = format!("$requestBase64 = '{input}'\n{WORKER}");
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let system = std::env::var_os("SystemRoot").ok_or_else(|| {
        BackendError::new(
            "WIN32_DEPENDENCY_MISSING",
            "未找到 Windows PowerShell 系统目录",
        )
    })?;
    let mut command =
        Command::new(PathBuf::from(system).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"));
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
        .with_hint("核对 HWND/PID、目标权限及返回状态；已加载或结果未知时不要强制重复安装")
        .with_retryable(false));
    }
    let mut data = payload["data"]
        .as_object()
        .cloned()
        .ok_or_else(|| BackendError::new("WIN32_ACTIVATION_PROVIDER_ERROR", "激活结果缺少对象"))?;
    data.insert(
        "installation".into(),
        json!({"registry":REGISTRY,"root":root,"selected_dll":dll}),
    );
    Ok((Value::Object(data), vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;
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
            select_dll(&root).unwrap_err().code,
            "WIN32_ACTIVATION_DLL_MISSING"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
