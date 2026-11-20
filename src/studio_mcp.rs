//! Negotiate using read-only discovery before any business call. Never replay a mutation.
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::core::transport::{
    decode_tool_result, mcp_on, notify_initialized, rpc_on, rpc_session, tools_from,
};
use crate::core::{BackendError, Res};

const PROTOCOL: &str = "2025-03-26";
static CLIENT: OnceLock<Mutex<Option<(String, Client)>>> = OnceLock::new();

enum Client {
    Legacy {
        endpoint: String,
    },
    Copilot {
        endpoint: String,
        session: String,
        tools: Vec<Value>,
    },
}

fn with_client<T>(timeout: u64, action: impl FnOnce(&Client) -> Res<T>) -> Res<T> {
    let endpoint = crate::core::mcp_endpoint();
    let mode = std::env::var("XBOT_MCP_MODE").unwrap_or_else(|_| "auto".into());
    let key = format!("{mode}\n{endpoint}");
    let mut cache = CLIENT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| BackendError::new("MCP_STATE", "MCP 会话锁不可用"))?;
    if cache.as_ref().is_none_or(|(cached, _)| cached != &key) {
        *cache = Some((key, Client::connect(&endpoint, &mode, timeout.min(15))?));
    }
    let result = action(&cache.as_ref().expect("connected").1);
    if matches!(cache.as_ref().map(|(_, c)| c), Some(Client::Copilot { .. }))
        && result
            .as_ref()
            .err()
            .is_some_and(|e| session_expired(e) || http_status(e) == Some(404))
    {
        *cache = None;
        return result.map_err(|e| {
            BackendError::new("MCP_SESSION_EXPIRED", "MCP 会话已失效，本次操作未自动重试")
                .with_data(json!({"cause":e.data,"cause_code":e.code,"outcome":"unknown"}))
                .with_retryable(false)
                .with_hint("先核对已有写入或运行结果；后续命令会重新建立会话")
        });
    }
    result
}

fn session_expired(error: &BackendError) -> bool {
    error.code == "-32001"
        || error
            .data
            .as_ref()
            .and_then(|d| d.pointer("/response/error/code"))
            == Some(&json!(-32001))
}

impl Client {
    fn connect(endpoint: &str, mode: &str, timeout: u64) -> Res<Self> {
        if !matches!(mode, "auto" | "legacy" | "copilot") {
            return Err(BackendError::new(
                "BAD_INPUT",
                "XBOT_MCP_MODE 必须是 auto、legacy 或 copilot",
            ));
        }
        if mode == "legacy" {
            return Ok(Self::Legacy {
                endpoint: endpoint.into(),
            });
        }
        if mode == "copilot" || is_copilot_url(endpoint) {
            let target = if mode == "copilot" {
                copilot_url(endpoint).unwrap_or_else(|| endpoint.into())
            } else {
                endpoint.into()
            };
            return Self::initialize(&target, timeout);
        }
        match rpc_on(endpoint, "tools/list", json!({}), timeout) {
            Ok(response) => {
                let tools = tools_from(response)?;
                if tools.iter().any(|t| t["name"] == "xbot-bash") {
                    Self::initialize(endpoint, timeout)
                } else {
                    Ok(Self::Legacy {
                        endpoint: endpoint.into(),
                    })
                }
            }
            Err(error) if session_expired(&error) => Self::initialize(endpoint, timeout),
            Err(error) if http_status(&error) == Some(404) => {
                if let Some(target) = copilot_url(endpoint) {
                    Self::initialize(&target, timeout)
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    fn initialize(endpoint: &str, timeout: u64) -> Res<Self> {
        let (response, session) = rpc_session(
            endpoint,
            "initialize",
            json!({
                "protocolVersion":PROTOCOL,"capabilities":{},
                "clientInfo":{"name":"xbot-cli","version":env!("CARGO_PKG_VERSION")}
            }),
            timeout,
            None,
            None,
        )?;
        if response["protocolVersion"] != PROTOCOL {
            return Err(BackendError::new(
                "MCP_PROTOCOL_UNSUPPORTED",
                "客户端返回了不支持的 MCP 协议版本",
            )
            .with_data(json!({"protocol_version":response["protocolVersion"]})));
        }
        let session = session.filter(|s| !s.is_empty()).ok_or_else(|| {
            BackendError::new("MCP_BAD_RESPONSE", "Copilot MCP 初始化未返回会话 ID")
        })?;
        notify_initialized(endpoint, &session, PROTOCOL, timeout)?;
        let (response, _) = rpc_session(
            endpoint,
            "tools/list",
            json!({}),
            timeout,
            Some(&session),
            Some(PROTOCOL),
        )?;
        let tools = tools_from(response)?;
        if !tools.iter().any(|t| t["name"] == "xbot-bash") {
            return Err(BackendError::new(
                "MCP_PROVIDER_UNSUPPORTED",
                "此 Copilot MCP 未提供 xbot-bash 工具",
            )
            .with_hint("旧版客户端使用 XBOT_MCP_MODE=legacy 和原 Studio MCP 地址"));
        }
        Ok(Self::Copilot {
            endpoint: endpoint.into(),
            session,
            tools,
        })
    }

    fn invoke(&self, tool: &str, args: Value, timeout: u64) -> Res<Value> {
        match self {
            Self::Legacy { endpoint } => {
                let legacy = match tool {
                    "flow.source.read" => "codeflow.read",
                    "flow.source.write" => "codeflow.write",
                    "flow.source.edit" => "codeflow.edit",
                    _ => tool,
                };
                mcp_on(endpoint, legacy, args, timeout)
            }
            Self::Copilot {
                endpoint, session, ..
            } => {
                let (name, arguments) = if matches!(tool, "xbot-bash" | "execute_cell") {
                    (tool, args)
                } else {
                    ("xbot-bash", translate(tool, &args)?)
                };
                let (response, _) = rpc_session(
                    endpoint,
                    "tools/call",
                    json!({"name":name,"arguments":arguments}),
                    timeout,
                    Some(session),
                    Some(PROTOCOL),
                )?;
                let result = decode_tool_result(response).or_else(|error| {
                    // Copilot reports a known runtime failure as an MCP tool error.
                    // Only recover an identified run outcome; command/transport errors stay errors.
                    if is_run_tool(tool) {
                        if let Some(state) = error.data.as_ref().and_then(|d| d.get("result")) {
                            let identified = state["run_id"]
                                .as_str()
                                .is_some_and(|id| !id.trim().is_empty());
                            if identified
                                && matches!(
                                    state["terminal_status"].as_str(),
                                    Some("failed" | "canceled" | "user_stopped" | "user_paused")
                                )
                            {
                                let mut state = state.clone();
                                state["mcp_error"] =
                                    json!({"code":error.code,"message":error.message});
                                return Ok(state);
                            }
                        }
                    }
                    Err(error)
                })?;
                normalize(tool, result)
            }
        }
    }
}

fn http_status(error: &BackendError) -> Option<u64> {
    error.data.as_ref()?.get("http_status")?.as_u64()
}

fn is_copilot_url(endpoint: &str) -> bool {
    url::Url::parse(endpoint).is_ok_and(|u| u.path().ends_with("/copilot-mcp/mcp"))
}

fn copilot_url(endpoint: &str) -> Option<String> {
    let mut url = url::Url::parse(endpoint).ok()?;
    let prefix = url.path().strip_suffix("/api/v1/mcp")?;
    url.set_path(&format!("{prefix}/api/v1/copilot-mcp/mcp"));
    Some(url.into())
}

pub fn call(tool: &str, args: Value, timeout: u64) -> Res<Value> {
    if !args.is_object() {
        return Err(BackendError::new(
            "BAD_ARGS",
            "MCP arguments 必须是 JSON 对象",
        ));
    }
    with_client(timeout, |client| client.invoke(tool, args, timeout))
}

pub fn list(timeout: u64) -> Res<Vec<Value>> {
    with_client(timeout, |client| match client {
        Client::Legacy { endpoint } => {
            tools_from(rpc_on(endpoint, "tools/list", json!({}), timeout)?)
        }
        Client::Copilot { tools, .. } => Ok(tools.clone()),
    })
}

pub fn is_copilot() -> Res<bool> {
    with_client(15, |client| Ok(matches!(client, Client::Copilot { .. })))
}

pub fn connection() -> Res<Value> {
    with_client(10, |client| {
        Ok(match client {
            Client::Legacy { endpoint } => json!({"provider":"legacy","endpoint":endpoint}),
            Client::Copilot { endpoint, .. } => {
                json!({"provider":"copilot","endpoint":endpoint,"session_initialized":true,"protocol_version":PROTOCOL})
            }
        })
    })
}

pub(crate) fn unsupported(tool: &str) -> BackendError {
    BackendError::new("MCP_CAPABILITY_UNAVAILABLE", format!("当前 MCP 不提供此接口: {tool}"))
        .with_data(json!({"tool":tool,"applied":false,"status":"not_started"}))
        .with_hint("6.4 使用 flow source read/write/edit 编辑可视化流程；tool studio list/schema 查看实际工具，guide mcp 查看兼容说明")
}

// ShadowBot's parser accepts quoted strings with backslash/quote escaping and literal newlines.
// Do not use a system shell or JSON string escaping (which would change source newlines).
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn text<'a>(args: &'a Value, key: &str) -> Res<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| BackendError::new("BAD_ARGS", format!("缺少字符串参数 {key}")))
}

fn option(command: &mut String, args: &Value, key: &str, flag: &str) -> Res<()> {
    if let Some(value) = args.get(key).filter(|v| !v.is_null()) {
        let raw = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => return Err(BackendError::new("BAD_ARGS", format!("{key} 必须是标量"))),
        };
        command.push_str(&format!(" --{flag} {}", quote(&raw)));
    }
    Ok(())
}

fn array_option(command: &mut String, args: &Value, key: &str, flag: &str) -> Res<()> {
    let items = args[key]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| BackendError::new("BAD_ARGS", format!("{key} 必须是非空字符串数组")))?;
    command.push_str(&format!(" --{flag}"));
    for item in items {
        let value = item
            .as_str()
            .ok_or_else(|| BackendError::new("BAD_ARGS", format!("{key} 必须是字符串数组")))?;
        command.push_str(&format!(" {}", quote(value)));
    }
    Ok(())
}

fn translate(tool: &str, args: &Value) -> Res<Value> {
    let path = match tool {
        "app.get_info" => "app info",
        "app.list_flows" => "flow list",
        "app.save" => "app save",
        "app.reload" => "app reload",
        "app.create_flow" => "flow create",
        "app.copy_flow" => "flow copy",
        "app.rename_flow" => "flow rename",
        "app.delete_flow" => "flow delete",
        "codeflow.read" | "flow.source.read" => "flow read",
        "codeflow.write" | "flow.source.write" => "flow write",
        "codeflow.edit" | "flow.source.edit" => "flow edit",
        "flow.list_parameters" => "flow parameter list",
        "flow.parameter.set" => "flow parameter set",
        "flow.parameter.remove" => "flow parameter remove",
        "diagnostics.snapshot" => "diagnostics snapshot",
        "app.run" => "run start",
        "app.wait_run_finish" => "run wait",
        "app.run_result" => "run result",
        "app.stop_run" => "run stop",
        "app.logs" => "run logs",
        "flow.run_variables" => "run variables",
        "selector.list" => "selector list",
        "selector.details" => "selector get",
        "app.global_variables.list" => "variable list",
        "app.resource.list" => "resource list",
        "app.resource.add" => "resource add",
        "app.resource.remove" => "resource remove",
        "app.resource.rename" => "resource rename",
        "python_pip" => match args["operation"].as_str().unwrap_or("list") {
            "list" => "python package list",
            "install" => "python package install",
            "uninstall" => "python package uninstall",
            _ => return Err(BackendError::new("BAD_ARGS", "未知的 Python 包操作")),
        },
        _ => return Err(unsupported(tool)),
    };
    let options: &[(&str, &str)] = match tool {
        "app.copy_flow" | "app.rename_flow" => &[("flow_id", "flow"), ("new_name", "name")],
        "codeflow.read" | "flow.source.read" => &[
            ("flow_id", "flow"),
            ("offset", "offset"),
            ("limit", "limit"),
            ("block_nos", "block-nos"),
        ],
        "codeflow.write" | "flow.source.write" | "flow.list_parameters" | "flow.parameter.set" => {
            &[("flow_id", "flow")]
        }
        "codeflow.edit" | "flow.source.edit" => &[
            ("flow_id", "flow"),
            ("old_string", "old-string"),
            ("new_string", "new-string"),
        ],
        "flow.parameter.remove" => &[("flow_id", "flow"), ("name", "name")],
        "app.run" => &[("flow_id", "flow")],
        "app.wait_run_finish" => &[
            ("run_id", "run-id"),
            ("poll_interval_ms", "poll-interval-ms"),
        ],
        "app.run_result" => &[("run_id", "run-id")],
        "flow.run_variables" => &[("flow_id", "flow"), ("block_nos", "block-nos")],
        "app.logs" => &[("cursor", "cursor"), ("limit", "limit")],
        "selector.list" => &[("query", "query"), ("offset", "offset"), ("limit", "limit")],
        "app.global_variables.list" => &[("offset", "offset"), ("limit", "limit")],
        "app.resource.add" => &[("file", "file")],
        "app.resource.rename" => &[("name", "name"), ("new_name", "new-name")],
        "python_pip" => &[("verbosity", "verbosity"), ("index_url", "index-url")],
        _ => &[],
    };
    let special: &[&str] = match tool {
        "app.create_flow" => &["flows"],
        "app.delete_flow" => &["flow_ids"],
        "app.rename_flow" => &["force"],
        "selector.details" => &["selector_names"],
        "app.resource.remove" => &["files"],
        "python_pip" => &["operation", "packages"],
        "codeflow.write" | "flow.source.write" => &["content"],
        "codeflow.edit" | "flow.source.edit" => &["replace_all"],
        "flow.parameter.set" => &["parameters"],
        "app.run" => &["wait_timeout", "suppress_error_dialogs"],
        "app.wait_run_finish" => &["timeout"],
        _ => &[],
    };
    for key in args.as_object().expect("object validated").keys() {
        if !options.iter().any(|(name, _)| name == key) && !special.contains(&key.as_str()) {
            return Err(unsupported(tool).with_hint(format!("6.4 兼容映射不支持参数 {key}；未执行操作。请查看 tool studio schema xbot-bash 和 guide mcp")));
        }
    }
    if args["force"] == true {
        return Err(unsupported(tool).with_hint("6.4 接口未提供旧版 force 参数；未执行重命名"));
    }
    let mut command = format!("xbot-bash {path}");
    let mut stdin = None;
    match tool {
        "app.create_flow" => {
            let flows = args["flows"]
                .as_array()
                .filter(|f| f.len() == 1)
                .ok_or_else(|| {
                    BackendError::new("BAD_ARGS", "6.4 每次创建一个流程；未执行批量创建")
                })?;
            option(&mut command, &flows[0], "flow_name", "name")?;
            option(&mut command, &flows[0], "kind", "kind")?;
        }
        "app.delete_flow" => {
            let ids = args["flow_ids"]
                .as_array()
                .filter(|f| f.len() == 1)
                .ok_or_else(|| {
                    BackendError::new("BAD_ARGS", "6.4 每次删除一个流程；未执行批量删除")
                })?;
            command.push_str(&format!(
                " --flow {}",
                quote(ids[0].as_str().ok_or_else(|| unsupported(tool))?)
            ));
        }
        "selector.details" => array_option(&mut command, args, "selector_names", "name")?,
        "app.resource.remove" => array_option(&mut command, args, "files", "files")?,
        "python_pip" => {
            let operation = args["operation"].as_str().unwrap_or("list");
            let packages = crate::python_cmds::package_specs(args, operation)?;
            if !packages.is_empty() {
                array_option(
                    &mut command,
                    &json!({"packages":packages}),
                    "packages",
                    "packages",
                )?;
            }
        }
        _ => {}
    }
    for (key, flag) in options {
        option(&mut command, args, key, flag)?;
    }
    if tool == "codeflow.read" && args.get("limit").is_none() {
        command.push_str(" --limit 2000");
    }
    if args["replace_all"] == true {
        command.push_str(" --replace-all");
    }
    if matches!(tool, "codeflow.write" | "flow.source.write") {
        stdin = Some(text(args, "content")?.to_owned());
        command.push_str(" --stdin");
    }
    if tool == "flow.parameter.set" {
        let parameters = args["parameters"]
            .as_array()
            .filter(|a| !a.is_empty())
            .ok_or_else(|| unsupported(tool))?;
        for parameter in parameters {
            let fields = parameter.as_object().ok_or_else(|| unsupported(tool))?;
            if fields.keys().any(|key| {
                ![
                    "name",
                    "direction",
                    "type",
                    "value_kind",
                    "value",
                    "description",
                ]
                .contains(&key.as_str())
            }) {
                return Err(unsupported(tool).with_hint("6.4 参数组含未映射字段，未执行操作"));
            }
            // ShadowBot's repeated parameter groups explicitly reject stdin / @stdin.
            for (key, flag) in [
                ("name", "name"),
                ("direction", "direction"),
                ("type", "type"),
                ("value_kind", "value-kind"),
                ("value", "value"),
                ("description", "description"),
            ] {
                option(&mut command, parameter, key, flag)?;
            }
        }
    }
    if tool == "app.run" {
        let seconds = args["wait_timeout"].as_u64().unwrap_or(90).min(110);
        command.push_str(&format!(" --timeout {seconds}"));
        if let Some(suppress) = args["suppress_error_dialogs"].as_bool() {
            command.push_str(&format!(
                " --suppress-error-dialogs {}",
                quote(&suppress.to_string())
            ));
        }
    } else if tool == "app.wait_run_finish" {
        let seconds = args["timeout"].as_u64().unwrap_or(90).clamp(1, 110);
        command.push_str(&format!(" --timeout {seconds}"));
    }
    let mut value = json!({"command":command,"explanation":"执行影刀工作区操作"});
    if let Some(input) = stdin {
        value["stdin"] = json!(input);
    }
    Ok(value)
}

fn normalize(tool: &str, mut result: Value) -> Res<Value> {
    if !matches!(tool, "xbot-bash" | "execute_cell") && !result.is_object() {
        return Err(
            BackendError::new("MCP_BAD_RESPONSE", "MCP 命令结果缺少对象结构").with_data(result),
        );
    }
    if tool == "app.get_info" {
        if let Some(name) = result.pointer("/metadata/name").cloned() {
            if result["app"].is_object() {
                result["app"]["name"] = name;
            }
        }
    }
    if tool == "app.create_flow" && result.get("flow_id").is_some() {
        return Ok(json!({"success":true,"results":[result]}));
    }
    if tool == "flow.list_parameters" && result.get("items").is_none() {
        if let Some(parameters) = result.get("parameters").filter(|p| p.is_array()).cloned() {
            result["items"] = parameters;
        }
    }
    if tool == "flow.list_parameters" {
        if let Some(items) = result["items"].as_array_mut() {
            for item in items {
                if item.get("kind").is_none() {
                    if let Some(kind) = item.get("value_kind").cloned() {
                        item["kind"] = kind;
                    }
                }
            }
        }
    }
    if tool == "diagnostics.snapshot" && result.get("static_errors").is_none() {
        if let Some(errors) = result
            .pointer("/static_check/errors")
            .or_else(|| result.get("errors"))
            .cloned()
        {
            result["static_errors"] = errors;
        }
    }
    if is_run_tool(tool) {
        normalize_run(&mut result);
    }
    Ok(result)
}

fn is_run_tool(tool: &str) -> bool {
    matches!(
        tool,
        "app.run" | "app.wait_run_finish" | "app.run_result" | "app.stop_run"
    )
}

fn normalize_run(state: &mut Value) {
    // This interpretation is specific to the negotiated Copilot provider. Legacy MCP
    // still requires its canonical run_succeeded field; completion alone is insufficient.
    match state["terminal_status"].as_str() {
        Some("running" | "stop_requested") => {
            state["active"] = json!(true);
            state["run_succeeded"] = Value::Null;
        }
        Some("completed") => {
            if state.get("active").is_none() {
                state["active"] = json!(false);
            }
            if state.get("run_succeeded").is_none()
                && state.get("latest_runtime_error").is_none_or(Value::is_null)
                && state.get("verification_error").is_none_or(Value::is_null)
                && state.get("mcp_error").is_none()
            {
                state["run_succeeded"] = json!(true);
            }
        }
        Some("failed") => {
            state["active"] = json!(false);
            state["run_succeeded"] = json!(false);
        }
        Some("canceled" | "user_stopped") => {
            state["active"] = json!(false);
            state["user_stopped"] = json!(true);
        }
        Some("paused" | "user_paused") => {
            state["active"] = json!(true);
            state["user_paused"] = json!(true);
        }
        _ => {}
    }
}
