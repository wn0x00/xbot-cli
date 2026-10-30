//! Bounded, read-only installation discovery. An explicit override is authoritative.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use serde_json::{json, Value};

use crate::core::BackendError;

#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub path: String,
    pub source: String,
    pub exists: bool,
}

#[derive(Debug, Serialize)]
pub struct Resolution {
    pub path: String,
    pub source: String,
    pub exists: bool,
    pub explicit: bool,
    pub candidates: Vec<Candidate>,
}

impl Resolution {
    pub fn evidence(&self) -> Value {
        serde_json::to_value(self).expect("installation resolution")
    }

    pub fn annotate(&self, mut error: BackendError) -> BackendError {
        let mut data = error.data.take().unwrap_or_else(|| json!({}));
        if !data.is_object() {
            data = json!({"response": data});
        }
        data["backend"] = self.evidence();
        error.data = Some(data);
        if matches!(error.code.as_str(), "SHELL_CLI_MISSING" | "SPAWN_FAILED") {
            error.message = format!("{}；实际后端: {}", error.message, self.path);
            error.hint = Some(if error.code == "SHELL_CLI_MISSING" {
                if self.explicit {
                    "XBOT_SHELL_CLI 覆盖路径不存在或不是文件。修正该变量为 shadowbot.shell-cli.exe 的完整路径，或移除覆盖后重新运行 doctor，使用自动发现。启动、登录或打开应用不能修复路径错误。".into()
                } else {
                    "没有发现可用的 shadowbot.shell-cli.exe。请确认影刀已完整安装，必要时修复安装；也可将 XBOT_SHELL_CLI 指向实际后端文件的完整路径后再运行 doctor。".into()
                }
            } else {
                "后端文件已定位但启动失败。检查错误中的系统原因、文件权限、安全软件拦截及安装完整性；不要重复执行可能已经生效的任务。".into()
            });
        } else if error.hint.is_none() {
            error.hint = Some(format!(
                "已尝试后端 {}（来源: {}）。路径存在不等于客户端服务可用；根据实际后端错误检查客户端连接、安装版本或登录状态。",
                self.path, self.source
            ));
        }
        error
    }
}

fn clean_path(raw: &str) -> PathBuf {
    let value = raw.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    let path = PathBuf::from(value);
    if path.is_absolute() || value.is_empty() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

fn candidate(path: PathBuf, source: &str) -> Candidate {
    Candidate {
        exists: path.is_file(),
        path: path.to_string_lossy().into_owned(),
        source: source.into(),
    }
}

fn version(path: &Path) -> Option<Vec<u32>> {
    let name = path.file_name()?.to_str()?.strip_prefix("shadowbot-")?;
    let parts: Vec<u32> = name
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (parts.len() >= 2 && parts.len() <= 4).then_some(parts)
}

fn select(
    override_path: Option<String>,
    roots: &[(PathBuf, String)],
    name: &str,
    fallback: &str,
) -> Resolution {
    let explicit = override_path.is_some();
    let mut candidates = Vec::new();
    if let Some(value) = override_path {
        candidates.push(candidate(clean_path(&value), "environment"));
    } else {
        for (root, source) in roots {
            candidates.push(candidate(root.join(name), source));
            let mut versions: Vec<_> = std::fs::read_dir(root)
                .into_iter()
                .flatten()
                .flatten()
                .take(128)
                .filter_map(|entry| {
                    let path = entry.path();
                    path.is_dir()
                        .then(|| version(&path).map(|v| (v, path)))
                        .flatten()
                })
                .collect();
            versions.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, directory) in versions {
                candidates.push(candidate(
                    directory.join(name),
                    &format!("{source}:version-directory"),
                ));
            }
        }
        candidates.push(candidate(PathBuf::from(fallback), "default-fallback"));
    }
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|item| seen.insert(item.path.to_lowercase()));
    let chosen = candidates
        .iter()
        .find(|c| c.exists)
        .unwrap_or(&candidates[0]);
    Resolution {
        path: chosen.path.clone(),
        source: chosen.source.clone(),
        exists: chosen.exists,
        explicit,
        candidates,
    }
}

pub fn shell() -> Resolution {
    let explicit = std::env::var("XBOT_SHELL_CLI").ok();
    select(
        explicit.clone(),
        if explicit.is_some() { &[] } else { roots() },
        "shadowbot.shell-cli.exe",
        crate::core::DEFAULT_SHELL_CLI,
    )
}

pub fn launcher() -> Resolution {
    let explicit = std::env::var("XBOT_LAUNCHER").ok();
    select(
        explicit.clone(),
        if explicit.is_some() { &[] } else { roots() },
        "ShadowBot.exe",
        crate::core::DEFAULT_LAUNCHER,
    )
}

fn roots() -> &'static [(PathBuf, String)] {
    static ROOTS: OnceLock<Vec<(PathBuf, String)>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let mut found = windows_roots();
        for (variable, suffix) in [
            ("ProgramFiles", "ShadowBot"),
            ("ProgramFiles(x86)", "ShadowBot"),
            ("LOCALAPPDATA", r"Programs\ShadowBot"),
            ("LOCALAPPDATA", "ShadowBot"),
        ] {
            if let Some(base) = std::env::var_os(variable) {
                found.push((
                    PathBuf::from(base).join(suffix),
                    format!("known-directory:{variable}"),
                ));
            }
        }
        found
    })
}

// Registry command values are parsed as an executable path, never executed.
fn command_executable(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let executable = if let Some(quoted) = value.strip_prefix('"') {
        quoted.split_once('"')?.0
    } else {
        let end = value.to_ascii_lowercase().find(".exe")? + 4;
        &value[..end]
    };
    let path = PathBuf::from(executable);
    path.is_absolute().then_some(path)
}

fn add_executable_root(roots: &mut Vec<(PathBuf, String)>, path: PathBuf, source: &str) {
    if let Some(parent) = path.parent() {
        roots.push((parent.into(), source.into()));
        if version(parent).is_some() {
            if let Some(base) = parent.parent() {
                roots.push((base.into(), format!("{source}:install-root")));
            }
        }
    }
}

#[cfg(windows)]
fn windows_roots() -> Vec<(PathBuf, String)> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Registry::*,
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    unsafe fn read(hive: HKEY, key: &str, value: &str) -> Option<String> {
        let key = wide(key);
        let value = wide(value);
        let mut length = 0;
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        RegGetValueW(
            hive,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            flags,
            None,
            None,
            Some(&mut length),
        )
        .ok()
        .ok()?;
        if length == 0 || length > 65536 {
            return None;
        }
        let mut buffer = vec![0u16; (length as usize).div_ceil(2)];
        RegGetValueW(
            hive,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut length),
        )
        .ok()
        .ok()?;
        Some(
            String::from_utf16_lossy(&buffer)
                .trim_end_matches('\0')
                .to_owned(),
        )
    }
    let mut roots = Vec::new();
    unsafe {
        if let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut valid = Process32FirstW(snapshot, &mut entry).is_ok();
            while valid {
                let length = entry
                    .szExeFile
                    .iter()
                    .position(|v| *v == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
                if [
                    "ShadowBot.exe",
                    "ShadowBot.Shell.exe",
                    "ShadowBot.Explorer.exe",
                ]
                .iter()
                .any(|n| name.eq_ignore_ascii_case(n))
                {
                    if let Ok(process) = OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION,
                        false,
                        entry.th32ProcessID,
                    ) {
                        let mut buffer = vec![0u16; 32768];
                        let mut size = buffer.len() as u32;
                        if QueryFullProcessImageNameW(
                            process,
                            PROCESS_NAME_WIN32,
                            PWSTR(buffer.as_mut_ptr()),
                            &mut size,
                        )
                        .is_ok()
                        {
                            add_executable_root(
                                &mut roots,
                                PathBuf::from(String::from_utf16_lossy(&buffer[..size as usize])),
                                "running-client",
                            );
                        }
                        let _ = CloseHandle(process);
                    }
                }
                valid = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
        }
        for (hive, label) in [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")] {
            for prefix in [r"Software", r"Software\WOW6432Node"] {
                for suffix in [
                    r"Classes\shadowbot\shell\open\command",
                    r"Microsoft\Windows\CurrentVersion\App Paths\ShadowBot.exe",
                ] {
                    let key = format!(r"{prefix}\{suffix}");
                    if let Some(path) =
                        read(hive, &key, "").and_then(|value| command_executable(&value))
                    {
                        add_executable_root(&mut roots, path, &format!("registry:{label}\\{key}"));
                    }
                }
                let key = format!(r"{prefix}\Microsoft\Windows\CurrentVersion\Uninstall");
                let mut handle = HKEY::default();
                let key_w = wide(&key);
                if RegOpenKeyExW(hive, PCWSTR(key_w.as_ptr()), None, KEY_READ, &mut handle).is_err()
                {
                    continue;
                }
                for index in 0..1024 {
                    let mut buffer = [0u16; 256];
                    let mut size = buffer.len() as u32;
                    if RegEnumKeyExW(
                        handle,
                        index,
                        Some(PWSTR(buffer.as_mut_ptr())),
                        &mut size,
                        None,
                        None,
                        None,
                        None,
                    )
                    .is_err()
                    {
                        break;
                    }
                    let subkey = format!(
                        r"{}\{}",
                        key,
                        String::from_utf16_lossy(&buffer[..size as usize])
                    );
                    let display = read(hive, &subkey, "DisplayName").unwrap_or_default();
                    if !display.to_ascii_lowercase().contains("shadowbot")
                        && !display.contains("影刀")
                    {
                        continue;
                    }
                    let source = format!("registry:{label}\\{subkey}");
                    if let Some(location) =
                        read(hive, &subkey, "InstallLocation").filter(|s| !s.trim().is_empty())
                    {
                        let path = clean_path(&location);
                        if path.is_absolute() {
                            roots.push((path, source.clone()));
                        }
                    }
                    if let Some(icon) =
                        read(hive, &subkey, "DisplayIcon").and_then(|s| command_executable(&s))
                    {
                        add_executable_root(&mut roots, icon, &source);
                    }
                }
                let _ = RegCloseKey(handle);
            }
        }
    }
    roots
}

#[cfg(not(windows))]
fn windows_roots() -> Vec<(PathBuf, String)> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "xbot-install-{}-{}",
            std::process::id(),
            ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn custom_install_spaces_unicode_and_root_shim_are_preferred() {
        let temporary = fixture();
        let root = temporary.join("自定义 Program Files");
        std::fs::create_dir_all(root.join("shadowbot-6.3.99")).unwrap();
        std::fs::write(root.join("shadowbot.shell-cli.exe"), b"shim").unwrap();
        std::fs::write(
            root.join("shadowbot-6.3.99/shadowbot.shell-cli.exe"),
            b"backend",
        )
        .unwrap();
        let result = select(
            None,
            &[(root.clone(), "registry:test".into())],
            "shadowbot.shell-cli.exe",
            "absent.exe",
        );
        assert_eq!(
            PathBuf::from(result.path),
            root.join("shadowbot.shell-cli.exe")
        );
        assert!(result.exists);
        assert_eq!(result.source, "registry:test");
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn stale_roots_and_missing_versions_are_skipped_with_numeric_version_order() {
        let temporary = fixture();
        for name in ["shadowbot-6.3.9", "shadowbot-6.3.10", "shadowbot-6.3.100"] {
            std::fs::create_dir_all(temporary.join(name)).unwrap();
        }
        for name in ["shadowbot-6.3.9", "shadowbot-6.3.10"] {
            std::fs::write(temporary.join(name).join("shell.exe"), b"backend").unwrap();
        }
        let result = select(
            None,
            &[
                (temporary.join("stale"), "old".into()),
                (temporary.clone(), "new".into()),
            ],
            "shell.exe",
            "missing.exe",
        );
        assert_eq!(
            PathBuf::from(&result.path),
            temporary.join("shadowbot-6.3.10/shell.exe")
        );
        assert!(result.candidates.iter().any(|c| !c.exists));
        assert!(result.source.ends_with(":version-directory"));
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn invalid_explicit_override_never_falls_back_to_an_installed_client() {
        let temporary = fixture();
        std::fs::write(temporary.join("shell.exe"), b"installed").unwrap();
        for value in ["missing.exe", ""] {
            let result = select(
                Some(value.into()),
                &[(temporary.clone(), "registry".into())],
                "shell.exe",
                "default.exe",
            );
            assert!(!result.exists);
            assert!(result.explicit);
            assert_eq!(result.candidates.len(), 1);
            let error = result.annotate(BackendError::new("SHELL_CLI_MISSING", "not found"));
            assert!(error.hint.unwrap().contains("XBOT_SHELL_CLI"));
            assert_eq!(error.data.unwrap()["backend"]["source"], "environment");
        }
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn executable_commands_are_parsed_without_execution_or_argument_inclusion() {
        assert_eq!(
            command_executable(r#""D:\Program Files\ShadowBot\ShadowBot.exe" %1"#),
            Some(r"D:\Program Files\ShadowBot\ShadowBot.exe".into())
        );
        assert_eq!(
            command_executable(r"D:\Program Files\ShadowBot\ShadowBot.exe,0"),
            Some(r"D:\Program Files\ShadowBot\ShadowBot.exe".into())
        );
        assert!(command_executable("relative.exe --flag").is_none());
        assert!(command_executable(r#""D:\unfinished.exe"#).is_none());
    }
}
