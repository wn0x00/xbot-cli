//! Backend transport and wire-response decoding. No Studio workflow decisions belong here.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use super::{mcp_endpoint, shell_cli_path, truncate, BackendError, Res};

#[derive(Debug)]
struct ProcessOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

impl ProcessOutput {
    fn evidence(&self) -> Value {
        json!({"exit_code": self.status.code(), "stdout": self.stdout, "stderr": self.stderr})
    }
}

fn captured(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&buffer.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
}

fn drain<R: Read + Send + 'static>(
    mut pipe: R,
    buffer: Arc<Mutex<Vec<u8>>>,
    finished: mpsc::Sender<std::io::Result<()>>,
) {
    thread::spawn(move || {
        let result = (|| {
            let mut chunk = [0_u8; 8192];
            loop {
                let size = pipe.read(&mut chunk)?;
                if size == 0 {
                    return Ok(());
                }
                buffer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .extend_from_slice(&chunk[..size]);
            }
        })();
        let _ = finished.send(result);
    });
}

/// Drain both pipes while the child is alive; retain both streams and its exit status.
fn run_capture(mut cmd: Command, timeout_secs: u64) -> Res<ProcessOutput> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                "SHELL_CLI_MISSING"
            } else {
                "SPAWN_FAILED"
            };
            BackendError::new(code, format!("无法启动后端进程: {e}"))
        })?;
    let stdout = Arc::new(Mutex::new(Vec::new()));
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let (sender, receiver) = mpsc::channel();
    drain(
        child.stdout.take().expect("piped stdout"),
        stdout.clone(),
        sender.clone(),
    );
    drain(
        child.stderr.take().expect("piped stderr"),
        stderr.clone(),
        sender,
    );
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let killed = child.kill().is_ok();
                if killed {
                    let _ = child.wait();
                }
                return Err(BackendError::new("TIMEOUT", format!("后端进程超过 {timeout_secs}s，执行结果尚未确认"))
                    .with_data(json!({"stdout": captured(&stdout), "stderr": captured(&stderr), "process_killed": killed, "outcome": "unknown"}))
                    .with_hint("先查询已有操作或任务状态；不要自动重复执行写入或启动任务"));
            }
            Err(e) => {
                if child.kill().is_ok() {
                    let _ = child.wait();
                }
                return Err(BackendError::new("WAIT_FAILED", format!("{e}"))
                    .with_data(json!({"stdout": captured(&stdout), "stderr": captured(&stderr), "outcome": "unknown"})));
            }
        }
    };
    for _ in 0..2 {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(BackendError::new("PROCESS_READ_FAILED", format!("读取后端输出失败: {e}"))
                .with_data(json!({"stdout": captured(&stdout), "stderr": captured(&stderr), "exit_code": status.code()}))),
            Err(_) => return Err(BackendError::new("TIMEOUT", "后端进程结束，但输出管道未在期限内关闭")
                .with_data(json!({"stdout": captured(&stdout), "stderr": captured(&stderr), "exit_code": status.code(), "outcome": "unknown"}))),
        }
    }
    Ok(ProcessOutput {
        status,
        stdout: captured(&stdout),
        stderr: captured(&stderr),
    })
}

/// Preserve the backend envelope and task summary, while rejecting contradictory process success.
pub fn shell_cli_raw(args: &[&str], timeout_secs: u64) -> Res<Value> {
    let mut cmd = Command::new(shell_cli_path());
    cmd.args(args)
        .arg("--timeout")
        .arg(format!("{timeout_secs}s"));
    decode_shell_output(run_capture(cmd, timeout_secs.saturating_add(15))?)
}

fn decode_shell_output(output: ProcessOutput) -> Res<Value> {
    let summary = parse_task_summary(&output.stdout).or_else(|| parse_task_summary(&output.stderr));
    let stdout_json = parse_mixed_json(&output.stdout);
    let stderr_json = parse_mixed_json(&output.stderr);
    let parsed = match (&stdout_json, &stderr_json) {
        (Some(value), _) if value.get("ok").is_some() => stdout_json,
        (_, Some(value)) if value.get("ok").is_some() => stderr_json,
        _ => stdout_json.or(stderr_json),
    };
    let mut envelope = match (parsed, summary) {
        (Some(mut value), Some(summary)) => {
            let object = value.as_object_mut().ok_or_else(|| {
                BackendError::new("SHELL_CLI_BAD_OUTPUT", "后端响应不是对象")
                    .with_data(output.evidence())
            })?;
            let data = object.entry("data").or_insert_with(|| json!({}));
            let data = data.as_object_mut().ok_or_else(|| {
                BackendError::new("SHELL_CLI_BAD_OUTPUT", "后端 data 不是对象")
                    .with_data(output.evidence())
            })?;
            if let Some(id) = summary.get("taskId") {
                data.entry("taskId").or_insert_with(|| id.clone());
            }
            if let Some(success) = summary.get("success").and_then(Value::as_str) {
                if let Ok(success) = success.parse::<bool>() {
                    data.insert("success".into(), json!(success));
                }
            }
            data.insert("task_summary".into(), json!(summary));
            value
        }
        (Some(value), None) => value,
        (None, Some(summary)) => {
            let success = summary
                .get("success")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<bool>().ok());
            let success = success.ok_or_else(|| {
                BackendError::new(
                    "SHELL_CLI_BAD_OUTPUT",
                    "task-summary 缺少可确认的 success 状态",
                )
                .with_data(output.evidence())
            })?;
            json!({"ok": success, "data": {"taskId": summary.get("taskId"), "success": success, "task_summary": summary}})
        }
        (None, None) => {
            return Err(BackendError::new(
                "SHELL_CLI_BAD_OUTPUT",
                "后端未返回有效 JSON 或 task-summary",
            )
            .with_data(output.evidence()))
        }
    };
    let ok = envelope.get("ok").and_then(Value::as_bool).ok_or_else(|| {
        BackendError::new("SHELL_CLI_BAD_OUTPUT", "后端响应缺少布尔型 ok")
            .with_data(output.evidence())
    })?;
    if !output.status.success() && ok {
        return Err(BackendError::new(
            "SHELL_CLI_EXIT",
            "后端进程以非零状态退出，不能确认操作成功",
        )
        .with_data(json!({"response": envelope, "process": output.evidence()})));
    }
    if !ok {
        envelope["process"] = output.evidence();
    }
    Ok(envelope)
}

pub fn shell_cli(args: &[&str], timeout_secs: u64) -> Res<Value> {
    let value = shell_cli_raw(args, timeout_secs)?;
    if value.get("ok").and_then(Value::as_bool) == Some(true)
        && value.pointer("/data/success").and_then(Value::as_bool) != Some(false)
    {
        return Ok(value.get("data").cloned().unwrap_or_else(|| json!({})));
    }
    Err(backend_error(&value, "SHELL_CLI_FAILED", "后端操作失败"))
}

fn parse_mixed_json(raw: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str(raw.trim()) {
        return Some(value);
    }
    let mut fallback = None;
    for (index, _) in raw.char_indices().filter(|(_, c)| *c == '{') {
        if let Some(Ok(value)) = serde_json::Deserializer::from_str(&raw[index..])
            .into_iter::<Value>()
            .next()
        {
            if value.get("ok").is_some() {
                return Some(value);
            }
            if fallback.is_none() {
                fallback = Some(value);
            }
        }
    }
    fallback
}

fn parse_task_summary(raw: &str) -> Option<Map<String, Value>> {
    let line = raw
        .lines()
        .rev()
        .find_map(|line| line.split_once("[task-summary]").map(|(_, tail)| tail))?;
    let values: Map<String, Value> = line
        .split_whitespace()
        .filter_map(|item| {
            item.split_once('=')
                .map(|(key, value)| (key.to_owned(), json!(value)))
        })
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn code_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|n| n.to_string()))
}

fn backend_error(value: &Value, fallback: &str, message: &str) -> BackendError {
    let code = ["/data/code", "/error/data/code", "/error/code", "/code"]
        .iter()
        .find_map(|path| value.pointer(path).and_then(code_value))
        .or_else(|| {
            value
                .get("apiCode")
                .and_then(Value::as_i64)
                .map(|n| format!("api_{n}"))
        })
        .unwrap_or_else(|| fallback.to_owned());
    let message = [
        "/error/data/message",
        "/error/message",
        "/error",
        "/data/message",
        "/message",
        "/text",
    ]
    .iter()
    .find_map(|path| {
        value
            .pointer(path)
            .and_then(Value::as_str)
            .filter(|message| !message.trim().is_empty())
    })
    .unwrap_or(message);
    BackendError::new(code, message).with_data(value.clone())
}

pub fn mcp(tool: &str, args: Value, timeout_secs: u64) -> Res<Value> {
    mcp_on(&mcp_endpoint(), tool, args, timeout_secs)
}

pub fn mcp_list(browser: bool, timeout_secs: u64) -> Res<Vec<Value>> {
    if browser {
        return Ok(crate::bridge::tools());
    }
    let endpoint = mcp_endpoint();
    let response = rpc_on(&endpoint, "tools/list", json!({}), timeout_secs)?;
    response
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| {
            BackendError::new("MCP_BAD_RESPONSE", "tools/list 响应缺少 tools 数组")
                .with_data(response)
        })
}

fn mcp_on(endpoint: &str, tool: &str, args: Value, timeout_secs: u64) -> Res<Value> {
    if !args.is_object() {
        return Err(BackendError::new(
            "BAD_ARGS",
            "MCP arguments 必须是 JSON 对象",
        ));
    }
    decode_tool_result(rpc_on(
        endpoint,
        "tools/call",
        json!({"name": tool, "arguments": args}),
        timeout_secs,
    )?)
}

fn decode_tool_result(response: Value) -> Res<Value> {
    if !response.is_object() {
        return Err(
            BackendError::new("MCP_BAD_RESPONSE", "MCP 工具结果不是对象").with_data(response),
        );
    }
    let texts: Vec<&str> = response
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect();
    let output = response
        .get("structuredContent")
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| match texts.first() {
            Some(text) => {
                serde_json::from_str(text).unwrap_or_else(|_| json!({"text": texts.join("\n")}))
            }
            None => response.clone(),
        });
    if response.get("isError").and_then(Value::as_bool) == Some(true)
        || output.get("success").and_then(Value::as_bool) == Some(false)
        || output.get("ok").and_then(Value::as_bool) == Some(false)
    {
        let mut error = backend_error(&output, "MCP_TOOL_FAILED", "MCP 工具执行失败");
        if error.code == "MCP_TOOL_FAILED" {
            if let Some(code) = response.get("code").and_then(code_value) {
                error.code = code;
            }
        }
        if let Some(issues) = output
            .get("issues")
            .filter(|value| !value.is_null() && **value != json!([]))
        {
            error = error.with_hint(truncate(&issues.to_string(), 300));
        }
        return Err(error.with_data(json!({"response": response, "result": output})));
    }
    Ok(output)
}

fn rpc_on(endpoint: &str, method: &str, params: Value, timeout_secs: u64) -> Res<Value> {
    let payload = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let client = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout_secs))
        .build();
    let response = client
        .post(endpoint)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream")
        .send_json(payload);
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().map_err(|e| {
                BackendError::new(
                    "MCP_READ_FAILED",
                    format!("HTTP {status} 响应读取失败: {e}"),
                )
            })?;
            let value = serde_json::from_str(&body)
                .unwrap_or_else(|_| json!({"text": truncate(&body, 4096)}));
            let mut error = backend_error(&value, "MCP_HTTP", &format!("MCP 返回 HTTP {status}"));
            error.data = Some(json!({"http_status": status, "response": value}));
            return Err(error);
        }
        Err(ureq::Error::Transport(error)) => {
            let backend = "studio-mcp";
            return Err(BackendError::new(
                "MCP_OFFLINE",
                format!("{backend} 不可达或响应中断: {error}"),
            )
            .with_hint("先检查连接和已有操作状态；写入或运行可能已开始，不要自动重试"));
        }
    };
    let event_stream = response
        .header("Content-Type")
        .map(|header| header.starts_with("text/event-stream"))
        .unwrap_or(false);
    let body = response
        .into_string()
        .map_err(|e| BackendError::new("MCP_READ_FAILED", format!("读取 MCP 响应失败: {e}")))?;
    decode_rpc_body(&body, event_stream)
}

fn decode_rpc_body(body: &str, event_stream: bool) -> Res<Value> {
    let body = body.trim();
    let response = if event_stream
        || body.starts_with("event:")
        || body.starts_with("data:")
        || body.starts_with(':')
    {
        let normalized = body.replace("\r\n", "\n");
        let mut found = None;
        for event in normalized.split("\n\n") {
            let payload = event
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|data| data.strip_prefix(' ').unwrap_or(data))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let value: Value = serde_json::from_str(&payload).map_err(|e| {
                BackendError::new("MCP_BAD_JSON", format!("SSE data 不是 JSON: {e}"))
            })?;
            if is_current_rpc_id(value.get("id")) {
                found = Some(value);
            }
        }
        found
            .ok_or_else(|| BackendError::new("MCP_BAD_RESPONSE", "SSE 缺少与请求 ID 对应的响应"))?
    } else {
        serde_json::from_str(body).map_err(|e| {
            BackendError::new("MCP_BAD_JSON", format!("{e}: {}", truncate(body, 200)))
        })?
    };
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || !is_current_rpc_id(response.get("id"))
    {
        return Err(
            BackendError::new("MCP_BAD_RESPONSE", "JSON-RPC 版本或响应 ID 不匹配")
                .with_data(response),
        );
    }
    if response.get("error").is_some() {
        return Err(backend_error(&response, "MCP_RPC", "MCP JSON-RPC 调用失败"));
    }
    response
        .get("result")
        .filter(|value| !value.is_null())
        .cloned()
        .ok_or_else(|| {
            BackendError::new("MCP_BAD_RESPONSE", "JSON-RPC 响应缺少非空 result")
                .with_data(response)
        })
}

fn is_current_rpc_id(id: Option<&Value>) -> bool {
    // Every request currently uses the exactly representable number 1. Some MCP
    // servers serialize it as 1.0; JSON-RPC string IDs must still remain distinct.
    id.and_then(Value::as_f64) == Some(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn fake_http(status: u16, content_type: &str, body: &str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let content_type = content_type.to_owned();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let size = socket.read(&mut chunk).unwrap();
                if size == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..size]);
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            write!(socket, "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        (endpoint, handle)
    }

    #[test]
    fn mcp_outer_error_is_not_success_and_preserves_backend_code() {
        let error = decode_tool_result(json!({"isError": true, "structuredContent": {"code": "FLOW_LOCKED", "message": "locked"}})).unwrap_err();
        assert_eq!(error.code, "FLOW_LOCKED");
        assert_eq!(error.message, "locked");
        assert_eq!(error.data.unwrap()["response"]["isError"], true);
    }

    #[test]
    fn mcp_plain_text_error_is_not_success() {
        let error = decode_tool_result(
            json!({"isError": true, "content": [{"type": "text", "text": "tool exploded"}]}),
        )
        .unwrap_err();
        assert_eq!(error.code, "MCP_TOOL_FAILED");
        assert_eq!(error.message, "tool exploded");
    }

    #[test]
    fn mcp_business_failure_preserves_code() {
        let error = decode_tool_result(json!({"content": [{"type": "text", "text": "{\"success\":false,\"code\":\"user_paused\",\"message\":\"paused\"}"}]})).unwrap_err();
        assert_eq!(error.code, "user_paused");
    }

    #[test]
    fn mcp_fill_error_is_not_hidden_by_selector_storage_message() {
        let failure = "Error: locator.fill: Error: Element is not an <input>, <textarea> or [contenteditable] element\nCall log:\n  - locator.fill(\"111\")";
        let result = json!({
            "success": false,
            "error": failure,
            "candidate_selectors": [],
            "message": "临时元素_e5 selectors have been stored."
        });
        for structured in [false, true] {
            let mut response = json!({
                "isError": true,
                "content": [{"type": "text", "text": result.to_string()}]
            });
            if structured {
                response["structuredContent"] = result.clone();
            }
            let error = decode_tool_result(response.clone()).unwrap_err();
            assert_eq!(error.code, "MCP_TOOL_FAILED");
            assert_eq!(error.message, failure);
            assert_eq!(
                error.data.unwrap(),
                json!({"response": response, "result": result})
            );
        }
    }

    #[test]
    fn mcp_successful_selector_storage_is_still_success() {
        let result = json!({
            "success": true,
            "message": "临时元素_e4 selectors have been stored."
        });
        for response in [
            json!({"isError": false, "structuredContent": result}),
            json!({"isError": false, "content": [{"type": "text", "text": result.to_string()}]}),
        ] {
            assert_eq!(decode_tool_result(response).unwrap(), result);
        }
    }

    #[test]
    fn backend_empty_error_message_falls_back_to_meaningful_message() {
        for empty_error in [json!(""), json!(" \n\t "), json!({"message": " "})] {
            let result =
                json!({"success": false, "error": empty_error, "message": "operation failed"});
            let error = decode_tool_result(json!({"isError": true, "structuredContent": result}))
                .unwrap_err();
            assert_eq!(error.message, "operation failed");
        }
        let error = backend_error(&json!({"error": " ", "message": ""}), "TEST", "fallback");
        assert_eq!(error.message, "fallback");
    }

    #[test]
    fn backend_explicit_error_retains_code_and_original_evidence() {
        for value in [
            json!({"code": "ELEMENT_NOT_EDITABLE", "error": "not editable", "message": "selectors stored"}),
            json!({"error": {"code": "ELEMENT_NOT_EDITABLE", "message": "not editable"}, "data": {"message": "selectors stored"}}),
        ] {
            let error = backend_error(&value, "MCP_TOOL_FAILED", "fallback");
            assert_eq!(error.code, "ELEMENT_NOT_EDITABLE");
            assert_eq!(error.message, "not editable");
            assert_eq!(error.data.unwrap(), value);
        }
    }

    #[test]
    fn rpc_requires_matching_response_and_non_null_result() {
        for value in [
            json!({"jsonrpc": "2.0", "id": 1}),
            json!({"jsonrpc": "2.0", "id": 1, "result": null}),
            json!({"jsonrpc": "2.0", "id": 2, "result": {}}),
        ] {
            assert_eq!(
                decode_rpc_body(&value.to_string(), false).unwrap_err().code,
                "MCP_BAD_RESPONSE"
            );
        }
    }

    #[test]
    fn rpc_accepts_equivalent_numeric_id_representations() {
        for id in ["1", "1.0", "1e0", "10e-1"] {
            let response = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"tools":[]}}}}"#);
            for event_stream in [false, true] {
                let body = if event_stream {
                    format!("data: {response}\n\n")
                } else {
                    response.clone()
                };
                assert_eq!(
                    decode_rpc_body(&body, event_stream).unwrap(),
                    json!({"tools": []}),
                    "id={id}, event_stream={event_stream}"
                );
            }
        }
    }

    #[test]
    fn rpc_rejects_non_matching_id_types_and_numbers() {
        for id in [
            r#""1""#, "1.5", "1.0001", "2", "0", "-1", "null", "true", "{}", "[]",
        ] {
            let response = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#);
            for event_stream in [false, true] {
                let body = if event_stream {
                    format!("data: {response}\n\n")
                } else {
                    response.clone()
                };
                assert_eq!(
                    decode_rpc_body(&body, event_stream).unwrap_err().code,
                    "MCP_BAD_RESPONSE",
                    "id={id}, event_stream={event_stream}"
                );
            }
        }
    }

    #[test]
    fn sse_numeric_id_ignores_later_non_matching_responses() {
        let body = concat!(
            "data: {\"jsonrpc\":\"2.0\",\"id\":1.0,\"result\":{\"tools\":[]}}\n\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":\"1\",\"result\":{\"wrong\":true}}\n\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1.5,\"result\":{\"wrong\":true}}\n\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n"
        );
        assert_eq!(decode_rpc_body(body, true).unwrap(), json!({"tools": []}));
    }

    #[test]
    fn rpc_error_retains_original_numeric_code() {
        for id in ["1", "1.0"] {
            let response = format!(
                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32602,"message":"invalid params"}}}}"#
            );
            for event_stream in [false, true] {
                let body = if event_stream {
                    format!("data: {response}\n\n")
                } else {
                    response.clone()
                };
                let error = decode_rpc_body(&body, event_stream).unwrap_err();
                assert_eq!(error.code, "-32602");
                assert_eq!(error.message, "invalid params");
            }
        }
    }

    #[test]
    fn sse_reads_multiline_response_and_ignores_later_notifications() {
        let body = ": heartbeat\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\ndata: \"result\":{\"tools\":[]}}\n\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\ndata: [DONE]\n\n";
        assert_eq!(decode_rpc_body(body, true).unwrap(), json!({"tools": []}));
    }

    #[test]
    fn http_success_uses_full_transport_decode() {
        let (endpoint, server) = fake_http(
            200,
            "application/json",
            r#"{"jsonrpc":"2.0","id":1.0,"result":{"content":[{"type":"text","text":"{\"success\":true,\"value\":42}"}]}}"#,
        );
        let result = mcp_on(&endpoint, "fake.read", json!({}), 5);
        server.join().unwrap();
        assert_eq!(result.unwrap()["value"], 42);
    }

    #[test]
    fn http_error_cannot_be_overridden_by_success_looking_body() {
        let (endpoint, server) = fake_http(
            503,
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{"success":true}}"#,
        );
        let error = rpc_on(&endpoint, "tools/list", json!({}), 5).unwrap_err();
        server.join().unwrap();
        assert_eq!(error.code, "MCP_HTTP");
        assert_eq!(error.data.unwrap()["http_status"], 503);
    }

    #[test]
    fn http_error_preserves_backend_error() {
        let (endpoint, server) = fake_http(
            409,
            "application/json",
            r#"{"error":{"code":"studio_busy","message":"already open"}}"#,
        );
        let error = rpc_on(&endpoint, "tools/call", json!({}), 5).unwrap_err();
        server.join().unwrap();
        assert_eq!(error.code, "studio_busy");
        assert_eq!(error.message, "already open");
    }

    fn child(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "core::transport::tests::process_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("XBOT_TRANSPORT_TEST_CHILD", mode);
        command
    }

    #[test]
    #[ignore = "Only launched as a child process by transport tests"]
    fn process_helper() {
        match std::env::var("XBOT_TRANSPORT_TEST_CHILD").as_deref() {
            Ok("large") => {
                let chunk = vec![b'x'; 8192];
                for _ in 0..64 {
                    std::io::stdout().write_all(&chunk).unwrap();
                    std::io::stderr().write_all(&chunk).unwrap();
                }
                std::io::stdout().flush().unwrap();
                std::io::stderr().flush().unwrap();
            }
            Ok("nonzero") => {
                print!("{{\"ok\":true,\"data\":{{}}}}");
                std::io::stdout().flush().unwrap();
                std::process::exit(7);
            }
            Ok("timeout") => {
                println!("operation already started");
                std::io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(10));
            }
            _ => {}
        }
    }

    #[test]
    fn process_drains_both_pipes_without_deadlock() {
        let output = run_capture(child("large"), 10).unwrap();
        assert!(output.status.success());
        assert!(output.stdout.len() >= 512 * 1024);
        assert_eq!(output.stderr.len(), 512 * 1024);
    }

    #[test]
    fn process_nonzero_exit_cannot_claim_success() {
        let output = run_capture(child("nonzero"), 5).unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            decode_shell_output(output).unwrap_err().code,
            "SHELL_CLI_EXIT"
        );
    }

    #[test]
    fn process_timeout_retains_partial_output_and_unknown_outcome() {
        let started = Instant::now();
        let error = run_capture(child("timeout"), 1).unwrap_err();
        assert_eq!(error.code, "TIMEOUT");
        let evidence = error.data.unwrap();
        assert_eq!(evidence["outcome"], "unknown");
        assert!(evidence["stdout"]
            .as_str()
            .unwrap()
            .contains("operation already started"));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    fn successful_status() -> ExitStatus {
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        }
    }

    #[test]
    fn shell_summary_overrides_initial_success_with_terminal_failure() {
        let output = ProcessOutput { status: successful_status(), stdout: "log\n{\"ok\":true,\"data\":{\"success\":true}}\n[task-summary] taskId=t123 success=false durationMs=22".into(), stderr: "diagnostic".into() };
        let value = decode_shell_output(output).unwrap();
        assert_eq!(value["data"]["taskId"], "t123");
        assert_eq!(value["data"]["success"], false);
    }

    #[test]
    fn shell_summary_only_is_supported() {
        let output = ProcessOutput {
            status: successful_status(),
            stdout: "[task-summary] taskId=t123 success=true durationMs=22".into(),
            stderr: String::new(),
        };
        let value = decode_shell_output(output).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["taskId"], "t123");
    }

    #[test]
    fn shell_error_on_stderr_is_not_hidden_by_stdout_diagnostics() {
        let output = ProcessOutput {
            status: successful_status(),
            stdout: "diagnostic {\"level\":\"warning\"}".into(),
            stderr:
                "{\"ok\":false,\"data\":{\"code\":\"studio_already_open\",\"message\":\"busy\"}}"
                    .into(),
        };
        let value = decode_shell_output(output).unwrap();
        let error = backend_error(&value, "SHELL_CLI_FAILED", "failed");
        assert_eq!(error.code, "studio_already_open");
        assert_eq!(error.message, "busy");
        assert!(error.data.unwrap()["process"]["stdout"]
            .as_str()
            .unwrap()
            .contains("warning"));
    }
}
