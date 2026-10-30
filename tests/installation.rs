use serde_json::Value;
use std::process::Command;

fn cli(backend: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xbot-cli"))
        .args(args)
        .env("XBOT_SHELL_CLI", backend)
        .env("XBOT_MCP_ENDPOINT", "http://127.0.0.1:1/unavailable")
        .output()
        .unwrap()
}

#[test]
fn missing_explicit_backend_is_configuration_failure_with_full_path_and_fix() {
    let path = r"X:\不存在 Program Files\影刀\shadowbot.shell-cli.exe";
    let output = cli(path, &["status", "--json"]);
    assert_eq!(output.status.code(), Some(3));
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["type"], "configuration");
    assert_eq!(value["error"]["code"], "SHELL_CLI_MISSING");
    assert_eq!(value["data"]["backend"]["path"], path);
    assert_eq!(value["data"]["backend"]["source"], "environment");
    assert_eq!(value["data"]["backend"]["explicit"], true);
    assert_eq!(
        value["data"]["backend"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(value["error"]["hint"]
        .as_str()
        .unwrap()
        .contains("XBOT_SHELL_CLI"));
}

#[test]
fn doctor_json_retains_each_channel_error_and_path_instead_of_generic_app_open_advice() {
    let path = r"X:\Custom Install\missing.exe";
    let output = cli(path, &["doctor", "--json"]);
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "DOCTOR_FAILED");
    assert_eq!(value["data"]["shell_cli"]["code"], "SHELL_CLI_MISSING");
    assert_eq!(value["data"]["shell_cli"]["backend"]["path"], path);
    assert_eq!(value["data"]["studio_mcp"]["code"], "MCP_OFFLINE");
    let output = cli(path, &["doctor", "--format", "text"]);
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(text.contains(path));
    assert!(text.contains("environment"));
    assert!(text.contains("XBOT_SHELL_CLI"));
    assert!(text.contains("XBOT_MCP_ENDPOINT"));
    assert!(!text.contains("app open"));
}

#[test]
fn existing_but_invalid_binary_reports_spawn_failure_and_evidence() {
    let root = std::env::temp_dir().join(format!("xbot-invalid-backend-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("invalid shadowbot.shell-cli.exe");
    std::fs::write(&path, b"not an executable").unwrap();
    let output = cli(path.to_str().unwrap(), &["status", "--json"]);
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "SPAWN_FAILED");
    assert_eq!(value["data"]["backend"]["exists"], true);
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains(path.to_str().unwrap()));
    assert!(value["error"]["hint"].as_str().unwrap().contains("权限"));
    std::fs::remove_dir_all(root).unwrap();
}
