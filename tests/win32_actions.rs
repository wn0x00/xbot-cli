//! Action grammar and offline previews never inspect or mutate business windows.
use serde_json::{json, Value};
use std::process::{Command, Output};

const SNAPSHOT: &str = "11111111-1111-4111-8111-111111111111";

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xbot-cli"))
        .args(args)
        .env("XBOT_UIA_PYTHON", "xbot-deliberately-missing-python")
        .env("XBOT_WIN32_STATE_DIR", "xbot-deliberately-missing-state")
        .output()
        .unwrap()
}

#[test]
fn action_discovery_declares_context_effects_and_timeout() {
    for action in [
        "inspect", "click", "dblclick", "fill", "type", "press", "check", "uncheck", "select",
        "expand", "collapse", "scroll", "focus", "hover",
    ] {
        assert!(invoke(&["help", "win32", action]).status.success());
        let output = invoke(&["schema", "win32", action]);
        assert!(output.status.success());
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
        let schema = &payload["data"];
        assert_eq!(schema["context"], "windows_uia");
        assert_eq!(schema["requires_studio"], false);
        assert_eq!(schema["supports_dry_run"], action != "inspect");
        assert_eq!(
            schema["effects"],
            if action == "inspect" {
                json!(["read"])
            } else {
                json!(["interact", "write_snapshot"])
            }
        );
        let timeout = schema["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|arg| arg["name"] == "timeout")
            .unwrap();
        assert_eq!(timeout["maximum"], 60);
    }
}

#[test]
fn action_preview_is_offline_and_never_echoes_typed_secrets() {
    for (action, extra) in [
        ("click", vec![]),
        ("click", vec!["--method", "physical"]),
        ("dblclick", vec![]),
        ("fill", vec!["secret {Enter} 中文"]),
        ("type", vec!["secret {Enter} 中文"]),
        ("press", vec!["Ctrl+A"]),
        ("check", vec![]),
        ("uncheck", vec![]),
        ("select", vec![]),
        ("expand", vec![]),
        ("collapse", vec![]),
        ("scroll", vec!["down", "--large"]),
        ("focus", vec![]),
        ("hover", vec![]),
    ] {
        let mut args = vec![
            "win32",
            action,
            "w4",
            "--snapshot",
            SNAPSHOT,
            "--dry-run",
            "--json",
        ];
        args.extend(extra);
        let output = invoke(&args);
        assert!(output.status.success(), "{:?}", output);
        assert!(output.stderr.is_empty());
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            payload["data"]["target"],
            json!({"snapshot_id":SNAPSHOT,"ref":"w4"})
        );
        assert_eq!(payload["data"]["validation"]["runtime"], "not_checked");
        assert_eq!(payload["data"]["confirmation_required"], false);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret"));
    }
}

#[test]
fn invalid_refs_snapshots_and_keyboard_scripts_fail_before_provider() {
    for extra in [
        vec!["click", "w0", "--snapshot", SNAPSHOT],
        vec!["click", "w5001", "--snapshot", SNAPSHOT],
        vec!["click", "w04", "--snapshot", SNAPSHOT],
        vec!["click", "e4", "--snapshot", SNAPSHOT],
        vec!["click", "w4", "--snapshot", "../file"],
        vec!["click", "w4"],
        vec!["click", "w4", "--snapshot", SNAPSHOT, "--method", "auto"],
        vec!["press", "w4", "{Ctrl}a{Enter}", "--snapshot", SNAPSHOT],
        vec!["fill", "w4", "--snapshot", SNAPSHOT],
        vec!["scroll", "w4", "diagonal", "--snapshot", SNAPSHOT],
        vec!["click", "w4", "--snapshot", SNAPSHOT, "--timeout", "61"],
        vec![
            "click",
            "w4",
            "--snapshot",
            SNAPSHOT,
            "--expect-app",
            SNAPSHOT,
        ],
        vec!["inspect", "w4", "--snapshot", SNAPSHOT],
    ] {
        let mut args = vec!["win32"];
        args.extend(extra);
        args.extend(["--dry-run", "--json"]);
        let output = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn missing_snapshot_fails_without_starting_python() {
    let output = invoke(&["win32", "click", "w4", "--snapshot", SNAPSHOT, "--json"]);
    assert!(!output.status.success());
    let payload: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(payload["error"]["code"], "WIN32_SNAPSHOT_MISSING");
}
