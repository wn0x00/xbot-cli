//! Black-box CLI checks against an isolated, local fake MCP server.
//! No installed client, application, browser, or account is used.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn invoke(args: &[&str], stdin: Option<&str>, endpoint: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xbot-cli"));
    command
        .args(args)
        .env("XBOT_SHELL_CLI", "__xbot_test_missing_shell__.exe")
        // Enterprise credentials must not select the transport for ordinary commands.
        .env("XBOT_PLATFORM_BASE_URL", "http://127.0.0.1:1")
        .env("XBOT_PLATFORM_ACCESS_TOKEN", "unused-enterprise-token")
        .env(
            "XBOT_MCP_ENDPOINT",
            endpoint.unwrap_or("http://127.0.0.1:1/unavailable"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(endpoint) = endpoint.filter(|endpoint| endpoint.starts_with(r"\\.\pipe\")) {
        command.env("XBOT_WEB_PIPE", endpoint);
    }
    let mut child = command.spawn().unwrap();
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

fn envelope(output: &Output, expected: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let (data, empty) = if expected == 0 {
        (&output.stdout, &output.stderr)
    } else {
        (&output.stderr, &output.stdout)
    };
    assert!(empty.is_empty(), "Unexpected output on the other stream");
    let value: Value = serde_json::from_slice(data).unwrap();
    assert_eq!(value["ok"], expected == 0);
    value
}

#[test]
fn missing_leaf_arguments_are_errors_but_command_groups_are_discoverable() {
    for args in [
        vec!["app", "open"],
        vec!["wiki", "read"],
        vec!["--expect-app", "invalid", "--dry-run"],
        vec!["--yes"],
    ] {
        let value = envelope(&invoke(&args, None, None), 2);
        assert_eq!(value["error"]["code"], "BAD_ARGS");
    }
    let server = MockMcp::new(|_| json!({}));
    for args in [
        vec![],
        vec!["app"],
        vec!["flow", "code"],
        vec!["tool", "studio"],
        vec!["--help"],
    ] {
        let help = invoke(&args, None, Some(&server.endpoint));
        assert!(help.status.success());
        assert!(help.stderr.is_empty());
        assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
        assert!(serde_json::from_slice::<Value>(&help.stdout).is_err());
    }
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn schemas_support_canonical_and_legacy_nested_paths() {
    for args in [
        vec!["schema", "tool", "studio", "call"],
        vec!["schema", "mcp", "call"],
        vec!["schema", "flow", "visual", "disable"],
        vec!["schema", "code", "write"],
        vec!["schema", "web", "click"],
        vec!["schema", "browser", "fill"],
    ] {
        envelope(&invoke(&args, None, None), 0);
    }
}

#[test]
fn web_aliases_share_help_schema_and_offline_previews() {
    for group in ["web", "browser"] {
        let schema = envelope(&invoke(&["schema", group, "click"], None, None), 0);
        assert_eq!(schema["data"]["name"], "web click");
        assert_eq!(schema["data"]["supports_dry_run"], true);
        assert_eq!(schema["data"]["effects"], json!(["interact"]));
        let preview = envelope(
            &invoke(
                &[
                    group,
                    "fill",
                    "e4",
                    "literal & | %PATH% $()",
                    "--dry-run",
                    "--json",
                ],
                None,
                None,
            ),
            0,
        );
        assert_eq!(preview["data"]["dry_run"], true);
        assert!(preview["data"]
            .to_string()
            .contains("literal & | %PATH% $()"));
    }
    for (alias, expected) in [
        ("attach", "active"),
        ("back", "go-back"),
        ("forward", "go-forward"),
    ] {
        let schema = envelope(&invoke(&["schema", "web", alias], None, None), 0);
        assert_eq!(schema["data"]["name"], format!("web {expected}"));
    }
}

#[test]
fn web_short_commands_send_literal_bridge_arguments_once() {
    for (cli, target, method, args) in [
        (
            vec![
                "fill",
                "e4",
                "中文 & $()",
                "--name",
                "姓名",
                "--no-snapshot",
            ],
            "locator",
            "fill",
            json!(["中文 & $()"]),
        ),
        (
            vec!["select", "e9", "red", "blue", "--no-snapshot"],
            "locator",
            "selectOption",
            json!([["red", "blue"]]),
        ),
        (
            vec!["press", "Control+A", "--ref", "e4", "--no-snapshot"],
            "locator",
            "press",
            json!(["Control+A"]),
        ),
        (
            vec!["press", "Enter", "--no-snapshot"],
            "keyboard",
            "press",
            json!(["Enter"]),
        ),
        (
            vec!["type", "hello", "--no-snapshot"],
            "keyboard",
            "type",
            json!(["hello"]),
        ),
        (
            vec!["goto", "example.com/path", "--no-snapshot"],
            "page",
            "goto",
            json!(["https://example.com/path"]),
        ),
        (
            vec!["go-back", "--no-snapshot"],
            "page",
            "goBack",
            json!([]),
        ),
        (
            vec!["go-forward", "--no-snapshot"],
            "page",
            "goForward",
            json!([]),
        ),
        (vec!["reload", "--no-snapshot"], "page", "reload", json!([])),
        (vec!["close"], "page", "close", json!([])),
    ] {
        let server = MockBridge::new(|_, _| None);
        let mut command = vec!["web"];
        command.extend(cli);
        envelope(&invoke(&command, None, Some(&server.endpoint)), 0);
        let requests = server.requests.lock().unwrap();
        let evaluations = requests
            .iter()
            .filter(|request| {
                request["method"] == "debugger.sendCommand"
                    && request["params"]["method"] == "Runtime.evaluate"
            })
            .map(|request| {
                request["params"]["commandParam"]["expression"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        match (target, method) {
            ("page", "goto") => {
                assert!(requests.iter().any(|request| {
                    request["method"] == "debugger.sendCommand"
                        && request["params"]["method"] == "Page.navigate"
                        && request["params"]["commandParam"]["url"] == args[0]
                }));
            }
            ("page", "goBack") => {
                assert!(evaluations.iter().any(|code| code.contains("history.back")))
            }
            ("page", "goForward") => assert!(evaluations
                .iter()
                .any(|code| code.contains("history.forward"))),
            ("page", "reload" | "close") => {
                let expected = if method == "reload" {
                    "Page.reload"
                } else {
                    "Page.close"
                };
                assert!(requests.iter().any(|request| {
                    request["method"] == "debugger.sendCommand"
                        && request["params"]["method"] == expected
                }));
            }
            _ => {
                let literal = args[0].to_string();
                let literal = literal.trim_matches(['"', '[', ']']);
                assert!(evaluations.iter().any(|code| code.contains(literal)));
            }
        }
    }
}

#[test]
fn web_open_takes_a_snapshot_and_preserves_backend_fields_and_refs() {
    let server = MockBridge::new(|_, _| None);
    let value = envelope(
        &invoke(
            &["web", "open", "example.com"],
            None,
            Some(&server.endpoint),
        ),
        0,
    );
    let calls = server.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|method| *method == "chrome.create")
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|method| *method == "chrome.query")
            .count(),
        0
    );
    assert_eq!(value["data"]["success"], true);
    assert_eq!(value["data"]["backend"], "chrome-bridge");
    assert_eq!(value["data"]["url"], "https://example.com/");
    assert_eq!(
        value["data"]["snapshot"]["acc_tree_meta"]["kind"], "full",
        "{value}"
    );
    assert!(value["data"]["snapshot"]["result"]
        .as_str()
        .unwrap()
        .contains("[ref=e4]"));
}

#[test]
fn web_command_surfaces_bridge_errors_without_retrying() {
    let server = MockBridge::new(|method, _| {
        (method == "debugger.sendCommand").then(|| json!({"__error":"stale ref"}))
    });
    let value = envelope(
        &invoke(
            &["web", "click", "e5", "--json"],
            None,
            Some(&server.endpoint),
        ),
        1,
    );
    assert_eq!(value["error"]["code"], "WEB_BRIDGE_FAILED");
    assert_eq!(value["error"]["message"], "stale ref");
    assert_eq!(value["error"]["retryable"], false);
    assert!(!server
        .calls()
        .iter()
        .any(|method| method == "Input.dispatchMouseEvent"));
}

#[test]
fn web_open_reports_missing_bridge() {
    let value = envelope(
        &invoke(
            &["web", "open", "example.com", "--json"],
            None,
            Some(r"\\.\pipe\xbot-cli-definitely-missing"),
        ),
        1,
    );
    assert_eq!(value["error"]["code"], "WEB_BRIDGE_UNAVAILABLE");
}

#[test]
fn web_snapshot_failure_does_not_turn_a_successful_click_into_a_retry() {
    let server = MockBridge::new(|method, params| {
        if method == "debugger.sendCommand"
            && params["method"] == "Runtime.evaluate"
            && params["commandParam"]["expression"]
                .as_str()
                .unwrap_or("")
                .contains("ariaSnapshot(document.body")
        {
            Some(json!({"__error":"snapshot unavailable"}))
        } else {
            None
        }
    });
    let value = envelope(
        &invoke(
            &["web", "click", "e5", "--button", "right"],
            None,
            Some(&server.endpoint),
        ),
        0,
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|request| {
                request["method"] == "debugger.sendCommand"
                    && request["params"]["method"] == "Input.dispatchMouseEvent"
                    && request["params"]["commandParam"]["type"] == "mousePressed"
            })
            .count(),
        1
    );
    assert_eq!(value["data"]["applied"], true);
    assert!(value["data"].get("warning").is_some());
    assert!(value["data"].to_string().contains("snapshot unavailable"));
}

#[test]
fn python_capture_failure_never_replays_an_action_or_changes_its_success() {
    let server = MockBridge::new(|method, params| {
        (method == "debugger.sendCommand"
            && params["method"] == "Runtime.evaluate"
            && params["commandParam"]["expression"]
                .as_str()
                .unwrap_or("")
                .contains("return window.__xbotPythonLocator"))
        .then(|| json!({"__error":"locator capture unavailable"}))
    });
    let value = envelope(
        &invoke(
            &["web", "click", "e4", "--no-snapshot"],
            None,
            Some(&server.endpoint),
        ),
        0,
    );
    assert_eq!(value["data"]["applied"], true);
    assert_eq!(value["data"]["python"]["status"], "unavailable");
    let requests = server.requests.lock().unwrap();
    let capture = requests
        .iter()
        .position(|r| {
            r["params"]["commandParam"]["expression"]
                .as_str()
                .unwrap_or("")
                .contains("return window.__xbotPythonLocator")
        })
        .unwrap();
    let clicks: Vec<_> = requests
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r["params"]["method"] == "Input.dispatchMouseEvent"
                && r["params"]["commandParam"]["type"] == "mousePressed"
        })
        .collect();
    assert_eq!(clicks.len(), 1);
    assert!(capture < clicks[0].0);
}

#[test]
fn no_python_skips_locator_capture_without_disabling_the_action() {
    let server = MockBridge::new(|_, _| None);
    let value = envelope(
        &invoke(
            &["web", "click", "e4", "--no-snapshot", "--no-python"],
            None,
            Some(&server.endpoint),
        ),
        0,
    );
    assert!(value["data"].get("python").is_none());
    let requests = server.requests.lock().unwrap();
    assert!(!requests
        .iter()
        .any(|r| r["params"]["commandParam"]["expression"]
            .as_str()
            .unwrap_or("")
            .contains("return window.__xbotPythonLocator")));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["params"]["method"] == "Input.dispatchMouseEvent"
                && r["params"]["commandParam"]["type"] == "mousePressed")
            .count(),
        1
    );
}

#[test]
fn web_failed_action_and_legacy_raw_action_are_never_repeated() {
    let server = MockBridge::new(|method, _| {
        (method == "debugger.sendCommand").then(|| json!({"__error":"stale ref"}))
    });
    let value = envelope(
        &invoke(
            &["web", "fill", "e4", "hello"],
            None,
            Some(&server.endpoint),
        ),
        1,
    );
    assert_eq!(value["error"]["retryable"], false);
    assert_eq!(
        server
            .calls()
            .iter()
            .filter(|method| *method == "Runtime.evaluate")
            .count(),
        0
    );
    assert_eq!(
        server
            .calls()
            .iter()
            .filter(|method| *method == "debugger.sendCommand")
            .count(),
        2
    );

    let server = MockBridge::new(|method, _| {
        (method == "debugger.sendCommand").then(|| json!({"result":{"value":"legacy"}}))
    });
    let value = envelope(
        &invoke(
            &[
                "browser",
                "act",
                "{\"target\":\"keyboard\",\"method\":\"press\",\"args\":[\"Tab\"]}",
            ],
            None,
            Some(&server.endpoint),
        ),
        0,
    );
    assert_eq!(
        server
            .calls()
            .iter()
            .filter(|method| *method == "debugger.sendCommand")
            .count(),
        2
    );
    assert_eq!(value["data"]["result"], "legacy");
}

#[test]
fn web_fill_reports_the_actual_bridge_error() {
    let reason = "Error: locator.fill: Element is not an <input> or <textarea>";
    let detail =
        format!("{reason}\nCall log:\n  - waiting for locator('button')\n    at internal.js:1");
    for format in ["json", "text"] {
        let detail = detail.clone();
        let server = MockBridge::new(move |method, _| {
            (method == "debugger.sendCommand").then(|| json!({"__error":detail}))
        });
        let result = invoke(
            &["web", "fill", "e74", "111", "--format", format],
            None,
            Some(&server.endpoint),
        );
        assert_eq!(
            server
                .calls()
                .iter()
                .filter(|method| *method == "debugger.sendCommand")
                .count(),
            2
        );
        if format == "json" {
            let value = envelope(&result, 1);
            assert_eq!(value["error"]["code"], "WEB_BRIDGE_FAILED");
            assert!(value["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with(reason));
            assert!(value["error"]["message"]
                .as_str()
                .unwrap()
                .contains("internal.js"));
            assert_eq!(value["error"]["retryable"], false);
            assert!(value["error"]["hint"]
                .as_str()
                .unwrap()
                .contains("snapshot"));
        } else {
            assert_eq!(result.status.code(), Some(1));
            assert!(result.stdout.is_empty());
            let text = String::from_utf8_lossy(&result.stderr);
            assert!(text.contains(reason));
            assert!(text.contains("snapshot"));
            assert!(!text.contains("selectors have been stored"));
            assert!(!text.contains("Call log:"));
            assert!(!text.contains("internal.js"));
        }
    }
}

#[test]
fn preview_rejects_bad_inputs_without_backend_and_can_read_stdin() {
    for args in [
        vec![
            "task",
            "run",
            "11111111-1111-1111-1111-111111111111",
            "--input",
            "broken",
            "--dry-run",
        ],
        vec![
            "flow",
            "params",
            "set",
            "main",
            "--operations",
            "null",
            "--dry-run",
        ],
        vec!["tool", "studio", "call", "x", "[]", "--dry-run"],
        vec!["flow", "run", "main", "--timeout", "0", "--dry-run"],
        vec![
            "flow",
            "wait",
            "existing-run",
            "--timeout",
            "18446744073709551615",
        ],
        vec![
            "flow",
            "run",
            "main",
            "--timeout",
            "18446744073709551615",
            "--dry-run",
        ],
    ] {
        envelope(&invoke(&args, None, None), 2);
    }
    let value = envelope(
        &invoke(
            &["flow", "code", "write", "main", "--file", "-", "--dry-run"],
            Some("print('一次读取')\n"),
            None,
        ),
        0,
    );
    assert_eq!(value["data"]["arguments"]["content"], "print('一次读取')\n");
    assert_eq!(value["data"]["validation"]["runtime"], "not_checked");
}

struct MockMcp {
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MockMcp {
    fn new(handler: impl Fn(&Value) -> Value + Send + 'static) -> Self {
        Self::serve(move |request| {
            let result = handler(request);
            (
                200,
                json!({"jsonrpc":"2.0", "id":request["id"], "result":result}),
            )
        })
    }

    fn serve(handler: impl Fn(&Value) -> (u16, Value) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !signal.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("mock accept: {e}"),
                };
                // Winsock accepted sockets inherit the listener's nonblocking mode.
                // Keep accept polling cancellable, but wait for each request's bytes.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0; 4096];
                let request = loop {
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0, "incomplete HTTP request");
                    buf.extend_from_slice(&chunk[..count]);
                    if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&buf[..at]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        if buf.len() >= at + 4 + length {
                            break serde_json::from_slice::<Value>(&buf[at + 4..at + 4 + length])
                                .unwrap();
                        }
                    }
                };
                log.lock().unwrap().push(request.clone());
                let (status, body) = handler(&request);
                let body = body.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            worker: Some(worker),
        }
    }
    fn calls(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter_map(|r| r["params"]["name"].as_str().map(str::to_owned))
            .collect()
    }
}

impl Drop for MockMcp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            if let Err(failure) = worker.join() {
                // A response assertion may already be unwinding after a worker error.
                // Preserve that failure instead of aborting the entire test process.
                if !thread::panicking() {
                    std::panic::resume_unwind(failure);
                }
            }
        }
    }
}

struct MockBridge {
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MockBridge {
    fn new(handler: impl Fn(&str, &Value) -> Option<Value> + Send + 'static) -> Self {
        let endpoint = format!(
            r"\\.\pipe\xbot-cli-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&stop);
        let server_endpoint = endpoint.clone();
        let worker = thread::spawn(move || {
            use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
            use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
            use windows::Win32::System::Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                PIPE_TYPE_BYTE, PIPE_WAIT,
            };
            while !signal.load(Ordering::SeqCst) {
                let wide = server_endpoint
                    .encode_utf16()
                    .chain(Some(0))
                    .collect::<Vec<_>>();
                let handle = unsafe {
                    CreateNamedPipeW(
                        windows::core::PCWSTR(wide.as_ptr()),
                        PIPE_ACCESS_DUPLEX,
                        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                        1,
                        1024 * 1024,
                        1024 * 1024,
                        0,
                        None,
                    )
                };
                assert_ne!(handle, INVALID_HANDLE_VALUE);
                let connected = unsafe { ConnectNamedPipe(handle, None) };
                if signal.load(Ordering::SeqCst) {
                    let _ = unsafe { CloseHandle(handle) };
                    break;
                }
                let _ = connected;
                let mut file = unsafe {
                    <std::fs::File as std::os::windows::io::FromRawHandle>::from_raw_handle(
                        handle.0,
                    )
                };
                while let Ok(request) = read_bridge_frame(&mut file) {
                    let method = request["method"].as_str().unwrap().to_owned();
                    let params = request["params"].clone();
                    log.lock()
                        .unwrap()
                        .push(json!({"method":method,"params":params}));
                    let response = handler(&method, &params)
                        .unwrap_or_else(|| default_bridge_response(&method, &params));
                    let response = if let Some(error) = response.get("__error") {
                        json!({"code":200,"status":"ok","result":{"content":null,"error":{"code":-1,"message":error}}})
                    } else {
                        json!({"code":200,"status":"ok","result":{"content":response,"error":null}})
                    };
                    if write_bridge_frame(&mut file, &response).is_err() {
                        break;
                    }
                }
                drop(file);
                let _ = unsafe { DisconnectNamedPipe(handle) };
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request["method"].as_str().unwrap().to_owned())
            .collect()
    }
}

impl Drop for MockBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.endpoint);
        if let Some(worker) = self.worker.take() {
            if let Err(failure) = worker.join() {
                if !thread::panicking() {
                    std::panic::resume_unwind(failure);
                }
            }
        }
    }
}

fn read_bridge_frame(file: &mut std::fs::File) -> std::io::Result<Value> {
    let mut length = [0_u8; 4];
    file.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    let mut payload = vec![0_u8; length];
    file.read_exact(&mut payload)?;
    Ok(serde_json::from_slice(&payload).unwrap())
}

fn write_bridge_frame(file: &mut std::fs::File, value: &Value) -> std::io::Result<()> {
    let payload = serde_json::to_vec(value).unwrap();
    file.write_all(&(payload.len() as u32).to_le_bytes())?;
    file.write_all(&payload)?;
    file.flush()
}

fn default_bridge_response(method: &str, params: &Value) -> Value {
    match method {
        "KeepAlive" => json!("KeepAlive"),
        "Extension.GetVersion" => json!("test-extension"),
        "chrome.query" => json!([1]),
        "chrome.create" => json!(1),
        "browser.getInfo" => {
            json!({"url":"https://example.com/","title":"Example Domain","active":true})
        }
        "debugger.sendCommand" if params["method"] == "Runtime.evaluate" => {
            let expression = params["commandParam"]["expression"].as_str().unwrap_or("");
            let value = if expression.contains("JSON.stringify({ua:") {
                json!("{\"ua\":\"Edg/130.0\",\"brands\":[]}")
            } else if expression.contains("$$.rect(el)") {
                json!({"x":100,"y":100,"w":20,"h":20})
            } else if expression.contains("return window.__xbotPythonLocator") {
                json!({"steps":[{"kind":"element","xpath":"//*[@id='test']","css":"#test"}],"origin":"https://example.com"})
            } else if expression.contains("__xbotPreviewFrames") {
                json!([])
            } else if expression == "location.origin" {
                json!("https://example.com")
            } else if expression.contains("ariaSnapshot(document.body") {
                json!("### AccTree\n- textbox Name [ref=e4]\n")
            } else if expression.contains("document.readyState") {
                json!("complete")
            } else if expression.contains("document.title") {
                json!("Example Domain")
            } else if expression.contains("location.href") {
                json!("https://example.com/")
            } else if expression.contains("window.__pwIS") {
                json!("cached")
            } else {
                json!(true)
            };
            json!({"result":{"value":value}})
        }
        _ => json!({}),
    }
}

#[test]
fn mock_mcp_accepts_delayed_request_headers_and_body() {
    let server = MockMcp::new(
        |request| json!({"structuredContent":{"received":request["params"]["arguments"]}}),
    );
    let address = server
        .endpoint
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/mcp")
        .unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = json!({
        "jsonrpc":"2.0", "id":1, "method":"tools/call",
        "params":{"name":"delayed.request","arguments":{"value":"once"}}
    })
    .to_string();

    // A TCP connection can arrive before either the HTTP headers or body.
    // Nonblocking accepted sockets on Windows must not treat this as failure.
    thread::sleep(Duration::from_millis(100));
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    thread::sleep(Duration::from_millis(100));
    stream.write_all(body.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let payload = response.split_once("\r\n\r\n").unwrap().1;
    let result: Value = serde_json::from_str(payload).unwrap();
    assert_eq!(
        result["result"]["structuredContent"]["received"]["value"],
        "once"
    );
    assert_eq!(server.calls(), ["delayed.request"]);
}

#[test]
fn mock_cleanup_preserves_primary_panic_and_propagates_worker_failure() {
    let panicking_mock = || MockMcp {
        endpoint: String::new(),
        requests: Arc::new(Mutex::new(Vec::new())),
        stop: Arc::new(AtomicBool::new(false)),
        worker: Some(thread::spawn(|| panic!("mock worker failure"))),
    };
    let failure = std::panic::catch_unwind(|| drop(panicking_mock())).unwrap_err();
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"mock worker failure"));

    let failure = std::panic::catch_unwind(|| {
        let _mock = panicking_mock();
        panic!("primary test failure");
    })
    .unwrap_err();
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"primary test failure")
    );
}

fn studio_fixture(request: &Value) -> Value {
    let content = match request["params"]["name"].as_str().unwrap_or("") {
        "app.get_info" => {
            json!({"app":{"app_id":"11111111-1111-1111-1111-111111111111"},"flows":{"active_flow_id":"main"}})
        }
        "app.list_flows" => {
            json!({"flows":[{"flow_id":"main","flow_name":"主流程","flow_kind":"code"}]})
        }
        "diagnostics.snapshot" => json!({"static_errors":[]}),
        _ => json!({}),
    };
    json!({"structuredContent":content})
}

#[test]
fn raw_stdin_is_executed_once_and_mcp_errors_cannot_report_success() {
    let server = MockMcp::new(
        |request| json!({"structuredContent":{"received":request["params"]["arguments"]}}),
    );
    let value = envelope(
        &invoke(
            &["tool", "studio", "call", "example", "-"],
            Some("{\"value\":42}"),
            Some(&server.endpoint),
        ),
        0,
    );
    assert_eq!(value["data"]["received"]["value"], 42);
    assert_eq!(server.calls(), ["example"]);
    let server = MockMcp::new(
        |_| json!({"isError":true,"content":[{"type":"text","text":"backend failure"}]}),
    );
    let result = invoke(
        &["tool", "studio", "call", "example"],
        None,
        Some(&server.endpoint),
    );
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let value: Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(value["ok"], false);
}

#[test]
fn application_assertion_blocks_writes_to_another_application() {
    let server = MockMcp::new(studio_fixture);
    let value = envelope(
        &invoke(
            &[
                "flow",
                "code",
                "write",
                "main",
                "--file",
                "-",
                "--expect-app",
                "22222222-2222-2222-2222-222222222222",
            ],
            Some("print('safe')"),
            Some(&server.endpoint),
        ),
        5,
    );
    assert_eq!(value["ok"], false);
    assert_eq!(server.calls(), ["app.get_info"]);
}

#[test]
fn verify_does_not_succeed_without_a_run_handle() {
    let server = MockMcp::new(studio_fixture);
    let result = invoke(
        &["flow", "verify", "main", "--timeout", "1"],
        None,
        Some(&server.endpoint),
    );
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let value: Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(value["ok"], false);
    assert_ne!(value["data"]["verified"], true);
    assert_eq!(
        server
            .calls()
            .iter()
            .filter(|name| name.as_str() == "app.run")
            .count(),
        1
    );
}

#[test]
fn wait_uses_existing_run_without_starting_a_second_run() {
    let server = MockMcp::new(|request| {
        assert_eq!(request["params"]["name"], "app.wait_run_finish");
        assert_eq!(request["params"]["arguments"]["run_id"], "existing-run");
        json!({"structuredContent":{"run_id":"existing-run", "terminal_status":"completed","run_succeeded":true,"result":{"outputs":{"answer":42}}}})
    });
    let value = envelope(
        &invoke(
            &["flow", "wait", "existing-run", "--timeout", "5"],
            None,
            Some(&server.endpoint),
        ),
        0,
    );
    assert_eq!(value["data"]["run_id"], "existing-run");
    assert_eq!(server.calls(), ["app.wait_run_finish"]);
}

#[test]
fn write_keeps_applied_evidence_when_followup_diagnostics_fail() {
    let server = MockMcp::new(|request| match request["params"]["name"].as_str() {
        Some("codeflow.write") => {
            assert_eq!(request["params"]["arguments"]["content"], "print('once')\n");
            json!({"structuredContent":{"success":true,"revision":"written-once"}})
        }
        Some("diagnostics.snapshot") => {
            json!({"isError":true,"content":[{"type":"text","text":"diagnostics unavailable"}]})
        }
        _ => studio_fixture(request),
    });
    let value = envelope(
        &invoke(
            &["flow", "code", "write", "main", "--file", "-"],
            Some("print('once')\n"),
            Some(&server.endpoint),
        ),
        1,
    );
    assert_eq!(value["error"]["code"], "MUTATION_DIAGNOSTICS_FAILED");
    assert_eq!(value["error"]["retryable"], false);
    assert_eq!(value["data"]["applied"], true);
    assert_eq!(value["data"]["result"]["revision"], "written-once");
    assert_eq!(
        server
            .calls()
            .iter()
            .filter(|name| name.as_str() == "codeflow.write")
            .count(),
        1
    );
}

#[test]
fn static_check_never_runs_and_verify_stops_on_static_errors() {
    let server = MockMcp::new(|request| {
        if request["params"]["name"] == "diagnostics.snapshot" {
            json!({"structuredContent":{"static_errors":[{"message":"invalid code"}]}})
        } else {
            studio_fixture(request)
        }
    });
    for command in ["check", "verify"] {
        let value = envelope(
            &invoke(&["flow", command, "main"], None, Some(&server.endpoint)),
            1,
        );
        assert_eq!(value["data"]["executed"], false);
    }
    assert!(!server.calls().iter().any(|name| name == "app.run"));
}

#[test]
fn completion_without_success_evidence_is_not_success() {
    let server = MockMcp::new(
        |_| json!({"structuredContent":{"run_id":"existing-run","terminal_status":"completed","run_succeeded":null}}),
    );
    let value = envelope(
        &invoke(
            &["flow", "wait", "existing-run"],
            None,
            Some(&server.endpoint),
        ),
        6,
    );
    assert_eq!(value["error"]["code"], "FLOW_RUN_UNKNOWN");
    assert_eq!(value["error"]["retryable"], false);
}
