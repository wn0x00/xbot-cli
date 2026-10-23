//! Prepare inputs once, then either describe or execute that same operation.
//! Preview is deliberately offline: application state is checked only on execution.
use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::cli::*;
use crate::core::{self, BackendError, Res};
use crate::{app_cmds, commands, extra_cmds, flow_cmds, input, web_cmds};

enum Operation {
    Command(Group),
    Platform(crate::platform::PreparedPlatform),
    TaskRun {
        app_id: String,
        inputs: Value,
        no_wait: bool,
        timeout: u64,
    },
    Parameters {
        flow_id: Option<String>,
        payload: Value,
    },
    CodeWrite {
        flow_id: Option<String>,
        content: String,
    },
    CodeEdit {
        flow_id: Option<String>,
        old: String,
        new: String,
        replace_all: bool,
    },
    VisualEdit {
        flow_id: Option<String>,
        payload: Value,
    },
    VisualFill {
        flow_id: Option<String>,
        payload: Value,
    },
    BrowserAct(Value),
    Web(web_cmds::PreparedWeb),
    ToolCall {
        browser: bool,
        tool: String,
        args: Value,
        timeout: u64,
    },
}

pub struct PreparedCommand {
    operation: Operation,
    preview: Option<Value>,
    pub confirmation_required: bool,
}

impl PreparedCommand {
    pub fn preview(&self, expected_app: Option<&str>) -> Res<CmdOut> {
        let mut data = self.preview.clone().ok_or_else(|| {
            BackendError::new("BAD_ARGS", "此命令不产生可预览的操作，无需 --dry-run")
        })?;
        data["dry_run"] = json!(true);
        data["validation"] = json!({"input": "validated", "runtime": "not_checked"});
        data["confirmation_required"] = json!(self.confirmation_required);
        if let Some(id) = expected_app {
            data["expected_app_id"] = json!(id);
        }
        Ok((data, vec![]))
    }

    pub fn confirmation_error(&self) -> BackendError {
        let preview = self.preview.as_ref();
        BackendError::new("CONFIRMATION_REQUIRED", "此操作需要明确确认")
            .with_data(json!({
                "action": preview.and_then(|p| p.get("action")),
                "target": preview.and_then(|p| p.get("target")),
            }))
            .with_hint("先使用 --dry-run 检查目标和影响；取得明确同意后追加 --yes")
    }

    pub fn execute(self, yes: bool) -> Res<CmdOut> {
        match self.operation {
            Operation::Platform(command) => command.execute(),
            Operation::Command(group) => commands::execute(group, yes),
            Operation::TaskRun {
                app_id,
                inputs,
                no_wait,
                timeout,
            } => app_cmds::app_run_prepared(&app_id, inputs, no_wait, timeout),
            Operation::Parameters { flow_id, payload } => {
                flow_cmds::flow_params_set_prepared(flow_id.as_deref(), payload)
            }
            Operation::CodeWrite { flow_id, content } => {
                extra_cmds::code_write_prepared(flow_id.as_deref(), &content)
            }
            Operation::CodeEdit {
                flow_id,
                old,
                new,
                replace_all,
            } => extra_cmds::code_edit_prepared(flow_id.as_deref(), &old, &new, replace_all),
            Operation::VisualEdit { flow_id, payload } => {
                flow_cmds::flow_blocks_set_prepared(flow_id.as_deref(), payload)
            }
            Operation::VisualFill { flow_id, payload } => {
                flow_cmds::flow_fill_prepared(flow_id.as_deref(), payload)
            }
            Operation::BrowserAct(args) => extra_cmds::browser_act_prepared(args),
            Operation::Web(command) => command.execute(),
            Operation::ToolCall {
                browser,
                tool,
                args,
                timeout,
            } => {
                if browser {
                    extra_cmds::browser_call_prepared(&tool, args, timeout)
                } else {
                    Ok((core::mcp(&tool, args, timeout)?, vec![]))
                }
            }
        }
    }
}

fn planned(
    action: &'static str,
    target: Value,
    arguments: Value,
    operation: Operation,
) -> PreparedCommand {
    PreparedCommand {
        operation,
        preview: Some(json!({"action": action, "target": target, "arguments": arguments})),
        confirmation_required: false,
    }
}

fn flow_target(flow_id: &Option<String>) -> Res<Value> {
    if flow_id.as_ref().is_some_and(|id| id.trim().is_empty()) {
        return Err(BackendError::new("BAD_INPUT", "flow_id 不能为空"));
    }
    Ok(
        json!({"flow_id": flow_id, "resolution": if flow_id.is_some() {"explicit_at_execution"} else {"current_at_execution"}}),
    )
}

fn nonempty(value: &str, name: &str) -> Res<()> {
    if value.trim().is_empty() {
        return Err(BackendError::new("BAD_INPUT", format!("{name} 不能为空")));
    }
    Ok(())
}

fn app_target(id: &str) -> Res<Value> {
    if !core::is_uuid(id) {
        return Err(BackendError::new("BAD_APP_ID", "app_id 必须是 UUID"));
    }
    Ok(json!({"app_id": id}))
}

fn tool_call(
    browser: bool,
    tool: String,
    raw: Option<String>,
    timeout: u64,
) -> Res<PreparedCommand> {
    nonempty(&tool, "tool")?;
    let args = input::browser_args(raw.as_deref())?;
    Ok(planned(
        if browser {
            "tool.browser.call"
        } else {
            "tool.studio.call"
        },
        json!({"tool": tool}),
        args.clone(),
        Operation::ToolCall {
            browser,
            tool,
            args,
            timeout,
        },
    ))
}

pub fn prepare(group: Group) -> Res<PreparedCommand> {
    // Compatibility routes immediately join the same preparation path.
    let group = match group {
        Group::Flow(FlowSub::Code(sub)) => Group::Code(sub),
        Group::Flow(FlowSub::Visual(sub)) => Group::Visual(sub),
        Group::Task(sub) => commands::task_as_app(sub),
        other => other,
    };
    match group {
        Group::Platform(sub) => {
            let command = crate::platform::prepare(sub)?;
            let preview = command.preview();
            let confirmation_required = command.confirmation_required();
            Ok(PreparedCommand {
                operation: Operation::Platform(command),
                preview,
                confirmation_required,
            })
        }
        Group::Win32(Win32Sub::Snapshot { ref hwnd, .. }) => {
            crate::win32_cmds::parse_hwnd(hwnd)?;
            Ok(PreparedCommand {
                operation: Operation::Command(group),
                preview: None,
                confirmation_required: false,
            })
        }
        Group::App(AppSub::Run {
            app_id,
            inputs,
            inputs_json,
            no_wait,
            timeout,
        }) => {
            let inputs = input::app_inputs(&inputs, inputs_json.as_deref())?;
            Ok(planned(
                "task.run",
                app_target(&app_id)?,
                json!({"inputs": inputs, "no_wait": no_wait, "timeout": timeout}),
                Operation::TaskRun {
                    app_id,
                    inputs,
                    no_wait,
                    timeout,
                },
            ))
        }
        Group::Flow(FlowSub::Params(FlowParamsSub::Set {
            flow_id,
            operations,
        })) => {
            let payload = input::flow_parameters(&operations)?;
            Ok(planned(
                "flow.params.set",
                flow_target(&flow_id)?,
                payload.clone(),
                Operation::Parameters { flow_id, payload },
            ))
        }
        Group::Code(CodeSub::Write { flow_id, file }) => {
            let content = core::read_file_or_stdin(&file)?;
            Ok(planned(
                "flow.code.write",
                flow_target(&flow_id)?,
                json!({"content": content}),
                Operation::CodeWrite { flow_id, content },
            ))
        }
        Group::Code(CodeSub::Edit {
            flow_id,
            old,
            new,
            replace_all,
        }) => {
            if old == "-" && new == "-" {
                return Err(BackendError::new(
                    "BAD_INPUT",
                    "--old 和 --new 不能同时读取 stdin",
                ));
            }
            let old = core::text_arg(&old)?;
            let new = core::text_arg(&new)?;
            if old.is_empty() {
                return Err(BackendError::new("BAD_INPUT", "待替换的原文不能为空"));
            }
            Ok(planned(
                "flow.code.edit",
                flow_target(&flow_id)?,
                json!({"old_string": old, "new_string": new, "replace_all": replace_all}),
                Operation::CodeEdit {
                    flow_id,
                    old,
                    new,
                    replace_all,
                },
            ))
        }
        Group::Visual(VisualSub::Edit { flow_id, file }) => {
            let payload = input::visual_edit(&file)?;
            Ok(planned(
                "flow.visual.edit",
                flow_target(&flow_id)?,
                payload.clone(),
                Operation::VisualEdit { flow_id, payload },
            ))
        }
        Group::Visual(VisualSub::Fill { flow_id, fills }) => {
            let payload = input::visual_fill(&fills)?;
            Ok(planned(
                "flow.visual.fill",
                flow_target(&flow_id)?,
                payload.clone(),
                Operation::VisualFill { flow_id, payload },
            ))
        }
        Group::Browser(BrowserSub::Act { args_json }) => {
            let args = input::browser_args(Some(&args_json))?;
            Ok(planned(
                "web.act",
                json!("active_page"),
                args.clone(),
                Operation::BrowserAct(args),
            ))
        }
        Group::Tool(ToolChannel::Studio(ToolSub::Call {
            tool,
            args_json,
            timeout,
        })) => tool_call(false, tool, args_json, timeout),
        Group::Tool(ToolChannel::Browser(ToolSub::Call {
            tool,
            args_json,
            timeout,
        }))
        | Group::Browser(BrowserSub::Call {
            tool,
            args_json,
            timeout,
        }) => tool_call(true, tool, args_json, timeout),
        Group::Browser(sub) => {
            if let Some(command) = web_cmds::prepare(&sub)? {
                let preview = Some(command.preview());
                Ok(PreparedCommand {
                    operation: Operation::Web(command),
                    preview,
                    confirmation_required: false,
                })
            } else {
                Ok(PreparedCommand {
                    operation: Operation::Command(Group::Browser(sub)),
                    preview: None,
                    confirmation_required: false,
                })
            }
        }
        Group::Mcp {
            action,
            args,
            timeout,
            full,
        } => {
            let required = match action.as_str() {
                "tools" => (0, 0),
                "schema" => (1, 1),
                "call" => (1, 2),
                _ => (0, 1),
            };
            if !(required.0..=required.1).contains(&args.len()) {
                return Err(BackendError::new("BAD_ARGS", "mcp 参数数量不正确")
                    .with_hint("使用 tool studio list|schema|call 查看推荐入口"));
            }
            match action.as_str() {
                "tools" => prepare(Group::Tool(ToolChannel::Studio(ToolSub::List {
                    full,
                    timeout,
                }))),
                "schema" => prepare(Group::Tool(ToolChannel::Studio(ToolSub::Schema {
                    tool: args[0].clone(),
                    timeout,
                }))),
                "call" => tool_call(false, args[0].clone(), args.get(1).cloned(), timeout),
                _ => tool_call(false, action, args.first().cloned(), timeout),
            }
        }
        group => {
            let description = describe(&group)?;
            let confirmation_required = matches!(
                &group,
                Group::App(
                    AppSub::Publish { .. }
                        | AppSub::Recycle { .. }
                        | AppSub::Open { force: true, .. }
                )
            );
            Ok(PreparedCommand {
                operation: Operation::Command(group),
                preview: description,
                confirmation_required,
            })
        }
    }
}

fn describe(group: &Group) -> Res<Option<Value>> {
    let (action, target, args) = match group {
        Group::App(AppSub::Create { name, desc }) => {
            nonempty(name, "name")?;
            (
                "app.create",
                json!({"name": name}),
                json!({"description": desc}),
            )
        }
        Group::App(AppSub::Open {
            app_id,
            force,
            timeout,
        }) => (
            "app.open",
            app_target(app_id)?,
            json!({"force": force, "timeout": timeout}),
        ),
        Group::App(AppSub::View { app_id, timeout }) => {
            ("app.view", app_target(app_id)?, json!({"timeout": timeout}))
        }
        Group::App(AppSub::Save) => ("app.save", json!("current"), json!({})),
        Group::App(AppSub::Sync) => ("app.sync", json!("current"), json!({})),
        Group::App(AppSub::Publish { app_id, log }) => {
            ("app.publish", app_target(app_id)?, json!({"log": log}))
        }
        Group::App(AppSub::Recycle { app_id }) => ("app.recycle", app_target(app_id)?, json!({})),
        Group::App(AppSub::Stop { task_id, reason }) => {
            nonempty(task_id, "task_id")?;
            (
                "task.stop",
                json!({"task_id": task_id}),
                json!({"reason": reason}),
            )
        }
        Group::Flow(FlowSub::Create { name, kind }) => {
            nonempty(name, "name")?;
            ("flow.create", json!({"name": name}), json!({"kind": kind}))
        }
        Group::Flow(FlowSub::Run { flow_id, timeout }) => (
            "flow.run",
            flow_target(flow_id)?,
            json!({"timeout": timeout}),
        ),
        Group::Flow(FlowSub::Verify { flow_id, timeout }) => (
            "flow.verify",
            flow_target(flow_id)?,
            json!({"timeout": timeout}),
        ),
        Group::Flow(FlowSub::Stop) => ("flow.stop", json!("current"), json!({})),
        Group::Visual(VisualSub::Enable { flow_id, block_ids })
        | Group::Visual(VisualSub::Disable { flow_id, block_ids }) => {
            for id in block_ids {
                nonempty(id, "block_id")?;
            }
            let op = if matches!(group, Group::Visual(VisualSub::Enable { .. })) {
                "uncomment"
            } else {
                "comment"
            };
            (
                "flow.visual.toggle",
                flow_target(flow_id)?,
                json!({"op": op, "block_ids": block_ids}),
            )
        }
        Group::Visual(VisualSub::Toggle { args }) => {
            let (flow_id, op, block_ids) = commands::parse_visual_toggle(args)?;
            for id in &block_ids {
                nonempty(id, "block_id")?;
            }
            (
                "flow.visual.toggle",
                flow_target(&flow_id)?,
                json!({"op": op, "block_ids": block_ids}),
            )
        }
        Group::Install { skills, global } => {
            if !skills {
                return Err(BackendError::new("BAD_ARGS", "请显式指定 --skills"));
            }
            (
                "install.skills",
                json!({"global": global}),
                json!({"skills": skills}),
            )
        }
        _ => return Ok(None),
    };
    Ok(Some(
        json!({"action": action, "target": target, "arguments": args}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parsed(args: &[&str]) -> Group {
        Cli::try_parse_from(std::iter::once("xbot-cli").chain(args.iter().copied()))
            .unwrap()
            .group
            .unwrap()
    }

    #[test]
    fn legacy_and_canonical_inputs_have_identical_plans() {
        let canonical = prepare(parsed(&[
            "flow",
            "visual",
            "fill",
            "main",
            "--fills",
            r#"[{"block_id":"b","inputs":{"x":1}}]"#,
        ]))
        .unwrap()
        .preview(None)
        .unwrap()
        .0;
        let legacy = prepare(parsed(&[
            "visual",
            "fill",
            "main",
            "--fills",
            r#"[{"block_id":"b","inputs":{"x":1}}]"#,
        ]))
        .unwrap()
        .preview(None)
        .unwrap()
        .0;
        assert_eq!(canonical, legacy);
        assert_eq!(canonical["validation"]["runtime"], "not_checked");
    }

    #[test]
    fn invalid_inputs_fail_before_execution_and_preview() {
        for args in [
            vec![
                "task",
                "run",
                "11111111-1111-1111-1111-111111111111",
                "--input",
                "broken",
            ],
            vec!["flow", "params", "set", "main", "--operations", "null"],
            vec!["tool", "studio", "call", "anything", "[]"],
            vec!["mcp", "tools", "extra"],
        ] {
            assert!(prepare(parsed(&args)).is_err(), "{args:?}");
        }
    }

    #[test]
    fn confirmation_uses_target_without_echoing_command_line() {
        let plan = prepare(parsed(&[
            "app",
            "publish",
            "11111111-1111-1111-1111-111111111111",
            "--log",
            "private release note",
        ]))
        .unwrap();
        assert!(plan.confirmation_required);
        let err = plan.confirmation_error();
        assert!(!err.message.contains("private release note"));
        assert!(!err
            .data
            .unwrap()
            .to_string()
            .contains("private release note"));
    }
}
