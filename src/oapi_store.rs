use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use base64::Engine;
use serde_json::Value;

use crate::core::{BackendError, Res};

const MAGIC: &[u8] = b"XBOT-OAPI-DPAPI1\n";
const MAX_AUTH_FILE_SIZE: u64 = 64 * 1024;

fn read_error(path: &Path, error: impl std::fmt::Display) -> BackendError {
    BackendError::new(
        "PLATFORM_CONFIG",
        format!("无法读取平台凭据文件 {}: {error}", path.display()),
    )
    .with_hint("修复文件权限或损坏内容；也可运行 oapi auth logout 后重新登录")
}

fn protect(input: &[u8]) -> Res<Vec<u8>> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input.len())
            .map_err(|_| BackendError::new("FILE_WRITE", "平台凭据内容过大"))?,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe {
        CryptProtectData(
            &input_blob,
            PCWSTR::null(),
            None,
            None,
            None,
            0,
            &mut output,
        )
    };
    let bytes = if result.is_ok() && output.cbData > 0 && !output.pbData.is_null() {
        Some(unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec())
    } else {
        None
    };
    if !output.pbData.is_null() {
        unsafe {
            LocalFree(Some(HLOCAL(output.pbData.cast())));
        }
    }
    result
        .map_err(|error| BackendError::new("FILE_WRITE", format!("无法加密平台凭据: {error}")))?;
    bytes.ok_or_else(|| BackendError::new("FILE_WRITE", "系统未返回加密后的平台凭据"))
}

fn unprotect(input: &[u8]) -> Res<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input.len())
            .map_err(|_| BackendError::new("PLATFORM_CONFIG", "平台凭据密文过大"))?,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe { CryptUnprotectData(&input_blob, None, None, None, None, 0, &mut output) };
    let bytes = if result.is_ok() && output.cbData > 0 && !output.pbData.is_null() {
        Some(unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec())
    } else {
        None
    };
    if !output.pbData.is_null() {
        unsafe {
            LocalFree(Some(HLOCAL(output.pbData.cast())));
        }
    }
    result.map_err(|error| {
        BackendError::new("PLATFORM_CONFIG", format!("无法解密平台凭据: {error}"))
            .with_hint("DPAPI 凭据只能由保存它的 Windows 用户解密；请重新登录")
    })?;
    bytes.ok_or_else(|| BackendError::new("PLATFORM_CONFIG", "系统未返回解密后的平台凭据"))
}

pub fn read(path: &Path) -> Res<Option<Value>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(read_error(path, error)),
    };
    if metadata.len() > MAX_AUTH_FILE_SIZE {
        return Err(read_error(path, "文件超过 64KB 限制"));
    }
    let bytes = fs::read(path).map_err(|error| read_error(path, error))?;
    if bytes.len() as u64 > MAX_AUTH_FILE_SIZE {
        return Err(read_error(path, "文件超过 64KB 限制"));
    }
    let plaintext = if let Some(encoded) = bytes.strip_prefix(MAGIC) {
        let encoded = std::str::from_utf8(encoded).map_err(|error| read_error(path, error))?;
        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|error| read_error(path, error))?;
        unprotect(&encrypted).map_err(|error| {
            error.with_data(serde_json::json!({"auth_file": path.display().to_string()}))
        })?
    } else {
        bytes
    };
    let value: Value =
        serde_json::from_slice(&plaintext).map_err(|error| read_error(path, error))?;
    if !value.is_object() {
        return Err(read_error(path, "顶层内容必须是 JSON 对象"));
    }
    Ok(Some(value))
}

fn replace(temp: &Path, path: &Path) -> Res<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| BackendError::new("FILE_WRITE", format!("无法替换平台凭据文件: {error}")))
}

pub fn write(path: &Path, value: &Value) -> Res<()> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|error| BackendError::new("FILE_WRITE", error.to_string()))?;
    }
    let plaintext = serde_json::to_vec_pretty(value)
        .map_err(|error| BackendError::new("FILE_WRITE", error.to_string()))?;
    let encrypted = protect(&plaintext)?;
    let mut encoded = MAGIC.to_vec();
    encoded.extend_from_slice(
        base64::engine::general_purpose::STANDARD
            .encode(encrypted)
            .as_bytes(),
    );
    encoded.push(b'\n');
    let file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("oapi-auth.json"))
        .to_string_lossy();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| BackendError::new("FILE_WRITE", error.to_string()))?;
        file.write_all(&encoded)
            .and_then(|()| file.sync_all())
            .map_err(|error| BackendError::new("FILE_WRITE", error.to_string()))?;
        replace(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub fn remove(path: &Path) -> Res<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(BackendError::new(
            "FILE_WRITE",
            format!("无法删除平台凭据文件 {}: {error}", path.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "xbot-oapi-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn encrypted_store_roundtrips_without_plaintext_secrets() {
        let path = scratch();
        let value = json!({"access_key_secret":"private-secret","access_token":"token"});
        write(&path, &value).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(MAGIC));
        assert!(!String::from_utf8_lossy(&bytes).contains("private-secret"));
        assert_eq!(read(&path).unwrap(), Some(value));
        write(&path, &json!({"access_token":"updated"})).unwrap();
        assert_eq!(
            read(&path).unwrap(),
            Some(json!({"access_token":"updated"}))
        );
        remove(&path).unwrap();
    }

    #[test]
    fn legacy_json_is_read_and_invalid_files_are_diagnostic() {
        let path = scratch();
        fs::write(&path, br#"{"access_key_id":"legacy"}"#).unwrap();
        assert_eq!(
            read(&path).unwrap(),
            Some(json!({"access_key_id":"legacy"}))
        );
        fs::write(&path, b"not json").unwrap();
        assert_eq!(read(&path).unwrap_err().code, "PLATFORM_CONFIG");
        remove(&path).unwrap();
        assert_eq!(read(&path).unwrap(), None);
        assert!(!remove(&path).unwrap());
    }
}
