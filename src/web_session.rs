//! Shell binding. A ref action never silently changes browser tabs.
use crate::core::{BackendError, Res};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use windows::Win32::{
    Foundation::{CloseHandle, FILETIME},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

#[derive(Serialize, Deserialize)]
pub struct Binding {
    pub pipe: String,
    pub tab_id: i64,
    pub document_id: Option<String>,
    pub updated: u64,
}

fn explicit() -> bool {
    ["XBOT_WEB_PIPE", "XBOT_WEB_TAB_ID"]
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|v| !v.trim().is_empty()))
}

fn binding_parent(current: u32, parents: &HashMap<u32, u32>, launcher: Option<u32>) -> Option<u32> {
    let parent = *parents.get(&current)?;
    if launcher == Some(parent) {
        parents.get(&parent).copied()
    } else {
        Some(parent)
    }
}

fn shell_identity() -> Option<String> {
    // The npm launcher is short-lived; bind to its caller, not its Node PID.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut parents = HashMap::new();
        let mut found = Process32FirstW(snapshot, &mut entry).is_ok();
        while found {
            parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            found = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        let launcher = std::env::var("XBOT_CLI_LAUNCHER_PID")
            .ok()
            .and_then(|v| v.parse::<u32>().ok());
        let pid = binding_parent(std::process::id(), &parents, launcher)?;
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user);
        let _ = CloseHandle(process);
        result.ok()?;
        Some(format!(
            "p{pid}-{}-{}",
            created.dwHighDateTime, created.dwLowDateTime
        ))
    }
}

fn path() -> Option<PathBuf> {
    if explicit() {
        return None;
    }
    let key = if let Ok(name) = std::env::var("XBOT_WEB_SESSION") {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(name)
    } else {
        shell_identity()?
    };
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("xbot-cli")
            .join("web-sessions")
            .join(format!("{key}.json")),
    )
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn load() -> Res<Option<Binding>> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let text = match fs::read(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(BackendError::new("WEB_SESSION_READ", error.to_string())),
    };
    let value: Binding = serde_json::from_slice(&text).map_err(|_| {
        BackendError::new(
            "WEB_SESSION_INVALID",
            "浏览器会话记录不完整；请先运行 web active 或 web open 重新绑定",
        )
    })?;
    if now().saturating_sub(value.updated) > 24 * 3600 {
        return Err(BackendError::new(
            "WEB_SESSION_EXPIRED",
            "浏览器会话已超过 24 小时；请运行 web active 或 web open 重新绑定，不会自动切换标签页",
        ));
    }
    Ok(Some(value))
}

pub fn save(pipe: &str, tab: i64, document: Option<&str>) -> std::io::Result<()> {
    let Some(path) = path() else {
        return Ok(());
    };
    fs::create_dir_all(path.parent().unwrap())?;
    let value = Binding {
        pipe: pipe.into(),
        tab_id: tab,
        document_id: document.map(str::to_owned),
        updated: now(),
    };
    fs::write(path, serde_json::to_vec(&value)?)
}

pub fn clear() {
    if let Some(path) = path() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_calls_keep_their_actual_parent() {
        let parents = HashMap::from([(10, 20), (20, 30)]);
        assert_eq!(binding_parent(10, &parents, None), Some(20));
        assert_eq!(binding_parent(10, &parents, Some(999)), Some(20));
        assert_eq!(binding_parent(99, &parents, None), None);
    }

    #[test]
    fn separate_npm_invocations_share_the_calling_shell_not_the_launcher() {
        let parents = HashMap::from([(10, 20), (20, 30), (11, 21), (21, 30)]);
        assert_eq!(binding_parent(10, &parents, Some(20)), Some(30));
        assert_eq!(binding_parent(11, &parents, Some(21)), Some(30));
        assert_eq!(binding_parent(10, &parents, None), Some(20));
        assert_eq!(binding_parent(11, &parents, None), Some(21));
        assert_eq!(
            binding_parent(10, &HashMap::from([(10, 20)]), Some(20)),
            None
        );
    }
}
