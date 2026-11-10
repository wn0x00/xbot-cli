//! app 命令组:应用生命周期(新建/打开/保存/同步/发布/运行/任务/回收)。

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use std::collections::HashSet;

use crate::core::{self, BackendError, Res};
use crate::{flow_cmds, input};

pub type CmdOut = (Value, Vec<String>);

pub(crate) const STUDIO_SYNC_ARGS: &[&str] = &["studio", "current", "sync"];

pub fn app_new(name: &str, desc: Option<&str>) -> Res<CmdOut> {
    let mut args = vec!["studio", "open", "--builder", "custom", "--name", name];
    if let Some(d) = desc {
        args.extend(["--description", d]);
    }
    if let Err(e) = core::shell_cli(&args, 180) {
        if e.code == "studio_already_open"
            || e.message.contains("已被打开")
            || e.message.to_lowercase().contains("busy")
        {
            return Err(BackendError::new("STUDIO_BUSY", e.message)
                .with_hint("Studio 已打开别的应用:先 xbot-cli app sync(保存上传并关闭),再重试"));
        }
        return Err(e);
    }
    // studio open 会保持 Studio 打开;等它就绪后从 studio-mcp 拿 app_id
    // (创建返回里未必带 UUID,以 get_info 为准)
    core::wait_studio_open(90)?;
    let info = core::mcp("app.get_info", json!({}), 130)?;
    let app_id = info
        .pointer("/app/app_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            BackendError::new("PARSE_FAILED", "Studio 已打开但 app.get_info 无 app_id")
                .with_hint(core::truncate(&info.to_string(), 300))
        })?;
    Ok((
        json!({"app_id": app_id, "name": name, "mode": "edit",
               "flows": info.get("flows")}),
        vec![
            "xbot-cli flow list                       # 先确认默认 main 的类型".into(),
            "xbot-cli flow create hello --kind code    # CodeFlow:新建代码流程后再 code write"
                .into(),
            "xbot-cli flow visual list main --detail        # VisualFlow:读取默认 main 的块".into(),
        ],
    ))
}

pub(crate) fn concise_app_rows(apps: &[Value]) -> Vec<Value> {
    apps.iter()
        .map(|app| {
            json!({
                "appId": app.get("appId"),
                "appName": app.get("appName"),
                "appType": app.get("appType"),
                "updateTime": app.get("updateTime"),
            })
        })
        .collect()
}

pub fn app_list(search: Option<&str>, page: u32, page_size: u32) -> Res<CmdOut> {
    let mut args = vec![
        "console".into(),
        "app".into(),
        "--page".into(),
        page.to_string(),
        "--page-size".into(),
        page_size.to_string(),
    ];
    if let Some(s) = search {
        args.extend(["--search".into(), s.into()]);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let d = core::shell_cli(&refs, 30)?;
    let apps = d
        .get("apps")
        .or_else(|| d.get("list"))
        .or_else(|| d.get("items"))
        .cloned()
        .unwrap_or(d);
    let (apps, count) = match apps {
        Value::Array(items) => {
            let concise = concise_app_rows(&items);
            let count = concise.len();
            (Value::Array(concise), json!(count))
        }
        other => (other, Value::Null),
    };
    Ok((json!({"apps": apps, "count": count}), vec![]))
}

pub fn app_open(app_id: &str, timeout: u64, force: bool) -> Res<CmdOut> {
    // 编辑模式打开(studio open --app-id)。只有编辑模式的改动才能 save/sync/publish。
    if force {
        close_studio_for_switch()?;
    }
    let d = core::edit_open_app(app_id, timeout.max(120))?;
    if d.get("already_open").and_then(Value::as_bool) == Some(true) {
        // 已开着:确认是同一个应用,且必须是编辑模式(View 会话要重开)
        core::require_expected_app(app_id)?;
        core::require_edit_mode(Some(app_id))?; // 同应用但 View 会话 → 报错提示重开
        core::wait_studio_open(15)?;
        let info = core::mcp("app.get_info", json!({}), 130)?;
        return Ok((
            json!({"app_id": app_id, "already_open": true, "mode": "edit",
                          "app": info.get("app"), "flows": info.get("flows")}),
            vec![],
        ));
    }
    let state = core::wait_studio_open(timeout)?;
    let info = core::mcp("app.get_info", json!({}), 130)?;
    let cur = info
        .pointer("/app/app_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if cur != app_id {
        return Err(BackendError::new(
            "OPEN_MISMATCH",
            format!("Studio 打开的是 {cur},不是 {app_id}"),
        ));
    }
    Ok((
        json!({"app_id": app_id, "mode": "edit",
               "studio": state.get("studioEngineStatus"),
               "app": info.get("app"), "flows": info.get("flows")}),
        vec![
            "xbot-cli flow list                # 查看流程".into(),
            "xbot-cli app save                 # 改完后保存".into(),
        ],
    ))
}

pub fn app_view(app_id: &str, timeout: u64) -> Res<CmdOut> {
    // URL 协议 View 模式:只读查看。改动不会持久化也不会上传,仅用于看一眼。
    core::url_open_app(app_id)?;
    let state = core::wait_studio_open(timeout)?;
    core::require_expected_app(app_id)?;
    Ok((
        json!({"app_id": app_id, "mode": "view(只读,改动不落盘不同步)",
               "studio": state.get("studioEngineStatus")}),
        vec!["xbot-cli app open <app-id>   # 要开发请用编辑模式重开".into()],
    ))
}

fn close_studio_for_switch() -> Res<()> {
    let state = core::shell_cli(&["system", "state"], 15)?;
    if state.get("hasStudioOpened").and_then(Value::as_bool) == Some(false) {
        return Ok(());
    }
    // A failed close cannot authorize continuing the switch to another app.
    let result =
        core::shell_cli(STUDIO_SYNC_ARGS, 60).map_err(|error| error.with_retryable(false))?;
    sleep(Duration::from_secs(2));
    let state = core::shell_cli(&["system", "state"], 15)?;
    if state.get("hasStudioOpened").and_then(Value::as_bool) != Some(false) {
        return Err(
            BackendError::new("STUDIO_BUSY", "未确认旧 Studio 已关闭，未继续切换应用")
                .with_data(json!({"close":result,"state":state}))
                .with_retryable(false),
        );
    }
    Ok(())
}

pub fn app_current() -> Res<CmdOut> {
    Ok((core::mcp("app.get_info", json!({}), 130)?, vec![]))
}

pub fn app_save() -> Res<CmdOut> {
    core::require_current_edit_mode()?;
    let r = core::mcp("app.save", json!({}), 130)?;
    let errs = flow_cmds::diagnose_after_mutation("save", &r)?;
    let empty = errs.as_array().map(|a| a.is_empty()).unwrap_or(true);
    let nxt = if empty {
        vec!["xbot-cli app sync   # 上传到云端(会关闭 Studio)".into()]
    } else {
        vec!["# 有静态错误,先修复再 sync".into()]
    };
    Ok((json!({"save": r, "static_errors": errs}), nxt))
}

pub fn app_sync() -> Res<CmdOut> {
    let app_id = core::require_current_edit_mode()?;
    let t0 = chrono::Local::now();
    let call = core::shell_cli(STUDIO_SYNC_ARGS, 300);
    sleep(Duration::from_secs(1));
    let closed = core::shell_cli(&["system", "state"], 15)
        .map(|st| st.get("hasStudioOpened").and_then(Value::as_bool) == Some(false))
        .unwrap_or(false);
    let uploaded = core::log_has_upload(&app_id, t0);
    let mut data = sync_outcome(&app_id, call, closed, uploaded)?;
    if !uploaded {
        data["note"] = json!("后端报告同步完成，但未观察到本次上传日志；若没有改动，可能无需上传");
    }
    Ok((
        data,
        vec!["xbot-cli app publish <app-id> --log '说明'  # 发布新版本".into()],
    ))
}

fn sync_outcome(app_id: &str, call: Res<Value>, closed: bool, uploaded: bool) -> Res<Value> {
    let evidence = json!({"app_id":app_id,"studio_closed":closed,"uploaded":uploaded});
    let response = call.map_err(|error| {
        let mut data = evidence.clone();
        data["synced"] = Value::Null;
        data["call_error"] = flow_cmds::error_evidence(&error);
        BackendError::new("SYNC_FAILED", "同步调用失败；Studio 关闭不能证明同步成功")
            .with_data(data)
            .with_retryable(false)
            .with_hint("检查应用与上传记录后再决定下一步；不要因 Studio 关闭就发布或重复同步")
    })?;
    if response.get("success").and_then(Value::as_bool) == Some(false) {
        let mut data = evidence;
        data["synced"] = json!(false);
        data["response"] = response;
        return Err(BackendError::new("SYNC_FAILED", "后端报告同步失败")
            .with_data(data)
            .with_retryable(false));
    }
    Ok(
        json!({"app_id":app_id,"synced":true,"studio_closed":closed,"uploaded":uploaded,"response":response}),
    )
}

pub fn app_publish(app_id: &str, log: Option<&str>) -> Res<CmdOut> {
    // Snapshot identities before publishing. A matching historical log is not a new release.
    let baseline = versions_of(app_id)?;
    let known: HashSet<String> = baseline.iter().filter_map(version_key).collect();
    if baseline
        .iter()
        .any(|version| version_key(version).is_none())
    {
        return Err(
            BackendError::new("VERSIONS_BAD_OUTPUT", "版本列表缺少稳定标识，未执行发布")
                .with_data(json!({"versions":baseline,"applied":false})),
        );
    }
    let publish = core::shell_cli(
        &[
            "console",
            "app",
            "publish",
            "--app-id",
            app_id,
            "--update-log",
            log.unwrap_or(""),
        ],
        180,
    )
    .map_err(|error| error.with_retryable(false))?;
    if publish.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(BackendError::new("PUBLISH_FAILED", "后端报告发布失败")
            .with_data(json!({"app_id":app_id,"response":publish}))
            .with_retryable(false));
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut last_error = None;
    while Instant::now() < deadline {
        sleep(Duration::from_secs(3));
        let versions = match versions_of(app_id) {
            Ok(versions) => versions,
            Err(error) => {
                last_error = Some(flow_cmds::error_evidence(&error));
                continue;
            }
        };
        for v in versions {
            if is_new_release(&v, &known, log) {
                return Ok((
                    json!({"confirmed":true,"app_id":app_id,"version":v.get("version"),"version_id":v.get("versionId"),"log":v.get("versionLog"),"publish":publish}),
                    vec![format!("xbot-cli task run {app_id}   # 运行已发布版本验证")],
                ));
            }
        }
    }
    Err(BackendError::new("PUBLISH_NOT_CONFIRMED",
        "publish 调用成功但 60s 内 versions 未出现新版本(服务端内容无变化?)")
        .with_data(json!({"app_id":app_id,"confirmed":false,"publish":publish,"verification_error":last_error,"baseline_version_ids":known}))
        .with_retryable(false)
        .with_hint(format!("先 xbot-cli app versions {app_id} 检查版本；发布结果未确认，不要自动重复发布")))
}

fn version_key(version: &Value) -> Option<String> {
    for key in ["versionId", "version"] {
        if let Some(value) = version
            .get(key)
            .filter(|v| v.is_number() || v.as_str().is_some_and(|s| !s.is_empty()))
        {
            return Some(format!("{key}:{value}"));
        }
    }
    None
}

fn is_new_release(version: &Value, known: &HashSet<String>, log: Option<&str>) -> bool {
    version_key(version).is_some_and(|key| !known.contains(&key))
        && log.filter(|s| !s.is_empty()).is_none_or(|expected| {
            version.get("versionLog").and_then(Value::as_str) == Some(expected)
        })
}

fn versions_of(app_id: &str) -> Res<Vec<Value>> {
    let d = core::shell_cli(&["console", "app", "versions", "--app-id", app_id], 30)?;
    if let Some(a) = d.as_array() {
        return Ok(a.clone());
    }
    for k in ["versions", "list", "items", "records"] {
        if let Some(a) = d.get(k).and_then(Value::as_array) {
            return Ok(a.clone());
        }
    }
    d.get("data")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| {
            BackendError::new("VERSIONS_BAD_OUTPUT", "版本响应缺少版本数组").with_data(d)
        })
}

pub fn app_versions(app_id: &str) -> Res<CmdOut> {
    Ok((json!({"versions": versions_of(app_id)?}), vec![]))
}

pub fn app_detail(app_id: &str) -> Res<CmdOut> {
    Ok((
        core::shell_cli(&["console", "app", "detail", "--app-id", app_id], 30)?,
        vec![],
    ))
}

pub fn app_run(
    app_id: &str,
    inputs: Vec<String>,
    inputs_json: Option<&str>,
    no_wait: bool,
    timeout: u64,
) -> Res<CmdOut> {
    app_run_prepared(
        app_id,
        input::app_inputs(&inputs, inputs_json)?,
        no_wait,
        timeout,
    )
}

pub fn app_run_prepared(app_id: &str, inputs: Value, no_wait: bool, timeout: u64) -> Res<CmdOut> {
    let inputs = input::object(inputs, "应用输入")?;
    let mut args = vec![
        "console".into(),
        "task".into(),
        "run".into(),
        "--app-id".into(),
        app_id.into(),
    ];
    if !inputs.as_object().unwrap().is_empty() {
        args.extend(["--inputs".into(), inputs.to_string()]);
    }
    if no_wait {
        args.push("--async".into()); // 若官方不支持会在输出里体现
    }
    // task run 默认同步+流式日志,耗时取决于应用本身
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let d = core::shell_cli_raw(&refs, timeout).map_err(|error| error.with_retryable(false))?;
    let data = d.get("data").cloned().unwrap_or(json!({}));
    let ok = d.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && data.get("success").and_then(Value::as_bool).unwrap_or(true);
    let task_id = data
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            data.get("task_id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .map(str::to_string)
        });
    let mut nxt = vec![];
    if let Some(t) = &task_id {
        nxt.push(format!("xbot-cli task logs {t}   # 拉取运行日志"));
    }
    if !ok {
        let code = data
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("RUN_FAILED");
        let msg = data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("运行失败");
        return Err(BackendError::new(code, msg)
            .with_data(json!({"task_id":task_id,"task":data,"response":d}))
            .with_retryable(false)
            .with_hint("xbot-cli task logs <task-id> 查日志，不要自动重复启动任务"));
    }
    if (!no_wait && data.get("success").and_then(Value::as_bool) != Some(true))
        || (no_wait && task_id.is_none())
    {
        return Err(BackendError::new(
            "RUN_NOT_CONFIRMED",
            "后端受理了任务，但响应不足以确认执行结果或后续查询标识",
        )
        .with_data(json!({"task_id":task_id,"task":data,"response":d}))
        .with_retryable(false)
        .with_hint("先检查任务状态与日志，不要自动重复启动任务"));
    }
    Ok((
        json!({"task": data, "task_id": task_id,"completed":!no_wait}),
        nxt,
    ))
}

pub fn app_stop(task_id: &str, reason: Option<&str>) -> Res<CmdOut> {
    let mut args = vec![
        "console".into(),
        "task".into(),
        "stop".into(),
        "--task-id".into(),
        task_id.into(),
    ];
    if let Some(r) = reason {
        args.extend(["--reason".into(), r.into()]);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok((core::shell_cli(&refs, 30)?, vec![]))
}

pub fn app_task_status(task_id: &str) -> Res<CmdOut> {
    Ok((
        core::shell_cli(&["console", "task", "status", "--task-id", task_id], 30)?,
        vec![],
    ))
}

pub fn app_task_logs(task_id: &str, cursor: Option<i64>) -> Res<CmdOut> {
    let mut args = vec![
        "console".into(),
        "task".into(),
        "logs".into(),
        "--task-id".into(),
        task_id.into(),
    ];
    if let Some(c) = cursor {
        args.extend(["--cursor".into(), c.to_string()]);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok((core::shell_cli(&refs, 30)?, vec![]))
}

pub fn app_recycle(app_id: &str) -> Res<CmdOut> {
    let d = core::shell_cli(
        &[
            "console",
            "app",
            "recycle",
            "--app-id",
            app_id,
            "--app-type",
            "developed",
        ],
        30,
    )?;
    Ok((json!({"recycled": true, "detail": d}), vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_studio_does_not_turn_a_failed_sync_into_success() {
        let error = sync_outcome(
            "app-1",
            Err(BackendError::new("TIMEOUT", "lost response")),
            true,
            false,
        )
        .unwrap_err();
        assert_eq!(error.code, "SYNC_FAILED");
        assert_eq!(error.retryable, Some(false));
        assert_eq!(error.data.unwrap()["studio_closed"], true);
        assert!(sync_outcome("app-1", Ok(json!({"success":false})), true, true).is_err());
    }

    #[test]
    fn unchanged_app_sync_can_succeed_without_an_upload_log() {
        let data = sync_outcome("app-1", Ok(json!({"success":true})), true, false).unwrap();
        assert_eq!(data["synced"], true);
        assert_eq!(data["uploaded"], false);
    }

    #[test]
    fn sync_and_editor_closure_are_reported_separately() {
        let response = json!({"success":true,"taskId":"sync-task"});
        let data = sync_outcome("app-1", Ok(response.clone()), false, true).unwrap();
        assert_eq!(data["app_id"], "app-1");
        assert_eq!(data["synced"], true);
        assert_eq!(data["studio_closed"], false);
        assert_eq!(data["uploaded"], true);
        assert_eq!(data["response"], response);
        assert!(data.get("published").is_none());
    }

    #[test]
    fn publish_requires_new_identity_even_when_log_is_reused() {
        let old = json!({"versionId":"old-id","version":"1.0","versionLog":"same log"});
        let known = HashSet::from([version_key(&old).unwrap()]);
        assert!(!is_new_release(&old, &known, Some("same log")));
        assert!(is_new_release(
            &json!({"versionId":"new-id","versionLog":"same log"}),
            &known,
            Some("same log")
        ));
        assert!(!is_new_release(
            &json!({"versionId":"new-id","versionLog":"other log"}),
            &known,
            Some("same log")
        ));
        assert!(!is_new_release(
            &json!({"versionLog":"same log"}),
            &known,
            Some("same log")
        ));
    }
}
