//! xbot-cli: parse -> prepare -> guard -> execute -> structured output.
mod app_cmds;
mod bridge;
mod cli;
mod commands;
mod core;
mod extra_cmds;
mod flow_cmds;
mod guide_cmds;
mod help;
mod input;
mod misc_cmds;
mod oapi;
mod oapi_cli;
mod output;
mod plan;
mod schema_cmds;
mod web_cmds;
mod win32_cmds;

use clap::FromArgMatches;
use cli::{Cli, Group};
use core::{BackendError, Res};
use output::{emit, OutputMode};
use serde_json::json;
use std::ffi::OsString;
use std::io::IsTerminal;

use help::command_path;

fn dispatch(cli: Cli, mut path: Vec<String>) -> Res<app_cmds::CmdOut> {
    let group = cli.group.expect("main handles missing command");
    if let Group::Mcp { ref action, .. } = group {
        path.push(if matches!(action.as_str(), "tools" | "schema" | "call") {
            action.clone()
        } else {
            "call".into()
        });
    }
    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
    let metadata = schema_cmds::command_metadata(&refs);
    if cli.expect_app.is_some() && !metadata.requires_studio {
        return Err(
            BackendError::new("BAD_ARGS", "--expect-app 只适用于操作当前 Studio 的命令")
                .with_hint("此命令使用显式资源 ID 或不依赖 Studio，请移除 --expect-app"),
        );
    }
    if let Some(id) = cli.expect_app.as_deref() {
        if !core::is_uuid(id) {
            return Err(BackendError::new(
                "BAD_APP_ID",
                "--expect-app 必须是应用 UUID",
            ));
        }
    }
    let plan = plan::prepare(group)?;
    if cli.dry_run {
        return plan.preview(cli.expect_app.as_deref());
    }
    if plan.confirmation_required && !cli.yes {
        return Err(plan.confirmation_error());
    }
    if let Some(id) = cli.expect_app.as_deref() {
        core::require_expected_app(id)?;
    }
    plan.execute(cli.yes)
}

/// Error/help paths still honor explicit output flags when full parsing cannot finish.
/// This only selects rendering; it never decides which command to execute.
fn requested_mode(args: &[OsString], terminal: bool, discovery: bool) -> OutputMode {
    let mut format = "auto";
    let mut force_json = false;
    let mut args = args.iter().skip(1).peekable();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        match arg.to_str() {
            Some("--json") => force_json = true,
            Some("--format") => {
                if let Some(next) = args.next().and_then(|v| v.to_str()) {
                    format = next;
                }
            }
            Some(value) if value.starts_with("--format=") => format = &value[9..],
            _ => {}
        }
    }
    if discovery && !force_json && format == "auto" {
        OutputMode::Text
    } else {
        output::resolve_mode(format, force_json, terminal)
    }
}

fn parser_error(root: &clap::Command, args: &[OsString], error: clap::Error) -> BackendError {
    use clap::error::ContextKind;
    let scope = help::partial_path(root, args);
    let invocation = format!("xbot-cli {}", scope.join(" "))
        .trim_end()
        .to_owned();
    let usage = error.get(ContextKind::Usage).map(ToString::to_string);
    let suggestions: Vec<String> = [
        ContextKind::SuggestedSubcommand,
        ContextKind::SuggestedArg,
        ContextKind::SuggestedValue,
        ContextKind::SuggestedCommand,
    ]
    .iter()
    .filter_map(|kind| error.get(*kind).map(ToString::to_string))
    .collect();
    let text = error.to_string();
    let message = text
        .split("\nUsage:")
        .next()
        .unwrap_or(&text)
        .trim()
        .trim_start_matches("error: ")
        .to_owned();
    BackendError::new("BAD_ARGS", message)
        .with_hint(format!("运行 {invocation} --help 查看参数和示例"))
        .with_data(json!({"usage":usage,"help_command":format!("{invocation} --help"),"suggestions":suggestions}))
}

fn show_help(root: &clap::Command, path: &[String], full: bool, mode: OutputMode) -> i32 {
    match help::show(root, path, full, mode) {
        Ok(code) => code,
        Err(error) => emit(false, None, Some(error), mode),
    }
}

fn run(args: Vec<OsString>, terminal: bool) -> i32 {
    let root = help::command_tree();
    let matches = match root.clone().try_get_matches_from(&args) {
        Ok(matches) => matches,
        Err(error) => {
            use clap::error::ErrorKind;
            match error.kind() {
                ErrorKind::DisplayHelp => {
                    let path = help::partial_path(&root, &args);
                    return show_help(&root, &path, true, requested_mode(&args, terminal, true));
                }
                ErrorKind::DisplayVersion => {
                    let mode = requested_mode(&args, terminal, true);
                    if matches!(mode, OutputMode::Text) {
                        return output::write_text(
                            &format!("xbot-cli {}", env!("CARGO_PKG_VERSION")),
                            true,
                            0,
                        );
                    }
                    return emit(
                        true,
                        Some(json!({"name":"xbot-cli","version":env!("CARGO_PKG_VERSION")})),
                        None,
                        mode,
                    );
                }
                _ => {
                    return emit(
                        false,
                        None,
                        Some(parser_error(&root, &args, error)),
                        requested_mode(&args, terminal, false),
                    )
                }
            }
        }
    };
    let path = command_path(&matches);
    // oapi auth <ak> <sk> 是登录动作而不是分组发现;只有裸分组才显示帮助。
    let positional_auth_login = matches
        .subcommand_matches("oapi")
        .and_then(|m| m.subcommand_matches("auth"))
        .and_then(|m| m.get_many::<String>("credentials"))
        .is_some_and(|values| values.len() > 0);
    if let Some(("help", help_matches)) = matches.subcommand() {
        let target: Vec<String> = help_matches
            .get_many::<String>("path")
            .map(|items| items.cloned().collect())
            .unwrap_or_default();
        return show_help(&root, &target, true, requested_mode(&args, terminal, true));
    }
    if path.is_empty() || (help::is_group(&root, &path) && !positional_auth_login) {
        if matches.get_flag("dry_run")
            || matches.get_flag("yes")
            || matches.get_one::<String>("expect_app").is_some()
        {
            let error =
                BackendError::new("BAD_ARGS", "操作选项需要配合明确的动作").with_hint(format!(
                    "运行 {} --help 查看可用动作",
                    format!("xbot-cli {}", path.join(" ")).trim_end()
                ));
            return emit(
                false,
                None,
                Some(error),
                requested_mode(&args, terminal, false),
            );
        }
        return show_help(&root, &path, false, requested_mode(&args, terminal, true));
    }
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => {
            return emit(
                false,
                None,
                Some(parser_error(&root, &args, error)),
                requested_mode(&args, terminal, false),
            )
        }
    };
    let mode = if !cli.json && cli.format == "auto" {
        match cli.group.as_ref() {
            Some(Group::Schema { .. }) => OutputMode::Json,
            Some(Group::Guide { .. } | Group::Version) => OutputMode::Text,
            _ => output::resolve_mode(&cli.format, false, terminal),
        }
    } else {
        output::resolve_mode(&cli.format, cli.json, terminal)
    };
    let is_version = matches!(cli.group.as_ref(), Some(Group::Version));
    match dispatch(cli, path) {
        Ok(_) if is_version && matches!(mode, OutputMode::Text) => {
            output::write_text(&format!("xbot-cli {}", env!("CARGO_PKG_VERSION")), true, 0)
        }
        Ok((data, _)) => emit(true, Some(data), None, mode),
        Err(error) => emit(false, None, Some(error), mode),
    }
}

fn main() {
    let terminal = std::io::stdout().is_terminal();
    let code = run(std::env::args_os().collect(), terminal);
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn new_command_tree_is_valid_and_nested_paths_are_discoverable() {
        Cli::command().debug_assert();
        let (schema, _) = schema_cmds::cmd_schema(
            &Cli::command(),
            &["flow".into(), "code".into(), "write".into()],
        )
        .unwrap();
        assert_eq!(schema["name"], "flow code write");
        let (root, _) = schema_cmds::cmd_schema(&Cli::command(), &[]).unwrap();
        let commands = root["commands"].as_array().unwrap();
        assert!(commands.iter().any(|c| c["name"] == "task"));
        assert!(commands.iter().any(|c| c["name"] == "tool"));
        assert!(!commands.iter().any(|c| c["name"] == "code"));
    }

    #[test]
    fn unused_expect_app_is_rejected_offline() {
        let matches = Cli::command()
            .try_get_matches_from([
                "xbot-cli",
                "status",
                "--expect-app",
                "11111111-1111-1111-1111-111111111111",
            ])
            .unwrap();
        let path = command_path(&matches);
        let cli = Cli::from_arg_matches(&matches).unwrap();
        let error = dispatch(cli, path).unwrap_err();
        assert_eq!(error.code, "BAD_ARGS");
    }
}
