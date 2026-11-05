//! flow 命令组:Studio 内开发调试；参数写入使用本地元数据适配器。

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{self, BackendError, Res};
use crate::input;

pub fn flow_list() -> Res<CmdOut> {
    Ok((core::mcp("app.list_flows", json!({}), 130)?, vec![]))
}

pub fn flow_new(name: &str, kind: &str) -> Res<CmdOut> {
    core::require_current_edit_mode()?;
    let r = core::mcp(
        "app.create_flow",
        json!({"flows": [{"flow_name": name, "kind": kind}]}),
        130,
    )?;
    let fid = r
        .get("results")
        .or_else(|| r.get("flows"))
        .and_then(Value::as_array)
        .and_then(|fs| {
            fs.iter().find_map(|f| {
                if f.get("flow_name").and_then(Value::as_str) == Some(name)
                    || f.get("flow_id").is_some()
                {
                    f.get("flow_id").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| name.to_string());
    let nxt = if kind == "code" {
        vec![format!("xbot-cli flow code write {fid} --file main.py")]
    } else {
        vec![format!("xbot-cli flow visual list {fid} --detail")]
    };
    Ok((json!({"created": r, "flow_id": fid}), nxt))
}

pub fn flow_get(flow_id: Option<&str>) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, None)?;
    let info = core::mcp("app.list_flows", json!({}), 130)?;
    let kind = info.get("flows").and_then(Value::as_array).and_then(|fs| {
        fs.iter().find_map(|f| {
            let matches = f.get("flow_id").and_then(Value::as_str) == Some(flow_id.as_str())
                || f.get("flow_name").and_then(Value::as_str) == Some(flow_id.as_str());
            if matches {
                f.get("flow_kind")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            }
        })
    });
    if kind.as_deref() == Some("visual") {
        return Ok((
            core::mcp(
                "flow.blocks.list",
                json!({"flow_id": flow_id, "detail": "detail"}),
                130,
            )?,
            vec![],
        ));
    }
    Ok((
        core::mcp("codeflow.read", json!({"flow_id": flow_id}), 130)?,
        vec![],
    ))
}

pub fn flow_blocks(
    flow_id: Option<&str>,
    detail: bool,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, Some("visual"))?;
    let mut args = json!({"flow_id": flow_id, "detail": if detail { "detail" } else { "simple" }});
    if let Some(o) = offset {
        args["offset"] = json!(o);
    }
    if let Some(l) = limit {
        args["limit"] = json!(l);
    }
    Ok((core::mcp("flow.blocks.list", args, 130)?, vec![]))
}

pub fn flow_toggle(flow_id: Option<&str>, op: &str, block_ids: &[String]) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, Some("visual"))?;
    core::require_current_edit_mode()?;
    Ok((
        core::mcp(
            "flow.toggle_blocks",
            json!({"flow_id": flow_id, "op": op, "block_ids": block_ids}),
            130,
        )?,
        vec![],
    ))
}

pub fn flow_snapshots(flow_id: Option<&str>, block_id: Option<&str>) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, Some("visual"))?;
    let mut args = json!({"flow_id": flow_id});
    if let Some(id) = block_id {
        args["block_id"] = json!(id);
    }
    Ok((core::mcp("flow.get_variable_snapshots", args, 130)?, vec![]))
}

pub fn flow_variables(
    flow_id: Option<&str>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, Some("visual"))?;
    let mut args = json!({"flow_id": flow_id});
    if let Some(o) = offset {
        args["offset"] = json!(o);
    }
    if let Some(l) = limit {
        args["limit"] = json!(l);
    }
    Ok((core::mcp("flow.list_variables", args, 130)?, vec![]))
}

pub fn flow_params(flow_id: Option<&str>) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, None)?;
    Ok((
        core::mcp("flow.list_parameters", json!({"flow_id": flow_id}), 130)?,
        vec![],
    ))
}

pub fn flow_params_set(flow_id: Option<&str>, operations: &str) -> Res<CmdOut> {
    flow_params_set_prepared(flow_id, input::flow_parameters(operations)?)
}

pub fn flow_params_set_prepared(flow_id: Option<&str>, payload: Value) -> Res<CmdOut> {
    crate::flow_parameters::set(flow_id, payload)
}

pub fn flow_blocks_set(flow_id: Option<&str>, file: &str) -> Res<CmdOut> {
    flow_blocks_set_prepared(flow_id, input::visual_edit(file)?)
}

pub fn flow_blocks_set_prepared(flow_id: Option<&str>, payload: Value) -> Res<CmdOut> {
    let mut payload = input::normalize_visual_edit(payload)?;
    let flow_id = core::resolve_flow_id(flow_id, Some("visual"))?;
    core::require_current_edit_mode()?;
    payload["flow_id"] = json!(flow_id);
    let r = core::mcp("flow.edit_blocks", payload, 130)?;
    let errors = diagnose_after_mutation("edit", &r)?;
    let mut nxt = vec![format!("xbot-cli flow run {flow_id}   # 运行验证")];
    if let Some(bid) = r
        .pointer("/block_region/inserted/0/block_id")
        .and_then(Value::as_str)
    {
        nxt.insert(
            0,
            format!(
                "xbot-cli flow visual fill {flow_id} --fills '@fills.json'   # 给块填字段(block_id={bid})"
            ),
        );
    }
    Ok((json!({"edit": r, "static_errors": errors}), nxt))
}

pub fn flow_fill(flow_id: Option<&str>, fills: &str) -> Res<CmdOut> {
    flow_fill_prepared(flow_id, input::visual_fill(fills)?)
}

pub fn flow_fill_prepared(flow_id: Option<&str>, payload: Value) -> Res<CmdOut> {
    let mut payload = input::normalize_visual_fill(payload)?;
    let flow_id = core::resolve_flow_id(flow_id, Some("visual"))?;
    core::require_current_edit_mode()?;
    payload["flow_id"] = json!(flow_id);
    let r = core::mcp("flow.fill_blocks", payload, 130)?;
    let errs = diagnose_after_mutation("fill", &r)?;
    let fill_failed = r
        .get("fill_errors")
        .and_then(Value::as_array)
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if fill_failed {
        return Err(
            BackendError::new("FLOW_FILL_FAILED", "部分或全部块字段填充失败")
                .with_data(json!({"applied": true, "fill": r, "static_errors": errs}))
                .with_retryable(false)
                .with_hint("只修复并重试 fill_errors 中的 block_id;其他块已应用,不要整批重试"),
        );
    }
    let empty = errs.as_array().map(|a| a.is_empty()).unwrap_or(true);
    let nxt = if empty {
        vec![format!("xbot-cli flow run {flow_id}   # 运行验证")]
    } else {
        vec!["# 有静态错误,先修复".into()]
    };
    Ok((json!({"fill": r, "static_errors": errs}), nxt))
}

pub fn flow_run(flow_id: Option<&str>, timeout: u64) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, None)?;
    core::require_current_edit_mode()?;
    let state = core::mcp("app.run", json!({"flow_id":flow_id,"wait_timeout":0}), 130)
        .map_err(|e| run_backend_error(None, &json!({}), e))?;
    continue_started_run(state, timeout)
}

fn continue_started_run(state: Value, timeout: u64) -> Res<CmdOut> {
    let run_id = state
        .get("run_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string);
    let Some(run_id) = run_id else {
        return Err(BackendError::new(
            "FLOW_RUN_UNKNOWN",
            "运行响应缺少 run_id，无法关联或确认执行结果",
        )
        .with_data(json!({"run":state,"outcome":"unknown"}))
        .with_retryable(false)
        .with_hint("先查看 Studio 和 flow logs；不要自动重新启动流程"));
    };
    wait_run(&run_id, state, timeout, |run_id, segment| {
        core::mcp(
            "app.wait_run_finish",
            json!({"run_id":run_id,"timeout":segment}),
            segment + 20,
        )
    })
}

pub fn flow_wait(run_id: &str, timeout: u64) -> Res<CmdOut> {
    if run_id.trim().is_empty() {
        return Err(BackendError::new("BAD_INPUT", "run_id 不能为空"));
    }
    wait_run(
        run_id,
        json!({"run_id":run_id,"active":true}),
        timeout,
        |run_id, segment| {
            core::mcp(
                "app.wait_run_finish",
                json!({"run_id":run_id,"timeout":segment}),
                segment + 20,
            )
        },
    )
}

fn wait_run<F>(run_id: &str, mut state: Value, timeout: u64, mut wait: F) -> Res<CmdOut>
where
    F: FnMut(&str, u64) -> Res<Value>,
{
    check_run_identity(run_id, &state)?;
    check_user_intervention(run_id, &state)?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    while !run_finished(&state) {
        if Instant::now() >= deadline {
            return Err(BackendError::new(
                "FLOW_RUN_TIMEOUT",
                format!("等待 {timeout}s 流程未结束"),
            )
            .with_data(run_data(run_id, &state))
            .with_retryable(false)
            .with_hint(format!(
                "xbot-cli flow wait {run_id} --timeout 600 继续等待同一次运行；不要重新启动流程"
            )));
        }
        // 每段不超过 110s,避开 MCP 传输 120s 上限;始终沿用同一个 run_id。
        let seg = deadline
            .saturating_duration_since(Instant::now())
            .as_secs()
            .clamp(1, 110);
        state = wait(run_id, seg).map_err(|e| run_backend_error(Some(run_id), &state, e))?;
        check_run_identity(run_id, &state)?;
        check_user_intervention(run_id, &state)?;
        if !run_finished(&state) {
            sleep(Duration::from_secs(1));
        }
    }
    match run_success(&state) {
        Some(true) => Ok((run_data(run_id, &state), vec![])),
        Some(false) => Err(BackendError::new("FLOW_RUN_FAILED", "流程运行失败")
            .with_data(run_data(run_id, &state))
            .with_retryable(false)
            .with_hint("xbot-cli flow logs 查运行日志;修复后重新运行")),
        None => Err(
            BackendError::new("FLOW_RUN_UNKNOWN", "流程已结束,但响应缺少可确认的成功状态")
                .with_data(run_data(run_id, &state))
                .with_retryable(false)
                .with_hint("xbot-cli flow logs 查日志;不要仅凭进程结束认定成功"),
        ),
    }
}

fn run_finished(state: &Value) -> bool {
    state
        .get("terminal_status")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
        || matches!(
            state.get("status").and_then(Value::as_str),
            Some("finish")
                | Some("finished")
                | Some("succeeded")
                | Some("failed")
                | Some("stopped")
        )
        || state.get("active").and_then(Value::as_bool) == Some(false)
}

fn run_success(state: &Value) -> Option<bool> {
    // The Studio contract defines this field as canonical. Completion alone is not success.
    if state.get("active").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    state.get("run_succeeded").and_then(Value::as_bool)
}

fn check_run_identity(run_id: &str, state: &Value) -> Res<()> {
    if state
        .get("run_id")
        .and_then(Value::as_str)
        .is_some_and(|id| id != run_id)
    {
        return Err(
            BackendError::new("FLOW_RUN_MISMATCH", "后端返回了不同的 run_id，停止等待")
                .with_data(run_data(run_id, state))
                .with_retryable(false),
        );
    }
    Ok(())
}

fn check_user_intervention(run_id: &str, state: &Value) -> Res<()> {
    let code = state.get("code").and_then(Value::as_str);
    if state.get("user_paused").and_then(Value::as_bool) == Some(true)
        || code == Some("user_paused")
    {
        return Err(BackendError::new(
            "FLOW_RUN_PAUSED",
            "用户暂停了流程;停止自动操作并等待用户决定",
        )
        .with_data(run_data(run_id, state))
        .with_retryable(false));
    }
    if state.get("user_stopped").and_then(Value::as_bool) == Some(true)
        || code == Some("user_stopped")
    {
        return Err(BackendError::new(
            "FLOW_RUN_STOPPED",
            "用户停止了流程;停止自动操作并等待用户决定",
        )
        .with_data(run_data(run_id, state))
        .with_retryable(false));
    }
    Ok(())
}

fn run_data(run_id: &str, state: &Value) -> Value {
    json!({"run_id": run_id, "run": state, "result": state.get("result"),
        "resume_command":format!("xbot-cli flow wait {run_id} --timeout 600")})
}

fn run_backend_error(run_id: Option<&str>, previous: &Value, error: BackendError) -> BackendError {
    let evidence = error_evidence(&error);
    let returned = error
        .data
        .as_ref()
        .and_then(|data| data.get("result"))
        .unwrap_or(previous);
    let id = run_id.or_else(|| returned.get("run_id").and_then(Value::as_str));
    let mut data = match id {
        Some(id) => run_data(id, returned),
        None => json!({"run":returned,"outcome":"unknown"}),
    };
    data["backend_error"] = evidence;
    let intervention = match error.code.as_str() {
        "user_paused" | "FLOW_RUN_PAUSED" => Some((
            "FLOW_RUN_PAUSED",
            "用户暂停了流程；停止自动操作并等待用户决定",
        )),
        "user_stopped" | "FLOW_RUN_STOPPED" => Some((
            "FLOW_RUN_STOPPED",
            "用户停止了流程；停止自动操作并等待用户决定",
        )),
        _ => None,
    };
    if let Some((code, message)) = intervention {
        data.as_object_mut().unwrap().remove("resume_command");
        return BackendError::new(code, message)
            .with_data(data)
            .with_retryable(false);
    }
    if let Some(run_id) = run_id {
        BackendError::new("FLOW_WAIT_FAILED", "等待响应失败，原流程可能仍在运行")
            .with_data(data)
            .with_retryable(false)
            .with_hint(format!(
                "xbot-cli flow wait {run_id} --timeout 600 查询同一次运行；不要重新启动流程"
            ))
    } else {
        BackendError::new(error.code, error.message)
            .with_data(data)
            .with_retryable(false)
            .with_hint("运行请求失败或结果未确认；先查看 Studio 和日志，不要自动重新启动流程")
    }
}

pub(crate) fn error_evidence(error: &BackendError) -> Value {
    json!({"code":error.code,"message":error.message,"data":error.data})
}

fn static_errors(diag: &Value) -> Res<Value> {
    diag.get("static_errors")
        .filter(|value| value.is_array())
        .cloned()
        .ok_or_else(|| {
            BackendError::new(
                "DIAGNOSTICS_BAD_OUTPUT",
                "诊断响应缺少 static_errors 数组，不能确认检查通过",
            )
            .with_data(diag.clone())
        })
}

pub(crate) fn diagnose_after_mutation(operation: &str, result: &Value) -> Res<Value> {
    let diagnostic =
        core::mcp("diagnostics.snapshot", json!({}), 130).and_then(|diag| static_errors(&diag));
    diagnostic.map_err(|error| mutation_diagnostics_error(operation, result, error))
}

fn mutation_diagnostics_error(
    operation: &str,
    result: &Value,
    error: BackendError,
) -> BackendError {
    let mut data = json!({"applied":true,"operation":operation,"result":result,"diagnostics_error":error_evidence(&error)});
    data[operation] = result.clone();
    BackendError::new(
        "MUTATION_DIAGNOSTICS_FAILED",
        "写入已返回成功，但后续诊断未完成",
    )
    .with_data(data)
    .with_retryable(false)
    .with_hint("先回读确认已应用的内容，再运行 flow check；不要重复执行本次写入")
}

pub fn flow_check(flow_id: Option<&str>) -> Res<CmdOut> {
    let flow_id = flow_id
        .map(|id| core::resolve_flow_id(Some(id), None))
        .transpose()?;
    let diag = core::mcp("diagnostics.snapshot", json!({}), 130)?;
    let errors = static_errors(&diag)?;
    if !errors.as_array().unwrap().is_empty() {
        return Err(
            BackendError::new("FLOW_STATIC_ERRORS", "静态诊断未通过,流程未运行").with_data(
                json!({"flow_id":flow_id,"scope":"app","static_errors":errors,"executed":false}),
            ),
        );
    }
    Ok((
        json!({"flow_id":flow_id,"checked":true,"scope":"app","static_errors":errors,"executed":false}),
        vec![],
    ))
}

pub fn flow_verify(flow_id: Option<&str>, timeout: u64) -> Res<CmdOut> {
    let flow_id = core::resolve_flow_id(flow_id, None)?;
    flow_check(Some(&flow_id))?;
    match flow_run(Some(&flow_id), timeout) {
        Ok((data, _)) => Ok((
            json!({
                "verified": true,
                "flow_id":flow_id,
                "run_id": data.get("run_id"),
                "outputs": data.pointer("/result/outputs"),
                "result":data.get("result"),
                "run":data.get("run"),
            }),
            vec![],
        )),
        Err(mut e) => {
            if matches!(e.code.as_str(), "FLOW_RUN_PAUSED" | "FLOW_RUN_STOPPED") {
                return Err(e);
            }
            if let Ok(logs) = core::mcp("app.logs", json!({"limit": 50}), 130) {
                let mut data = e.data.take().unwrap_or(json!({}));
                data["log_tail"] = logs;
                return Err(e.with_data(data));
            }
            Err(e)
        }
    }
}

pub fn flow_stop() -> Res<CmdOut> {
    Ok((core::mcp("app.stop_run", json!({}), 130)?, vec![]))
}

pub fn flow_logs(cursor: Option<i64>, limit: Option<i64>) -> Res<CmdOut> {
    let mut args = json!({"limit": limit.unwrap_or(50)});
    if let Some(c) = cursor {
        args["cursor"] = json!(c);
    }
    Ok((core::mcp("app.logs", args, 130)?, vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_run_id_never_verifies_a_run() {
        let error =
            continue_started_run(json!({"success":true,"run_succeeded":true}), 1).unwrap_err();
        assert_eq!(error.code, "FLOW_RUN_UNKNOWN");
        assert_eq!(error.retryable, Some(false));
    }

    #[test]
    fn completed_without_canonical_success_is_unknown() {
        for state in [
            json!({"terminal_status":"completed"}),
            json!({"terminal_status":"succeeded","run_succeeded":null}),
            json!({"active":false}),
        ] {
            let error = wait_run("run-1", state, 5, |_, _| {
                panic!("completed runs must not be waited again")
            })
            .unwrap_err();
            assert_eq!(error.code, "FLOW_RUN_UNKNOWN");
        }
    }

    #[test]
    fn canonical_run_success_preserves_typed_outputs() {
        let state = json!({"run_id":"run-1","active":false,"run_succeeded":true,"result":{"outputs":{"items":[1,2]}}});
        let (data, _) = wait_run("run-1", state, 5, |_, _| panic!("finished")).unwrap();
        assert_eq!(data["result"]["outputs"]["items"], json!([1, 2]));
    }

    #[test]
    fn wait_failure_retains_run_identity_and_cause() {
        let error = wait_run(
            "run-1",
            json!({"run_id":"run-1","active":true}),
            2,
            |id, _| {
                assert_eq!(id, "run-1");
                Err(BackendError::new("MCP_TIMEOUT", "network failed")
                    .with_data(json!({"detail":"lost"})))
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "FLOW_WAIT_FAILED");
        assert_eq!(error.retryable, Some(false));
        let data = error.data.unwrap();
        assert_eq!(data["run_id"], "run-1");
        assert_eq!(data["run"]["active"], true);
        assert_eq!(data["backend_error"]["data"]["detail"], "lost");
    }

    #[test]
    fn timeout_does_not_restart_or_lose_run_id() {
        let error = wait_run("run-1", json!({"active":true}), 0, |_, _| {
            panic!("deadline already elapsed")
        })
        .unwrap_err();
        assert_eq!(error.code, "FLOW_RUN_TIMEOUT");
        assert_eq!(error.retryable, Some(false));
        assert_eq!(error.data.unwrap()["run_id"], "run-1");
    }

    #[test]
    fn pause_and_stop_terminate_waiting_in_success_and_error_envelopes() {
        for (flag, code) in [
            ("user_paused", "FLOW_RUN_PAUSED"),
            ("user_stopped", "FLOW_RUN_STOPPED"),
        ] {
            let mut state = json!({"run_id":"run-1","active":true});
            state[flag] = json!(true);
            let error = wait_run("run-1", state.clone(), 5, |_, _| {
                panic!("user intervention ends automation")
            })
            .unwrap_err();
            assert_eq!(error.code, code);
            let error = run_backend_error(
                None,
                &json!({}),
                BackendError::new(flag, "intervened").with_data(json!({"result":state})),
            );
            assert_eq!(error.code, code);
            assert_eq!(error.data.unwrap()["run_id"], "run-1");
        }
    }

    #[test]
    fn wait_must_not_switch_to_another_run() {
        let error = wait_run("run-1", json!({"active":true}), 5, |_, _| {
            Ok(json!({"run_id":"run-2","active":false,"run_succeeded":true}))
        })
        .unwrap_err();
        assert_eq!(error.code, "FLOW_RUN_MISMATCH");
        assert_eq!(error.data.unwrap()["run_id"], "run-1");
    }

    #[test]
    fn absent_diagnostics_are_not_an_empty_error_list() {
        assert!(static_errors(&json!({})).is_err());
        assert!(static_errors(&json!({"static_errors":null})).is_err());
        assert_eq!(
            static_errors(&json!({"static_errors":[]})).unwrap(),
            json!([])
        );
    }

    #[test]
    fn diagnostic_failure_keeps_applied_mutation_and_disables_retry() {
        let result = json!({"block_region":{"inserted":[{"block_id":"new-id"}]}});
        let error =
            mutation_diagnostics_error("edit", &result, BackendError::new("MCP_TIMEOUT", "late"));
        assert_eq!(error.code, "MUTATION_DIAGNOSTICS_FAILED");
        assert_eq!(error.retryable, Some(false));
        let data = error.data.unwrap();
        assert_eq!(data["applied"], true);
        assert_eq!(data["edit"], result);
        assert_eq!(data["result"], result);
    }
}
