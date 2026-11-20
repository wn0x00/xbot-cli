//! Activation discovery and preview must not access the desktop or backend.
use serde_json::{json, Value};
use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xbot-cli"))
        .args(args)
        .env("XBOT_SHELL_CLI", "xbot-deliberately-missing-backend")
        .env("SystemRoot", "xbot-deliberately-missing-system")
        .output()
        .unwrap()
}

#[test]
fn activation_discovery_reports_effects_and_actual_timeout_limit() {
    let help = invoke(&["help", "win32", "activate"]);
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let output = invoke(&["schema", "win32", "activate"]);
    assert!(output.status.success());
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    let schema = &payload["data"];
    assert_eq!(schema["effects"], json!(["modify_target_process"]));
    assert_eq!(schema["requires_studio"], false);
    assert_eq!(schema["supports_dry_run"], true);
    assert_eq!(schema["confirmation"], "none");
    let timeout = schema["args"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["name"] == "timeout")
        .unwrap();
    assert_eq!(timeout["maximum"], 60);
    assert_eq!(timeout["defaults"], json!(["30"]));
    let provider = schema["args"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["name"] == "provider")
        .unwrap();
    assert_eq!(provider["defaults"], json!(["auto"]));
}

#[test]
fn activation_preview_is_offline_and_explicitly_scoped() {
    let output = invoke(&[
        "win32",
        "activate",
        "--hwnd",
        "0X7b",
        "--pid",
        "456",
        "--dry-run",
        "--json",
    ]);
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty());
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["data"]["action"], "win32.activate");
    assert_eq!(payload["data"]["target"], json!({"hwnd":"0x7b","pid":456}));
    assert_eq!(payload["data"]["arguments"]["restore"], false);
    let arguments = &payload["data"]["arguments"];
    assert!(arguments["discovery"].as_str().unwrap().contains("x86|x64"));
    assert!(arguments["worker"].as_str().unwrap().contains("SysWOW64"));
    assert_eq!(payload["data"]["validation"]["runtime"], "not_checked");
    assert_eq!(payload["data"]["confirmation_required"], false);
}

#[test]
fn activation_rejects_invalid_scope_and_studio_assertions_offline() {
    for options in [
        vec!["--hwnd", "0", "--pid", "1"],
        vec!["--hwnd", "xyz", "--pid", "1"],
        vec!["--hwnd", "1", "--pid", "0"],
        vec!["--hwnd", "1"],
        vec!["--pid", "1"],
        vec!["--hwnd", "1", "--pid", "1", "--timeout", "0"],
        vec!["--hwnd", "1", "--pid", "1", "--timeout", "61"],
        vec!["--hwnd", "1", "--pid", "1", "--provider", "arbitrary-dll"],
        vec![
            "--hwnd",
            "1",
            "--pid",
            "1",
            "--expect-app",
            "11111111-1111-1111-1111-111111111111",
        ],
    ] {
        let mut args = vec!["win32", "activate", "--dry-run", "--json"];
        args.extend(options);
        let output = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let payload: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(payload["ok"], false);
    }
}

#[test]
fn each_provider_has_an_offline_preview() {
    for provider in ["auto", "wework", "shadowbot"] {
        let output = invoke(&[
            "win32",
            "activate",
            "--pid",
            "1",
            "--hwnd",
            "1",
            "--provider",
            provider,
            "--dry-run",
            "--json",
        ]);
        assert!(output.status.success());
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(payload["data"]["arguments"]["provider"], provider);
    }
}
