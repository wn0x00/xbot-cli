//! Public ShadowBot DLL inventory and startup of an explicitly selected browser.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::app_cmds::CmdOut;
use crate::core::{self, BackendError, Res};

const HELPER: &[u8] = include_bytes!("browser_assets/BrowserQuery.dll");
const CONFIG: &[u8] = include_bytes!("browser_assets/BrowserQuery.runtimeconfig.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    pub id: String,
    pub name: String,
    pub installed: Option<bool>,
    pub registered: bool,
    pub pipe_name: Option<String>,
    pub web_supported: bool,
    #[serde(flatten)]
    pub details: Map<String, Value>,
}

#[derive(Clone, Debug)]
pub struct BrowserTarget {
    pub id: String,
    pub pipe_prefix: String,
    pub exe_path: Option<PathBuf>,
}

impl BrowserTarget {
    pub fn start_if_needed(&self) -> Res<bool> {
        let executable = self
            .exe_path
            .as_deref()
            .filter(|path| path.is_absolute() && path.is_file())
            .ok_or_else(|| {
                BackendError::new(
                    "WEB_BROWSER_EXECUTABLE_MISSING",
                    format!("无法启动 {}：DLL 未返回有效的浏览器程序路径", self.id),
                )
                .with_data(json!({"browser":self.id,"exePath":self.exe_path,"applied":false}))
                .with_hint("检查浏览器安装和 web browser list --all；不会启动其他浏览器")
            })?;
        if executable_is_running(executable)? {
            return Ok(false);
        }
        launch_detached(executable, "about:blank").map_err(|error| {
            BackendError::new(
                "WEB_BROWSER_START_FAILED",
                format!("启动指定浏览器 {} 失败: {error}", self.id),
            )
            .with_data(json!({"browser":self.id,"exePath":executable,"applied":false}))
            .with_hint("检查浏览器程序路径和运行权限；不会启动其他浏览器")
            .with_retryable(false)
        })?;
        Ok(true)
    }
}

fn launch_command_line(executable: &Path, arguments: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    std::iter::once(b'"' as u16)
        .chain(executable.as_os_str().encode_wide())
        .chain("\" ".encode_utf16())
        .chain(arguments.encode_utf16())
        .chain(Some(0))
        .collect()
}

fn launch_detached(executable: &Path, arguments: &str) -> windows::core::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION, STARTUPINFOW,
    };

    let application: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut command_line = launch_command_line(executable, arguments);
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // Do not pass CLI/PowerShell capture handles to a long-lived browser.
    unsafe {
        CreateProcessW(
            PCWSTR(application.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            DETACHED_PROCESS,
            None,
            PCWSTR::null(),
            &startup,
            &mut process,
        )?;
        let _ = CloseHandle(process.hThread);
        let _ = CloseHandle(process.hProcess);
    }
    Ok(())
}

fn executable_is_running(executable: &Path) -> Res<bool> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    let expected = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.into());
    let expected_name = executable.file_name().unwrap_or_default().to_string_lossy();
    let mut running = false;
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|error| {
            BackendError::new(
                "WEB_BROWSER_PROCESS_QUERY_FAILED",
                format!("检查浏览器进程失败: {error}"),
            )
        })?;
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
            if name.eq_ignore_ascii_case(&expected_name) {
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
                        let actual =
                            PathBuf::from(String::from_utf16_lossy(&buffer[..size as usize]));
                        let actual = actual.canonicalize().unwrap_or(actual);
                        running = actual
                            .to_string_lossy()
                            .eq_ignore_ascii_case(&expected.to_string_lossy());
                    }
                    let _ = CloseHandle(process);
                }
            }
            if running {
                break;
            }
            valid = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    Ok(running)
}

struct HelperFiles(PathBuf);

impl Drop for HelperFiles {
    fn drop(&mut self) {
        for name in ["BrowserQuery.dll", "BrowserQuery.runtimeconfig.json"] {
            let _ = fs::remove_file(self.0.join(name));
        }
        let _ = fs::remove_dir(&self.0);
    }
}

pub fn inventory() -> Res<Vec<BrowserInfo>> {
    let backend = crate::installation::shell();
    let directory = PathBuf::from(&backend.path)
        .parent()
        .ok_or_else(|| BackendError::new("BROWSER_DLL_MISSING", "无法定位影刀安装目录"))?
        .to_owned();
    for name in [
        "ShadowBot.Common.dll",
        "ShadowBot.Runtime.dll",
        "ShadowBot.UIAutomation.dll",
    ] {
        if !directory.join(name).is_file() {
            return Err(BackendError::new(
                "BROWSER_DLL_MISSING",
                format!("影刀安装目录缺少 {name}"),
            )
            .with_data(backend.evidence())
            .with_hint(
                "检查影刀安装；可用 XBOT_SHELL_CLI 指向实际版本目录中的 shadowbot.shell-cli.exe",
            ));
        }
    }
    let runtime = directory
        .parent()
        .unwrap_or(&directory)
        .join(r"dotnet.win-x64\dotnet.exe");
    if !runtime.is_file() {
        return Err(
            BackendError::new("BROWSER_RUNTIME_MISSING", "未找到影刀自带的 .NET 运行时")
                .with_data(json!({"path":runtime,"installation":backend.evidence()}))
                .with_hint(
                    "请检查影刀安装完整性；本适配器需要影刀的 .NET 6 Windows Desktop 运行时",
                ),
        );
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let helper_directory = std::env::temp_dir().join(format!(
        "xbot-cli-browser-query-{}-{nonce}",
        std::process::id()
    ));
    let io_error = |error: std::io::Error| {
        BackendError::new(
            "BROWSER_QUERY_IO",
            format!("无法准备浏览器查询适配器: {error}"),
        )
    };
    fs::create_dir(&helper_directory).map_err(io_error)?;
    let files = HelperFiles(helper_directory);
    fs::write(files.0.join("BrowserQuery.dll"), HELPER).map_err(io_error)?;
    fs::write(files.0.join("BrowserQuery.runtimeconfig.json"), CONFIG).map_err(io_error)?;

    let mut command = Command::new(runtime);
    command
        .arg(files.0.join("BrowserQuery.dll"))
        .arg(&directory);
    let output = core::run_capture(command, 20)?;
    let response: Value = serde_json::from_str(output.stdout.trim()).map_err(|_| {
        BackendError::new("BROWSER_QUERY_FAILED", "影刀 DLL 查询未返回有效 JSON")
            .with_data(json!({"stderr":output.stderr,"stdout":output.stdout}))
    })?;
    if !output.status.success() || response["ok"] != true {
        return Err(BackendError::new("BROWSER_QUERY_FAILED", "影刀 DLL 查询失败")
            .with_data(json!({"response":response,"stderr":output.stderr,"exit_code":output.status.code()}))
            .with_hint("这是已安装 DLL 的公开成员适配器；升级影刀后可能需要更新适配器，不会回退为浏览器配置扫描"));
    }
    serde_json::from_value(response["data"]["browsers"].clone()).map_err(|error| {
        BackendError::new(
            "BROWSER_QUERY_FAILED",
            format!("浏览器列表结构无效: {error}"),
        )
    })
}

pub fn list(all: bool) -> Res<CmdOut> {
    let browsers = listed(inventory()?, all);
    Ok((
        json!({
            "command":"web.browser.list",
            "backend":"shadowbot-dll",
            "browsers":browsers,
            "count":browsers.len(),
            "all":all,
            "note":"installed 为 DLL 判定；registered 仅表示自定义浏览器已注册，不等于插件已启用或已连接"
        }),
        vec![],
    ))
}

fn listed(browsers: Vec<BrowserInfo>, all: bool) -> Vec<BrowserInfo> {
    browsers
        .into_iter()
        .filter(|browser| all || browser.installed == Some(true) || browser.registered)
        .filter(|browser| {
            browser
                .details
                .get("exePath")
                .and_then(Value::as_str)
                .is_some_and(|path| !path.trim().is_empty())
        })
        .collect()
}

pub fn normalize_id(id: &str) -> Res<String> {
    let id = id.trim().to_lowercase();
    if id.is_empty() || id.chars().any(char::is_control) {
        return Err(BackendError::new(
            "BAD_INPUT",
            "浏览器 ID 不能为空或包含控制字符",
        ));
    }
    Ok(match id.as_str() {
        "360" => "bro360".into(),
        "msedge" => "edge".into(),
        _ => id,
    })
}

/// Adds an assertion to this request, never to process-global environment.
pub fn select_argument(arguments: &mut Value, browser: Option<&str>) -> Res<()> {
    let Some(browser) = browser else {
        return Ok(());
    };
    let browser = normalize_id(browser)?;
    for field in ["browser", "mode"] {
        if let Some(existing) = arguments.get(field).and_then(Value::as_str) {
            if normalize_id(existing)? != browser {
                return Err(BackendError::new(
                    "BAD_INPUT",
                    format!("--browser 与 {field} 指定的浏览器不一致"),
                ));
            }
        }
    }
    arguments["browser"] = json!(browser);
    Ok(())
}

pub fn target(id: &str) -> Res<BrowserTarget> {
    let id = normalize_id(id)?;
    if id == "cef" {
        return Ok(BrowserTarget {
            id,
            pipe_prefix: "b99c93f4-6881-4d74-af43-2f3463fe1cc5".into(),
            exe_path: None,
        });
    }
    target_from(&id, inventory()?)
}

fn target_from(id: &str, browsers: Vec<BrowserInfo>) -> Res<BrowserTarget> {
    let matches: Vec<_> = browsers
        .iter()
        .filter(|b| b.id.eq_ignore_ascii_case(id))
        .collect();
    if matches.len() != 1 {
        return Err(BackendError::new(
            if matches.is_empty() {
                "BAD_INPUT"
            } else {
                "WEB_BROWSER_AMBIGUOUS"
            },
            format!("浏览器 ID 不存在或有重复注册: {id}"),
        )
        .with_hint("运行 xbot-cli web browser list --all，使用唯一的 id"));
    }
    let browser = matches[0];
    if browser.installed != Some(true) && !browser.registered {
        return Err(
            BackendError::new("WEB_BROWSER_NOT_INSTALLED", format!("{id} 未安装影刀插件"))
                .with_hint("请在目标浏览器安装影刀插件后再操作"),
        );
    }
    let prefix = browser
        .pipe_name
        .as_deref()
        .filter(|p| !p.trim().is_empty());
    if !browser.web_supported || prefix.is_none() {
        return Err(BackendError::new(
            "WEB_BROWSER_UNSUPPORTED",
            format!("当前 Web 命名管道通道不支持 {id}"),
        )
        .with_hint("插件已安装不代表当前 Web 通道支持该浏览器；不会改用其他浏览器"));
    }
    Ok(BrowserTarget {
        id: browser.id.to_lowercase(),
        pipe_prefix: prefix.unwrap().into(),
        exe_path: browser
            .details
            .get("exePath")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from),
    })
}

pub fn matches_pipe(path: &str, prefix: &str) -> bool {
    let name = path.strip_prefix(r"\\.\pipe\").unwrap_or(path);
    name.eq_ignore_ascii_case(prefix)
        || name
            .get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
            && name.as_bytes().get(prefix.len()) == Some(&b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser(
        id: &str,
        installed: Option<bool>,
        registered: bool,
        prefix: Option<&str>,
    ) -> BrowserInfo {
        let mut details = Map::new();
        details.insert("exePath".into(), json!(format!(r"C:\Browsers\{id}.exe")));
        BrowserInfo {
            id: id.into(),
            name: id.into(),
            installed,
            registered,
            pipe_name: prefix.map(str::to_owned),
            web_supported: prefix.is_some(),
            details,
        }
    }

    #[test]
    fn installed_and_registered_are_distinct_and_uninstalled_are_opt_in() {
        let rows = vec![
            browser("chrome", Some(true), false, Some("c")),
            browser("firefox", Some(false), false, None),
            browser("custom", None, true, Some("p")),
        ];
        assert_eq!(listed(rows.clone(), false).len(), 2);
        assert_eq!(listed(rows, true).len(), 3);
        assert_eq!(
            serde_json::to_value(browser("custom", None, true, Some("p"))).unwrap()["installed"],
            Value::Null
        );
    }

    #[test]
    fn list_excludes_missing_or_blank_executable_paths_even_with_all() {
        for path in [
            None,
            Some(Value::Null),
            Some(json!("")),
            Some(json!(" \t\r\n ")),
            Some(json!(42)),
        ] {
            let mut rows = vec![
                browser("installed", Some(true), false, Some("i")),
                browser("registered", None, true, Some("r")),
                browser("uninstalled", Some(false), false, None),
            ];
            for row in &mut rows {
                row.details.remove("exePath");
                if let Some(path) = &path {
                    row.details.insert("exePath".into(), path.clone());
                }
            }
            rows.push(browser("bro360", Some(true), false, Some("b")));
            for all in [false, true] {
                let result = listed(rows.clone(), all);
                assert_eq!(
                    result.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
                    ["bro360"],
                    "exePath={path:?}, all={all}"
                );
            }
        }
    }

    #[test]
    fn selectors_fail_closed_and_preserve_dll_prefixes() {
        assert_eq!(normalize_id(" 360 ").unwrap(), "bro360");
        assert!(normalize_id(" ").is_err());
        let entry = browser("custom", None, true, Some("unique-guid"));
        assert_eq!(
            target_from("custom", vec![entry.clone()])
                .unwrap()
                .pipe_prefix,
            "unique-guid"
        );
        assert!(target_from("custom", vec![entry.clone(), entry]).is_err());
        assert!(target_from("unknown", vec![]).is_err());
        assert_eq!(
            target_from("firefox", vec![browser("firefox", Some(true), false, None)])
                .unwrap_err()
                .code,
            "WEB_BROWSER_UNSUPPORTED"
        );
        assert!(matches_pipe(
            r"\\.\pipe\unique-guid-user-profile",
            "unique-guid"
        ));
        assert!(!matches_pipe("different-guid-other", "unique-guid"));
        assert!(!matches_pipe("unique-guid2-user", "unique-guid"));
    }

    #[test]
    fn request_selection_rejects_conflicting_modes_and_raw_arguments() {
        let mut args = json!({"mode":"edge"});
        assert!(select_argument(&mut args, Some("chrome")).is_err());
        select_argument(&mut args, Some("msedge")).unwrap();
        assert_eq!(args["browser"], "edge");
        assert!(select_argument(&mut args, Some("chrome")).is_err());
    }

    #[test]
    fn launching_uses_only_the_selected_executable_and_a_blank_page() {
        let target = target_from(
            "custom",
            vec![browser("custom", None, true, Some("custom-pipe"))],
        )
        .unwrap();
        let executable = target.exe_path.as_deref().unwrap();
        assert_eq!(executable, Path::new(r"C:\Browsers\custom.exe"));
        assert_eq!(
            String::from_utf16_lossy(&launch_command_line(executable, "about:blank")),
            "\"C:\\Browsers\\custom.exe\" about:blank\0"
        );
        assert_eq!(
            String::from_utf16_lossy(&launch_command_line(
                Path::new(r"C:\Program Files\浏览器\chrome.exe"),
                "about:blank"
            )),
            "\"C:\\Program Files\\浏览器\\chrome.exe\" about:blank\0"
        );
    }

    #[test]
    fn startup_rejects_missing_or_relative_executables() {
        for path in [
            None,
            Some(PathBuf::from("chrome.exe")),
            Some(std::env::temp_dir().join("__xbot_missing_browser_executable__.exe")),
        ] {
            let target = BrowserTarget {
                id: "chrome".into(),
                pipe_prefix: "chrome-prefix".into(),
                exe_path: path,
            };
            assert_eq!(
                target.start_if_needed().unwrap_err().code,
                "WEB_BROWSER_EXECUTABLE_MISSING"
            );
        }
    }

    #[test]
    fn startup_does_not_spawn_an_already_running_executable() {
        let target = BrowserTarget {
            id: "current-test-process".into(),
            pipe_prefix: "test-prefix".into(),
            exe_path: Some(std::env::current_exe().unwrap()),
        };
        assert!(executable_is_running(target.exe_path.as_deref().unwrap()).unwrap());
        assert!(!target.start_if_needed().unwrap());
    }

    #[test]
    fn detached_browser_does_not_hold_cli_output_open() {
        let started = std::time::Instant::now();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "browser_cmds::tests::detached_parent_helper",
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "CLI output was held open by the detached child"
        );
    }

    #[test]
    #[ignore = "Only launched as a child process by the detached-output regression test"]
    fn detached_parent_helper() {
        launch_detached(
            &std::env::current_exe().unwrap(),
            "--ignored --exact browser_cmds::tests::detached_child_helper",
        )
        .unwrap();
    }

    #[test]
    #[ignore = "Only launched by detached_parent_helper"]
    fn detached_child_helper() {
        std::thread::sleep(std::time::Duration::from_secs(6));
    }
}
