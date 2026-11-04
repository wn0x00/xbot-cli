//! code / browser / wiki 命令组。
//!
//! code    — codeflow.read/write/edit(部分替换),CodeFlow 的代码面
//! browser — Chrome Bridge 的 CLI 兼容封装,浏览器自动化
//! wiki    — 影刀官方知识库(与内置 opencode 的 skill 同源),块原型/SDK/规范都在这

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{self, BackendError, Res};
use crate::{flow_cmds, input};

// ---------- code(CodeFlow 代码面) ----------

pub fn code_read(flow_id: Option<&str>) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, Some("code"))?;
    Ok((
        core::mcp("codeflow.read", json!({"flow_id": flow_id}), 130)?,
        vec![],
    ))
}

pub fn code_write(flow_id: Option<&str>, file: &str) -> Res<CmdOut> {
    code_write_prepared(flow_id, &core::read_file_or_stdin(file)?)
}

pub fn code_write_prepared(flow_id: Option<&str>, content: &str) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, Some("code"))?;
    core::require_current_edit_mode()?;
    let r = codeflow_source_write(&flow_id, content, None)?;
    verify_source_applied(&flow_id, content)?;
    let errs = flow_cmds::diagnose_after_mutation("write", &r)?;
    let empty = errs.as_array().map(|a| a.is_empty()).unwrap_or(true);
    let nxt = if empty {
        vec![format!("xbot-cli flow run {flow_id}   # 运行验证")]
    } else {
        vec!["# 存在静态错误,先修复再运行".into()]
    };
    Ok((json!({"write": r, "static_errors": errs}), nxt))
}

/// 源码替换只走进程内通道(CodeFlow.SetSourceCode),官方 codeflow.write/edit
/// 会重建参数并重置中文表单与调用方传参;不可用时硬失败,不回退。
fn codeflow_source_write(flow_id: &str, source: &str, expect_md5: Option<String>) -> Res<Value> {
    crate::inproc::client::ensure_bridge()?;
    let mut body = json!({"flow": flow_id, "source": source});
    if let Some(md5) = expect_md5 {
        body["expectMd5"] = json!(md5);
    }
    crate::inproc::client::post("/studio/codeflow/source", &body, 130)
}

fn source_content(flow_id: &str) -> Res<String> {
    let read = core::mcp("codeflow.read", json!({"flow_id": flow_id}), 130)?;
    read.get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            BackendError::new("MCP_BAD_RESPONSE", "codeflow.read 响应缺少 content 文本")
                .with_data(read)
        })
}

fn md5_hex(text: &str) -> String {
    format!("{:x}", md5::compute(text.as_bytes()))
}

fn verify_source_applied(flow_id: &str, expected: &str) -> Res<()> {
    let actual = source_content(flow_id)?;
    if actual != expected {
        return Err(BackendError::new(
            "CODE_SOURCE_MISMATCH",
            "源码写入已返回成功，但回读内容与请求不一致",
        )
        .with_data(json!({
            "flow_id": flow_id,
            "expected": {"chars": expected.chars().count(), "md5": md5_hex(expected)},
            "actual": {"chars": actual.chars().count(), "md5": md5_hex(&actual)},
        }))
        .with_hint("不要自动重复写入;先 code read 检查当前内存源码状态"));
    }
    Ok(())
}

/// 部分替换:exact-text(与 codeflow.edit 语义一致,old 默认须唯一)。
pub fn code_edit(flow_id: Option<&str>, old: &str, new: &str, replace_all: bool) -> Res<CmdOut> {
    code_edit_prepared(
        flow_id,
        &core::text_arg(old)?,
        &core::text_arg(new)?,
        replace_all,
    )
}

pub fn code_edit_prepared(
    flow_id: Option<&str>,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Res<CmdOut> {
    if old.is_empty() || old == new {
        return Err(BackendError::new(
            "BAD_INPUT",
            "old 必须非空，且 new 必须与 old 不同",
        ));
    }
    let flow_id = core::resolve_flow_id(flow_id, Some("code"))?;
    core::require_current_edit_mode()?;
    // 与官方 codeflow.edit 同一语义:读出原文后精确替换;old 默认须唯一。
    let current = source_content(&flow_id)?;
    let matches = current.matches(old).count();
    if matches == 0 {
        return Err(BackendError::new("BAD_INPUT", "old 在当前源码中不存在")
            .with_data(json!({"flow_id": flow_id, "occurrences": 0})));
    }
    if !replace_all && matches > 1 {
        return Err(BackendError::new(
            "BAD_INPUT",
            format!("old 在当前源码中出现 {matches} 次，不唯一"),
        )
        .with_data(json!({"flow_id": flow_id, "occurrences": matches}))
        .with_hint("提供更多上下文使 old 唯一，或传 --replace-all"));
    }
    let updated = if replace_all {
        current.replace(old, new)
    } else {
        current.replacen(old, new, 1)
    };
    let r = codeflow_source_write(&flow_id, &updated, Some(md5_hex(&current)))?;
    verify_source_applied(&flow_id, &updated)?;
    let errs = flow_cmds::diagnose_after_mutation("edit", &r)?;
    let empty = errs.as_array().map(|a| a.is_empty()).unwrap_or(true);
    let nxt = if empty {
        vec![format!("xbot-cli flow run {flow_id}   # 运行验证")]
    } else {
        vec!["# 存在静态错误,先修复再运行".into()]
    };
    Ok((json!({"edit": r, "static_errors": errs}), nxt))
}

// ---------- browser(Chrome Bridge) ----------

pub fn browser_act(args_json: &str) -> Res<CmdOut> {
    browser_act_prepared(input::browser_args(Some(args_json))?)
}

pub fn browser_act_prepared(args: Value) -> Res<CmdOut> {
    let args = input::object(args, "浏览器参数")?;
    Ok((core::mcp_browser("web_action", args, 60)?, vec![]))
}

pub fn browser_eval(code: &str) -> Res<CmdOut> {
    // web_evaluate_playwright 只读;参数名按工具 schema 为 code
    Ok((
        core::mcp_browser("web_evaluate_playwright", json!({"code": code}), 60)?,
        vec![],
    ))
}

pub fn browser_call(tool: &str, args_json: Option<&str>, timeout: u64) -> Res<CmdOut> {
    browser_call_prepared(tool, input::browser_args(args_json)?, timeout)
}

pub fn browser_call_prepared(tool: &str, args: Value, timeout: u64) -> Res<CmdOut> {
    let args = input::object(args, "浏览器参数")?;
    Ok((core::mcp_browser(tool, args, timeout)?, vec![]))
}

pub fn browser_tools(full: bool) -> Res<CmdOut> {
    let tools = core::mcp_list(true, 30)?;
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

pub fn browser_schema(tool: &str) -> Res<CmdOut> {
    let tools = core::mcp_list(true, 30)?;
    let found = tools
        .iter()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(tool))
        .cloned()
        .ok_or_else(|| {
            let names: Vec<&str> = tools
                .iter()
                .filter_map(|t| t.get("name").and_then(Value::as_str))
                .collect();
            BackendError::new("MCP_TOOL_UNKNOWN", format!("未知浏览器 MCP 工具: {tool}"))
                .with_hint(format!("可用工具: {}", names.join(", ")))
        })?;
    Ok((
        found,
        vec!["xbot-cli browser call <tool> '<json>'   # 按 schema 调用".into()],
    ))
}

// ---------- wiki(官方知识库) ----------

pub fn wiki_list() -> Res<CmdOut> {
    Ok((
        core::mcp("wiki.list", json!({}), 30)?,
        vec![
            "xbot-cli wiki read <doc_id>...   # 读具体文档".into(),
            "xbot-cli wiki search <关键词>     # 模糊搜索".into(),
        ],
    ))
}

pub fn wiki_search(query: &str) -> Res<CmdOut> {
    Ok((
        core::mcp("wiki.search", json!({"query": query}), 30)?,
        vec![],
    ))
}

pub fn wiki_read(doc_ids: &[String]) -> Res<CmdOut> {
    Ok((
        core::mcp("wiki.read", json!({"doc_ids": doc_ids}), 30)?,
        vec![],
    ))
}
