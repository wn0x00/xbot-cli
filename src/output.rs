//! Presentation is separate from the stable JSON envelope and backend execution.

use std::io::{self, Write};

use serde_json::Value;

use crate::core::{self, BackendError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Text,
    Json,
    Pretty,
}

/// Explicit formats work identically in terminals and pipes. Auto keeps redirected
/// operation results machine-readable while making an interactive terminal readable.
pub fn resolve_mode(format: &str, json: bool, stdout_is_terminal: bool) -> OutputMode {
    if json {
        return OutputMode::Json;
    }
    match format {
        "text" => OutputMode::Text,
        "json" => OutputMode::Json,
        "pretty" => OutputMode::Pretty,
        _ if stdout_is_terminal => OutputMode::Text,
        _ => OutputMode::Json,
    }
}

/// Success belongs to stdout and failure belongs to stderr, in every format.
pub fn emit(ok: bool, data: Option<Value>, error: Option<BackendError>, mode: OutputMode) -> i32 {
    let (value, exit_code) = core::envelope(ok, data, error);
    write_text(&render(&value, mode), ok, exit_code)
}

/// Help and version use the same pipe-safe writer as operation results.
pub fn write_text(text: &str, success: bool, exit_code: i32) -> i32 {
    let mut text = text.to_owned();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let written = if success {
        io::stdout().lock().write_all(text.as_bytes())
    } else {
        io::stderr().lock().write_all(text.as_bytes())
    };
    match written {
        // A downstream consumer may stop once it has the records it needs.
        Ok(()) => exit_code,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => exit_code,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "无法写入命令输出: {error}");
            if exit_code == 0 {
                1
            } else {
                exit_code
            }
        }
    }
}

pub fn render(envelope: &Value, mode: OutputMode) -> String {
    match mode {
        OutputMode::Json => serde_json::to_string(envelope).expect("serialize envelope"),
        OutputMode::Pretty => serde_json::to_string_pretty(envelope).expect("serialize envelope"),
        OutputMode::Text if envelope.get("ok").and_then(Value::as_bool) == Some(true) => {
            render_success(envelope.get("data"))
        }
        OutputMode::Text => render_error(envelope),
    }
}

fn render_success(data: Option<&Value>) -> String {
    let Some(data) = data else {
        return "完成".into();
    };
    if data["command"] == "client.info" {
        return render_client(data);
    }
    if data["command"] == "win32.list" {
        return render_windows(data);
    }
    if let Some(tree) = crate::win32_snapshot::render(data) {
        return tree;
    }
    if data["command"]
        .as_str()
        .is_some_and(|command| command.starts_with("win32."))
        && data.get("source_snapshot_id").is_some()
    {
        let mut lines = vec![format!(
            "{}: {} [ref={}]",
            scalar(&data["command"]),
            if data["applied"] == true {
                "动作已返回"
            } else if data["outcome"] == "no_change" {
                "已是目标状态，无需修改"
            } else {
                "控件实时信息"
            },
            scalar(&data["ref"])
        )];
        lines.push(format!("来源快照: {}", scalar(&data["source_snapshot_id"])));
        if let Some(method) = data["execution"]["method"].as_str() {
            lines.push(match method {
                "invoke" => "方式: invoke（调用 UIA InvokePattern，未发送鼠标点击）".into(),
                "physical" => "方式: physical（物理鼠标操作）".into(),
                other => format!("方式: {other}"),
            });
            if let Some(target) = data.get("target_control") {
                lines.push(format!(
                    "目标: {} {} bounds={}",
                    scalar(&target["control_type"]),
                    serde_json::to_string(&target["name"]).unwrap_or_default(),
                    scalar(&target["bounds"])
                ));
            }
            if let Some(point) = data["execution"]["point"].as_array() {
                lines.push(format!(
                    "操作位置: {}（屏幕物理像素，实时控件中心）",
                    scalar(&Value::Array(point.clone()))
                ));
            }
            if data["execution"]["effect_verified"] == false {
                lines.push("界面效果: 未自动验证，请核对后续快照".into());
            }
        }
        if let Some(control) = data.get("control") {
            let mut display = control.clone();
            if let Some(properties) = display.as_object_mut() {
                for value in properties.values_mut() {
                    if let Some(text) = value.as_str() {
                        *value = Value::String(
                            text.chars()
                                .flat_map(|character| {
                                    if character.is_control() {
                                        character.escape_default().collect::<Vec<_>>()
                                    } else {
                                        vec![character]
                                    }
                                })
                                .collect(),
                        );
                    }
                }
            }
            render_value(&display, 0, &mut lines);
        }
        if let Some(warning) = data["warning"]["message"].as_str() {
            lines.push(format!("警告: {warning}"));
        }
        if let Some(tree) = crate::win32_snapshot::render(&data["snapshot"]) {
            lines.push(String::new());
            lines.push(tree);
        }
        return lines.join("\n");
    }
    if data["command"] == "flow.params.types" {
        let rows: Vec<Value> = data["types"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| {
                serde_json::json!({
                    "id":item["id"],"name":item["name"],
                    "scope":match item["scope"].as_str() {
                        Some("appMainFlowParameter") => "应用主流程参数",
                        Some(scope) => scope,
                        None => "未声明"
                    }
                })
            })
            .collect();
        let mut lines = Vec::new();
        render_value(
            &serde_json::json!({"count":rows.len(),"types":rows}),
            0,
            &mut lines,
        );
        if let Some(note) = data["note"].as_str() {
            lines.push(note.into());
        }
        return lines.join("\n");
    }
    if let Some(command) = data
        .get("command")
        .and_then(Value::as_str)
        .filter(|c| c.starts_with("web."))
    {
        return render_web(data, command);
    }
    if let Some(markdown) = data.get("markdown").and_then(Value::as_str) {
        return markdown.into();
    }
    // codeflow.read returns source in content. Do not indent or JSON-escape source.
    if let Some(content) = data.get("content").and_then(Value::as_str) {
        return content.into();
    }
    let mut lines = Vec::new();
    render_value(data, 0, &mut lines);
    lines.join("\n")
}

fn render_client(data: &Value) -> String {
    let known = |value: &Value| value.as_str().unwrap_or("未知").to_owned();
    let mut lines = vec![
        format!(
            "影刀客户端: {}（{}）",
            known(&data["client"]["version"]),
            match data["client"]["running"].as_bool() {
                Some(true) => "已发现运行进程",
                Some(false) => "未运行",
                None => "进程查询失败",
            }
        ),
        format!("安装目录: {}", known(&data["client"]["install_dir"])),
        format!("版本来源: {}", known(&data["client"]["version_source"])),
        format!("xbot-cli: {}", known(&data["cli"]["version"])),
        format!("CLI 路径: {}", known(&data["cli"]["executable"])),
    ];
    if let Some(processes) = data["client"]["processes"].as_array() {
        let rows: Vec<Value> = processes
            .iter()
            .map(|p| {
                serde_json::json!({
                    "PID":p["pid"],"进程":p["name"],"版本":p["version"],"路径":p["executable"]
                })
            })
            .collect();
        render_value(&serde_json::json!({"进程":rows}), 0, &mut lines);
        for process in processes {
            if let Some(error) = process["path_error"].as_str() {
                lines.push(format!(
                    "PID {} 路径查询失败: {error}",
                    scalar(&process["pid"])
                ));
            }
        }
    }
    if let Some(error) = data["client"]["error"]["message"].as_str() {
        lines.push(format!("进程查询: {error}"));
    }
    lines.push(format!(
        "shell-cli: {}（{}）",
        known(&data["shell_cli"]["backend"]["path"]),
        known(&data["shell_cli"]["backend"]["source"])
    ));
    for (key, label) in [("shell_cli", "客户端状态"), ("studio_mcp", "MCP 连接")] {
        if data[key]["ok"] != true {
            lines.push(format!(
                "{label}: [{}] {}",
                known(&data[key]["error"]["code"]),
                known(&data[key]["error"]["message"])
            ));
        }
    }
    if data["shell_cli"]["ok"] == true {
        let state = &data["shell_cli"]["state"];
        lines.push(format!(
            "客户端状态: {} / Studio={} / 运行任务={}",
            known(&state["currentMode"]),
            scalar(&state["hasStudioOpened"]),
            scalar(&state["hasRunningTask"])
        ));
    }
    if data["studio_mcp"]["ok"] == true {
        let connection = &data["studio_mcp"]["connection"];
        lines.push(format!(
            "MCP: {}（mode={}） {}",
            known(&connection["provider"]),
            known(&data["mcp_mode"]),
            known(&connection["endpoint"])
        ));
    }
    if data["partial"] == true {
        lines.push("部分信息不可用；已保留其余查询结果，详见 --json。".into());
    }
    if let Some(notes) = data["notes"].as_array() {
        lines.extend(notes.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    lines.join("\n")
}

fn render_windows(data: &Value) -> String {
    let rows = data["windows"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if rows.is_empty() {
        return "未找到匹配窗口".into();
    }
    // Keep long titles in the table. Escape control characters and separators
    // for terminal output, while retaining the exact strings in JSON.
    fn cell(value: &Value) -> String {
        scalar(value)
            .chars()
            .map(|character| match character {
                '|' => "\\|".into(),
                control if control.is_control() => control.escape_default().to_string(),
                other => other.to_string(),
            })
            .collect()
    }
    let mut lines = vec![
        format!("找到 {} 个窗口", rows.len()),
        "PID | HWND | HWND_HEX | 状态 | 进程名 | 窗口标题".into(),
        "--- | --- | --- | --- | --- | ---".into(),
    ];
    for window in rows {
        let state = if window["visible"] == false {
            "隐藏"
        } else if window["minimized"] == true {
            "最小化"
        } else {
            "可见"
        };
        lines.push(format!(
            "{} | {} | {} | {} | {} | {}",
            cell(&window["pid"]),
            cell(&window["hwnd"]),
            cell(&window["hwnd_hex"]),
            state,
            cell(&window["process_name"]),
            cell(&window["title"])
        ));
    }
    lines.join("\n")
}

/// Show the observed accessibility tree directly. The JSON renderer retains
/// the complete action response, snapshot metadata, and warning evidence.
fn render_web(data: &Value, command: &str) -> String {
    if command == "web.browser.list" {
        let rows: Vec<Value> = data["browsers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|browser| {
                let status = if browser["registered"] == true {
                    "已注册"
                } else if browser["installed"] == true {
                    "已安装"
                } else {
                    "未安装"
                };
                serde_json::json!({
                    "id":browser["id"].as_str().unwrap_or("?"),
                    "name":browser["name"].as_str().unwrap_or("?"),
                    "status":status,
                    "webSupported":if browser["webSupported"] == true { "支持" } else { "不支持" }
                })
            })
            .collect();
        let mut lines = Vec::new();
        render_value(
            &serde_json::json!({"count":rows.len(),"browsers":rows}),
            0,
            &mut lines,
        );
        if let Some(note) = data["note"].as_str() {
            lines.push(note.into());
        }
        return lines.join("\n");
    }
    if command == "web.xpath" {
        if let Some(paths) = data["xpaths"].as_array().filter(|a| !a.is_empty()) {
            let mut lines: Vec<&str> = data["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            lines.extend(paths.iter().filter_map(Value::as_str));
            return lines.join("\n");
        }
        return format!(
            "无法生成 XPath: {}",
            data["reason"].as_str().unwrap_or("没有可靠的语义候选")
        );
    }
    let status = match command {
        "web.open" => "已打开网页",
        "web.active" => "已连接当前浏览器页",
        "web.snapshot" => "页面快照",
        "web.click" => "已点击元素",
        "web.dblclick" => "已双击元素",
        "web.fill" => "已填写输入框",
        "web.type" => "已输入文字",
        "web.press" => "已按键",
        "web.hover" => "已悬停元素",
        "web.check" => "已勾选",
        "web.uncheck" => "已取消勾选",
        "web.select" => "已选择选项",
        "web.goto" => "已跳转网页",
        "web.reload" => "已刷新网页",
        "web.go-back" => "已返回上一页",
        "web.go-forward" => "已前进到下一页",
        "web.close" => "已关闭页面",
        _ => "网页操作已完成",
    };
    let mut lines = vec![status.to_owned()];
    let snapshot = if command == "web.snapshot" {
        Some(data)
    } else {
        data.get("snapshot")
    };
    let context = snapshot
        .and_then(|s| s.get("browser_context"))
        .or_else(|| data.get("browser_context"));
    if let Some(url) = data
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        lines.push(format!("URL: {url}"));
    }
    if let Some(context) = context {
        if let Some(tab) = context["tab_id"].as_i64() {
            lines.push(format!("标签页: {tab}"));
        }
        let product = context
            .get("product_name")
            .and_then(Value::as_str)
            .or_else(|| context.get("mode").and_then(Value::as_str));
        if let Some(product) = product.filter(|s| !s.is_empty()) {
            let id = context.get("browser_id").filter(|v| !v.is_null());
            lines.push(match id {
                Some(id) => format!("浏览器: {product} ({})", scalar(id)),
                None => format!("浏览器: {product}"),
            });
        }
    }
    if let Some(python) = data.get("python") {
        lines.push(String::new());
        if let Some(code) = python["code"].as_str() {
            lines.push("影刀 Python：".into());
            lines.push("```python".into());
            lines.push(code.to_owned());
            lines.push("```".into());
        } else if let Some(reason) = python["reason"].as_str() {
            lines.push(format!("Python 未生成: {reason}"));
        }
        if let Some(requires) = python["requires"].as_array() {
            lines.push(format!(
                "依赖: {}",
                requires
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if let Some(warnings) = python["warnings"].as_array() {
            for warning in warnings.iter().filter_map(Value::as_str) {
                lines.push(format!("代码提示: {warning}"));
            }
        }
        if let Some(xpath) = python["locator"]["shadowbot_xpath"].as_str() {
            lines.push("XPath（影刀语法，请使用影刀验证；不是普通浏览器 XPath）:".into());
            lines.push(xpath.to_owned());
        }
    }
    if let Some(snapshot) = snapshot {
        if snapshot
            .pointer("/acc_tree_meta/kind")
            .and_then(Value::as_str)
            == Some("diff")
        {
            let index = snapshot
                .pointer("/acc_tree_meta/diff_index")
                .and_then(Value::as_u64);
            lines.push(match index {
                Some(index) => format!("快照: 增量 #{index}"),
                None => "快照: 增量".into(),
            });
        }
        if let Some(text) = crate::web_cmds::snapshot_text(snapshot) {
            lines.push(String::new());
            lines.push(
                crate::web_snapshot::render(snapshot, &data["snapshot_display"])
                    .unwrap_or_else(|| text.to_owned()),
            );
        }
        if let Some(warnings) = snapshot["frame_warnings"].as_array() {
            for warning in warnings.iter().filter_map(Value::as_str) {
                lines.push(format!("Frame 警告: {warning}"));
            }
        }
    }
    if let Some(warning) = data["session_warning"].as_str() {
        lines.push(format!("会话警告: {warning}"));
    }
    if let Some(path) = data.get("snapshot_file").and_then(Value::as_str) {
        lines.push(format!("快照已保存: {path}"));
    }
    if let Some(warning) = data.get("warning") {
        if let Some(message) = warning.get("message").and_then(Value::as_str) {
            lines.push(format!("警告: {message}"));
        }
        if let Some(message) = warning.pointer("/cause/message").and_then(Value::as_str) {
            lines.push(format!("原因: {message}"));
        }
        if let Some(hint) = warning.get("hint").and_then(Value::as_str) {
            lines.push(format!("提示: {hint}"));
        }
    }
    lines.join("\n")
}

fn render_value(value: &Value, indent: usize, lines: &mut Vec<String>) {
    let padding = " ".repeat(indent);
    match value {
        Value::Object(fields) if fields.is_empty() => lines.push(format!("{padding}(空)")),
        Value::Object(fields) => {
            for (key, value) in fields {
                if is_single_line(value) {
                    lines.push(format!("{padding}{key}: {}", scalar(value)));
                } else {
                    lines.push(format!("{padding}{key}:"));
                    render_value(value, indent + 2, lines);
                }
            }
        }
        Value::Array(items) if items.is_empty() => lines.push(format!("{padding}(无)")),
        Value::Array(items) => {
            if let Some(table) = render_table(items, &padding) {
                lines.extend(table);
                return;
            }
            for (index, item) in items.iter().enumerate() {
                if is_single_line(item) {
                    lines.push(format!("{padding}- {}", scalar(item)));
                } else {
                    lines.push(format!("{padding}[{}]", index + 1));
                    render_value(item, indent + 2, lines);
                }
            }
        }
        Value::String(text) => {
            for line in text.lines() {
                lines.push(format!("{padding}{line}"));
            }
            if text.is_empty() {
                lines.push(padding);
            }
        }
        value => lines.push(format!("{padding}{}", scalar(value))),
    }
}

/// Compact rectangular data gets a table; nested or lengthy rows use the object
/// tree instead. No result is truncated to fit a terminal.
fn render_table(items: &[Value], padding: &str) -> Option<Vec<String>> {
    let first = items.first()?.as_object()?;
    if first.is_empty() || first.len() > 6 {
        return None;
    }
    let keys: Vec<&str> = first.keys().map(String::as_str).collect();
    let mut rows = Vec::new();
    for item in items {
        let object = item.as_object()?;
        if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
            return None;
        }
        let mut row = Vec::new();
        for key in &keys {
            let value = &object[*key];
            if !is_single_line(value) {
                return None;
            }
            let cell = scalar(value);
            if cell.contains('|') || cell.chars().count() > 72 {
                return None;
            }
            row.push(cell);
        }
        rows.push(row.join(" | "));
    }
    let mut table = vec![
        format!("{padding}{}", keys.join(" | ")),
        format!("{padding}{}", vec!["---"; keys.len()].join(" | ")),
    ];
    table.extend(rows.into_iter().map(|row| format!("{padding}{row}")));
    Some(table)
}

fn is_single_line(value: &Value) -> bool {
    match value {
        Value::Array(_) | Value::Object(_) => false,
        Value::String(text) => !text.contains(['\n', '\r']),
        _ => true,
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "—".into(),
        _ => value.to_string(),
    }
}

fn render_error(envelope: &Value) -> String {
    let error = &envelope["error"];
    let code = error["code"].as_str().unwrap_or("ERROR");
    let message = error["message"].as_str().unwrap_or("命令执行失败");
    // Playwright call logs can contain long DOM excerpts and stack traces.
    // Keep the failure reason in text; JSON retains the complete backend error.
    let message = if matches!(
        code,
        "MCP_TOOL_FAILED" | "WEB_BRIDGE_FAILED" | "WEB_BRIDGE_EVAL_FAILED"
    ) {
        message
            .split_once("\nCall log:")
            .map_or(message, |(reason, _)| reason.trim_end())
    } else {
        message
    };
    let mut lines = vec![format!("错误 [{code}]: {message}")];
    if let Some(data) = envelope.get("data") {
        if let Some(usage) = data.get("usage").and_then(Value::as_str) {
            lines.push(format!("\n{usage}"));
        }
        if code == "DOCTOR_FAILED" {
            for channel in ["shell_cli", "studio_mcp"] {
                let result = &data[channel];
                lines.push(format!(
                    "{channel}: {}",
                    if result["ok"] == true {
                        "正常"
                    } else {
                        "失败"
                    }
                ));
                for (label, value) in [
                    ("实际后端", &result["backend"]["path"]),
                    ("发现来源", &result["backend"]["source"]),
                    ("服务地址", &result["endpoint"]),
                    ("原因", &result["error"]),
                ] {
                    if let Some(text) = value.as_str() {
                        lines.push(format!("  {label}: {text}"));
                    }
                }
            }
        } else {
            let mut recovery = Vec::new();
            collect_recovery(data, "", &mut recovery);
            lines.extend(recovery);
        }
    }
    if let Some(hint) = error["hint"].as_str().filter(|hint| !hint.is_empty()) {
        lines.push(format!("提示: {hint}"));
    }
    lines.push("可用 --json 输出完整错误详情。".into());
    lines.join("\n")
}

fn is_recovery_field(key: &str) -> bool {
    matches!(
        key,
        "applied"
            | "operation"
            | "outcome"
            | "run_id"
            | "task_id"
            | "app_id"
            | "expected_app_id"
            | "actual_app_id"
            | "flow_id"
            | "resume_command"
            | "confirmed"
            | "uploaded"
            | "closed"
            | "active"
            | "status"
            | "run_succeeded"
            | "user_paused"
            | "user_stopped"
            | "path"
            | "risk"
            | "expected_kind"
            | "requested"
            | "active_flow_id"
            | "candidates"
    )
}

/// Keep actionable evidence without dumping a backend response or captured
/// process output. Wrapper traversal also preserves nested unknown outcomes.
fn collect_recovery(value: &Value, prefix: &str, lines: &mut Vec<String>) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, value) in object {
        let name = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if key == "backend" {
            for field in ["path", "source", "exists"] {
                if let Some(value) = value.get(field) {
                    lines.push(format!("{name}.{field}: {}", scalar(value)));
                }
            }
        } else if is_recovery_field(key) {
            if is_single_line(value) {
                let text = if key == "outcome" && value.as_str() == Some("unknown") {
                    "unknown (结果未知，请先查询状态，避免重复执行)".into()
                } else {
                    scalar(value)
                };
                lines.push(format!("{name}: {text}"));
            } else {
                lines.push(format!("{name}:"));
                render_value(value, 2, lines);
            }
        } else if matches!(
            key.as_str(),
            "data" | "run" | "task" | "result" | "cause" | "response" | "process"
        ) {
            collect_recovery(value, &name, lines);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn doctor_failure_prints_resolved_backend_and_specific_fix_in_text() {
        let text = render(
            &json!({
                "ok":false,
                "error":{"code":"DOCTOR_FAILED","message":"unavailable","hint":"修正 XBOT_SHELL_CLI 或移除覆盖"},
                "data":{"shell_cli":{"ok":false,"backend":{"path":r"D:\Custom Dir\shell.exe","source":"environment"},"error":"not found"},
                "studio_mcp":{"ok":true}}
            }),
            OutputMode::Text,
        );
        assert!(text.contains(r"D:\Custom Dir\shell.exe"));
        assert!(text.contains("environment"));
        assert!(text.contains("not found"));
        assert!(text.contains("XBOT_SHELL_CLI"));
        assert!(!text.contains("app open"));
    }

    #[test]
    fn automatic_mode_only_uses_terminal_for_default() {
        assert_eq!(resolve_mode("auto", false, true), OutputMode::Text);
        assert_eq!(resolve_mode("auto", false, false), OutputMode::Json);
        for terminal in [true, false] {
            assert_eq!(resolve_mode("text", false, terminal), OutputMode::Text);
            assert_eq!(resolve_mode("json", false, terminal), OutputMode::Json);
            assert_eq!(resolve_mode("pretty", false, terminal), OutputMode::Pretty);
            assert_eq!(resolve_mode("text", true, terminal), OutputMode::Json);
        }
    }

    #[test]
    fn json_and_pretty_preserve_full_envelope() {
        let value =
            json!({"ok":false,"error":{"code":"FAILED"},"data":{"process":{"stdout":"debug"}}});
        assert_eq!(render(&value, OutputMode::Json), value.to_string());
        let pretty = render(&value, OutputMode::Pretty);
        assert_eq!(serde_json::from_str::<Value>(&pretty).unwrap(), value);
        assert!(pretty.contains('\n'));
    }

    #[test]
    fn guide_and_source_preserve_raw_text() {
        let markdown = "# Guide\n\nUse `xbot-cli status`.\n";
        let source = "def main():\n    print('你好')\n";
        assert_eq!(
            render(
                &json!({"ok":true,"data":{"topic":"cli","markdown":markdown}}),
                OutputMode::Text
            ),
            markdown
        );
        assert_eq!(
            render(
                &json!({"ok":true,"data":{"flow_id":"main","content":source}}),
                OutputMode::Text
            ),
            source
        );
    }

    #[test]
    fn parameter_types_are_a_table_with_unspecified_scopes_labeled_honestly() {
        let value = json!({"ok":true,"data":{
            "command":"flow.params.types","types":[
                {"id":"list","name":"列表","scope":null,"source":"long resource name"},
                {"id":"file","name":"文件路径","scope":"appMainFlowParameter","source":"long resource name"}
            ],"note":"static catalog"
        }});
        let text = render(&value, OutputMode::Text);
        assert!(text.contains("id | name | scope"));
        assert!(text.contains("list | 列表 | 未声明"));
        assert!(text.contains("file | 文件路径 | 应用主流程参数"));
        assert!(!text.contains("long resource name"));
        assert_eq!(render(&value, OutputMode::Json), value.to_string());
    }

    #[test]
    fn tables_and_nested_results_are_readable_without_losing_records() {
        let text = render(
            &json!({"ok":true,"data":{
                "flows":[{"id":"one","name":"第一"},{"id":"two","name":"第二"}],
                "state":{"active":false},"notes":["first","second"]
            }}),
            OutputMode::Text,
        );
        assert!(text.contains("id | name\n  --- | ---\n  one | 第一\n  two | 第二"));
        assert!(text.contains("state:\n  active: false"));
        assert!(text.contains("notes:\n  - first\n  - second"));
        assert!(!text.contains("\u{1b}"));
    }

    #[test]
    fn window_table_keeps_long_titles_and_escapes_terminal_controls_without_changing_json() {
        let title = format!("{}|\n\t\u{1b}[31m", "很长的窗口标题".repeat(25));
        let value = json!({"ok":true,"data":{"command":"win32.list","windows":[
            {"pid":123,"hwnd":456,"hwnd_hex":"0x1c8","title":title,"process_name":null,"visible":true,"minimized":false},
            {"pid":124,"hwnd":457,"hwnd_hex":"0x1c9","title":"隐藏窗口","process_name":"fixture.exe","visible":false,"minimized":false}
        ]}});
        let text = render(&value, OutputMode::Text);
        assert!(text.contains("PID | HWND | HWND_HEX | 状态 | 进程名 | 窗口标题"));
        assert!(text.contains("123 | 456 | 0x1c8 | 可见 | —"));
        assert!(text.contains(&"很长的窗口标题".repeat(25)));
        assert!(text.contains("\\|\\n\\t\\u{1b}[31m"));
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("隐藏窗口"));
        assert_eq!(render(&value, OutputMode::Json), value.to_string());
        let empty = json!({"ok":true,"data":{"command":"win32.list","windows":[]}});
        assert_eq!(render(&empty, OutputMode::Text), "未找到匹配窗口");
    }

    #[test]
    fn windows_snapshot_text_is_a_tree_while_json_retains_provider_metadata() {
        let value = json!({"ok":true,"data":{
            "backend":"uiautomation","snapshot_id":"snap-1","traversal_complete":true,
            "scope":{"process_id":123,"hwnd":"0x456"},"nodes":[
                {"ref":"w1","parent_ref":null,"control_type":"WindowControl","name":"窗口","is_password":false},
                {"ref":"w2","parent_ref":"w1","control_type":"ButtonControl","name":"确定","is_password":false,
                    "runtime_id":[1,2,3],"bounds":[10,20,30,40]}
            ],"backend_version":"test-provider","limits":{"max_nodes":1000}
        }});
        let text = render(&value, OutputMode::Text);
        assert!(text.contains("window \"窗口\" [ref=w1]\n└─ button \"确定\" [ref=w2]"));
        assert!(!text.contains("runtime_id"));
        assert!(!text.contains("test-provider"));
        for mode in [OutputMode::Json, OutputMode::Pretty] {
            assert_eq!(
                serde_json::from_str::<Value>(&render(&value, mode)).unwrap(),
                value
            );
        }
    }

    #[test]
    fn windows_action_shows_only_the_new_tree_and_keeps_followup_warning() {
        let value = json!({"ok":true,"data":{
            "command":"win32.click","source_snapshot_id":"old-snap","ref":"w4","applied":true,
            "warning":{"message":"只补观察，不要重复动作"},
            "snapshot":{"backend":"uiautomation","snapshot_id":"new-snap",
                "traversal_complete":true,"refs_resolvable_for_actions":true,
                "nodes":[{"ref":"w1","parent_ref":null,"control_type":"WindowControl",
                    "name":"窗口","is_password":false}]}
        }});
        let text = render(&value, OutputMode::Text);
        assert!(text.contains("动作已返回"));
        assert!(text.contains("old-snap"));
        assert!(text.contains("new-snap"));
        assert!(text.contains("不要重复动作"));
        assert!(text.contains("--snapshot"));
        assert_eq!(text.matches("window \"窗口\"").count(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(&render(&value, OutputMode::Json)).unwrap(),
            value
        );
    }

    #[test]
    fn windows_click_distinguishes_invoke_from_physical_coordinates() {
        let mut value = json!({"ok":true,"data":{
            "command":"win32.click","source_snapshot_id":"snap","ref":"w11","applied":true,
            "target_control":{"name":"通讯录","control_type":"ButtonControl","bounds":[397,362,472,407]},
            "execution":{"method":"invoke","mouse_input":false,"effect_verified":false}
        }});
        let text = render(&value, OutputMode::Text);
        assert!(text.contains("未发送鼠标点击"));
        assert!(text.contains("通讯录"));
        assert!(text.contains("未自动验证"));
        assert!(!text.contains("操作位置:"));
        value["data"]["execution"] = json!({"method":"physical","mouse_input":true,
            "effect_verified":false,"point":[434,384],"coordinate_space":"screen_physical_pixels"});
        let text = render(&value, OutputMode::Text);
        assert!(text.contains("方式: physical"));
        assert!(text.contains("434"));
        assert!(text.contains("384"));
        assert!(text.contains("屏幕物理像素"));
        assert!(!text.contains("未发送鼠标点击"));
        assert_eq!(
            serde_json::from_str::<Value>(&render(&value, OutputMode::Json)).unwrap(),
            value
        );
    }

    #[test]
    fn browser_list_uses_the_shared_table_without_changing_json() {
        let value = json!({"ok":true,"data":{
            "command":"web.browser.list","count":3,"browsers":[
                {"id":"edge","name":"Microsoft Edge","installed":true,"registered":false,
                    "webSupported":true,"exePath":r"C:\Browsers\msedge.exe"},
                {"id":"custom","name":"自定义浏览器","installed":null,"registered":true,
                    "webSupported":true,"exePath":r"C:\Browsers\custom.exe"},
                {"id":"firefox","name":"Firefox","installed":false,"registered":false,
                    "webSupported":false,"exePath":r"C:\Browsers\firefox.exe"}
            ],"note":"注册不代表插件已启用或已连接"
        }});
        let text = render(&value, OutputMode::Text);
        assert_eq!(
            text,
            "browsers:\n  id | name | status | webSupported\n  --- | --- | --- | ---\n  edge | Microsoft Edge | 已安装 | 支持\n  custom | 自定义浏览器 | 已注册 | 支持\n  firefox | Firefox | 未安装 | 不支持\ncount: 3\n注册不代表插件已启用或已连接"
        );
        assert!(!text.contains('\t'));
        assert!(!text.contains("exePath"));
        assert_eq!(
            serde_json::from_str::<Value>(&render(&value, OutputMode::Json)).unwrap(),
            value
        );
    }

    #[test]
    fn empty_browser_list_is_readable() {
        let text = render(
            &json!({"ok":true,"data":{
                "command":"web.browser.list","count":0,"browsers":[]
            }}),
            OutputMode::Text,
        );
        assert_eq!(text, "browsers:\n  (无)\ncount: 0");
    }

    #[test]
    fn text_errors_keep_recovery_without_backend_dump() {
        let (value, exit) = core::envelope(false, None, Some(
            BackendError::new("FLOW_RUN_TIMEOUT", "等待超时")
                .with_hint("继续等待同一次运行")
                .with_data(json!({
                    "applied":true,"run_id":"run-1","task_id":"task-1",
                    "resume_command":"xbot-cli flow wait run-1",
                    "cause":{"data":{"process":{"outcome":"unknown","stdout":"DEBUG_SECRET","stderr":"RAW_TRACE"}}},
                    "unrelated":{"large":"RAW_OBJECT"}
                }))
        ));
        let text = render(&value, OutputMode::Text);
        assert_eq!(exit, 4);
        for expected in [
            "错误 [FLOW_RUN_TIMEOUT]: 等待超时",
            "applied: true",
            "run_id: run-1",
            "task_id: task-1",
            "xbot-cli flow wait run-1",
            "结果未知",
            "继续等待同一次运行",
            "--json",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        for hidden in [
            "DEBUG_SECRET",
            "RAW_TRACE",
            "RAW_OBJECT",
            "stdout:",
            "stderr:",
        ] {
            assert!(!text.contains(hidden), "unexpected {hidden}: {text}");
        }
        assert!(render(&value, OutputMode::Json).contains("DEBUG_SECRET"));
    }

    #[test]
    fn web_text_shows_snapshot_once_without_dumping_backend_objects() {
        let tree = "### AccTree\n- textbox Name [e4]\n- button Greet [e5]";
        let value = json!({"ok":true,"data":{
            "command":"web.open","applied":true,"url":"https://example.com/",
            "browser_context":{"mode":"edge","product_name":"Microsoft Edge","browser_id":42},
            "result":{"debug":"BACKEND_OBJECT","image":"data:image/png;base64,DO_NOT_PRINT"},
            "snapshot":{"success":true,"result":tree,"acc_tree_meta":{"kind":"diff","diff_index":3},
                "browser_context":{"mode":"edge","product_name":"Microsoft Edge","browser_id":42}}
        }});
        let text = render(&value, OutputMode::Text);
        for expected in [
            "已打开网页",
            "URL: https://example.com/",
            "浏览器: Microsoft Edge (42)",
            "快照: 增量 #3",
            tree,
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert_eq!(text.matches(tree).count(), 1);
        assert_eq!(text.matches("Microsoft Edge").count(), 1);
        for hidden in [
            "BACKEND_OBJECT",
            "DO_NOT_PRINT",
            "success:",
            "browser_context:",
            "applied:",
        ] {
            assert!(!text.contains(hidden), "unexpected {hidden}: {text}");
        }
        assert!(render(&value, OutputMode::Json).contains("DO_NOT_PRINT"));
    }

    #[test]
    fn python_code_precedes_snapshot_and_does_not_replace_it() {
        let value = json!({"ok":true,"data":{"command":"web.click",
            "python":{"status":"ready","code":"web_page.find_by_xpath(\"//button\").click()"},
            "snapshot":{"result":"- button Example [ref=e4]"}}});
        let text = render(&value, OutputMode::Text);
        assert!(text.contains("影刀 Python："));
        assert!(!text.contains("独立预览版"));
        assert!(text.find("```python").unwrap() < text.find("[ref=e4]").unwrap());
        assert_eq!(text.matches("[ref=e4]").count(), 1);
    }

    #[test]
    fn shadowbot_paths_are_labeled_even_when_python_is_unavailable() {
        let path = "//x-form/xbotShadowRoot//input";
        let notice = "影刀语法，请使用影刀验证";
        let xpath = json!({"ok":true,"data":{"command":"web.xpath",
            "warnings":[notice],"xpaths":[path]}});
        assert_eq!(
            render(&xpath, OutputMode::Text),
            format!("{notice}\n{path}")
        );
        let action = json!({"ok":true,"data":{"command":"web.fill",
            "python":{"status":"unavailable","reason":"跨域适配器不可用",
            "locator":{"shadowbot_xpath":path}}}});
        let text = render(&action, OutputMode::Text);
        assert!(text.contains(notice));
        assert!(text.contains(path));
    }

    #[test]
    fn closed_shadow_refusals_are_prominent_in_xpath_and_action_output() {
        let reason = crate::web_python::CLOSED_SHADOW_REASON;
        for data in [
            json!({"command":"web.xpath","status":"unavailable","xpaths":[],
                "error_code":"WEB_SHADOW_CLOSED","reason":reason}),
            json!({"command":"web.fill","python":{"status":"unavailable",
                "error_code":"WEB_SHADOW_CLOSED","reason":reason,"code":null}}),
        ] {
            let text = render(&json!({"ok":true,"data":data}), OutputMode::Text);
            assert!(text.contains("【不支持 shadow-root(closed)】"));
            assert!(text.contains("无法生成 XPath 或等价 Python"));
            assert!(!text.contains("```python"));
            assert!(!text.contains("find_by_xpath"));
            assert!(!text.contains("/xbotShadowRoot"));
        }
    }

    #[test]
    fn web_snapshot_and_warning_keep_refs_and_distinguish_success_from_observation_failure() {
        let tree = "### AccTree\n- textbox [e4]";
        let value = json!({"ok":true,"data":{"command":"web.snapshot","result":tree,"snapshot_file":"page.txt"}});
        let text = render(&value, OutputMode::Text);
        assert!(text.contains(tree));
        assert!(text.contains("快照已保存: page.txt"));
        let warning = json!({"ok":true,"data":{"command":"web.click","applied":true,
            "warning":{"message":"网页操作已成功，但后续快照获取失败","hint":"先运行 xbot-cli web snapshot；不要重复点击", "cause":{"message":"offline","data":{"raw":"HIDDEN"}}}}});
        let text = render(&warning, OutputMode::Text);
        assert!(text.starts_with("已点击元素"));
        assert!(text.contains("原因: offline"));
        assert!(text.contains("不要重复点击"));
        assert!(!text.contains("HIDDEN"));
    }
}
