//! Explicit snapshot IDs bind refs to a saved process instance and UIA ancestry.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::core::{self, BackendError, Res};

pub(crate) struct ActionLock(windows::Win32::Foundation::HANDLE);

impl ActionLock {
    pub(crate) fn acquire(snapshot: &Value) -> Res<Self> {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
        let pid = snapshot["scope"]["process_id"]
            .as_u64()
            .ok_or_else(|| BackendError::new("WIN32_SNAPSHOT_INVALID", "快照缺少进程"))?;
        let instance = snapshot["process_instance"]
            .as_str()
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or_else(|| BackendError::new("WIN32_SNAPSHOT_INVALID", "快照进程实例无效"))?;
        unsafe {
            let handle = CreateMutexW(
                None,
                false,
                &HSTRING::from(format!("Local\\xbot-cli-win32-action-{pid}-{instance}")),
            )
            .map_err(|_| BackendError::new("WIN32_ACTION_BUSY", "无法锁定目标进程"))?;
            match WaitForSingleObject(handle, 0) {
                WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
                _ => {
                    let _ = CloseHandle(handle);
                    Err(
                        BackendError::new("WIN32_ACTION_BUSY", "同一目标进程已有动作正在执行")
                            .with_retryable(false),
                    )
                }
            }
        }
    }
}

impl Drop for ActionLock {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Threading::ReleaseMutex(self.0);
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub(crate) fn validate_id(id: &str) -> Res<()> {
    if !core::is_uuid(id) {
        return Err(BackendError::new(
            "BAD_INPUT",
            "--snapshot 必须是 snapshot 返回的 UUID",
        ));
    }
    Ok(())
}

fn directory() -> Res<PathBuf> {
    if let Some(path) = std::env::var_os("XBOT_WIN32_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("xbot-cli").join("win32-snapshots"))
        .ok_or_else(|| BackendError::new("WIN32_SNAPSHOT_STORE", "无法定位本地快照目录"))
}

pub(crate) fn path(id: &str) -> Res<PathBuf> {
    validate_id(id)?;
    Ok(directory()?.join(format!("{}.json", id.to_ascii_lowercase())))
}

pub(crate) fn receipt(id: &str) -> Res<PathBuf> {
    Ok(path(id)?.with_extension("used"))
}

const MAX_SNAPSHOT_FILES: usize = 64;
const MAX_SNAPSHOT_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

fn remove_snapshot(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("used"));
}

fn prune_at(directory: &Path, now: std::time::SystemTime) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut snapshots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let extension = path.extension().and_then(|value| value.to_str());
        if extension != Some("json") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        if now
            .duration_since(modified)
            .is_ok_and(|age| age.as_secs() > 24 * 3600)
        {
            remove_snapshot(&path);
            continue;
        }
        snapshots.push((path, modified, metadata.len()));
    }
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|value| value == "used")
                && !path.with_extension("json").is_file()
            {
                let _ = fs::remove_file(path);
            }
        }
    }
    snapshots.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let mut total_bytes = snapshots.iter().map(|item| item.2).sum::<u64>();
    while snapshots.len() > MAX_SNAPSHOT_FILES || total_bytes > MAX_SNAPSHOT_TOTAL_BYTES {
        let (path, _, size) = &snapshots[0];
        remove_snapshot(path);
        total_bytes = total_bytes.saturating_sub(*size);
        snapshots.remove(0);
    }
}

pub(crate) fn save(data: &mut Value) -> Res<()> {
    let id = data["snapshot_id"]
        .as_str()
        .ok_or_else(|| BackendError::new("WIN32_SNAPSHOT_STORE", "快照缺少编号"))?;
    let path = path(id)?;
    let parent = path.parent().expect("snapshot parent");
    fs::create_dir_all(parent)
        .map_err(|_| BackendError::new("WIN32_SNAPSHOT_STORE", "无法创建本地快照目录"))?;
    prune_at(parent, std::time::SystemTime::now());
    data["ref_scope"] = json!("snapshot_scoped_actions");
    data["refs_resolvable_for_actions"] = json!(true);
    data["snapshot_file"] = json!(path);
    let bytes = serde_json::to_vec(data).expect("serialize snapshot");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| BackendError::new("WIN32_SNAPSHOT_STORE", "无法保存新的快照文件"))?;
    file.write_all(&bytes)
        .map_err(|_| BackendError::new("WIN32_SNAPSHOT_STORE", "快照文件写入失败"))?;
    Ok(())
}

pub(crate) fn load(id: &str) -> Res<Value> {
    let path = path(id)?;
    let metadata = fs::metadata(&path).map_err(|_| {
        BackendError::new(
            "WIN32_SNAPSHOT_MISSING",
            "未找到快照，请先运行 win32 snapshot",
        )
    })?;
    if metadata.len() > 16 * 1024 * 1024 {
        return Err(BackendError::new(
            "WIN32_SNAPSHOT_INVALID",
            "快照文件超出预算",
        ));
    }
    if metadata
        .modified()
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|age| age.as_secs() > 24 * 3600)
    {
        return Err(BackendError::new(
            "WIN32_SNAPSHOT_EXPIRED",
            "快照已超过 24 小时，请重新采集",
        ));
    }
    let bytes =
        fs::read(path).map_err(|_| BackendError::new("WIN32_SNAPSHOT_STORE", "无法读取快照"))?;
    let data: Value = serde_json::from_slice(&bytes)
        .map_err(|_| BackendError::new("WIN32_SNAPSHOT_INVALID", "快照 JSON 无效"))?;
    if !data["snapshot_id"]
        .as_str()
        .is_some_and(|value| value.eq_ignore_ascii_case(id))
        || data["backend"] != "uiautomation"
        || data["refs_resolvable_for_actions"] != true
        || !data["process_instance"].is_string()
    {
        return Err(BackendError::new(
            "WIN32_SNAPSHOT_INVALID",
            "快照不支持动作，请用当前 CLI 重新采集",
        ));
    }
    if receipt(id)?.exists() {
        return Err(BackendError::new(
            "WIN32_SNAPSHOT_USED",
            "该快照已执行过动作，请使用动作返回的新快照或重新采集",
        )
        .with_retryable(false));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_store_prunes_expired_or_excess_files_only() {
        let root = std::env::temp_dir().join(format!(
            "xbot-snapshot-prune-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        for index in 0..70 {
            let path = root.join(format!("{index:02}.json"));
            fs::write(&path, b"{}").unwrap();
            if index % 2 == 0 {
                fs::write(path.with_extension("used"), b"").unwrap();
            }
        }
        fs::write(root.join("unrelated.txt"), b"keep").unwrap();
        prune_at(&root, std::time::SystemTime::now());
        assert_eq!(
            fs::read_dir(&root)
                .unwrap()
                .flatten()
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "json"))
                .count(),
            MAX_SNAPSHOT_FILES
        );
        assert!(root.join("unrelated.txt").is_file());
        for entry in fs::read_dir(&root).unwrap().flatten() {
            if entry
                .path()
                .extension()
                .is_some_and(|value| value == "used")
            {
                assert!(entry.path().with_extension("json").is_file());
            }
        }
        prune_at(
            &root,
            std::time::SystemTime::now() + std::time::Duration::from_secs(25 * 3600),
        );
        assert_eq!(
            fs::read_dir(&root)
                .unwrap()
                .flatten()
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "json"))
                .count(),
            0
        );
        assert!(root.join("unrelated.txt").is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
