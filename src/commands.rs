//! Execution routing; parsed writes are prepared by plan.rs first.
use crate::app_cmds::CmdOut;
use crate::cli::*;
use crate::core::{self, BackendError, Res};
use crate::{app_cmds, extra_cmds, flow_cmds, guide_cmds, misc_cmds, schema_cmds, web_cmds};
use clap::CommandFactory;
use serde_json::{json, Value};

pub fn execute(group: Group, yes: bool) -> Res<CmdOut> {
    use app_cmds as a;
    use extra_cmds as e;
    use flow_cmds as f;
    use misc_cmds as m;
    match group {
        Group::Version => Ok((
            json!({"name":"xbot-cli","version":env!("CARGO_PKG_VERSION")}),
            vec![],
        )),
        Group::Help { .. } => Err(BackendError::new("BAD_ARGS", "帮助应在执行前处理")),
        Group::Task(sub) => execute(task_as_app(sub), yes),
        Group::Tool(channel) => tool_command(channel),
        Group::Status => m::cmd_status(),
        Group::Doctor => m::cmd_doctor(),
        Group::Diag => m::cmd_diag(),
        Group::State => m::cmd_state(),
        Group::Win32(sub) => crate::win32_cmds::execute(sub),
        Group::App(sub) => match sub {
            AppSub::Create { name, desc } => a::app_new(&name, desc.as_deref()),
            AppSub::List {
                search,
                page,
                page_size,
            } => a::app_list(search.as_deref(), page, page_size),
            AppSub::Open {
                app_id,
                timeout,
                force,
            } => a::app_open(&app_id, timeout, force),
            AppSub::View { app_id, timeout } => a::app_view(&app_id, timeout),
            AppSub::Current => a::app_current(),
            AppSub::Save => a::app_save(),
            AppSub::Sync => a::app_sync(),
            AppSub::Publish { app_id, log } => a::app_publish(&app_id, log.as_deref()),
            AppSub::Versions { app_id } => a::app_versions(&app_id),
            AppSub::Detail { app_id } => a::app_detail(&app_id),
            AppSub::Run {
                app_id,
                inputs,
                inputs_json,
                no_wait,
                timeout,
            } => a::app_run(&app_id, inputs, inputs_json.as_deref(), no_wait, timeout),
            AppSub::Stop { task_id, reason } => a::app_stop(&task_id, reason.as_deref()),
            AppSub::TaskStatus { task_id } => a::app_task_status(&task_id),
            AppSub::TaskLogs { task_id, cursor } => a::app_task_logs(&task_id, cursor),
            AppSub::Recycle { app_id } => a::app_recycle(&app_id),
        },
        Group::Flow(sub) => match sub {
            FlowSub::Code(sub) => execute(Group::Code(sub), yes),
            FlowSub::Visual(sub) => execute(Group::Visual(sub), yes),
            FlowSub::Check { flow_id } => f::flow_check(flow_id.as_deref()),
            FlowSub::Wait { run_id, timeout } => f::flow_wait(&run_id, timeout),
            FlowSub::List => f::flow_list(),
            FlowSub::Create { name, kind } => f::flow_new(&name, &kind),
            FlowSub::Get { flow_id } => f::flow_get(flow_id.as_deref()),
            FlowSub::Params(params) => match params {
                FlowParamsSub::List { flow_id } => f::flow_params(flow_id.as_deref()),
                FlowParamsSub::Set {
                    flow_id,
                    operations,
                } => f::flow_params_set(flow_id.as_deref(), &operations),
            },
            FlowSub::Run { flow_id, timeout } => f::flow_run(flow_id.as_deref(), timeout),
            FlowSub::Verify { flow_id, timeout } => f::flow_verify(flow_id.as_deref(), timeout),
            FlowSub::Stop => f::flow_stop(),
            FlowSub::Logs { cursor, limit } => f::flow_logs(cursor, limit),
        },
        Group::Code(sub) => match sub {
            CodeSub::Read { flow_id } => e::code_read(flow_id.as_deref()),
            CodeSub::Write { flow_id, file } => e::code_write(flow_id.as_deref(), &file),
            CodeSub::Edit {
                flow_id,
                old,
                new,
                replace_all,
            } => e::code_edit(flow_id.as_deref(), &old, &new, replace_all),
        },
        Group::Visual(sub) => match sub {
            VisualSub::Enable { flow_id, block_ids } => {
                f::flow_toggle(flow_id.as_deref(), "uncomment", &block_ids)
            }
            VisualSub::Disable { flow_id, block_ids } => {
                f::flow_toggle(flow_id.as_deref(), "comment", &block_ids)
            }
            VisualSub::List {
                flow_id,
                detail,
                offset,
                limit,
            } => f::flow_blocks(flow_id.as_deref(), detail, offset, limit),
            VisualSub::Edit { flow_id, file } => f::flow_blocks_set(flow_id.as_deref(), &file),
            VisualSub::Fill { flow_id, fills } => f::flow_fill(flow_id.as_deref(), &fills),
            VisualSub::Toggle { args } => {
                let (flow_id, op, block_ids) = parse_visual_toggle(&args)?;
                f::flow_toggle(flow_id.as_deref(), &op, &block_ids)
            }
            VisualSub::Variables {
                flow_id,
                offset,
                limit,
            } => f::flow_variables(flow_id.as_deref(), offset, limit),
            VisualSub::Snapshots { flow_id, block_id } => {
                f::flow_snapshots(flow_id.as_deref(), block_id.as_deref())
            }
        },
        Group::Browser(sub) => match sub {
            BrowserSub::Act { args_json } => e::browser_act(&args_json),
            BrowserSub::Eval { code } => e::browser_eval(&code),
            BrowserSub::Tools { full } => e::browser_tools(full),
            BrowserSub::Schema { tool } => e::browser_schema(&tool),
            BrowserSub::Call {
                tool,
                args_json,
                timeout,
            } => e::browser_call(&tool, args_json.as_deref(), timeout),
            sub => web_cmds::prepare(&sub)?
                .ok_or_else(|| BackendError::new("BAD_ARGS", "未知网页操作"))?
                .execute(),
        },
        Group::Wiki(sub) => match sub {
            WikiSub::List => e::wiki_list(),
            WikiSub::Search { query } => e::wiki_search(&query),
            WikiSub::Read { doc_ids } => e::wiki_read(&doc_ids),
        },
        Group::Schema { path } => schema_cmds::cmd_schema(&Cli::command(), &path),
        Group::Guide { topic } => guide_cmds::cmd_guide(topic.as_deref()),
        Group::Install { skills, global } => {
            if !skills {
                Err(BackendError::new("BAD_ARGS", "请显式指定 --skills"))
            } else {
                guide_cmds::install_skills(global, yes)
            }
        }
        Group::Mcp {
            action,
            args,
            timeout,
            full,
        } => m::cmd_mcp(&action, &args, timeout, full),
    }
}

pub fn parse_visual_toggle(args: &[String]) -> Res<(Option<String>, String, Vec<String>)> {
    fn check_op(op: &str) -> Res<()> {
        if matches!(op, "comment" | "uncomment") {
            Ok(())
        } else {
            Err(
                BackendError::new("BAD_ARGS", format!("invalid value '{op}' for '<OP>'"))
                    .with_hint("op 必须是 comment 或 uncomment"),
            )
        }
    }
    if args.len() < 2 {
        return Err(BackendError::new(
            "BAD_ARGS",
            "visual toggle [flow_id] comment|uncomment <block-id...>",
        ));
    }
    if matches!(args[0].as_str(), "comment" | "uncomment") {
        let op = args[0].clone();
        check_op(&op)?;
        Ok((None, op, args[1..].to_vec()))
    } else {
        if args.len() < 3 {
            return Err(BackendError::new(
                "BAD_ARGS",
                "用法: visual toggle [flow_id] <op> <block-id...>",
            ));
        }
        let op = args[1].clone();
        check_op(&op)?;
        Ok((Some(args[0].clone()), op, args[2..].to_vec()))
    }
}

pub fn task_as_app(task: TaskSub) -> Group {
    Group::App(match task {
        TaskSub::Run {
            app_id,
            inputs,
            inputs_json,
            no_wait,
            timeout,
        } => AppSub::Run {
            app_id,
            inputs,
            inputs_json,
            no_wait,
            timeout,
        },
        TaskSub::Stop { task_id, reason } => AppSub::Stop { task_id, reason },
        TaskSub::Status { task_id } => AppSub::TaskStatus { task_id },
        TaskSub::Logs { task_id, cursor } => AppSub::TaskLogs { task_id, cursor },
    })
}

fn tool_command(channel: ToolChannel) -> Res<CmdOut> {
    let (browser, sub) = match channel {
        ToolChannel::Studio(sub) => (false, sub),
        ToolChannel::Browser(sub) => (true, sub),
    };
    match sub {
        ToolSub::List { full, timeout } => {
            let tools = core::mcp_list(browser, timeout)?;
            let items: Vec<Value> = tools
                .iter()
                .map(|t| {
                    if full {
                        json!({"name": t.get("name"), "description": t.get("description")})
                    } else {
                        t.get("name").cloned().unwrap_or(Value::Null)
                    }
                })
                .collect();
            Ok((json!({"tools": items, "count": items.len()}), vec![]))
        }
        ToolSub::Schema { tool, timeout } => {
            let tools = core::mcp_list(browser, timeout)?;
            let found = tools
                .into_iter()
                .find(|t| t.get("name").and_then(Value::as_str) == Some(&tool))
                .ok_or_else(|| {
                    BackendError::new("MCP_TOOL_UNKNOWN", format!("未知工具: {tool}"))
                })?;
            Ok((found, vec![]))
        }
        ToolSub::Call { .. } => Err(BackendError::new(
            "MCP_BAD_RESPONSE",
            "工具调用必须先经过输入准备",
        )),
    }
}
