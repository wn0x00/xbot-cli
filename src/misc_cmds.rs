//! 杂项命令:diag / state / doctor / mcp 能力与调用。

use std::path::Path;

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{self, BackendError, Res};

pub fn cmd_diag() -> Res<CmdOut> {
    Ok((core::mcp("diagnostics.snapshot", json!({}), 130)?, vec![]))
}

/// 自升级由 npm 启动器在原生进程启动前完成;原生程序运行时无法替换自身。
pub fn cmd_update() -> Res<CmdOut> {
    Err(
        BackendError::new("UPDATE_REQUIRES_LAUNCHER", "自升级由 npm 启动器执行").with_hint(
            "使用 npm 安装的 xbot-cli update 升级;或手动运行 npm i -g @guanzhu.me/xbot-cli@latest",
        ),
    )
}

pub fn cmd_state() -> Res<CmdOut> {
    Ok((core::shell_cli(&["system", "state"], 15)?, vec![]))
}

pub fn cmd_status() -> Res<CmdOut> {
    let state = core::shell_cli(&["system", "state"], 15)?;
    let opened = state.get("hasStudioOpened").and_then(Value::as_bool) == Some(true);
    let mut data = json!({
        "console": {
            "mode": state.get("currentMode"),
            "module": state.get("currentModule"),
            "has_running_task": state.get("hasRunningTask"),
        },
        "studio": {"opened": opened, "engine": state.get("studioEngineStatus")},
    });
    if opened {
        match core::mcp("app.get_info", json!({}), 15) {
            Ok(info) => {
                let app = info.get("app").cloned().unwrap_or(Value::Null);
                data["app"] = json!({
                    "app_id": app.get("app_id"),
                    "name": app.get("name").or_else(|| app.get("app_name")),
                    "flow_count": info.get("flows").and_then(Value::as_array).map(|a| a.len()),
                });
            }
            Err(e) => {
                data["studio"]["mcp_error"] = json!({"code": e.code, "message": e.message});
            }
        }
    }
    Ok((data, vec![]))
}

pub fn cmd_doctor() -> Res<CmdOut> {
    let mut report = json!({});
    match core::shell_cli(&["system", "state"], 10) {
        Ok(st) => {
            report["shell_cli"] = json!({"ok": true,
                "hasStudioOpened": st.get("hasStudioOpened"),
                "mode": st.get("currentMode")});
        }
        Err(e) => report["shell_cli"] = json!({"ok": false, "error": e.message}),
    }
    // 服务可达即可;"No Studio workspace" 只是当前没开应用,属正常态。
    match core::mcp("app.get_info", json!({}), 8) {
        Ok(info) => {
            report["studio_mcp"] = json!({"ok": true,
                "app": info.pointer("/app/app_id")});
        }
        Err(e) if e.code == "MCP_OFFLINE" => {
            report["studio_mcp"] = json!({"ok": false, "error": e.message});
        }
        Err(e) => {
            report["studio_mcp"] = json!({"ok": true, "app": Value::Null,
                "note": core::truncate(&e.message, 80)});
        }
    }
    // View 是可选只读入口,缺失不影响 MCP 开发主链路。
    let launcher = core::launcher_path();
    report["url_protocol_view_only"] = json!({"ok": true,
        "available": Path::new(&launcher).exists(), "launcher": launcher});
    let ok = report
        .as_object()
        .map(|m| {
            m.values()
                .all(|v| v.get("ok").and_then(Value::as_bool) == Some(true))
        })
        .unwrap_or(false);
    if !ok {
        return Err(
            BackendError::new("DOCTOR_FAILED", "一项或多项必需通道不可用")
                .with_data(report)
                .with_hint(
                    "确认影刀客户端已启动并登录;开发前先用 xbot-cli app open <app-id> 打开应用",
                ),
        );
    }
    Ok((report, vec![]))
}

pub fn cmd_mcp(action: &str, args: &[String], timeout: u64, full: bool) -> Res<CmdOut> {
    match action {
        "tools" => {
            require_arity(args, 0, 0)?;
            mcp_tools(false, full)
        }
        "schema" => {
            require_arity(args, 1, 1)?;
            mcp_schema(false, &args[0])
        }
        "call" => {
            require_arity(args, 1, 2)?;
            mcp_call(&args[0], args.get(1).map(String::as_str), timeout)
        }
        tool => {
            require_arity(args, 0, 1)?;
            mcp_call(tool, args.first().map(String::as_str), timeout)
        }
    }
}

fn require_arity(args: &[String], min: usize, max: usize) -> Res<()> {
    if args.len() < min || args.len() > max {
        return Err(BackendError::new("BAD_ARGS", "mcp 子命令参数数量不正确")
            .with_hint("xbot-cli mcp tools | xbot-cli mcp schema <tool> | xbot-cli mcp call <tool> [json|@file|-]"));
    }
    Ok(())
}

fn mcp_call(tool: &str, args_json: Option<&str>, timeout: u64) -> Res<CmdOut> {
    let args = match args_json {
        Some(raw) => core::json_arg(raw)?,
        None => json!({}),
    };
    Ok((core::mcp(tool, args, timeout)?, vec![]))
}

fn mcp_tools(browser: bool, full: bool) -> Res<CmdOut> {
    let tools = core::mcp_list(browser, 30)?;
    let items: Vec<Value> = if full {
        tools
            .iter()
            .map(|t| json!({"name": t.get("name"), "description": t.get("description")}))
            .collect()
    } else {
        tools
            .iter()
            .map(|t| t.get("name").cloned().unwrap_or(Value::Null))
            .collect()
    };
    Ok((json!({"tools": items, "count": items.len()}), vec![]))
}

fn mcp_schema(browser: bool, tool: &str) -> Res<CmdOut> {
    let tools = core::mcp_list(browser, 30)?;
    let found = tools
        .iter()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(tool))
        .cloned()
        .ok_or_else(|| {
            let names: Vec<&str> = tools
                .iter()
                .filter_map(|t| t.get("name").and_then(Value::as_str))
                .collect();
            BackendError::new("MCP_TOOL_UNKNOWN", format!("未知 MCP 工具: {tool}"))
                .with_hint(format!("可用工具: {}", names.join(", ")))
        })?;
    Ok((
        found,
        vec!["xbot-cli mcp call <tool> '<json>'   # 按 schema 调用".into()],
    ))
}
