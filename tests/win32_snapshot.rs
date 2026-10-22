//! Offline command contracts: no desktop access or backend required.
use serde_json::Value;
use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xbot-cli"))
        .args(args)
        .env("XBOT_UIA_PYTHON", "xbot-deliberately-missing-python")
        .env("XBOT_SHELL_CLI", "xbot-deliberately-missing-backend")
        .output()
        .unwrap()
}

#[test]
fn discovery_is_offline_and_describes_readonly_context() {
    for args in [vec!["win32"], vec!["help", "win32", "snapshot"]] {
        let output = invoke(&args);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.is_empty());
    }
    let output = invoke(&["schema", "win32", "snapshot"]);
    assert!(output.status.success());
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["data"]["effects"], serde_json::json!(["read"]));
    assert_eq!(payload["data"]["context"], "windows_uia");
    assert_eq!(payload["data"]["requires_studio"], false);
    assert_eq!(payload["data"]["supports_dry_run"], false);
}

#[test]
fn invalid_scope_and_budgets_fail_before_starting_provider() {
    for args in [
        vec!["win32", "snapshot", "--hwnd", "xyz", "--pid", "1"],
        vec!["win32", "snapshot", "--hwnd", "0", "--pid", "1"],
        vec!["win32", "snapshot", "--hwnd", "1", "--pid", "0"],
        vec!["win32", "snapshot", "--hwnd", "1"],
        vec![
            "win32",
            "snapshot",
            "--hwnd",
            "1",
            "--pid",
            "1",
            "--max-nodes",
            "5001",
        ],
        vec![
            "win32",
            "snapshot",
            "--hwnd",
            "1",
            "--pid",
            "1",
            "--max-depth",
            "65",
        ],
        vec![
            "win32",
            "snapshot",
            "--hwnd",
            "1",
            "--pid",
            "1",
            "--timeout",
            "61",
        ],
    ] {
        let mut args = args;
        args.push("--json");
        let output = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let payload: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(payload["ok"], false);
    }
}

#[test]
fn missing_runtime_returns_structured_error() {
    let output = invoke(&["win32", "snapshot", "--hwnd", "1", "--pid", "1", "--json"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let payload: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(
        payload["error"]["code"],
        if cfg!(windows) {
            "WIN32_DEPENDENCY_MISSING"
        } else {
            "WIN32_UNSUPPORTED_PLATFORM"
        },
    );
}

#[test]
fn native_snapshot_rejects_studio_assertion_and_dry_run() {
    for option in [
        vec!["--expect-app", "11111111-1111-1111-1111-111111111111"],
        vec!["--dry-run"],
    ] {
        let mut args = vec!["win32", "snapshot", "--hwnd", "1", "--pid", "1", "--json"];
        args.extend(option);
        let output = invoke(&args);
        assert_eq!(output.status.code(), Some(2));
        let payload: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(payload["error"]["code"], "BAD_ARGS");
    }
}
