//! One app dependency contract, mapped to legacy python_pip or Copilot package commands.
use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::cli::PythonPackageSub;
use crate::core::{self, BackendError, Res};

pub struct PreparedPackage {
    operation: &'static str,
    args: Value,
    timeout: u64,
}

pub fn prepare(sub: PythonPackageSub) -> Res<PreparedPackage> {
    let (operation, packages, index_url, options) = match sub {
        PythonPackageSub::List { options } => ("list", vec![], None, options),
        PythonPackageSub::Install {
            packages,
            index_url,
            options,
        } => ("install", packages, index_url, options),
        PythonPackageSub::Uninstall { packages, options } => ("uninstall", packages, None, options),
    };
    let mut args = json!({"operation":operation,"verbosity":options.verbosity});
    if operation != "list" {
        let items: Vec<Value> = packages
            .iter()
            .map(|spec| match spec.split_once("==") {
                Some((name, version)) => json!({"package_name":name,"version":version}),
                None => json!({"package_name":spec}),
            })
            .collect();
        args["packages"] = json!(items);
    }
    if let Some(url) = index_url {
        args["index_url"] = json!(url);
    }
    package_specs(&args, operation)?;
    Ok(PreparedPackage {
        operation,
        args,
        timeout: options.timeout,
    })
}

impl PreparedPackage {
    pub fn preview(&self) -> Option<Value> {
        (self.operation != "list").then(|| {
            json!({
                "action":format!("python.package.{}", self.operation),
                "target":{"scope":"current_studio_app","resolution":"current_at_execution"},
                "arguments":self.args,"timeout":self.timeout,
                "effects":["modify_python_environment","write_app_dependencies"],
                "backend":"auto: legacy python_pip / copilot python package"
            })
        })
    }

    pub fn execute(self) -> Res<CmdOut> {
        let command = format!("python.package.{}", self.operation);
        let tools = core::mcp_list(false, 15)?;
        if !tools
            .iter()
            .any(|tool| tool["name"] == "python_pip" || tool["name"] == "xbot-bash")
        {
            return Err(BackendError::new("MCP_CAPABILITY_UNAVAILABLE", "当前客户端未提供应用 Python 依赖管理接口")
                .with_data(json!({"command":command,"applied":false,"status":"not_started"}))
                .with_hint("用 client info 和 tool studio list 核对当前接口；旧版需提供 python_pip，新版需提供 xbot-bash"));
        }
        let app_id = if self.operation == "list" {
            core::require_current_app_id()?
        } else {
            core::require_current_edit_mode()?
        };
        let result = core::mcp("python_pip", self.args, self.timeout).map_err(|mut error| {
            let mut details = error.data.take().unwrap_or_else(|| json!({}));
            if !details.is_object() { details = json!({"response":details}); }
            details["command"] = json!(command);
            details["app_id"] = json!(app_id);
            details["operation"] = json!(self.operation);
            if self.operation != "list" {
                if details["applied"] != false && details["status"] != "not_started" {
                    if details.get("outcome").is_none() { details["outcome"] = json!("unknown"); }
                    if details.get("status").is_none() { details["status"] = json!("outcome_unknown"); }
                }
                let recovery = "安装/卸载没有自动重试。先查看应用依赖和返回的 pip 日志确认已完成及部分完成项；超时只结束 CLI 等待，不代表客户端 pip 已停止。";
                error.hint = Some(match error.hint.take() {
                    Some(hint) => format!("{hint}\n{recovery}"),
                    None => recovery.into(),
                });
            }
            error.with_data(details).with_retryable(false)
        })?;
        Ok((
            json!({"command":command,"app_id":app_id,"result":result}),
            vec![],
        ))
    }
}

/// Validate the shared legacy shape before translating it into quoted Copilot arguments.
/// Only names / exact versions are accepted; pip options, requirements files and paths are not specs.
pub(crate) fn package_specs(args: &Value, operation: &str) -> Res<Vec<String>> {
    let bad = |message| BackendError::new("BAD_INPUT", message);
    if let Some(verbosity) = args.get("verbosity") {
        if verbosity.as_u64().is_none_or(|n| n > 4) {
            return Err(bad("verbosity 必须为 0–4 的整数"));
        }
    }
    if let Some(index) = args.get("index_url") {
        if operation != "install" {
            return Err(bad("只有 install 可以指定 index_url"));
        }
        let value = index
            .as_str()
            .ok_or_else(|| bad("index_url 必须是 HTTP(S) 地址"))?;
        let url = url::Url::parse(value).map_err(|_| bad("index_url 必须是 HTTP(S) 地址"))?;
        if value.len() > 2048
            || !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || value.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(bad("index_url 必须是长度不超过 2048 的 HTTP(S) 地址"));
        }
    }
    if operation == "list" {
        if args.get("packages").is_some() {
            return Err(bad("list 不接受 packages"));
        }
        return Ok(vec![]);
    }
    if !matches!(operation, "install" | "uninstall") {
        return Err(bad("未知的 Python 包操作"));
    }
    let packages = args["packages"]
        .as_array()
        .filter(|items| (1..=50).contains(&items.len()))
        .ok_or_else(|| bad("一次需要 1–50 个包"))?;
    let mut names = BTreeSet::new();
    let mut specs = Vec::new();
    for item in packages {
        let fields = item
            .as_object()
            .ok_or_else(|| bad("每个包必须包含 package_name"))?;
        if fields
            .keys()
            .any(|key| !["package_name", "version"].contains(&key.as_str()))
        {
            return Err(bad("包参数只接受 package_name 和可选 version"));
        }
        let name = item["package_name"]
            .as_str()
            .ok_or_else(|| bad("包名必须为字符串"))?;
        if name.is_empty()
            || name.len() > 200
            || !name.as_bytes()[0].is_ascii_alphanumeric()
            || !name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err(bad("包名需为 Python 发行包名；仅支持包名或 NAME==VERSION，不接受路径、URL、pip 选项或版本范围"));
        }
        // Python distribution identity is case-insensitive and merges runs of . _ -.
        let normalized = name
            .to_ascii_lowercase()
            .split(['.', '_', '-'])
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        if !names.insert(normalized) {
            return Err(bad("同一批次不能重复指定同一个包（忽略大小写和 ._- 差异）"));
        }
        let spec = if let Some(version) = item.get("version") {
            if operation != "install" {
                return Err(bad("uninstall 只接受包名，不接受版本"));
            }
            let version = version
                .as_str()
                .ok_or_else(|| bad("版本必须为非空字符串"))?;
            if version.is_empty()
                || version.len() > 200
                || !version
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".!+_-".contains(&c))
            {
                return Err(bad(
                    "安装版本需使用 NAME==VERSION；具体版本是否可用由客户端检查",
                ));
            }
            format!("{name}=={version}")
        } else {
            name.to_owned()
        };
        specs.push(spec);
    }
    Ok(specs)
}
