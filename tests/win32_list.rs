//! Read-only discovery contracts, including an owned native window fixture.
use serde_json::{json, Value};
use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xbot-cli"))
        .args(args)
        .env("XBOT_SHELL_CLI", "xbot-deliberately-missing-backend")
        .env("XBOT_UIA_PYTHON", "xbot-deliberately-missing-python")
        .output()
        .unwrap()
}

#[test]
fn discovery_is_offline_and_declares_read_only_non_studio_context() {
    let output = invoke(&["help", "win32", "list"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output = invoke(&["schema", "win32", "list"]);
    assert!(output.status.success());
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["data"]["effects"], json!(["read"]));
    assert_eq!(data["data"]["requires_studio"], false);
    assert_eq!(data["data"]["supports_dry_run"], false);
}

#[test]
fn invalid_queries_and_inapplicable_globals_fail_before_enumerating() {
    for options in [
        vec!["--title", ""],
        vec!["--title", "  "],
        vec!["--pid", "0"],
        vec!["--exact"],
        vec!["--dry-run"],
        vec!["--expect-app", "11111111-1111-1111-1111-111111111111"],
    ] {
        let mut args = vec!["win32", "list", "--json"];
        args.extend(options);
        let output = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let payload: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(payload["ok"], false);
    }
}

#[cfg(windows)]
#[test]
fn native_windows_are_searchable_without_python_and_duplicates_are_retained() {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, ShowWindow, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE,
        WS_OVERLAPPEDWINDOW,
    };
    struct Fixture(HWND);
    impl Fixture {
        fn create(title: &str, visible: bool) -> Self {
            let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            let hwnd = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    PCWSTR(title.as_ptr()),
                    WS_OVERLAPPEDWINDOW,
                    20,
                    20,
                    240,
                    80,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
            };
            if visible {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
            }
            Self(hwnd)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    let pid = std::process::id().to_string();
    let title = format!("xbot-list-{pid}-测试[A]*");
    let first = Fixture::create(&title, true);
    let second = Fixture::create(&title, true);
    let _suffix = Fixture::create(&(title.clone() + " more"), true);
    let _hidden = Fixture::create(&title, false);
    let query = |options: &[&str]| -> Value {
        let mut args = vec!["win32", "list", "--title", &title, "--pid", &pid, "--json"];
        args.extend_from_slice(options);
        let output = invoke(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        result["data"].clone()
    };
    let partial = query(&[]);
    assert_eq!(partial["count"], 3);
    let exact = query(&["--exact"]);
    assert_eq!(exact["count"], 2);
    let expected = [first.0 .0 as usize as u64, second.0 .0 as usize as u64];
    for row in exact["windows"].as_array().unwrap() {
        let hwnd = row["hwnd"].as_u64().unwrap();
        assert!(expected.contains(&hwnd));
        assert_eq!(row["hwnd_hex"], format!("0x{hwnd:x}"));
        assert_eq!(row["pid"], std::process::id());
        assert_eq!(row["title"], title);
        assert_eq!(row["visible"], true);
        assert_eq!(row["class_name"], "Static");
        assert!(row["process_name"]
            .as_str()
            .unwrap()
            .starts_with("win32_list-"));
    }
    let all = query(&["--all"]);
    assert_eq!(all["count"], 4);
    assert_eq!(
        all["windows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["visible"] == false)
            .count(),
        1
    );
    let uppercase = title.to_uppercase();
    let output = invoke(&[
        "win32", "list", "--title", &uppercase, "--pid", &pid, "--exact", "--json",
    ]);
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["data"]["count"], 2);
    let text = invoke(&[
        "win32", "list", "--title", &title, "--pid", &pid, "--format", "text",
    ]);
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("PID | HWND | HWND_HEX"));
    assert!(text.contains(&title));
    assert!(text.contains(&expected[0].to_string()));
    let missing = title + "-not-present";
    let output = invoke(&["win32", "list", "--title", &missing, "--json"]);
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["data"]["count"], 0);
    assert_eq!(data["data"]["windows"], json!([]));
}
