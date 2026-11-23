use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Pipes::PeekNamedPipe;

use crate::core::{BackendError, Res};

pub(crate) const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

pub(crate) struct NamedPipe {
    pub(crate) file: std::fs::File,
    pub(crate) inbuf: Vec<u8>,
    pub(crate) deadline: Instant,
}

impl NamedPipe {
    pub(crate) fn connect(
        path: &str,
        connect_timeout: Duration,
        operation_timeout: Duration,
    ) -> Res<Self> {
        let started = Instant::now();
        let deadline = started + operation_timeout;
        loop {
            match OpenOptions::new().read(true).write(true).open(path) {
                Ok(file) => {
                    return Ok(Self {
                        file,
                        inbuf: Vec::new(),
                        deadline,
                    });
                }
                Err(_) if started.elapsed() < connect_timeout => thread::sleep(
                    Duration::from_millis(250)
                        .min(connect_timeout.saturating_sub(started.elapsed())),
                ),
                Err(error) => {
                    return Err(BackendError::new(
                        "WEB_BRIDGE_UNAVAILABLE",
                        format!("连接影刀 Chrome Bridge 命名管道失败: {path}: {error}"),
                    )
                    .with_hint("先打开带影刀扩展的 Chrome、Edge 或影刀浏览器"));
                }
            }
        }
    }

    pub(crate) fn send(&mut self, method: &str, params: Value) -> Res<Value> {
        self.write_frame(&json!({"method":method,"options":{},"params":params}))?;
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            let message = self.read_frame(remaining.min(Duration::from_secs(30)))?;
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                match method {
                    "BridgeGetInterfaceVersion" => {
                        self.write_frame(&json!({"method":"Bridge.GetInterfaceVersion","params":{"version":"3.1.1"}}))?;
                    }
                    "BridgeRegisterWindowId" => {
                        static NEXT_HWND: AtomicI64 = AtomicI64::new(1000);
                        let window = message.get("params").cloned().unwrap_or_else(|| json!({}));
                        self.write_frame(&json!({
                            "method":"Bridge.Callback",
                            "requestId":message.get("requestId"),
                            "params":{"wndId":window.get("wndId"),"hwnd":NEXT_HWND.fetch_add(1, Ordering::Relaxed)}
                        }))?;
                    }
                    _ => {}
                }
                continue;
            }
            return response_content(message);
        }
    }

    fn write_frame(&mut self, value: &Value) -> Res<()> {
        let frame = encode_frame(value)?;
        self.file.write_all(&frame).map_err(|error| {
            BackendError::new(
                "WEB_BRIDGE_WRITE_FAILED",
                format!("写入 Chrome Bridge 失败: {error}"),
            )
        })?;
        self.file.flush().map_err(|error| {
            BackendError::new(
                "WEB_BRIDGE_WRITE_FAILED",
                format!("刷新 Chrome Bridge 失败: {error}"),
            )
        })
    }

    fn read_frame(&mut self, timeout: Duration) -> Res<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.inbuf.len() >= 4 {
                let length = u32::from_le_bytes([
                    self.inbuf[0],
                    self.inbuf[1],
                    self.inbuf[2],
                    self.inbuf[3],
                ]) as usize;
                if length > MAX_FRAME_SIZE {
                    return Err(BackendError::new(
                        "WEB_BRIDGE_BAD_RESPONSE",
                        format!("Chrome Bridge 响应帧过大: {length}"),
                    ));
                }
                if self.inbuf.len() >= 4 + length {
                    let payload = self.inbuf[4..4 + length].to_vec();
                    self.inbuf.drain(..4 + length);
                    return decode_payload(&payload);
                }
            }
            if Instant::now() >= deadline {
                return Err(
                    BackendError::new("WEB_BRIDGE_TIMEOUT", "读取 Chrome Bridge 响应超时")
                        .with_hint("操作结果可能未知；先检查页面，不要自动重试"),
                );
            }
            if self.peek_available()? == 0 {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            let mut chunk = [0_u8; 8192];
            let count = self.file.read(&mut chunk).map_err(|error| {
                BackendError::new(
                    "WEB_BRIDGE_READ_FAILED",
                    format!("读取 Chrome Bridge 失败: {error}"),
                )
            })?;
            if count == 0 {
                return Err(BackendError::new(
                    "WEB_BRIDGE_READ_FAILED",
                    "Chrome Bridge 管道已关闭",
                ));
            }
            self.inbuf.extend_from_slice(&chunk[..count]);
        }
    }

    fn peek_available(&self) -> Res<u32> {
        let mut available = 0_u32;
        let handle = HANDLE(std::os::windows::io::AsRawHandle::as_raw_handle(&self.file));
        unsafe { PeekNamedPipe(handle, None, 0, None, Some(&mut available), None) }.map_err(
            |error| {
                BackendError::new(
                    "WEB_BRIDGE_READ_FAILED",
                    format!("检查 Chrome Bridge 响应失败: {error}"),
                )
            },
        )?;
        Ok(available)
    }
}

pub(crate) fn response_content(message: Value) -> Res<Value> {
    if let Some(error) = message
        .pointer("/result/error")
        .filter(|error| !error.is_null())
    {
        let detail = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string());
        return Err(BackendError::new("WEB_BRIDGE_FAILED", detail).with_data(message));
    }
    if message
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 200)
    {
        return Err(
            BackendError::new("WEB_BRIDGE_FAILED", "Chrome Bridge 返回失败").with_data(message),
        );
    }
    Ok(message
        .pointer("/result/content")
        .cloned()
        .unwrap_or(message))
}

pub(crate) fn encode_frame(value: &Value) -> Res<Vec<u8>> {
    let payload = serde_json::to_vec(value).map_err(|error| {
        BackendError::new(
            "WEB_BRIDGE_BAD_REQUEST",
            format!("序列化 Bridge 请求失败: {error}"),
        )
    })?;
    if payload.len() > MAX_FRAME_SIZE {
        return Err(BackendError::new(
            "WEB_BRIDGE_BAD_REQUEST",
            "Chrome Bridge 请求帧过大",
        ));
    }
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub(crate) fn decode_payload(payload: &[u8]) -> Res<Value> {
    serde_json::from_slice(payload).map_err(|error| {
        BackendError::new(
            "WEB_BRIDGE_BAD_RESPONSE",
            format!("Chrome Bridge 响应不是有效 JSON: {error}"),
        )
        .with_data(json!({"text":String::from_utf8_lossy(payload)}))
    })
}
