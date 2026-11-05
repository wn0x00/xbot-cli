//! Local parameter metadata adapter, coordinated with Studio save/reload/readback.
//! The helper reads managed resources without loading client assemblies.

use std::fs::{self, File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{self, BackendError, Res};

const HELPER: &[u8] = include_bytes!("parameter_assets/FlowParameters.dll");
const CONFIG: &[u8] = include_bytes!("parameter_assets/FlowParameters.runtimeconfig.json");

struct Adapter {
    directory: PathBuf,
    installation: PathBuf,
    runtime: PathBuf,
}

impl Drop for Adapter {
    fn drop(&mut self) {
        for name in [
            "FlowParameters.dll",
            "FlowParameters.runtimeconfig.json",
            "request.json",
        ] {
            let _ = fs::remove_file(self.directory.join(name));
        }
        let _ = fs::remove_dir(&self.directory);
    }
}

impl Adapter {
    fn new() -> Res<Self> {
        let backend = crate::installation::shell();
        let installation = PathBuf::from(&backend.path)
            .parent()
            .ok_or_else(|| BackendError::new("PARAMETER_RUNTIME_MISSING", "无法定位影刀安装目录"))?
            .to_owned();
        let runtime = installation
            .parent()
            .unwrap_or(&installation)
            .join(r"dotnet.win-x64\dotnet.exe");
        if !installation
            .join("ShadowBot.Runtime.Development.dll")
            .is_file()
            || !runtime.is_file()
        {
            return Err(BackendError::new(
                "PARAMETER_RUNTIME_MISSING",
                "缺少影刀类型资源 DLL 或自带的 .NET 运行时",
            )
            .with_data(json!({"installation":backend.evidence(),"runtime":runtime}))
            .with_hint(
                "检查影刀安装；可用 XBOT_SHELL_CLI 指向实际版本目录中的 shadowbot.shell-cli.exe",
            ));
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "xbot-cli-flow-parameters-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&directory).map_err(io_error)?;
        let adapter = Self {
            directory,
            installation,
            runtime,
        };
        fs::write(adapter.directory.join("FlowParameters.dll"), HELPER).map_err(io_error)?;
        fs::write(
            adapter.directory.join("FlowParameters.runtimeconfig.json"),
            CONFIG,
        )
        .map_err(io_error)?;
        Ok(adapter)
    }

    fn call(&self, action: &str, mut request: Value) -> Res<Value> {
        request["action"] = json!(action);
        request["installation"] = json!(self.installation);
        let file = self.directory.join("request.json");
        fs::write(
            &file,
            serde_json::to_vec(&request).expect("adapter request"),
        )
        .map_err(io_error)?;
        let mut command = Command::new(&self.runtime);
        command
            .arg(self.directory.join("FlowParameters.dll"))
            .arg(file);
        let output = core::run_capture(command, 30)?;
        let response: Value = serde_json::from_str(output.stdout.trim()).map_err(|_| {
            BackendError::new("PARAMETER_ADAPTER_FAILED", "参数适配器没有返回有效 JSON").with_data(
                json!({"stdout":output.stdout,"stderr":output.stderr,"outcome":"unknown"}),
            )
        })?;
        if !output.status.success() || response["ok"] != true {
            return Err(BackendError::new(
                response["error"]["code"]
                    .as_str()
                    .unwrap_or("PARAMETER_ADAPTER_FAILED"),
                response["error"]["message"]
                    .as_str()
                    .unwrap_or("参数适配器失败"),
            )
            .with_data(response["data"].clone())
            .with_retryable(false));
        }
        Ok(response["data"].clone())
    }
}

fn io_error(error: std::io::Error) -> BackendError {
    BackendError::new(
        "PARAMETER_ADAPTER_FAILED",
        format!("参数适配器文件操作失败: {error}"),
    )
}

// Deny sharing while this process holds the handle; no stale lock survives a crash.
fn lock(app_id: &str) -> Res<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(std::env::temp_dir().join(format!("xbot-cli-flow-parameters-{app_id}.lock")))
        .map_err(|error| {
            BackendError::new(
                "PARAMETER_CONFLICT",
                format!("无法取得应用参数写入锁: {error}"),
            )
            .with_hint("等待当前参数修改结束；不要并发修改同一应用")
        })
}

pub fn types() -> Res<CmdOut> {
    let catalog = Adapter::new()?.call("catalog", json!({}))?;
    Ok((
        json!({
            "command":"flow.params.types", "backend":"local-client-resources",
            "types":catalog["types"], "count":catalog["count"],
            "artifact":catalog["artifact"], "sha256":catalog["sha256"],
            "note":"类型来自已安装客户端。未声明 scope 不等于所有场景均可选；扩展或动态类型可能不在静态目录中。"
        }),
        vec![],
    ))
}

fn session(app_id: &str) -> Res<Value> {
    let info = core::mcp("app.get_info", json!({}), 30)?;
    if info.pointer("/app/app_id").and_then(Value::as_str) != Some(app_id) {
        return Err(
            BackendError::new("APP_MISMATCH", "当前应用已切换，停止参数修改").with_data(info),
        );
    }
    if info
        .pointer("/app/has_unsaved_changes")
        .and_then(Value::as_bool)
        .is_none()
    {
        return Err(
            BackendError::new("MCP_BAD_RESPONSE", "客户端没有返回未保存修改状态").with_data(info),
        );
    }
    Ok(info)
}

fn dirty(info: &Value) -> bool {
    info.pointer("/app/has_unsaved_changes") == Some(&Value::Bool(true))
}

fn idle() -> Res<()> {
    let state = core::shell_cli(&["system", "state"], 15)?;
    if state["hasStudioOpened"] != true
        || state["hasRunningTask"] != false
        || !matches!(state["studioEngineStatus"].as_str(), Some("none" | "idle"))
    {
        return Err(BackendError::new(
            "STUDIO_BUSY",
            "编辑器未打开、正在运行或状态无法确认，不能 reload",
        )
        .with_data(state));
    }
    Ok(())
}

fn clean_session(app_id: &str) -> Res<()> {
    if dirty(&session(app_id)?) {
        return Err(BackendError::new(
            "PARAMETER_CONFLICT",
            "出现新的未保存编辑，停止以免 reload 丢失修改",
        ));
    }
    idle()
}

fn canonical(items: &Value) -> Res<Value> {
    let items = items
        .as_array()
        .ok_or_else(|| BackendError::new("MCP_BAD_RESPONSE", "参数回读缺少 items 数组"))?;
    let mut result = Vec::new();
    for item in items {
        let mut next = json!({});
        for field in ["name", "direction", "type", "kind", "value", "description"] {
            let fallback = match field {
                "kind" => "Expression",
                "value" | "description" => "",
                _ => {
                    if item[field].as_str().is_none() {
                        return Err(
                            BackendError::new("MCP_BAD_RESPONSE", "参数回读缺少身份字段")
                                .with_data(item.clone()),
                        );
                    }
                    ""
                }
            };
            next[field] = json!(item[field].as_str().unwrap_or(fallback));
        }
        result.push(next);
    }
    Ok(json!(result))
}

fn readback(flow_id: &str) -> Res<Value> {
    let response = core::mcp("flow.list_parameters", json!({"flow_id":flow_id}), 30)?;
    canonical(&response["items"])
}

fn stage_error(mut error: BackendError, stage: &str, progress: &Value) -> BackendError {
    error.data = Some(json!({
        "stage":stage, "progress":progress, "cause":error.data,
        "applied":progress["written"], "runtime_verified":false
    }));
    error.retryable = Some(false);
    if progress["written"] != false {
        error.hint = Some("磁盘参数可能已写入；先检查当前应用、参数文件和客户端回读，不要整批重试或保存旧内存覆盖文件。CLI 不会自动回滚、同步或关闭编辑器。".into());
    }
    error
}

pub fn set(flow_id: Option<&str>, payload: Value) -> Res<CmdOut> {
    let payload = crate::input::normalize_parameters(payload)?;
    let app_id = core::require_current_edit_mode()?;
    let flow_id = core::resolve_flow_id(flow_id, None)?;
    let _lock = lock(&app_id)?;
    let info = session(&app_id)?;
    idle()?;
    let adapter = Adapter::new()?;
    let users_root =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
            BackendError::new("PARAMETER_RUNTIME_MISSING", "LOCALAPPDATA 未设置")
        })?)
        .join(r"ShadowBot\users");
    let context = adapter.call(
        "context",
        json!({
            "users_root":users_root,"app_id":app_id,"flow_id":flow_id
        }),
    )?;
    let catalog = adapter.call("catalog", json!({}))?;
    let request = json!({
        "package_dir":context["package_dir"],"app_id":app_id,"flow_id":flow_id,
        "operations":payload["operations"],"types":catalog["types"]
    });
    // Validate the entire batch before saving existing edits or replacing any metadata.
    let mut plan = adapter.call("plan", request.clone())?;
    let mut progress = json!({
        "app_id":app_id,"flow_id":flow_id,"saved_existing_changes":false,
        "written":false,"reloaded":false,"readback_verified":false
    });
    if dirty(&info) {
        session(&app_id).map_err(|error| stage_error(error, "before_save", &progress))?;
        idle().map_err(|error| stage_error(error, "before_save", &progress))?;
        core::mcp("app.save", json!({}), 130)
            .map_err(|error| stage_error(error, "save_existing", &progress))?;
        progress["saved_existing_changes"] = json!(true);
        clean_session(&app_id).map_err(|error| stage_error(error, "after_save", &progress))?;
        plan = adapter
            .call("plan", request)
            .map_err(|error| stage_error(error, "replan_after_save", &progress))?;
    }
    let before =
        readback(&flow_id).map_err(|error| stage_error(error, "read_before_write", &progress))?;
    if before != plan["before_parameters"] {
        return Err(stage_error(
            BackendError::new("PARAMETER_CONFLICT", "磁盘参数与客户端内存不一致，未写入")
                .with_hint("先核对并处理尚未 reload 的外部修改，再执行参数操作"),
            "read_before_write",
            &progress,
        ));
    }
    clean_session(&app_id).map_err(|error| stage_error(error, "before_write", &progress))?;
    if plan["changed"] == true {
        progress["written"] = Value::Null;
        match adapter.call("apply", json!({"plan":plan})) {
            Ok(_) => progress["written"] = json!(true),
            Err(error) => {
                if let Some(written) = error.data.as_ref().and_then(|data| data.get("written")) {
                    progress["written"] = written.clone();
                }
                return Err(stage_error(error, "write", &progress));
            }
        }
        clean_session(&app_id).map_err(|error| stage_error(error, "before_reload", &progress))?;
        progress["reloaded"] = Value::Null;
        core::mcp("app.reload", json!({}), 130)
            .map_err(|error| stage_error(error, "reload", &progress))?;
        progress["reloaded"] = json!(true);
        session(&app_id).map_err(|error| stage_error(error, "after_reload", &progress))?;
    }
    let actual = readback(&flow_id).map_err(|error| stage_error(error, "readback", &progress))?;
    if actual != plan["parameters"] {
        return Err(stage_error(
            BackendError::new(
                "PARAMETER_READBACK_MISMATCH",
                "reload 后参数回读与写入计划不一致",
            ),
            "readback",
            &progress,
        ));
    }
    progress["readback_verified"] = json!(true);
    progress["command"] = json!("flow.params.set");
    progress["backend"] = json!("local-metadata+studio-reload");
    progress["success"] = json!(true);
    progress["flow_kind"] = plan["flow_kind"].clone();
    progress["parameter_count"] = json!(actual.as_array().map_or(0, Vec::len));
    progress["changes"] = plan["changes"].clone();
    progress["warnings"] = plan["warnings"].clone();
    progress["runtime_verified"] = json!(false);
    progress["note"] =
        json!("参数已回读确认；CLI 不修改运行时 Python 或签名文件，运行结果需另行验证。");
    Ok((
        progress,
        vec![format!(
            "xbot-cli flow params list {flow_id} --expect-app {app_id}"
        )],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_readback_normalizes_absent_initializer_without_evaluating_values() {
        assert_eq!(
            canonical(&json!([{"name":"result","type":"list","direction":"Out","kind":"Expression","value":null}])).unwrap(),
            json!([{"name":"result","type":"list","direction":"Out","kind":"Expression","value":"","description":""}])
        );
        assert!(canonical(&json!([{"type":"str"}])).is_err());
    }

    #[test]
    fn failure_retains_unknown_write_outcome_and_disables_retry() {
        let error = stage_error(
            BackendError::new("TIMEOUT", "timeout"),
            "write",
            &json!({"written":null,"reloaded":false}),
        );
        assert_eq!(error.retryable, Some(false));
        assert!(error.data.unwrap()["applied"].is_null());
        assert!(error.hint.unwrap().contains("不要整批重试"));
    }
}
