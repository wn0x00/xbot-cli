//! Machine-readable command contract, derived from clap plus shared semantic metadata.

use std::any::TypeId;

use clap::{Arg, Command};
use serde_json::{json, Value};

use crate::app_cmds::CmdOut;
use crate::core::{BackendError, Res};

/// Shared with dispatch: discovery and execution use the same context/effect policy.
#[derive(Clone, Copy, Debug)]
pub struct CommandMetadata {
    pub effects: &'static [&'static str],
    pub context: &'static str,
    pub requires_studio: bool,
    pub supports_dry_run: bool,
    pub confirmation: &'static str,
}

/// Normalize compatibility entry points before looking up command semantics.
pub fn canonical_path(path: &[&str]) -> Vec<String> {
    let words: Vec<&str> = match path {
        ["code", rest @ ..] => [vec!["flow", "code"], rest.to_vec()].concat(),
        ["visual", rest @ ..] => [vec!["flow", "visual"], rest.to_vec()].concat(),
        ["app", "run", rest @ ..] => [vec!["task", "run"], rest.to_vec()].concat(),
        ["app", "stop", rest @ ..] => [vec!["task", "stop"], rest.to_vec()].concat(),
        ["app", "task-status", rest @ ..] => [vec!["task", "status"], rest.to_vec()].concat(),
        ["app", "task-logs", rest @ ..] => [vec!["task", "logs"], rest.to_vec()].concat(),
        ["app", "new", rest @ ..] => [vec!["app", "create"], rest.to_vec()].concat(),
        ["flow", "new", rest @ ..] => [vec!["flow", "create"], rest.to_vec()].concat(),
        ["flow", "read", rest @ ..] => [vec!["flow", "get"], rest.to_vec()].concat(),
        ["mcp"] => vec!["tool", "studio"],
        ["mcp", "tools", rest @ ..] => [vec!["tool", "studio", "list"], rest.to_vec()].concat(),
        ["mcp", action @ ("schema" | "call"), rest @ ..] => {
            [vec!["tool", "studio", action], rest.to_vec()].concat()
        }
        ["web" | "browser", "tools", rest @ ..] => {
            [vec!["tool", "browser", "list"], rest.to_vec()].concat()
        }
        ["web" | "browser", action @ ("schema" | "call"), rest @ ..] => {
            [vec!["tool", "browser", action], rest.to_vec()].concat()
        }
        ["web" | "browser", "attach", rest @ ..] => [vec!["web", "active"], rest.to_vec()].concat(),
        ["web" | "browser", "back", rest @ ..] => [vec!["web", "go-back"], rest.to_vec()].concat(),
        ["web" | "browser", "forward", rest @ ..] => {
            [vec!["web", "go-forward"], rest.to_vec()].concat()
        }
        ["browser", rest @ ..] => [vec!["web"], rest.to_vec()].concat(),
        _ => path.to_vec(),
    };
    words.into_iter().map(str::to_owned).collect()
}

pub fn command_metadata(path: &[&str]) -> CommandMetadata {
    let normalized = canonical_path(path);
    let words: Vec<&str> = normalized.iter().map(String::as_str).collect();
    let (effects, context, supports_dry_run, confirmation): (
        &'static [&'static str],
        &'static str,
        bool,
        &'static str,
    ) = match words.as_slice() {
        ["app", "current"] => (&["read"], "studio", false, "none"),
        ["app", "save"] => (&["write"], "studio", true, "none"),
        ["app", "sync"] => (&["write", "close_studio"], "studio", true, "none"),
        ["app", "open"] => (&["change_studio_context"], "console", true, "when_force"),
        ["app", "create"] => (&["write", "change_studio_context"], "console", true, "none"),
        ["app", "view"] => (&["open_view"], "console", true, "none"),
        ["app", "publish" | "recycle"] => (&["write"], "console", true, "always"),
        ["app", ..] | ["status" | "doctor" | "state"] => (&["read"], "console", false, "none"),
        ["task", "run"] => (&["execute"], "console", true, "none"),
        ["task", "stop"] => (&["stop_execution"], "console", true, "none"),
        ["task", ..] => (&["read"], "console", false, "none"),
        ["platform", "api", "call"] => (&["endpoint_defined"], "platform", true, "when_mutating"),
        ["platform", "api", ..] | ["platform", "docs"] | ["platform", "auth", "status"] => {
            (&["read"], "local", false, "none")
        }
        ["platform", "auth", "check"] => (&["authenticate"], "platform", false, "none"),
        ["platform", ..] => (&["read"], "platform", true, "none"),
        ["flow", "run" | "verify"] => (&["execute"], "studio", true, "none"),
        ["flow", "stop"] => (&["stop_execution"], "studio", true, "none"),
        ["flow", "create"]
        | ["flow", "params", "set"]
        | ["flow", "code", "write" | "edit"]
        | ["flow", "visual", "edit" | "fill" | "toggle" | "enable" | "disable"] => {
            (&["write"], "studio", true, "none")
        }
        ["flow", ..] | ["diag"] => (&["read"], "studio", false, "none"),
        ["web", "open" | "active" | "goto" | "go-back" | "go-forward" | "reload" | "close"] => {
            (&["change_browser_context"], "browser", true, "none")
        }
        ["web", "act" | "click" | "dblclick" | "fill" | "type" | "press" | "hover" | "select" | "check"
        | "uncheck"] => (&["interact"], "browser", true, "none"),
        ["web", "snapshot"] => (&["read", "write_file_when_output"], "browser", true, "none"),
        ["web", ..] => (&["read"], "browser", false, "none"),
        ["win32", ..] => (&["read"], "windows_uia", false, "none"),
        ["tool", "studio", "call"] => (&["tool_defined"], "studio", true, "tool_defined"),
        ["tool", "browser", "call"] => (&["tool_defined"], "browser", true, "tool_defined"),
        ["tool", ..] => (&["read"], "service", false, "none"),
        ["wiki", ..] => (&["read"], "service", false, "none"),
        ["install"] => (&["write_files"], "local", true, "when_overwriting"),
        _ => (&["read"], "local", false, "none"),
    };
    CommandMetadata {
        effects,
        context,
        requires_studio: context == "studio",
        supports_dry_run,
        confirmation,
    }
}

pub fn cmd_schema(root: &Command, path: &[String]) -> Res<CmdOut> {
    // Build first: clap resolves inferred positions, value arity and inherited globals here.
    let mut built = root.clone();
    built.build();
    let requested: Vec<&str> = path.iter().map(String::as_str).collect();
    let normalized = canonical_path(&requested);
    let mut command = &built;
    let mut actual = Vec::new();
    for segment in &normalized {
        command = command
            .get_subcommands()
            .find(|c| c.get_name() == segment || c.get_all_aliases().any(|a| a == segment))
            .ok_or_else(|| {
                BackendError::new("BAD_ARGS", format!("未知命令路径: {}", path.join(" ")))
                    .with_hint("先用 xbot-cli schema 或 xbot-cli schema <group> 查看可用命令")
            })?;
        actual.push(command.get_name());
    }
    let metadata = command_metadata(&actual);
    let mut data = json!({
        "schema_version": 1,
        "name": if actual.is_empty() { built.get_name().to_string() } else { actual.join(" ") },
        "requested_path": path.join(" "),
        "about": command.get_about().map(ToString::to_string),
        "aliases": command.get_all_aliases().collect::<Vec<_>>(),
        "args": command_args(&built, command),
        "commands": command_summaries(command),
        "examples": examples(command, &actual),
        "effects": metadata.effects,
        "context": metadata.context,
        "requires_studio": metadata.requires_studio,
        "supports_dry_run": metadata.supports_dry_run,
        "confirmation": metadata.confirmation,
        "input_shapes": input_shapes(&actual),
        "dynamic_schema_hint": dynamic_schema_hint(&actual),
    });
    if path.is_empty() {
        data["contract"] = json!({
            "success": "stdout; auto uses terminal text or non-terminal JSON; exit 0",
            "error": "stderr in selected result format; semantic non-zero exit",
            "help": "bare command and groups show concise text help, exit 0; --help or help <path> show details; --json requests compact structured help; missing leaf arguments fail",
            "formats": ["auto", "text", "json", "pretty"],
            "discovery": "help is text even in pipes unless a JSON format is explicit; schema defaults to JSON; guide defaults to Markdown text",
            "exit_codes": {"0":"success","1":"operation","2":"validation","3":"authentication","4":"network","5":"conflict","6":"internal_or_protocol","10":"confirmation"},
            "dry_run": "local input validation and action preview only; runtime context, permissions and backend acceptance are not verified",
            "expect_app": "assert the current application before Studio commands; dry-run does not verify this runtime condition",
            "routing": {
                "default_application_backend": "shell-cli",
                "default_flow_backend": "studio-mcp",
                "platform_requires_explicit_command": true,
                "automatic_platform_fallback": false,
            },
        });
    }
    Ok((data, vec![]))
}

fn command_summaries(command: &Command) -> Vec<Value> {
    command
        .get_subcommands()
        .filter(|c| c.get_name() != "help" && !c.is_hide_set())
        .map(|c| {
            json!({
                "name": c.get_name(),
                "about": c.get_about().map(ToString::to_string),
                "aliases": c.get_all_aliases().collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn command_args(root: &Command, command: &Command) -> Vec<Value> {
    let mut args: Vec<Value> = root
        .get_arguments()
        .filter(|a| a.is_global_set() && !a.is_hide_set())
        .map(arg_json)
        .collect();
    let mut names: Vec<String> = args
        .iter()
        .filter_map(|a| a["name"].as_str().map(str::to_owned))
        .collect();
    for arg in command.get_arguments().filter(|a| !a.is_hide_set()) {
        let name = arg.get_id().as_str().to_string();
        if !names.contains(&name) {
            names.push(name);
            args.push(arg_json(arg));
        }
    }
    args
}

fn value_type(arg: &Arg) -> &'static str {
    let id = arg.get_value_parser().type_id();
    if id == TypeId::of::<bool>() {
        "boolean"
    } else if [
        TypeId::of::<u64>(),
        TypeId::of::<u32>(),
        TypeId::of::<u16>(),
        TypeId::of::<u8>(),
        TypeId::of::<usize>(),
        TypeId::of::<i64>(),
        TypeId::of::<i32>(),
        TypeId::of::<i16>(),
        TypeId::of::<i8>(),
        TypeId::of::<isize>(),
    ]
    .iter()
    .any(|expected| id == *expected)
    {
        "integer"
    } else if [TypeId::of::<f32>(), TypeId::of::<f64>()]
        .iter()
        .any(|expected| id == *expected)
    {
        "number"
    } else if id == TypeId::of::<std::path::PathBuf>() {
        "path"
    } else {
        "string"
    }
}

fn arg_json(arg: &Arg) -> Value {
    let value_names: Vec<String> = arg
        .get_value_names()
        .map(|names| names.iter().map(ToString::to_string).collect())
        .unwrap_or_default();
    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();
    let choices: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_string())
        .collect();
    let arity = arg.get_num_args().map(|n| {
        json!({
            "min": n.min_values(),
            "max": if n.max_values() == usize::MAX { None } else { Some(n.max_values()) },
        })
    });
    json!({
        "name": arg.get_id().as_str(),
        "long": arg.get_long(),
        "short": arg.get_short().map(|c| c.to_string()),
        "aliases": arg.get_all_aliases().unwrap_or_default(),
        "positional": arg.is_positional(),
        "position": arg.get_index(),
        "action": format!("{:?}", arg.get_action()),
        "value_type": value_type(arg),
        "minimum": match arg.get_id().as_str() { "timeout" | "limit" | "page" | "page_size" => Some(1), "offset" => Some(0), _ => None },
        "maximum": if arg.get_id().as_str() == "timeout" { Some(crate::cli::MAX_TIMEOUT_SECS) } else { None },
        "arity": arity,
        "required": arg.is_required_set(),
        "global": arg.is_global_set(),
        "value_names": value_names,
        "defaults": defaults,
        "choices": choices,
        "help": arg.get_help().map(ToString::to_string),
    })
}

fn json_input(schema: Value) -> Value {
    json!({"encoding": ["literal_json", "@file", "stdin:-"], "schema": schema})
}

fn nonempty_text() -> Value {
    json!({"type":"string", "minLength":1, "pattern":"\\S"})
}

fn array_shape(items: Value, min_items: usize) -> Value {
    json!({"type":"array", "minItems":min_items, "items":items})
}

fn parameter_operations_shape() -> Value {
    array_shape(
        json!({
            "type":"object", "additionalProperties":false, "required":["op","name"],
            "properties":{
                "op":{"enum":["upsert","remove"]}, "name":nonempty_text(),
                "direction":{}, "type":{}, "kind":{"enum":["Text","Expression"]},
                "value":{"type":"string"}, "description":{"type":"string"}
            },
            "if":{"properties":{"op":{"const":"upsert"}}},
            "then":{"required":["direction","type"], "properties":{
                "direction":{"enum":["In","Out"]},
                "type":{"enum":["str","file","int","bool","float","any"]}
            }}
        }),
        1,
    )
}

fn visual_blocks_shape() -> Value {
    array_shape(
        json!({
            "type":"object", "additionalProperties":false,
            "required":["prototype_name","comment"],
            "properties":{"prototype_name":nonempty_text(),"comment":nonempty_text()}
        }),
        1,
    )
}

fn visual_fills_shape() -> Value {
    array_shape(
        json!({
            "type":"object", "additionalProperties":false, "required":["block_id"],
            "properties":{
                "block_id":nonempty_text(), "inputs":{"type":"object"},
                "outputs":{"type":"object", "additionalProperties":nonempty_text()}
            }
        }),
        0,
    )
}

fn visual_groups_shape() -> Value {
    array_shape(
        json!({
            "type":"object", "additionalProperties":false,
            "required":["block_id","input","columns"],
            "properties":{
                "block_id":nonempty_text(), "input":nonempty_text(), "fixed":{"type":"object"},
                "columns":{"type":"object","minProperties":1,"additionalProperties":array_shape(json!({}),1)}
            }
        }),
        0,
    )
}

fn input_shapes(path: &[&str]) -> Value {
    match path {
        ["platform", "api", "call"] => json!({
            "body":json_input(json!({"type":"object"})),
            "query":json_input(json!({"type":"object","additionalProperties":{"type":["string","number","boolean","null"]}})),
            "path":json_input(json!({"type":"object","additionalProperties":{"type":"string","minLength":1}})),
            "file":{"encoding":["file_path"],"endpoint":"file.upload"},
            "constraints":["at most one parameter can read stdin", "GET parameters use query", "required top-level fields and documented path parameters are validated offline"]
        }),
        ["flow", "params", "set"] => json!({"operations": {
            "encoding":["literal_json","@file","stdin:-"],
            "schema":{"oneOf":[parameter_operations_shape(), {
                "type":"object", "additionalProperties":false, "required":["operations"],
                "properties":{"operations":parameter_operations_shape()}
            }]},
            "examples":[[{"op":"upsert","name":"text","direction":"In","type":"str","value":"hello"}], [{"op":"remove","name":"old_parameter"}]]
        }}),
        ["flow", "visual", "fill"] => {
            let mut fills = visual_fills_shape();
            fills["minItems"] = json!(1);
            json!({"fills":{
                "encoding":["literal_json","@file","stdin:-"],
                "schema":{"oneOf":[fills, {
                    "type":"object", "additionalProperties":false,
                    "properties":{"fills":visual_fills_shape(),"group_fills":visual_groups_shape()},
                    "anyOf":[{"required":["fills"],"properties":{"fills":{"minItems":1}}},{"required":["group_fills"],"properties":{"group_fills":{"minItems":1}}}]
                }]},
                "constraints":["fills.block_id values must be unique", "group_fills block_id/input pairs must be unique", "all columns in a group must have the same nonzero length"],
                "examples":[[{"block_id":"<actual-block-id>","inputs":{"snippet":"print('hi')"}}]]
            }})
        }
        ["flow", "visual", "edit"] => {
            json!({"file": {"encoding":["file_path", "stdin:-"], "schema":{"oneOf":[
                visual_blocks_shape(),
                {"type":"object", "additionalProperties":false,"required":["op","block_index","blocks"], "properties":{"op":{"const":"insert"},"blocks":visual_blocks_shape(),"block_index":{"type":"integer","minimum":-1}}},
                {"type":"object", "additionalProperties":false,"required":["op","block_ids"], "properties":{"op":{"const":"remove"},"block_ids":array_shape(nonempty_text(),1)}}
            ]},"examples":[{"op":"insert","block_index":-1,"blocks":[{"prototype_name":"programing.snippet","comment":"print greeting"}]}, {"op":"remove","block_ids":["<actual-block-id>"]}]}})
        }
        ["flow", "code", "write"] => {
            json!({"file":{"encoding":["file_path", "stdin:-"],"content_type":"text/x-python"}})
        }
        ["flow", "code", "edit"] => {
            json!({"old":{"encoding":["literal_text", "@file", "stdin:-"]},"new":{"encoding":["literal_text", "@file", "stdin:-"]}})
        }
        ["task", "run"] => {
            json!({"inputs_json":json_input(json!({"type":"object"})),"inputs":{"encoding":"repeated --input KEY=VALUE", "value":"JSON value if valid JSON, otherwise string", "precedence":"inputs_json overrides repeated input keys"}})
        }
        ["web", "act"] => json!({"args_json":json_input(json!({"type":"object"}))}),
        ["tool", _, "call"] => {
            json!({"args_json":json_input(json!({"type":"object"})), "backend_fields":"Fetch the selected tool schema; fields depend on tool and installed client version"})
        }
        _ => json!({}),
    }
}

fn dynamic_schema_hint(path: &[&str]) -> Option<&'static str> {
    match path {
        ["platform", "api", "call"] => Some("xbot-cli platform api schema <endpoint>"),
        ["tool", "studio", ..] => Some("xbot-cli tool studio schema <tool>"),
        ["tool", "browser", ..] => Some("xbot-cli tool browser schema <tool>"),
        ["web", "act" | "click" | "dblclick" | "fill" | "type" | "press" | "hover" | "select" | "check"
        | "uncheck" | "goto" | "reload" | "go-back" | "go-forward" | "close"] => {
            Some("xbot-cli tool browser schema web_action")
        }
        ["flow", "params", "set"] => Some("xbot-cli tool studio schema flow.write_parameters"),
        ["flow", "visual", "edit"] => Some("xbot-cli tool studio schema flow.edit_blocks"),
        ["flow", "visual", "fill"] => Some("xbot-cli tool studio schema flow.fill_blocks"),
        _ => None,
    }
}

fn examples(command: &Command, path: &[&str]) -> Vec<String> {
    if let Some(help) = command.get_after_help() {
        return vec![help.to_string()];
    }
    let examples: &[&str] = match path {
        ["web", "open"] => &[
            "xbot web open example.com",
            "xbot web open https://example.com --mode edge",
        ],
        ["web", "active"] => &["xbot web active"],
        ["web", "snapshot"] => &["xbot web snapshot", "xbot web snapshot --output page.txt"],
        ["web", "click"] => &[
            "xbot web click e5",
            "xbot web click e5 --button right --no-snapshot",
        ],
        ["web", "fill"] => &["xbot web fill e4 \"hello\""],
        ["web", "press"] => &["xbot web press Enter", "xbot web press Control+A --ref e4"],
        ["web", "select"] => &["xbot web select e9 blue"],
        ["web", "goto"] => &["xbot web goto https://example.com/page"],
        ["flow", "check"] => &["xbot-cli flow check main --expect-app <app-id>"],
        ["flow", "wait"] => &["xbot-cli flow wait <run-id> --timeout 600"],
        ["flow", "code", "read"] => &["xbot-cli flow code read main"],
        ["flow", "code", "write"] => {
            &["xbot-cli flow code write main --file main.py --expect-app <app-id>"]
        }
        ["flow", "code", "edit"] => {
            &["xbot-cli flow code edit main --old @before.txt --new @after.txt"]
        }
        ["flow", "params", "set"] => &["xbot-cli flow params set main --operations @params.json"],
        ["flow", "visual", "edit"] => &["xbot-cli flow visual edit main --file ops.json"],
        ["flow", "visual", "fill"] => &["xbot-cli flow visual fill main --fills @fills.json"],
        ["flow", "visual", "enable"] => &["xbot-cli flow visual enable <block-id> --flow main"],
        ["flow", "visual", "disable"] => &["xbot-cli flow visual disable <block-id> --flow main"],
        ["task", "run"] => &["xbot-cli task run <app-id> --inputs-json @inputs.json"],
        ["task", "status"] => &["xbot-cli task status <task-id>"],
        ["task", "logs"] => &["xbot-cli task logs <task-id>"],
        ["tool", "studio", "call"] => &["xbot-cli tool studio call app.get_info '{}'"],
        ["tool", "browser", "call"] => &["xbot-cli tool browser call web_get_active '{}'"],
        _ => &[],
    };
    examples.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{value_parser, ArgAction};

    fn fixture() -> Command {
        Command::new("xbot-cli")
            .arg(
                Arg::new("dry_run")
                    .long("dry-run")
                    .global(true)
                    .action(ArgAction::SetTrue),
            )
            .subcommand(
                Command::new("flow")
                    .subcommand(
                        Command::new("code").subcommand(
                            Command::new("edit")
                                .alias("replace")
                                .arg(Arg::new("flow_id").index(1))
                                .arg(Arg::new("old").long("old").required(true)),
                        ),
                    )
                    .subcommand(
                        Command::new("wait")
                            .arg(Arg::new("run_id").required(true))
                            .arg(
                                Arg::new("timeout")
                                    .long("timeout")
                                    .value_parser(value_parser!(u64))
                                    .default_value("600"),
                            ),
                    )
                    .subcommand(Command::new("visual").subcommand(
                        Command::new("toggle").arg(Arg::new("args").required(true).num_args(2..)),
                    )),
            )
            .subcommand(
                Command::new("tool")
                    .subcommand(Command::new("studio").subcommand(Command::new("call"))),
            )
            .subcommand(Command::new("code").hide(true))
    }

    fn inspect(path: &[&str]) -> Value {
        cmd_schema(
            &fixture(),
            &path.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .unwrap()
        .0
    }

    #[test]
    fn schema_resolves_canonical_and_compatibility_paths() {
        assert_eq!(inspect(&["code", "edit"])["name"], "flow code edit");
        assert_eq!(
            inspect(&["flow", "code", "replace"])["name"],
            "flow code edit"
        );
        assert_eq!(inspect(&["mcp", "call"])["name"], "tool studio call");
        assert!(inspect(&[])["commands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["name"] != "code"));
    }

    #[test]
    fn schema_reports_built_arity_positions_types_and_globals_once() {
        let wait = inspect(&["flow", "wait"]);
        let args = wait["args"].as_array().unwrap();
        assert_eq!(args.iter().filter(|a| a["name"] == "dry_run").count(), 1);
        let timeout = args.iter().find(|a| a["name"] == "timeout").unwrap();
        assert_eq!(timeout["value_type"], "integer");
        assert_eq!(timeout["arity"], json!({"min":1,"max":1}));
        let run_id = args.iter().find(|a| a["name"] == "run_id").unwrap();
        assert_eq!(run_id["position"], 1);
        let toggle = inspect(&["flow", "visual", "toggle"]);
        let positional = toggle["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == "args")
            .unwrap();
        assert_eq!(positional["arity"], json!({"min":2,"max":null}));
    }

    #[test]
    fn semantics_distinguish_read_execution_and_confirmation() {
        let check = command_metadata(&["flow", "check"]);
        assert_eq!(check.effects, &["read"]);
        assert!(check.requires_studio);
        assert!(!check.supports_dry_run);
        assert_eq!(command_metadata(&["flow", "verify"]).effects, &["execute"]);
        assert_eq!(command_metadata(&["app", "publish"]).confirmation, "always");
        assert_eq!(
            command_metadata(&["app", "open"]).confirmation,
            "when_force"
        );
        assert!(!command_metadata(&["task", "run"]).requires_studio);
        assert!(command_metadata(&["code", "edit"]).requires_studio);
    }

    #[test]
    fn structured_payloads_expose_shape_and_dynamic_schema() {
        assert_eq!(
            input_shapes(&["task", "run"])["inputs_json"]["schema"]["type"],
            "object"
        );
        assert_eq!(
            input_shapes(&["flow", "params", "set"])["operations"]["schema"]["oneOf"][0]["items"]
                ["type"],
            "object"
        );
        let raw = inspect(&["tool", "studio", "call"]);
        assert_eq!(
            raw["dynamic_schema_hint"],
            "xbot-cli tool studio schema <tool>"
        );
        assert_eq!(raw["effects"], json!(["tool_defined"]));
    }

    #[test]
    fn documented_payload_examples_pass_the_actual_input_validators() {
        let params = input_shapes(&["flow", "params", "set"]);
        for example in params["operations"]["examples"].as_array().unwrap() {
            crate::input::normalize_parameters(example.clone()).unwrap();
        }
        let edits = input_shapes(&["flow", "visual", "edit"]);
        for example in edits["file"]["examples"].as_array().unwrap() {
            crate::input::normalize_visual_edit(example.clone()).unwrap();
        }
        let fills = input_shapes(&["flow", "visual", "fill"]);
        for example in fills["fills"]["examples"].as_array().unwrap() {
            crate::input::normalize_visual_fill(example.clone()).unwrap();
        }
    }
}
