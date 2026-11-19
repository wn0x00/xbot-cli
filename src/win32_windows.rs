//! Read-only discovery of top-level windows; no UIA or ShadowBot dependency.
use serde_json::Value;

use crate::app_cmds::CmdOut;
use crate::core::{BackendError, Res};

pub(crate) fn validate_title(title: Option<&str>) -> Res<()> {
    if title.is_some_and(|title| title.trim().is_empty() || title.contains('\0')) {
        return Err(BackendError::new(
            "BAD_INPUT",
            "--title 不能为空白或包含 NUL；列出全部窗口时省略该参数",
        ));
    }
    Ok(())
}

pub(crate) fn execute(
    title: Option<&str>,
    exact: bool,
    pid: Option<u32>,
    all: bool,
) -> Res<CmdOut> {
    validate_title(title)?;
    Ok((list(title, exact, pid, all)?, vec![]))
}

#[cfg(not(windows))]
fn list(_title: Option<&str>, _exact: bool, _pid: Option<u32>, _all: bool) -> Res<Value> {
    Err(BackendError::new(
        "WIN32_UNSUPPORTED_PLATFORM",
        "win32 list 仅支持 Windows",
    ))
}

#[cfg(windows)]
fn list(title: Option<&str>, exact: bool, pid: Option<u32>, all: bool) -> Res<Value> {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use serde_json::json;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsIconic, IsWindow, IsWindowVisible,
    };

    struct Search {
        title: Option<String>,
        exact: bool,
        pid: Option<u32>,
        all: bool,
        windows: Vec<Value>,
        paths: HashMap<u32, Option<PathBuf>>,
    }

    unsafe fn process_path(pid: u32) -> Option<PathBuf> {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        result
            .ok()
            .map(|()| PathBuf::from(String::from_utf16_lossy(&buffer[..size as usize])))
    }

    // EnumWindows calls back synchronously on this thread. The state pointer is
    // valid for the entire enumeration; no references cross the callback boundary.
    unsafe extern "system" fn collect(hwnd: HWND, parameter: LPARAM) -> BOOL {
        let search = &mut *(parameter.0 as *mut Search);
        let mut pid = 0;
        if GetWindowThreadProcessId(hwnd, Some(&mut pid)) == 0
            || search.pid.is_some_and(|expected| expected != pid)
        {
            return true.into();
        }
        let visible = IsWindowVisible(hwnd).as_bool();
        if !search.all && !visible {
            return true.into();
        }
        let length = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buffer = vec![0u16; length.saturating_add(512)];
        let copied = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
        let title = String::from_utf16_lossy(&buffer[..copied]);
        if !search.all && title.is_empty() {
            return true.into();
        }
        if let Some(query) = &search.title {
            let normalized = title.to_lowercase();
            if (search.exact && normalized != *query)
                || (!search.exact && !normalized.contains(query))
            {
                return true.into();
            }
        }
        let mut class = [0u16; 256];
        let class_length = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let path = search.paths.entry(pid).or_insert_with(|| process_path(pid));
        let name = path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let mut current_pid = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut current_pid));
        if !IsWindow(Some(hwnd)).as_bool() || current_pid != pid {
            return true.into();
        }
        let handle = hwnd.0 as usize as u64;
        search.windows.push(json!({
            "title":title,"pid":pid,"hwnd":handle,"hwnd_hex":format!("0x{handle:x}"),
            "process_name":name,"process_path":path,"class_name":String::from_utf16_lossy(&class[..class_length]),
            "visible":visible,"minimized":IsIconic(hwnd).as_bool(),
        }));
        true.into()
    }

    let mut search = Search {
        title: title.map(str::to_lowercase),
        exact,
        pid,
        all,
        windows: Vec::new(),
        paths: HashMap::new(),
    };
    unsafe { EnumWindows(Some(collect), LPARAM(&mut search as *mut Search as isize)) }.map_err(
        |error| BackendError::new("WIN32_WINDOW_ENUM_FAILED", format!("枚举窗口失败: {error}")),
    )?;
    search.windows.sort_by(|left, right| {
        left["title"]
            .as_str()
            .cmp(&right["title"].as_str())
            .then(left["pid"].as_u64().cmp(&right["pid"].as_u64()))
            .then(left["hwnd"].as_u64().cmp(&right["hwnd"].as_u64()))
    });
    Ok(
        json!({"command":"win32.list","query":{"title":title,"exact":exact,"pid":pid,"all":all},
        "count":search.windows.len(),"windows":search.windows}),
    )
}
