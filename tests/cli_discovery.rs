//! Discovery and output are tested at the process boundary, with stdout piped.
//! An isolated HTTP canary proves that local help never consults a backend.
use serde_json::Value;
use std::io::Write;
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

fn invoke(args: &[&str]) -> Output {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let received = Arc::clone(&requests);
    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&stop);
    let worker = thread::spawn(move || {
        while !finished.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    received.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.write_all(
                        b"HTTP/1.1 500 Unexpected Backend Call\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("canary accept: {error}"),
            }
        }
    });
    let output = Command::new(env!("CARGO_BIN_EXE_xbot-cli"))
        .args(args)
        .env("XBOT_SHELL_CLI", "__xbot_test_missing_shell__.exe")
        .env("XBOT_MCP_ENDPOINT", &endpoint)
        .env("XBOT_BROWSER_MCP_ENDPOINT", &endpoint)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    stop.store(true, Ordering::SeqCst);
    worker.join().unwrap();
    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "local command {args:?} contacted a backend"
    );
    output
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn json_success(output: &Output) -> Value {
    let body = stdout(output);
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["ok"], true);
    value
}

fn json_error(args: &[&str]) -> Value {
    let output = invoke(args);
    assert_eq!(
        output.status.code(),
        Some(2),
        "args={args:?} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "BAD_ARGS");
    value
}

#[test]
fn bare_groups_print_bounded_text_help_even_when_piped() {
    for (args, child) in [
        (vec![], "status"),
        (vec!["app"], "open"),
        (vec!["flow"], "verify"),
        (vec!["flow", "code"], "write"),
        (vec!["tool", "studio"], "call"),
        (vec!["web"], "fill"),
        (vec!["browser"], "fill"),
        (vec!["code"], "read"),
        (vec!["visual"], "enable"),
    ] {
        let body = stdout(&invoke(&args));
        assert!(body.contains("Usage:"), "args={args:?}: {body}");
        assert!(body.contains(child), "args={args:?}: {body}");
        assert!(body.contains("--help"), "args={args:?}: {body}");
        assert!(body.lines().count() < 50, "args={args:?}: {body}");
        assert!(serde_json::from_str::<Value>(&body).is_err());
        assert!(!body.contains("input_shapes"));
    }
}

#[test]
fn explicit_help_forms_discover_the_same_command() {
    for args in [
        vec!["app", "--help"],
        vec!["app", "-h"],
        vec!["help", "app"],
    ] {
        let body = stdout(&invoke(&args));
        assert!(body.contains("Usage:"));
        for command in ["create", "open", "publish"] {
            assert!(body.contains(command), "args={args:?}: {body}");
        }
        assert!(serde_json::from_str::<Value>(&body).is_err());
    }
    for args in [
        vec!["flow", "code", "write", "--help"],
        vec!["help", "flow", "code", "write"],
    ] {
        let body = stdout(&invoke(&args));
        assert!(body.contains("--file"));
        assert!(body.contains("Usage:"));
    }
}

#[test]
fn version_is_one_text_line_or_explicit_json_without_backend_access() {
    for args in [
        vec!["--version"],
        vec!["-V"],
        vec!["version"],
        vec!["version", "--format", "auto"],
        vec!["version", "--format", "text"],
    ] {
        let body = stdout(&invoke(&args));
        assert_eq!(body.lines().count(), 1);
        assert_eq!(body.trim(), concat!("xbot-cli ", env!("CARGO_PKG_VERSION")));
        assert!(serde_json::from_str::<Value>(&body).is_err());
    }
    let value = json_success(&invoke(&["--json", "--version"]));
    assert_eq!(value["data"]["name"], "xbot-cli");
    assert_eq!(value["data"]["version"], env!("CARGO_PKG_VERSION"));
    for args in [
        vec!["version", "--json"],
        vec!["--json", "version"],
        vec!["version", "--format", "json"],
        vec!["version", "--format", "pretty"],
    ] {
        assert_eq!(json_success(&invoke(&args)), value);
    }
}

#[test]
fn update_subcommand_is_discoverable_and_defers_to_the_npm_launcher() {
    let root = json_success(&invoke(&["schema"]));
    assert!(root["data"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["name"] == "update"));
    let schema = json_success(&invoke(&["schema", "update"]));
    assert_eq!(schema["data"]["name"], "update");
    assert_eq!(schema["data"]["context"], "local");
    assert_eq!(
        schema["data"]["effects"],
        serde_json::json!(["self_update"])
    );
    let help = stdout(&invoke(&["update", "--help"]));
    assert!(help.contains("Usage: xbot-cli update"));
    assert!(help.contains("--check"));
    // 原生程序正在运行时无法替换自身;通过 npm 启动器执行才是真正的升级。
    for args in [
        vec!["update", "--json"],
        vec!["update", "--check", "--json"],
    ] {
        let output = invoke(&args);
        assert!(!output.status.success(), "args={args:?}");
        assert!(output.stdout.is_empty());
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "UPDATE_REQUIRES_LAUNCHER");
    }
}

#[test]
fn version_subcommand_is_discoverable_and_validates_arguments() {
    let root = json_success(&invoke(&["schema"]));
    assert!(root["data"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["name"] == "version"));
    let schema = json_success(&invoke(&["schema", "version"]));
    assert_eq!(schema["data"]["name"], "version");
    assert_eq!(schema["data"]["context"], "local");
    assert_eq!(schema["data"]["supports_dry_run"], false);
    let help = stdout(&invoke(&["version", "--help"]));
    assert!(help.contains("Usage: xbot-cli version"));
    for args in [
        vec!["version", "unexpected", "--json"],
        vec!["version", "--unknown", "--json"],
        vec!["version", "--format", "invalid", "--json"],
        vec!["version", "--dry-run", "--json"],
        vec!["version", "--expect-app", "anything", "--json"],
    ] {
        json_error(&args);
    }
}

#[test]
fn explicit_json_discovery_is_compact_and_distinct_from_schema() {
    let root = json_success(&invoke(&["--json"]));
    assert!(root["data"]["commands"].is_array());
    assert!(root["data"]["name"].as_str().is_some());
    for args in [
        vec!["--json", "app"],
        vec!["app", "--json"],
        vec!["app", "--format", "json"],
        vec!["--json", "app", "--help"],
        vec!["--json", "help", "app"],
    ] {
        let output = invoke(&args);
        let value = json_success(&output);
        assert_eq!(stdout(&output).lines().count(), 1);
        assert!(value["data"]["name"].as_str().unwrap().contains("app"));
        let commands = value["data"]["commands"].as_array().unwrap();
        assert!(commands.iter().any(|command| command["name"] == "open"));
        assert!(value["data"].get("input_shapes").is_none());
        assert!(value["data"].get("contract").is_none());
    }
}

#[test]
fn explicit_pretty_json_is_indented_and_keeps_the_help_shape() {
    let compact = json_success(&invoke(&["app", "--json"]));
    let output = invoke(&["app", "--format", "pretty"]);
    let body = stdout(&output);
    let pretty = json_success(&output);
    assert_eq!(pretty, compact);
    assert!(body.lines().count() > 1);
    assert!(body.lines().any(|line| line.starts_with("  \"")));
}

#[test]
fn schema_remains_machine_readable_without_an_output_flag() {
    for args in [
        vec!["schema"],
        vec!["schema", "app"],
        vec!["schema", "flow", "code", "write"],
    ] {
        let value = json_success(&invoke(&args));
        assert_eq!(value["data"]["schema_version"], 1);
        assert!(value["data"]["args"].is_array());
    }
}

#[test]
fn guide_is_readable_markdown_unless_json_is_requested() {
    let index = stdout(&invoke(&["guide"]));
    assert!(index.contains("cli-contract"));
    assert!(serde_json::from_str::<Value>(&index).is_err());
    let markdown = stdout(&invoke(&["guide", "cli-contract"]));
    assert!(markdown.trim_start().starts_with('#'));
    assert!(markdown.contains("xbot-cli"));
    let value = json_success(&invoke(&["guide", "cli-contract", "--json"]));
    assert_eq!(value["data"]["topic"], "cli-contract");
    assert_eq!(
        value["data"]["markdown"].as_str().unwrap().trim(),
        markdown.trim()
    );
}

#[test]
fn piped_business_output_is_json_and_explicit_text_is_readable() {
    let args = ["app", "create", "preview-app", "--dry-run"];
    let value = json_success(&invoke(&args));
    assert!(value["data"].to_string().contains("preview-app"));
    let mut explicit = args.to_vec();
    explicit.extend(["--format", "text"]);
    let body = stdout(&invoke(&explicit));
    assert!(body.contains("preview-app"));
    assert!(serde_json::from_str::<Value>(&body).is_err());
    explicit.truncate(args.len());
    explicit.extend(["--format", "auto"]);
    assert_eq!(json_success(&invoke(&explicit)), value);
}

#[test]
fn legacy_mcp_call_is_an_action_and_never_group_help() {
    let value = json_success(&invoke(&["mcp", "call", "app.get_info", "{}", "--dry-run"]));
    assert_eq!(value["data"]["dry_run"], true);
    assert!(value["data"].to_string().contains("app.get_info"));
    assert!(value["data"].get("commands").is_none());
}

#[test]
fn missing_leaf_arguments_retain_parameter_names_and_usage() {
    for (args, required) in [
        (vec!["--json", "app", "open"], "APP_ID"),
        (vec!["--json", "flow", "code", "write"], "--file"),
        (vec!["--json", "wiki", "read"], "DOC_IDS"),
    ] {
        let value = json_error(&args);
        let evidence = value.to_string();
        assert!(evidence.contains(required), "{evidence}");
        assert!(evidence.to_lowercase().contains("usage"), "{evidence}");
    }
}

#[test]
fn typo_and_unknown_option_errors_keep_actionable_details() {
    let value = json_error(&["--json", "app", "opne"]);
    let evidence = value.to_string();
    assert!(evidence.contains("opne"));
    assert!(evidence.contains("open"));
    let value = json_error(&["app", "--json", "--unknown-option"]);
    assert!(value.to_string().contains("--unknown-option"));
}

#[test]
fn missing_option_values_and_invalid_values_cannot_turn_into_help() {
    for args in [
        vec!["--format"],
        vec!["--expect-app"],
        vec!["app", "--format"],
        vec!["app", "--expect-app"],
        vec!["--json", "app", "--format", "bogus"],
        vec!["--json", "app", "--expect-app=123", "--unknown-option"],
        vec!["--json", "flow", "code", "write", "--file"],
        vec!["--json", "app", "open", "id", "--timeout", "bad"],
    ] {
        json_error(&args);
    }
}

#[test]
fn action_flags_without_an_action_remain_validation_errors() {
    for args in [
        vec!["--yes"],
        vec!["--dry-run"],
        vec!["--expect-app", "11111111-1111-1111-1111-111111111111"],
        vec!["app", "--yes"],
        vec!["flow", "code", "--dry-run"],
    ] {
        json_error(&args);
    }
    let body = stdout(&invoke(&["--yes", "--help"]));
    assert!(body.contains("Usage:"));
}

#[test]
fn contradictory_output_requests_are_rejected() {
    for args in [
        vec!["--json", "--format", "text"],
        vec!["app", "--format", "text", "--json"],
    ] {
        let output = invoke(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let body = String::from_utf8_lossy(&output.stderr);
        assert!(body.contains("--json"));
        assert!(body.contains("--format"));
    }
}

#[test]
fn explicit_text_errors_use_stderr_and_show_usage_only_once() {
    for (args, detail) in [
        (vec!["--format", "text", "app", "open"], "APP_ID"),
        (
            vec!["--format", "text", "app", "--unknown-option"],
            "--unknown-option",
        ),
    ] {
        let output = invoke(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let body = String::from_utf8_lossy(&output.stderr);
        assert!(body.contains(detail), "args={args:?}: {body}");
        assert_eq!(body.matches("Usage:").count(), 1, "args={args:?}: {body}");
        assert!(serde_json::from_str::<Value>(&body).is_err());
    }
}
