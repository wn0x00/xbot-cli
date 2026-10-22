//! Offline, progressive command discovery. Help is not a schema export.
use clap::{ArgAction, ArgMatches, Command, CommandFactory};
use serde_json::{json, Value};
use std::ffi::OsString;

use crate::cli::Cli;
use crate::core::{BackendError, Res};
use crate::output::{self, OutputMode};
use crate::schema_cmds;

pub fn command_tree() -> Command {
    fn configure(command: Command) -> Command {
        let is_group = command.has_subcommands();
        let command = command
            .color(clap::ColorChoice::Never)
            .disable_help_subcommand(true);
        let command = if is_group {
            command
                .subcommand_required(false)
                .arg_required_else_help(false)
        } else {
            command
        };
        command.mut_subcommands(configure)
    }
    let mut root = configure(Cli::command()).bin_name("xbot-cli");
    root.build();
    root
}

pub fn command_path(matches: &ArgMatches) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = matches;
    while let Some((name, child)) = current.subcommand() {
        path.push(name.to_owned());
        current = child;
    }
    path
}

/// Only used for rendering parser errors or explicit help; it never dispatches work.
pub fn partial_path(root: &Command, args: &[OsString]) -> Vec<String> {
    fn relaxed(command: Command) -> Command {
        command
            .ignore_errors(true)
            .subcommand_required(false)
            .arg_required_else_help(false)
            .mut_args(|arg| {
                let action = arg.get_action();
                if matches!(
                    action,
                    ArgAction::Help
                        | ArgAction::HelpShort
                        | ArgAction::HelpLong
                        | ArgAction::Version
                ) {
                    arg.required(false).action(ArgAction::SetTrue)
                } else {
                    arg.required(false)
                }
            })
            .mut_subcommands(relaxed)
    }
    relaxed(root.clone())
        .try_get_matches_from(args)
        .map(|matches| command_path(&matches))
        .unwrap_or_default()
}

pub fn find_command(root: &Command, path: &[String]) -> Res<Command> {
    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
    let canonical = schema_cmds::canonical_path(&refs);
    let mut command = root;
    for name in &canonical {
        command = command
            .get_subcommands()
            .find(|c| c.get_name() == name || c.get_all_aliases().any(|alias| alias == name))
            .ok_or_else(|| {
                BackendError::new("BAD_ARGS", format!("未知命令路径: {}", path.join(" ")))
                    .with_hint("运行 xbot-cli help 查看命令，再逐层查看分组帮助")
            })?;
    }
    Ok(command.clone())
}

pub fn is_group(root: &Command, path: &[String]) -> bool {
    // Dispatch must inspect the parsed grammar, not a canonical help alias.
    // Legacy `mcp` takes its action as a value and is an executable leaf.
    let mut command = root;
    for name in path {
        let Some(child) = command.get_subcommands().find(|c| c.get_name() == name) else {
            return false;
        };
        command = child;
    }
    command.has_subcommands()
}

fn examples(path: &[&str]) -> &'static [&'static str] {
    match path {
        [] => &[
            "xbot-cli status",
            "xbot-cli app list",
            "xbot-cli app open <app-id>",
        ],
        ["app"] => &[
            "xbot-cli app list",
            "xbot-cli app open <app-id>",
            "xbot-cli app save",
        ],
        ["flow"] => &[
            "xbot-cli flow list",
            "xbot-cli flow get main",
            "xbot-cli flow check main",
        ],
        ["flow", "code"] => &[
            "xbot-cli flow code read main",
            "xbot-cli flow code write main --file main.py --dry-run",
        ],
        ["flow", "visual"] => &[
            "xbot-cli flow visual list main",
            "xbot-cli flow visual fill main --fills @fills.json --dry-run",
        ],
        ["flow", "params"] => &[
            "xbot-cli flow params list main",
            "xbot-cli flow params set main --operations @params.json --dry-run",
        ],
        ["task"] => &[
            "xbot-cli task run <app-id> --inputs-json @inputs.json",
            "xbot-cli task status <task-id>",
        ],
        ["win32"] => &[
            "xbot-cli help win32 snapshot",
            "xbot-cli schema win32 snapshot",
        ],
        ["win32", "snapshot"] => {
            &["xbot-cli win32 snapshot --hwnd <HWND> --pid <PID> --timeout 10 --json"]
        }
        ["tool"] => &["xbot-cli tool studio list", "xbot-cli tool browser list"],
        ["tool", "studio"] => &[
            "xbot-cli tool studio list",
            "xbot-cli tool studio schema app.get_info",
            "xbot-cli tool studio call app.get_info '{}'",
        ],
        ["tool", "browser"] => &[
            "xbot-cli tool browser list",
            "xbot-cli tool browser schema web_get_active",
        ],
        ["web"] => &[
            "xbot web open example.com",
            "xbot web fill e4 \"hello\"",
            "xbot web click e5",
            "xbot web snapshot --output page.txt",
        ],
        ["wiki"] => &["xbot-cli wiki list", "xbot-cli wiki search CodeFlow"],
        _ => &[],
    }
}

pub fn show(root: &Command, path: &[String], full: bool, mode: OutputMode) -> Res<i32> {
    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
    let canonical = schema_cmds::canonical_path(&refs);
    let words: Vec<&str> = canonical.iter().map(String::as_str).collect();
    let mut command = find_command(root, path)?;
    let name = if canonical.is_empty() {
        "xbot-cli".into()
    } else {
        canonical.join(" ")
    };
    let invocation = if canonical.is_empty() {
        "xbot-cli".into()
    } else {
        format!("xbot-cli {}", canonical.join(" "))
    };
    let usage = command.render_usage().to_string();
    let sample = examples(&words);
    let children: Vec<Value> = command.get_subcommands().filter(|c| !c.is_hide_set()).map(|c| {
        json!({"name":c.get_name(),"about":c.get_about().map(ToString::to_string).unwrap_or_default()})
    }).collect();
    let schema_command = format!("xbot-cli schema {}", canonical.join(" "))
        .trim_end()
        .to_owned();
    if matches!(mode, OutputMode::Json | OutputMode::Pretty) {
        let mut data = json!({
            "name":name, "about":command.get_about().map(ToString::to_string).unwrap_or_default(),
            "usage":usage, "commands":children, "examples":sample,
            "help":format!("{invocation} --help"), "schema":schema_command,
        });
        if full {
            data["options"] = json!(command.get_arguments().filter(|arg| !arg.is_hide_set()).map(|arg| {
                json!({"name":arg.get_long().map(|s| format!("--{s}")).unwrap_or_else(|| arg.get_id().to_string()),
                    "required":arg.is_required_set(), "help":arg.get_help().map(ToString::to_string).unwrap_or_default()})
            }).collect::<Vec<_>>());
        }
        return Ok(output::emit(true, Some(data), None, mode));
    }
    if full {
        if !sample.is_empty() {
            command = command.after_help(format!(
                "示例:\n{}\n\n机器输出: --json\n参数结构: {schema_command}",
                sample
                    .iter()
                    .map(|s| format!("  {s}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        Ok(output::write_text(
            &command.render_long_help().to_string(),
            true,
            0,
        ))
    } else {
        let mut lines = vec![format!(
            "{invocation} — {}\n\n{usage}",
            command
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default()
        )];
        if !children.is_empty() {
            lines.push("\n命令:".into());
            for child in &children {
                lines.push(format!(
                    "  {:<12} {}",
                    child["name"].as_str().unwrap_or_default(),
                    child["about"].as_str().unwrap_or_default()
                ));
            }
        }
        if !sample.is_empty() {
            lines.push("\n示例:".into());
            for example in sample {
                lines.push(format!("  {example}"));
            }
        }
        lines.push(format!(
            "\n详细帮助: {invocation} --help\n机器输出: --json    参数结构: {schema_command}"
        ));
        Ok(output::write_text(&lines.join("\n"), true, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_parse_without_actions_but_leaf_arguments_still_validate() {
        for args in [
            vec!["xbot-cli"],
            vec!["xbot-cli", "app"],
            vec!["xbot-cli", "flow", "code"],
            vec!["xbot-cli", "tool", "studio"],
        ] {
            let matches = command_tree().try_get_matches_from(args).unwrap();
            assert!(is_group(&command_tree(), &command_path(&matches)));
        }
        assert!(command_tree()
            .try_get_matches_from(["xbot-cli", "app", "open"])
            .is_err());
    }

    #[test]
    fn help_scope_uses_the_parser_including_values_and_aliases() {
        let args = ["xbot-cli", "--format", "json", "app", "open", "--help"].map(OsString::from);
        assert_eq!(partial_path(&command_tree(), &args), ["app", "open"]);
        assert!(find_command(&command_tree(), &["not-a-command".into()]).is_err());
    }
}
