//! Experimental UIA observation through an isolated, bounded Python worker.
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::app_cmds::CmdOut;
use crate::cli::Win32Sub;
use crate::core::{BackendError, Res};

const SNAPSHOT_WORKER: &str = include_str!("../skills/xbot-app-dev/scripts/win32/snapshot.py");
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;

pub(crate) fn parse_hwnd(value: &str) -> Res<u64> {
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16)
    } else {
        value.parse::<u64>()
    };
    parsed
        .ok()
        .filter(|hwnd| *hwnd > 0 && *hwnd <= isize::MAX as u64)
        .ok_or_else(|| {
            BackendError::new("BAD_INPUT", "--hwnd 必须是非零十进制或 0x 十六进制窗口句柄")
        })
}

pub fn execute(sub: Win32Sub) -> Res<CmdOut> {
    let Win32Sub::Snapshot {
        hwnd,
        pid,
        max_nodes,
        max_depth,
        timeout,
    } = sub;
    let hwnd = parse_hwnd(&hwnd)?;
    if !cfg!(windows) {
        return Err(BackendError::new(
            "WIN32_UNSUPPORTED_PLATFORM",
            "win32 snapshot 仅支持 Windows",
        ));
    }
    let python = std::env::var_os("XBOT_UIA_PYTHON").unwrap_or_else(|| "python".into());
    let mut command = Command::new(python);
    command
        .args(["-I", "-X", "utf8", "-c", SNAPSHOT_WORKER])
        .args(["--hwnd", &format!("0x{hwnd:x}"), "--pid", &pid.to_string()])
        .args([
            "--max-nodes",
            &max_nodes.to_string(),
            "--max-depth",
            &max_depth.to_string(),
        ])
        .args(["--timeout", &timeout.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let started = Instant::now();
    let mut child = command.spawn().map_err(|_| {
        BackendError::new("WIN32_DEPENDENCY_MISSING", "无法启动 UIA Python 提供器").with_hint(
            "设置 XBOT_UIA_PYTHON 为已安装 uiautomation 的 Python 路径；不会自动安装依赖",
        )
    })?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out_reader = thread::spawn(move || {
        let mut data = Vec::new();
        stdout
            .by_ref()
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut data)
            .map(|_| data)
    });
    let err_reader = thread::spawn(move || {
        // Drain diagnostics without retaining potentially private desktop text.
        std::io::copy(&mut stderr, &mut std::io::sink())
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < Duration::from_secs(timeout) => {
                thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(
                    BackendError::new("TIMEOUT", "UIA 快照达到时间上限，未得到可用快照")
                        .with_retryable(false),
                );
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(BackendError::new(
                    "WIN32_PROVIDER_ERROR",
                    "UIA 提供器状态读取失败",
                ));
            }
        }
    };
    let output = out_reader.join();
    let _ = err_reader.join();
    let status = status?;
    let output = output
        .map_err(|_| BackendError::new("WIN32_PROVIDER_ERROR", "UIA 输出读取线程失败"))?
        .map_err(|_| BackendError::new("WIN32_PROVIDER_ERROR", "UIA 输出读取失败"))?;
    if output.len() as u64 > OUTPUT_LIMIT {
        return Err(BackendError::new(
            "WIN32_PROVIDER_ERROR",
            "UIA 输出超出预算",
        ));
    }
    let payload: Value = serde_json::from_slice(&output)
        .map_err(|_| BackendError::new("WIN32_PROVIDER_ERROR", "UIA 提供器未返回有效 JSON"))?;
    if payload["ok"] != true || !status.success() {
        let code = payload["code"].as_str().unwrap_or("WIN32_PROVIDER_ERROR");
        return Err(BackendError::new(code, "UIA 快照失败，未执行界面动作")
            .with_hint("检查 HWND/PID、依赖、权限和目标窗口状态；不要据此修改业务定位"));
    }
    let data = payload
        .get("data")
        .filter(|data| data.is_object())
        .ok_or_else(|| BackendError::new("WIN32_PROVIDER_ERROR", "UIA 提供器缺少快照对象"))?;
    Ok((data.clone(), vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_accept_decimal_and_hex_and_reject_invalid_values() {
        assert_eq!(parse_hwnd("123").unwrap(), 123);
        assert_eq!(parse_hwnd("0X7b").unwrap(), 123);
        for invalid in ["0", "-1", "0x0", "xyz", "1;whoami", "18446744073709551616"] {
            assert!(parse_hwnd(invalid).is_err(), "{invalid}");
        }
    }
}
