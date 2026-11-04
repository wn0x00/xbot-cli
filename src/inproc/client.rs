//! 进程内通道的 HTTP 客户端与注入编排。
//!
//! ensure_bridge 幂等:监听器健康且 buildId 与本 exe 内嵌 identity 一致 → 复用;
//! 同协议旧版本占用 → /control/stop 后重注;影刀未运行 → 硬报错(不自动拉客户端,
//! 也不回退官方 codeflow.write/edit)。

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::core::{BackendError, Res};
use crate::inproc::{embed, inject};

pub const BASE_PORT: u16 = 5057;

fn endpoint() -> String {
    std::env::var("XBOT_INPROC_ENDPOINT")
        .unwrap_or_else(|_| format!("http://127.0.0.1:{BASE_PORT}"))
}

pub fn get(path: &str, timeout_secs: u64) -> Res<Value> {
    request("GET", path, None, timeout_secs)
}

pub fn post(path: &str, body: &Value, timeout_secs: u64) -> Res<Value> {
    request("POST", path, Some(body.clone()), timeout_secs)
}

fn request(method: &str, path: &str, body: Option<Value>, timeout_secs: u64) -> Res<Value> {
    let url = format!("{}{}", endpoint(), path);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout_secs))
        .build();
    let request = match method {
        "GET" => agent.get(&url),
        _ => agent.post(&url).set("Content-Type", "application/json"),
    };
    let response = match body {
        Some(body) => request.send_json(body),
        None => request.call(),
    };
    match response {
        Ok(response) => {
            let body = response.into_string().map_err(|e| {
                BackendError::new("INPROC_READ", format!("读取 inproc 响应失败: {e}"))
            })?;
            decode(&body, &url)
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            let value: Value =
                serde_json::from_str(&body).unwrap_or_else(|_| json!({"detail": body}));
            let detail = value
                .get("detail")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("inproc 操作失败");
            Err(
                BackendError::new("INPROC_FAILED", format!("HTTP {status}: {detail}"))
                    .with_data(json!({"url": url, "status": status, "response": value})),
            )
        }
        Err(ureq::Error::Transport(error)) => Err(BackendError::new(
            "INPROC_OFFLINE",
            format!("inproc 控制面不可达: {error}"),
        )),
    }
}

fn decode(body: &str, url: &str) -> Res<Value> {
    let value: Value = serde_json::from_str(body).map_err(|e| {
        BackendError::new("INPROC_BAD_RESPONSE", format!("inproc 响应不是 JSON: {e}"))
            .with_data(json!({"url": url, "body": crate::core::truncate(body, 512)}))
    })?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let detail = value
            .get("detail")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("inproc 操作被拒绝");
        return Err(BackendError::new("INPROC_FAILED", detail).with_data(value));
    }
    Ok(value)
}

fn health() -> Res<Value> {
    get("/health", 5)
}

fn healthy() -> bool {
    matches!(health(), Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(true)
        && v.get("hasApplication").and_then(Value::as_bool) == Some(true)
        && v.get("buildId").and_then(Value::as_str) == Some(embed::build_id()))
}

pub fn ensure_bridge() -> Res<()> {
    if healthy() {
        return Ok(());
    }
    // 显式端点视为外部托管监听:只探测不注入,绝不触碰真实 ShadowBot 进程。
    if std::env::var_os("XBOT_INPROC_ENDPOINT").is_some() {
        return Err(BackendError::new(
            "INPROC_OFFLINE",
            format!("inproc 控制面不可达或 buildId 不符: {}", endpoint()),
        )
        .with_hint("确认监听已启动且与本 exe 内嵌 payload 同源;或清除 XBOT_INPROC_ENDPOINT 让 CLI 自动注入"));
    }
    if let Ok(health) = health() {
        if health.get("controlProtocol").and_then(Value::as_u64) == Some(2) {
            // Same-protocol listener with a stale/absent build — ask it to step
            // down, then reinject the embedded build.
            let _ = post("/control/stop", &json!({}), 5);
            wait_listener_down(Duration::from_secs(5))?;
        } else {
            return Err(BackendError::new(
                "INPROC_PORT_CONFLICT",
                format!("{} 被未知服务占用", endpoint()),
            )
            .with_hint("不终止未知进程;释放该端口或设置 XBOT_INPROC_ENDPOINT 指向其他端口"));
        }
    }
    let shim = embed::ensure_extracted()?;
    if inject::find_shell_pid().is_none() {
        return Err(BackendError::new(
            "INPROC_NO_SHELL",
            "ShadowBot.Shell.exe 未运行,无法加载进程内组件",
        )
        .with_hint(
            "先启动影刀客户端并用编辑模式打开目标应用;源码写入不回退官方 codeflow.write/edit",
        ));
    }
    let pid = inject::inject_into_shell(&shim)?;
    wait_ready(pid)
}

fn wait_ready(pid: u32) -> Res<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if healthy() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Err(BackendError::new(
        "INPROC_TIMEOUT",
        format!("注入 Shell pid={pid} 后控制面未就绪 (20s);可查看 %TEMP%\\xbot_shim.log"),
    ))
}

fn wait_listener_down(timeout: Duration) -> Res<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if health().is_err() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(BackendError::new(
        "INPROC_TIMEOUT",
        "旧 inproc 监听器未退出",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_ok_false_with_detail() {
        assert_eq!(
            decode(r#"{"ok":false,"detail":"no project"}"#, "u")
                .unwrap_err()
                .message,
            "no project"
        );
        assert!(decode(r#"{"ok":true,"data":1}"#, "u").is_ok());
        assert!(decode("not json", "u").is_err());
    }
}
