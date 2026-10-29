//! 三通道后端封装 + 统一输出 envelope。
//!
//! 通道(全部为影刀内置/官方能力,零注入零反射):
//!   shell-cli  官方 shadowbot.shell-cli.exe — 应用生命周期/任务/发布/版本
//!              studio open --app-id <uuid> 以【编辑模式】打开应用到 Studio
//!              studio open --builder custom --name X 新建并打开(编辑模式)
//!   mcp        127.0.0.1:42500 — studio-mcp,Studio 内开发调试主力通道
//!   url        shadowbot:View?robot-uuid=<uuid> — 只读 View 模式(不持久化、不同步)

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use chrono::TimeZone;
use serde_json::{json, Map, Value};

#[path = "transport.rs"]
mod transport;
pub use transport::{mcp, mcp_list, shell_cli, shell_cli_raw};

pub const DEFAULT_SHELL_CLI: &str = r"C:\Program Files\ShadowBot\shadowbot.shell-cli.exe";
pub const DEFAULT_MCP_ENDPOINT: &str = "http://127.0.0.1:42500/api/v1/mcp";
pub const DEFAULT_LAUNCHER: &str = r"C:\Program Files\ShadowBot\ShadowBot.exe";

pub fn shell_cli_path() -> String {
    std::env::var("XBOT_SHELL_CLI").unwrap_or_else(|_| DEFAULT_SHELL_CLI.into())
}

pub fn mcp_endpoint() -> String {
    std::env::var("XBOT_MCP_ENDPOINT").unwrap_or_else(|_| DEFAULT_MCP_ENDPOINT.into())
}

/// 兼容既有调用方；实际传输全部使用 Chrome Bridge 命名管道。
pub fn mcp_browser(tool: &str, args: Value, _timeout_secs: u64) -> Res<Value> {
    crate::bridge::call(tool, args)
}

pub fn launcher_path() -> String {
    std::env::var("XBOT_LAUNCHER").unwrap_or_else(|_| DEFAULT_LAUNCHER.into())
}

// ---------- 错误 ----------

#[derive(Debug)]
pub struct BackendError {
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
    pub data: Option<Value>,
    pub retryable: Option<bool>,
}

impl BackendError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            hint: None,
            data: None,
            retryable: None,
        }
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = Some(retryable);
        self
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for BackendError {}

pub type Res<T> = Result<T, BackendError>;

// ---------- envelope ----------

/// Build the stable machine-readable result without choosing a presentation or stream.
pub fn envelope(ok: bool, data: Option<Value>, error: Option<BackendError>) -> (Value, i32) {
    let mut env = Map::new();
    let mut exit_code = if ok { 0 } else { 1 };
    env.insert("ok".into(), json!(ok));
    if let Some(d) = data {
        env.insert("data".into(), d);
    }
    if let Some(e) = error {
        let BackendError {
            code,
            message,
            hint,
            data,
            retryable,
        } = e;
        let (kind, exit, default_retryable) = classify_error(&code);
        let retryable = retryable.unwrap_or(default_retryable);
        exit_code = exit;
        if let Some(d) = data {
            env.insert("data".into(), d);
        }
        let mut eo = Map::new();
        eo.insert("type".into(), json!(kind));
        eo.insert("code".into(), json!(code));
        eo.insert("message".into(), json!(message));
        eo.insert("retryable".into(), json!(retryable));
        if let Some(h) = hint {
            eo.insert("hint".into(), json!(h));
        }
        env.insert("error".into(), Value::Object(eo));
    }
    (Value::Object(env), exit_code)
}

fn classify_error(code: &str) -> (&'static str, i32, bool) {
    if code == "CONFIRMATION_REQUIRED" {
        return ("confirmation", 10, false);
    }
    if matches!(
        code,
        "BAD_ARGS"
            | "BAD_JSON"
            | "BAD_INPUT"
            | "BAD_APP_ID"
            | "MCP_TOOL_UNKNOWN"
            | "FLOW_ID_REQUIRED"
            | "FLOW_NOT_FOUND"
            | "FLOW_KIND_MISMATCH"
            | "FILE_READ"
    ) {
        return ("validation", 2, false);
    }
    if matches!(
        code,
        "api_4011" | "SHELL_CLI_MISSING" | "AUTH_REQUIRED" | "PLATFORM_AUTH" | "PLATFORM_CONFIG"
    ) {
        return ("authentication", 3, false);
    }
    if matches!(
        code,
        "MCP_OFFLINE"
            | "MCP_HTTP"
            | "MCP_READ_FAILED"
            | "TIMEOUT"
            | "OPEN_TIMEOUT"
            | "FLOW_RUN_TIMEOUT"
            | "PLATFORM_NETWORK"
            | "PLATFORM_TIMEOUT"
            | "PLATFORM_RATE_LIMIT"
    ) {
        // A transport failure does not prove that a write or run was not applied.
        return ("network", 4, false);
    }
    if matches!(
        code,
        "STUDIO_BUSY"
            | "OPEN_MISMATCH"
            | "VIEW_MODE_READONLY"
            | "already_running"
            | "SKILL_CONFLICT"
            | "APP_MISMATCH"
            | "APP_CONTEXT_REQUIRED"
    ) {
        return ("conflict", 5, false);
    }
    if matches!(
        code,
        "MCP_BAD_JSON"
            | "MCP_BAD_RESPONSE"
            | "SHELL_CLI_BAD_OUTPUT"
            | "PARSE_FAILED"
            | "FLOW_RUN_UNKNOWN"
            | "PLATFORM_BAD_RESPONSE"
    ) {
        return ("protocol", 6, false);
    }
    ("operation", 1, false)
}

// ---------- UUID ----------

pub fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect()
    }
}

// ---------- URL 协议(只读 View 模式) ----------

pub fn url_open_app(app_id: &str) -> Res<()> {
    if !is_uuid(app_id) {
        return Err(BackendError::new(
            "BAD_APP_ID",
            format!("app-id 必须是 UUID: {app_id}"),
        ));
    }
    Command::new("cmd")
        .args([
            "/c",
            "start",
            "",
            &format!("shadowbot:View?robot-uuid={app_id}"),
        ])
        .output()
        .map_err(|e| BackendError::new("URL_OPEN_FAILED", format!("{e}")))?;
    Ok(())
}

// ---------- 编辑模式打开 + View 检测 ----------

/// studio open --app-id:以编辑模式打开已有应用到 Studio(可保存/同步/发布)。
/// studio_already_open 时返回 {"already_open": true}。
pub fn edit_open_app(app_id: &str, timeout_secs: u64) -> Res<Value> {
    if !is_uuid(app_id) {
        return Err(BackendError::new(
            "BAD_APP_ID",
            format!("app-id 必须是 UUID: {app_id}"),
        ));
    }
    match shell_cli(&["studio", "open", "--app-id", app_id], timeout_secs) {
        Ok(d) => Ok(d),
        Err(e) if e.code == "studio_already_open" => Ok(json!({"already_open": true})),
        Err(e) => Err(e),
    }
}

pub fn wait_studio_open(timeout_secs: u64) -> Res<Value> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    while Instant::now() < deadline {
        match shell_cli(&["studio", "current", "get"], 15) {
            Ok(d) => {
                if d.get("hasStudioOpened").and_then(Value::as_bool) == Some(true) {
                    return Ok(d);
                }
            }
            Err(e) => {
                let text = format!("{}{}", e.code, e.message);
                if !text.contains("not_open") {
                    return Err(e);
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(BackendError::new(
        "OPEN_TIMEOUT",
        format!("等待 {timeout_secs}s Studio 仍未打开"),
    )
    .with_hint("确认影刀客户端已启动并登录"))
}

/// 定位 %LOCALAPPDATA%\ShadowBot\users\<uid>\apps\<app-id>。
fn apps_dir_of(app_id: &str) -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?).join(r"ShadowBot\users");
    for uid in fs::read_dir(root).ok()?.flatten() {
        let candidate = uid.path().join("apps").join(app_id);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// View(只读)会话特征:工作目录为 apps/<uuid>_temp;编辑模式直接写 apps/<uuid>。
pub fn is_view_mode(app_id: &str) -> bool {
    apps_dir_of(app_id)
        .map(|p| Path::new(&format!("{}_temp", p.display())).is_dir())
        .unwrap_or(false)
}

/// 写操作/sync 前置拦截:View 会话里改了也落不了盘、传不上云。
pub fn require_edit_mode(app_id: Option<&str>) -> Res<()> {
    let id = app_id.filter(|id| !id.is_empty()).ok_or_else(|| {
        BackendError::new("APP_CONTEXT_REQUIRED", "未确认当前应用，不能执行写入")
            .with_hint("先打开目标应用并确认 session status")
    })?;
    if is_view_mode(id) {
        return Err(BackendError::new(
            "VIEW_MODE_READONLY",
            "当前 Studio 是 URL View(只读)会话:编辑写入临时副本,sync 不上传",
        )
        .with_hint("关闭只读会话，然后用 xbot-cli app open <app-id> 以编辑模式重开"));
    }
    Ok(())
}

/// Strict current-session lookup for operations that must not continue without a target.
pub fn require_current_app_id() -> Res<String> {
    app_id_from_info(&mcp("app.get_info", json!({}), 10)?)
}

fn app_id_from_info(info: &Value) -> Res<String> {
    info.pointer("/app/app_id")
        .and_then(Value::as_str)
        .filter(|id| is_uuid(id))
        .map(str::to_owned)
        .ok_or_else(|| {
            BackendError::new("MCP_BAD_RESPONSE", "app.get_info 缺少有效 app_id")
                .with_data(info.clone())
        })
}

pub fn require_current_edit_mode() -> Res<String> {
    let app_id = require_current_app_id()?;
    require_edit_mode(Some(&app_id))?;
    Ok(app_id)
}

pub fn require_expected_app(expected: &str) -> Res<()> {
    let actual = require_current_app_id()?;
    if actual != expected {
        return Err(
            BackendError::new("APP_MISMATCH", "当前 Studio 应用与预期目标不一致")
                .with_data(json!({"expected_app_id": expected, "actual_app_id": actual})),
        );
    }
    Ok(())
}

/// 省略 flow_id 时优先活动流程;类型不符时仅在唯一候选时自动选择。
pub fn resolve_flow_id(flow_id: Option<&str>, kind: Option<&str>) -> Res<String> {
    let active = if flow_id.filter(|id| !id.is_empty()).is_none() {
        mcp("app.get_info", json!({}), 30)?
            .pointer("/flows/active_flow_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    } else {
        None
    };
    let listed = mcp("app.list_flows", json!({}), 30)?;
    let flows = listed
        .get("flows")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            BackendError::new("MCP_BAD_RESPONSE", "app.list_flows 缺少 flows 数组")
                .with_data(listed.clone())
        })?;
    resolve_flow_from_list(flow_id, kind, active.as_deref(), flows)
}

fn resolve_flow_from_list(
    flow_id: Option<&str>,
    kind: Option<&str>,
    active: Option<&str>,
    flows: &[Value],
) -> Res<String> {
    if let Some(requested) = flow_id.filter(|id| !id.is_empty()) {
        // IDs win over names; duplicate names must not silently select an arbitrary flow.
        let exact = flows
            .iter()
            .find(|flow| flow.get("flow_id").and_then(Value::as_str) == Some(requested));
        let matches: Vec<&Value> = if let Some(flow) = exact {
            vec![flow]
        } else {
            flows
                .iter()
                .filter(|flow| flow.get("flow_name").and_then(Value::as_str) == Some(requested))
                .collect()
        };
        if matches.is_empty() {
            return Err(BackendError::new(
                "FLOW_NOT_FOUND",
                format!("当前应用中不存在流程: {requested}"),
            ));
        }
        if matches.len() > 1 {
            return Err(BackendError::new(
                "FLOW_ID_REQUIRED",
                format!("流程名称不唯一: {requested}"),
            )
            .with_data(json!({"candidates": matches}))
            .with_hint("显式指定 flow_id"));
        }
        let flow = matches[0];
        let actual_kind = flow.get("flow_kind").and_then(Value::as_str);
        if let Some(expected) = kind {
            if actual_kind != Some(expected) {
                return Err(BackendError::new(
                    "FLOW_KIND_MISMATCH",
                    format!("流程类型不是 {expected}"),
                )
                .with_data(
                    json!({"requested": requested, "expected_kind": expected, "flow": flow}),
                ));
            }
        }
        return flow
            .get("flow_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                BackendError::new("MCP_BAD_RESPONSE", "流程记录缺少 flow_id")
                    .with_data(flow.clone())
            });
    }
    if let Some(active_id) = active {
        if flows.iter().any(|f| {
            f.get("flow_id").and_then(Value::as_str) == Some(active_id)
                && kind
                    .map(|k| f.get("flow_kind").and_then(Value::as_str) == Some(k))
                    .unwrap_or(true)
        }) {
            return Ok(active_id.to_string());
        }
    }
    let candidates: Vec<&Value> = flows
        .iter()
        .filter(|f| {
            kind.map(|k| f.get("flow_kind").and_then(Value::as_str) == Some(k))
                .unwrap_or(true)
        })
        .collect();
    if candidates.len() == 1 {
        if let Some(id) = candidates[0].get("flow_id").and_then(Value::as_str) {
            return Ok(id.to_string());
        }
    }
    let choices: Vec<Value> = candidates
        .iter()
        .map(|f| {
            json!({
                "flow_id": f.get("flow_id"),
                "flow_name": f.get("flow_name"),
                "flow_kind": f.get("flow_kind"),
            })
        })
        .collect();
    Err(BackendError::new(
        "FLOW_ID_REQUIRED",
        "无法自动确定流程:当前活动流程不匹配且候选不唯一",
    )
    .with_data(json!({"active_flow_id": active, "candidates": choices}))
    .with_hint("xbot-cli flow list 查看流程,然后显式传入 flow_id"))
}

/// 扫影刀日志(since 之后)有无该应用的 `UploadAsync success`,作为真上传证据。
pub fn log_has_upload(app_id: &str, since: chrono::DateTime<chrono::Local>) -> bool {
    let log_dir =
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join(r"ShadowBot\log");
    for back in 0..=1_i64 {
        let day = (since - chrono::Duration::days(back))
            .format("%Y%m%d")
            .to_string();
        let path = log_dir.join(format!("{day}.log"));
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            if line.contains("UploadAsync success") && line.contains(app_id) {
                // 行首时间戳 2026-10-13 18:41:16,259
                if let Ok(t) = chrono::NaiveDateTime::parse_from_str(
                    line.get(..19).unwrap_or(""),
                    "%Y-%m-%d %H:%M:%S",
                ) {
                    let lt = chrono::Local.from_local_datetime(&t).single();
                    if let Some(lt) = lt {
                        if lt >= since - chrono::Duration::seconds(2) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// ---------- 子命令公共小工具 ----------

/// 读 --file 内容,'-' 读 stdin。
pub fn read_file_or_stdin(file: &str) -> Res<String> {
    if file == "-" {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| BackendError::new("STDIN_READ", format!("{e}")))?;
        Ok(s)
    } else {
        fs::read_to_string(file).map_err(|e| BackendError::new("FILE_READ", format!("{file}: {e}")))
    }
}

/// 文本参数:`-` 读 stdin,`@path` 读文件,其余按字面文本。
pub fn text_arg(raw: &str) -> Res<String> {
    if raw == "-" {
        read_file_or_stdin(raw)
    } else if let Some(path) = raw.strip_prefix('@') {
        fs::read_to_string(path).map_err(|e| BackendError::new("FILE_READ", format!("{path}: {e}")))
    } else {
        Ok(raw.to_string())
    }
}

/// JSON 参数:`-` 读 stdin,`@path` 读文件,其余按字面 JSON 解析。
pub fn json_arg(raw: &str) -> Res<Value> {
    let text = text_arg(raw)?;
    serde_json::from_str(&text).map_err(|e| BackendError::new("BAD_JSON", format!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_current_app_cannot_pass_write_precondition() {
        assert_eq!(
            require_edit_mode(None).unwrap_err().code,
            "APP_CONTEXT_REQUIRED"
        );
        assert_eq!(
            app_id_from_info(&json!({})).unwrap_err().code,
            "MCP_BAD_RESPONSE"
        );
    }

    fn flows() -> Vec<Value> {
        vec![
            json!({"flow_id": "code-id", "flow_name": "code-name", "flow_kind": "code"}),
            json!({"flow_id": "visual-id", "flow_name": "visual-name", "flow_kind": "visual"}),
        ]
    }

    #[test]
    fn explicit_flow_name_is_resolved_and_type_checked() {
        assert_eq!(
            resolve_flow_from_list(Some("code-name"), Some("code"), None, &flows()).unwrap(),
            "code-id"
        );
        assert_eq!(
            resolve_flow_from_list(Some("code-id"), Some("visual"), None, &flows())
                .unwrap_err()
                .code,
            "FLOW_KIND_MISMATCH"
        );
        assert_eq!(
            resolve_flow_from_list(Some("missing"), None, None, &flows())
                .unwrap_err()
                .code,
            "FLOW_NOT_FOUND"
        );
    }

    #[test]
    fn implicit_flow_prefers_active_then_unique_matching_type() {
        assert_eq!(
            resolve_flow_from_list(None, None, Some("visual-id"), &flows()).unwrap(),
            "visual-id"
        );
        assert_eq!(
            resolve_flow_from_list(None, Some("code"), Some("visual-id"), &flows()).unwrap(),
            "code-id"
        );
        assert_eq!(
            resolve_flow_from_list(None, None, None, &flows())
                .unwrap_err()
                .code,
            "FLOW_ID_REQUIRED"
        );
    }

    #[test]
    fn duplicate_flow_names_require_explicit_id() {
        let duplicates = vec![
            json!({"flow_id":"first", "flow_name":"same", "flow_kind":"code"}),
            json!({"flow_id":"second", "flow_name":"same", "flow_kind":"code"}),
        ];
        assert_eq!(
            resolve_flow_from_list(Some("same"), None, None, &duplicates)
                .unwrap_err()
                .code,
            "FLOW_ID_REQUIRED"
        );
    }

    #[test]
    fn unknown_write_outcomes_are_not_automatically_retryable() {
        for code in [
            "TIMEOUT",
            "MCP_OFFLINE",
            "FLOW_RUN_TIMEOUT",
            "MCP_READ_FAILED",
        ] {
            assert!(!classify_error(code).2);
        }
        assert_eq!(classify_error("FLOW_RUN_UNKNOWN").0, "protocol");
        assert_eq!(classify_error("FLOW_NOT_FOUND").0, "validation");
    }

    #[test]
    fn success_envelope_keeps_existing_machine_contract() {
        let (value, exit) = envelope(true, Some(json!({"items": [1, 2]})), None);
        assert_eq!(exit, 0);
        assert_eq!(value, json!({"ok": true, "data": {"items": [1, 2]}}));
        assert_eq!(envelope(true, None, None), (json!({"ok": true}), 0));
    }

    #[test]
    fn error_envelope_preserves_evidence_and_retryability_override() {
        let error = BackendError::new("MCP_OFFLINE", "connection closed")
            .with_hint("check the same run")
            .with_data(json!({"run_id": "r1", "outcome": "unknown"}))
            .with_retryable(true);
        let (value, exit) = envelope(false, Some(json!({"old": true})), Some(error));
        assert_eq!(exit, 4);
        assert_eq!(
            value,
            json!({
                "ok": false,
                "data": {"run_id": "r1", "outcome": "unknown"},
                "error": {
                    "type": "network", "code": "MCP_OFFLINE",
                    "message": "connection closed", "hint": "check the same run", "retryable": true
                }
            })
        );
    }
}
