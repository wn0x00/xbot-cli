//! 自包含 payload:XbotShim.dll + XbotInjectee.dll + runtimeconfig 编译期嵌入。
//! 首次使用释放到按内容寻址的缓存目录(版本互不覆盖),返回 shim 路径供注入。

use std::path::{Path, PathBuf};

use crate::core::{BackendError, Res};

const SHIM: &[u8] = include_bytes!("../../payload/deploy/XbotShim.dll");
const INJECTEE: &[u8] = include_bytes!("../../payload/deploy/XbotInjectee.dll");
const RUNTIMECONFIG: &[u8] = include_bytes!("../../payload/deploy/XbotInjectee.runtimeconfig.json");

const PAYLOAD_HASH: &str = env!("XBOT_PAYLOAD_HASH");
const IDENTITY: &str = include_str!("../../payload/deploy/XbotInjectee.identity");
pub fn build_id() -> &'static str {
    IDENTITY.trim()
}

pub fn ensure_extracted() -> Res<PathBuf> {
    let cache = cache_dir()?;
    std::fs::create_dir_all(&cache)
        .map_err(|e| BackendError::new("INPROC_EXTRACT", format!("mkdir {}: {e}", cache.display())))?;
    // A unique native-module path retriggers DllMain even when returning to an older managed build.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| BackendError::new("INPROC_EXTRACT", format!("clock: {e}")))?
        .as_nanos();
    let shim = cache.join(format!(
        "XbotShim-bootstrap-{}-{nonce}.dll",
        std::process::id()
    ));
    write_if_changed(&shim, SHIM)?;
    write_if_changed(&cache.join("XbotInjectee.dll"), INJECTEE)?;
    write_if_changed(&cache.join(format!("{}.dll", build_id())), INJECTEE)?;
    write_if_changed(&cache.join("XbotInjectee.identity"), IDENTITY.as_bytes())?;
    write_if_changed(&cache.join("XbotInjectee.runtimeconfig.json"), RUNTIMECONFIG)?;
    Ok(shim)
}

fn write_if_changed(path: &Path, expected: &[u8]) -> Res<()> {
    if let Ok(existing) = std::fs::read(path) {
        if existing == expected {
            return Ok(());
        }
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, expected)
        .map_err(|e| BackendError::new("INPROC_EXTRACT", format!("write {}: {e}", tmp.display())))?;
    match std::fs::rename(&tmp, path) {
        Ok(_) => Ok(()),
        Err(_) => {
            // File may be in use by an existing Shell process — if existing
            // bytes already match what we want, that's fine.
            if let Ok(existing) = std::fs::read(path) {
                if existing == expected {
                    let _ = std::fs::remove_file(&tmp);
                    return Ok(());
                }
            }
            Err(BackendError::new(
                "INPROC_EXTRACT",
                format!("could not replace {} (loaded by another process)", path.display()),
            ))
        }
    }
}

fn cache_dir() -> Res<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| BackendError::new("INPROC_EXTRACT", "LOCALAPPDATA not set"))?;
    Ok(PathBuf::from(base)
        .join("guanzhu-xbot")
        .join(format!("payload-{PAYLOAD_HASH}")))
}
