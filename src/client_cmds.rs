//! Read-only environment inventory. Running identity and selected backends are independent.
use std::path::Path;

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{self, BackendError, Res};
use crate::installation::{self, RunningClient};

pub fn info() -> Res<CmdOut> {
    let client = match installation::running_clients() {
        Ok(processes) => summarize(processes),
        Err(error) => json!({"running":null,"version":null,"version_source":null,
            "install_dir":null,"processes":[],"detection_complete":false,"error":error_value(error)}),
    };
    let backend = installation::shell();
    let shell = match core::shell_cli(&["system", "state"], 10) {
        Ok(state) => json!({"ok":true,"backend":backend.evidence(),"state":state}),
        Err(error) => json!({"ok":false,"backend":backend.evidence(),"error":error_value(error)}),
    };
    // Even forced legacy mode must perform a read before claiming the service is reachable.
    let mcp = match crate::studio_mcp::list(10).and_then(|_| crate::studio_mcp::connection()) {
        Ok(connection) => json!({"ok":true,"connection":connection}),
        Err(error) => json!({"ok":false,"configured_endpoint":core::mcp_endpoint(),
            "error":error_value(error)}),
    };
    let mut notes = Vec::new();
    if client["running"] == false {
        notes.push("未发现正在运行的影刀客户端；已安装的后端路径不代表客户端已启动。");
    } else if client["version"].is_null() || client["install_dir"].is_null() {
        notes.push(
            "无法唯一确认当前客户端版本或目录，请查看进程明细；不会从最大安装目录推断运行版本。",
        );
    }
    if backend.explicit {
        notes.push("shell-cli 使用 XBOT_SHELL_CLI 覆盖配置；该路径不代表正在运行的客户端。");
    }
    let partial = client["detection_complete"] != true || shell["ok"] != true || mcp["ok"] != true;
    Ok((
        json!({
            "command":"client.info",
            "cli":{"version":env!("CARGO_PKG_VERSION"),"executable":std::env::current_exe().ok()},
            "client":client,"shell_cli":shell,"studio_mcp":mcp,
            "mcp_mode":std::env::var("XBOT_MCP_MODE").unwrap_or_else(|_| "auto".into()),
            "partial":partial,"notes":notes
        }),
        vec![],
    ))
}

fn error_value(error: BackendError) -> Value {
    json!({"code":error.code,"message":error.message,"hint":error.hint,"details":error.data})
}

fn summarize(processes: Vec<RunningClient>) -> Value {
    let processes: Vec<Value> = processes
        .into_iter()
        .map(|process| {
            let directory = process
                .executable
                .as_deref()
                .and_then(|path| Path::new(path).parent());
            let (version, source) = process
                .executable
                .as_deref()
                .and_then(product_version)
                .map(|version| (Some(version), Some("executable_product_version")))
                .unwrap_or_else(|| {
                    (
                        directory.and_then(directory_version),
                        Some("installation_directory"),
                    )
                });
            json!({"pid":process.pid,"name":process.name,"executable":process.executable,
            "install_dir":directory,"version_source":version.as_ref().and(source),
            "version":version,"path_error":process.path_error})
        })
        .collect();
    let complete = processes
        .iter()
        .all(|p| p["executable"].is_string() && p["version"].is_string());
    let directory = unique(&processes, "install_dir").filter(|_| complete);
    let version = unique(&processes, "version").filter(|_| directory.is_some());
    let source = unique(&processes, "version_source").filter(|_| version.is_some());
    json!({"running":!processes.is_empty(),"version":version,"version_source":source,
        "install_dir":directory,"processes":processes,"detection_complete":complete})
}

fn unique(processes: &[Value], field: &str) -> Option<String> {
    let first = processes.first()?.get(field)?.as_str()?;
    processes
        .iter()
        .all(|p| p[field].as_str() == Some(first))
        .then(|| first.to_owned())
}

fn directory_version(directory: &Path) -> Option<String> {
    let version = directory
        .file_name()?
        .to_str()?
        .strip_prefix("shadowbot-")?;
    let parts: Vec<&str> = version.split('.').collect();
    ((2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u32>().is_ok()))
    .then(|| version.into())
}

#[cfg(windows)]
fn product_version(path: &str) -> Option<String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
        if size == 0 || size > 16 * 1024 * 1024 {
            return None;
        }
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        GetFileVersionInfoW(
            PCWSTR(path.as_ptr()),
            None,
            size,
            buffer.as_mut_ptr().cast(),
        )
        .ok()?;
        let mut value = std::ptr::null_mut();
        let mut length = 0;
        if !VerQueryValueW(buffer.as_ptr().cast(), w!("\\"), &mut value, &mut length).as_bool()
            || value.is_null()
            || (length as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let info = std::ptr::read_unaligned(value.cast::<VS_FIXEDFILEINFO>());
        if info.dwSignature != 0xfeef04bd {
            return None;
        }
        let mut parts = vec![
            info.dwProductVersionMS >> 16,
            info.dwProductVersionMS & 65535,
            info.dwProductVersionLS >> 16,
            info.dwProductVersionLS & 65535,
        ];
        if parts.iter().all(|p| *p == 0) {
            return None;
        }
        if parts[3] == 0 {
            parts.pop();
        }
        Some(
            parts
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("."),
        )
    }
}

#[cfg(not(windows))]
fn product_version(_path: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, path: Option<&str>) -> RunningClient {
        RunningClient {
            pid,
            name: "ShadowBot.Shell.exe".into(),
            executable: path.map(str::to_owned),
            path_error: None,
        }
    }

    #[test]
    fn no_process_is_not_an_installed_or_running_version() {
        let info = summarize(vec![]);
        assert_eq!(info["running"], false);
        assert!(info["version"].is_null());
        assert!(info["install_dir"].is_null());
        assert_eq!(info["detection_complete"], true);
    }

    #[test]
    fn unknown_process_path_prevents_choosing_another_process_as_current() {
        let info = summarize(vec![
            process(1, Some("C:/absent/shadowbot-6.4.14/ShadowBot.Shell.exe")),
            process(2, None),
        ]);
        assert_eq!(info["running"], true);
        assert_eq!(info["detection_complete"], false);
        assert!(info["version"].is_null());
        assert!(info["install_dir"].is_null());
        assert_eq!(info["processes"][0]["version"], "6.4.14");
    }

    #[test]
    fn multiple_installations_are_listed_without_arbitrary_selection() {
        let info = summarize(vec![
            process(1, Some("C:/absent/shadowbot-6.3.39/ShadowBot.Shell.exe")),
            process(2, Some("C:/absent/shadowbot-6.4.14/ShadowBot.Explorer.exe")),
        ]);
        assert_eq!(info["detection_complete"], true);
        assert!(info["version"].is_null());
        assert!(info["install_dir"].is_null());
        let same = summarize(vec![
            process(1, Some("C:/absent/shadowbot-6.4.14/ShadowBot.Shell.exe")),
            process(2, Some("C:/absent/shadowbot-6.4.14/ShadowBot.Explorer.exe")),
        ]);
        assert_eq!(same["version"], "6.4.14");
        assert_eq!(same["version_source"], "installation_directory");
    }
}
