//! Both protocol generations are exercised at the process boundary; no installed client is used.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

static PARAMETER_TEST_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug)]
struct Request {
    path: String,
    headers: String,
    body: Value,
}
struct Server {
    endpoint: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new(handler: impl Fn(&Request) -> (u16, Value, &'static str) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/api/v1/mcp", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let worker = thread::spawn(move || {
            while !signal.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 4096];
                let request = loop {
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).into_owned();
                        let size: usize = headers
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse().unwrap())
                            })
                            .unwrap();
                        if bytes.len() >= end + 4 + size {
                            break Request {
                                path: headers
                                    .lines()
                                    .next()
                                    .unwrap()
                                    .split_whitespace()
                                    .nth(1)
                                    .unwrap()
                                    .into(),
                                headers,
                                body: serde_json::from_slice(&bytes[end + 4..end + 4 + size])
                                    .unwrap(),
                            };
                        }
                    }
                };
                log.lock().unwrap().push(request.clone());
                let (status, value, extra) = handler(&request);
                let body = if status == 202 {
                    String::new()
                } else {
                    value.to_string()
                };
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",body.len()).unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn modern(handler: impl Fn(&Value) -> Value + Send + 'static) -> Self {
        Self::new(move |r| {
            if r.path == "/api/v1/mcp" {
                return (404, json!({}), "");
            }
            assert_eq!(r.path, "/api/v1/copilot-mcp/mcp");
            let result = match r.body["method"].as_str().unwrap() {
                "initialize" => {
                    return (
                        200,
                        json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{}}}),
                        "Mcp-Session-Id: test-session\r\n",
                    )
                }
                "notifications/initialized" => {
                    assert!(!r.body.as_object().unwrap().contains_key("id"));
                    assert!(r
                        .headers
                        .to_lowercase()
                        .contains("mcp-session-id: test-session"));
                    return (202, Value::Null, "");
                }
                "tools/list" => json!({"tools":[{"name":"xbot-bash"},{"name":"execute_cell"}]}),
                "tools/call" => {
                    assert!(r
                        .headers
                        .to_lowercase()
                        .contains("mcp-session-id: test-session"));
                    assert!(r
                        .headers
                        .to_lowercase()
                        .contains("mcp-protocol-version: 2025-03-26"));
                    if r.body["params"]["name"] == "xbot-bash" {
                        assert!(r.body.pointer("/params/arguments/explanation").is_some());
                    }
                    handler(&r.body["params"]["arguments"])
                }
                other => panic!("unexpected method {other}"),
            };
            let envelope = match result.get("__rpc_error") {
                Some(error) => json!({"jsonrpc":"2.0","id":1,"error":error}),
                None => json!({"jsonrpc":"2.0","id":1.0,"result":result}),
            };
            (200, envelope, "")
        })
    }

    fn log(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(w) = self.worker.take() {
            let result = w.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn invoke(server: &Server, args: &[&str], stdin: Option<&str>) -> Output {
    invoke_mode(server, args, stdin, None)
}
fn invoke_mode(server: &Server, args: &[&str], stdin: Option<&str>, mode: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xbot-cli"));
    command
        .args(args)
        .arg("--json")
        .env("XBOT_MCP_ENDPOINT", &server.endpoint)
        .env_remove("XBOT_MCP_MODE")
        .env("XBOT_SHELL_CLI", "__missing_shell__.exe")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(mode) = mode {
        command.env("XBOT_MCP_MODE", mode);
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
fn payload(output: &Output) -> Value {
    let bytes = if output.status.success() {
        &output.stdout
    } else {
        &output.stderr
    };
    serde_json::from_slice(bytes)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(bytes)))
}
fn tool(value: Value) -> Value {
    json!({"structuredContent":value})
}
fn app() -> Value {
    tool(
        json!({"app":{"app_id":"11111111-1111-1111-1111-111111111111"},"metadata":{"name":"兼容测试"}}),
    )
}
fn flows() -> Value {
    tool(json!({"flows":[{"flow_id":"main","flow_name":"主流程","flow_kind":"visual"}]}))
}

#[test]
fn python_packages_use_legacy_objects_for_list_install_and_uninstall() {
    for (args, expected) in [
        (
            vec!["python", "package", "list", "--verbosity", "0"],
            json!({"operation":"list","verbosity":0}),
        ),
        (
            vec![
                "python",
                "package",
                "install",
                "openpyxl==3.1.5",
                "requests",
                "--index-url",
                "https://example.test/simple",
            ],
            json!({"operation":"install","verbosity":1,"index_url":"https://example.test/simple",
                "packages":[{"package_name":"openpyxl","version":"3.1.5"},{"package_name":"requests"}]}),
        ),
        (
            vec!["python", "package", "uninstall", "openpyxl", "requests"],
            json!({"operation":"uninstall","verbosity":1,
                "packages":[{"package_name":"openpyxl"},{"package_name":"requests"}]}),
        ),
    ] {
        let server = Server::new(move |r| {
            let result = match r.body["method"].as_str().unwrap() {
                "tools/list" => json!({"tools":[{"name":"python_pip"},{"name":"app.get_info"}]}),
                "tools/call" => match r.body["params"]["name"].as_str().unwrap() {
                    "app.get_info" => app(),
                    "python_pip" => {
                        assert_eq!(r.body["params"]["arguments"], expected);
                        tool(
                            json!({"success":true,"packages":[{"name":"example","version":"1.0"}],"log_path":"pip.log"}),
                        )
                    }
                    other => panic!("unexpected tool {other}"),
                },
                other => panic!("unexpected method {other}"),
            };
            (200, json!({"jsonrpc":"2.0","id":1,"result":result}), "")
        });
        let output = invoke(&server, &args, None);
        assert!(output.status.success(), "{}", payload(&output));
        assert_eq!(payload(&output)["data"]["result"]["log_path"], "pip.log");
        assert_eq!(
            server
                .log()
                .iter()
                .filter(|r| r.body["params"]["name"] == "python_pip")
                .count(),
            1
        );
    }
}

#[test]
fn python_packages_map_to_native_commands_and_preserve_results() {
    for (args, expected) in [
        (vec!["python", "package", "list", "--verbosity", "4"],
            "xbot-bash python package list --verbosity \"4\""),
        (vec!["python", "package", "install", "openpyxl==3.1.5", "requests", "--index-url", "https://example.test/simple"],
            "xbot-bash python package install --packages \"openpyxl==3.1.5\" \"requests\" --verbosity \"1\" --index-url \"https://example.test/simple\""),
        (vec!["python", "package", "uninstall", "openpyxl", "requests"],
            "xbot-bash python package uninstall --packages \"openpyxl\" \"requests\" --verbosity \"1\""),
    ] {
        let server = Server::modern(move |a| {
            if a["command"] == "xbot-bash app info" { return app(); }
            assert_eq!(a["command"], expected);
            tool(json!({"success":true,"log_path":"pip.log","packages":[{"name":"example","version":"1.0"}]}))
        });
        let output = invoke(&server, &args, None);
        assert!(output.status.success(), "{}", payload(&output));
        assert_eq!(payload(&output)["data"]["app_id"], "11111111-1111-1111-1111-111111111111");
        assert_eq!(payload(&output)["data"]["result"]["packages"][0]["version"], "1.0");
        assert_eq!(server.log().iter().filter(|r| r.body["params"]["arguments"]["command"] == expected).count(), 1);
    }
}

#[test]
fn python_package_previews_and_invalid_specs_never_contact_client() {
    let server = Server::new(|_| panic!("offline validation must not contact client"));
    for args in [
        vec![
            "python",
            "package",
            "install",
            "requests==1.2.3",
            "--dry-run",
            "--expect-app",
            "11111111-1111-1111-1111-111111111111",
        ],
        vec!["python", "package", "uninstall", "requests", "--dry-run"],
    ] {
        let output = invoke(&server, &args, None);
        assert!(output.status.success(), "{}", payload(&output));
        assert_eq!(payload(&output)["data"]["dry_run"], true);
        assert_eq!(
            payload(&output)["data"]["validation"]["runtime"],
            "not_checked"
        );
    }
    for args in [
        vec!["python", "package", "install"],
        vec!["python", "package", "install", "requests>=1"],
        vec!["python", "package", "install", "requests=="],
        vec!["python", "package", "install", "requests;print(1)"],
        vec!["python", "package", "install", "requests", "Requests==1.2"],
        vec!["python", "package", "install", "pkg-name", "pkg__name"],
        vec!["python", "package", "uninstall", "requests==1.2"],
        vec!["python", "package", "install", "C:/tmp/pkg.whl"],
        vec![
            "python",
            "package",
            "install",
            "requests",
            "--index-url",
            "file:///tmp/index",
        ],
        vec![
            "python",
            "package",
            "uninstall",
            "requests",
            "--index-url",
            "https://example.test",
        ],
        vec!["python", "package", "list", "--verbosity", "5"],
        vec!["python", "package", "list", "--timeout", "0"],
        vec!["python", "package", "list", "--dry-run"],
    ] {
        let output = invoke(&server, &args, None);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            payload(&output)
        );
    }
    assert!(server.log().is_empty());
}

#[test]
fn python_package_failure_keeps_partial_logs_and_never_replays() {
    let server = Server::modern(|a| {
        if a["command"] == "xbot-bash app info" {
            return app();
        }
        assert_eq!(
            a["command"],
            "xbot-bash python package install --packages \"first\" \"second\" --verbosity \"1\""
        );
        json!({"isError":true,"structuredContent":{"success":false,"message":"pip failed",
            "partial":true,"installed":["first"],"log_path":"partial.log"}})
    });
    let output = invoke(
        &server,
        &["python", "package", "install", "first", "second"],
        None,
    );
    assert!(!output.status.success());
    let result = payload(&output);
    assert_eq!(result["data"]["result"]["installed"], json!(["first"]));
    assert_eq!(result["data"]["result"]["log_path"], "partial.log");
    assert_eq!(result["data"]["outcome"], "unknown");
    assert_eq!(result["error"]["retryable"], false);
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.body["params"]["arguments"]["command"]
                .as_str()
                .is_some_and(|s| s.starts_with("xbot-bash python package install")))
            .count(),
        1
    );
}

#[test]
fn python_package_scope_and_missing_legacy_capability_block_mutations() {
    let server = Server::modern(|a| {
        assert_eq!(a["command"], "xbot-bash app info");
        app()
    });
    let output = invoke(
        &server,
        &[
            "python",
            "package",
            "uninstall",
            "requests",
            "--expect-app",
            "22222222-2222-2222-2222-222222222222",
        ],
        None,
    );
    assert_eq!(payload(&output)["error"]["code"], "APP_MISMATCH");
    let unavailable = Server::new(|r| {
        assert_eq!(r.body["method"], "tools/list");
        (
            200,
            json!({"jsonrpc":"2.0","id":1,"result":{"tools":[]}}),
            "",
        )
    });
    let output = invoke(
        &unavailable,
        &["python", "package", "install", "requests"],
        None,
    );
    assert_eq!(
        payload(&output)["error"]["code"],
        "MCP_CAPABILITY_UNAVAILABLE"
    );
    assert_eq!(payload(&output)["data"]["applied"], false);
    assert!(unavailable
        .log()
        .iter()
        .all(|r| r.body["method"] == "tools/list"));
}

#[test]
fn legacy_discovery_keeps_original_tool_names_and_does_not_initialize() {
    let server = Server::new(|r| {
        assert_eq!(r.path, "/api/v1/mcp");
        let result = if r.body["method"] == "tools/list" {
            json!({"tools":[{"name":"app.get_info"}]})
        } else {
            assert_eq!(r.body["params"]["name"], "app.get_info");
            app()
        };
        (200, json!({"jsonrpc":"2.0","id":1,"result":result}), "")
    });
    assert!(invoke(&server, &["app", "current"], None).status.success());
    assert_eq!(server.log().len(), 2);
}

#[test]
fn discovery_404_negotiates_copilot_and_reports_actual_tools() {
    let server = Server::modern(|_| panic!("discovery must not execute tools"));
    let output = invoke(&server, &["tool", "studio", "list"], None);
    assert!(output.status.success(), "{}", payload(&output));
    assert_eq!(
        payload(&output)["data"]["tools"],
        json!(["xbot-bash", "execute_cell"])
    );
    assert_eq!(server.log().len(), 4);
}

#[test]
fn client_info_reports_discovered_copilot_without_opening_or_calling_a_workspace() {
    let server = Server::modern(|_| panic!("client info must not execute tools"));
    let output = invoke(&server, &["client", "info"], None);
    assert!(output.status.success(), "{}", payload(&output));
    let data = &payload(&output)["data"];
    assert_eq!(data["command"], "client.info");
    assert_eq!(data["cli"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(data["shell_cli"]["ok"], false);
    assert_eq!(data["shell_cli"]["backend"]["source"], "environment");
    assert_eq!(data["studio_mcp"]["connection"]["provider"], "copilot");
    assert_eq!(data["studio_mcp"]["ok"], true);
    assert_eq!(data["partial"], true);
    assert_eq!(server.log().len(), 4);
}

#[test]
fn client_info_legacy_mode_verifies_reachability_and_keeps_channel_failure_separate() {
    let server = Server::new(|r| {
        assert_eq!(r.body["method"], "tools/list");
        (503, json!({"error":"offline"}), "")
    });
    let output = invoke_mode(&server, &["client", "info"], None, Some("legacy"));
    assert!(output.status.success(), "{}", payload(&output));
    let data = &payload(&output)["data"];
    assert_eq!(data["studio_mcp"]["ok"], false);
    assert_eq!(data["partial"], true);
    assert_eq!(data["mcp_mode"], "legacy");
    assert!(data["client"]["processes"].is_array());
    assert!(data["studio_mcp"].get("connection").is_none());
    assert_eq!(server.log().len(), 1);

    let online = Server::new(|r| {
        assert_eq!(r.body["method"], "tools/list");
        (
            200,
            json!({"jsonrpc":"2.0","id":1,"result":{"tools":[]}}),
            "",
        )
    });
    let output = invoke_mode(&online, &["client", "info"], None, Some("legacy"));
    assert_eq!(
        payload(&output)["data"]["studio_mcp"]["connection"]["provider"],
        "legacy"
    );
    assert_eq!(online.log().len(), 1);
}

#[test]
fn source_write_preserves_stdin_compiled_feedback_and_reuses_one_session() {
    let source = "xbot_visual.programing.log(text=\"a\\b'c\")\r\n";
    let server = Server::modern(move |a| {
        let command = a["command"].as_str().unwrap();
        if command == "xbot-bash flow list" {
            flows()
        } else if command == "xbot-bash app info" {
            app()
        } else {
            assert_eq!(command, "xbot-bash flow write --flow \"main\" --stdin");
            assert_eq!(a["stdin"], source);
            tool(
                json!({"success":true,"content":"compiled","ready_to_run":false,"static_check":{"errors":[{"message":"fix me"}]}}),
            )
        }
    });
    let output = invoke(
        &server,
        &["flow", "source", "write", "main", "--file", "-"],
        Some(source),
    );
    let value = payload(&output);
    assert!(output.status.success(), "{value}");
    assert_eq!(value["data"]["result"]["content"], "compiled");
    assert_eq!(value["data"]["result"]["ready_to_run"], false);
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.body["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.body["params"]["arguments"]["command"]
                .as_str()
                .is_some_and(|s| s.contains("flow write")))
            .count(),
        1
    );
}

#[test]
fn parameter_batch_uses_native_mcp_without_shell_or_metadata_reload() {
    let _guard = PARAMETER_TEST_LOCK.lock().unwrap();
    let server = Server::modern(|a| {
        match a["command"].as_str().unwrap() {
        "xbot-bash app info" => app(),
        "xbot-bash flow list" => flows(),
        "xbot-bash flow parameter list --flow \"main\"" => tool(json!({"items":[]})),
        "xbot-bash flow parameter set --flow \"main\" --name \"result\" --direction \"Out\" --type \"list\" --value-kind \"Expression\" --name \"count\" --direction \"In\" --type \"int\" --value \"0\"" => {
            assert!(a.get("stdin").is_none(), "native parameter groups reject stdin");
            tool(json!({"success":true,"content":"saved","parameters":[{"name":"result"},{"name":"count"}]}))
        }
        other => panic!("unexpected {other}"),
    }
    });
    let output = invoke(
        &server,
        &["flow", "params", "set", "main", "--operations", "-"],
        Some(
            r#"[{"op":"upsert","name":"result","direction":"Out","type":"list","kind":"Expression"},{"op":"upsert","name":"count","direction":"In","type":"int","value":"0"}]"#,
        ),
    );
    assert!(output.status.success(), "{}", payload(&output));
    assert_eq!(payload(&output)["data"]["backend"], "copilot-mcp");
}

#[test]
fn legacy_block_edit_has_explicit_capability_error_on_modern_client() {
    let server = Server::modern(|a| {
        if a["command"] == "xbot-bash flow list" {
            flows()
        } else {
            app()
        }
    });
    let output = invoke(
        &server,
        &["flow", "visual", "fill", "main", "--fills", "-"],
        Some(r#"[{"block_id":"7","inputs":{"text":"value"}}]"#),
    );
    assert_eq!(
        payload(&output)["error"]["code"],
        "MCP_CAPABILITY_UNAVAILABLE"
    );
    assert_eq!(payload(&output)["data"]["applied"], false);
    assert!(!server
        .log()
        .iter()
        .any(|r| r.body["params"]["arguments"]["command"]
            .as_str()
            .is_some_and(|s| s.contains("flow write") || s.contains("flow edit"))));
}

#[test]
fn failed_mutation_is_never_replayed_or_retried_on_other_protocol() {
    let server = Server::modern(|a| {
        if a["command"] == "xbot-bash flow list" {
            flows()
        } else if a["command"] == "xbot-bash app info" {
            app()
        } else {
            json!({"isError":true,"structuredContent":{"success":false,"code":"SAVE_FAILED","message":"may have saved"}})
        }
    });
    let output = invoke(
        &server,
        &["flow", "source", "write", "main", "--file", "-"],
        Some("pass"),
    );
    assert_eq!(payload(&output)["error"]["code"], "SAVE_FAILED");
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.body["params"]["arguments"]["command"]
                .as_str()
                .is_some_and(|s| s.contains("flow write")))
            .count(),
        1
    );
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.path == "/api/v1/mcp")
            .count(),
        1
    );
}

#[test]
fn service_failure_does_not_trigger_endpoint_fallback() {
    let server = Server::new(|_| (503, json!({"error":{"message":"unavailable"}}), ""));
    let output = invoke(&server, &["app", "current"], None);
    assert_eq!(payload(&output)["error"]["code"], "MCP_HTTP");
    assert_eq!(server.log().len(), 1);
}

#[test]
fn legacy_tool_errors_are_not_reclassified_as_copilot_session_expiration() {
    let server = Server::new(|r| {
        let value = if r.body["method"] == "tools/list" {
            json!({"jsonrpc":"2.0","id":1,"result":{"tools":[]}})
        } else {
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"legacy error"}})
        };
        (200, value, "")
    });
    let output = invoke(&server, &["app", "current"], None);
    assert_eq!(payload(&output)["error"]["code"], "-32001");
    assert_eq!(server.log().len(), 2);
}

#[test]
fn unmapped_legacy_arguments_fail_before_mutation_instead_of_being_dropped() {
    let server = Server::modern(|_| panic!("unsupported mapping must not execute tools"));
    let output = invoke(
        &server,
        &["tool", "studio", "call", "app.resource.add", "-"],
        Some(r#"{"file":"a.txt","unexpected":"ignored?"}"#),
    );
    assert_eq!(
        payload(&output)["error"]["code"],
        "MCP_CAPABILITY_UNAVAILABLE"
    );
    assert_eq!(payload(&output)["data"]["applied"], false);
}

#[test]
fn dry_run_source_edit_is_offline_and_validates_shared_stdin() {
    let server = Server::new(|_| panic!("dry-run contacted backend"));
    let output = invoke(
        &server,
        &[
            "flow",
            "source",
            "edit",
            "main",
            "--old",
            "a",
            "--new",
            "-",
            "--dry-run",
        ],
        Some("b\n"),
    );
    assert!(output.status.success(), "{}", payload(&output));
    assert_eq!(payload(&output)["data"]["arguments"]["new_string"], "b\n");
    assert!(server.log().is_empty());
}

#[test]
fn expired_session_does_not_initialize_again_or_replay_a_write() {
    let server = Server::modern(|a| match a["command"].as_str().unwrap() {
        "xbot-bash app info" => app(),
        "xbot-bash flow list" => flows(),
        _ => json!({"__rpc_error":{"code":-32001,"message":"Unknown session"}}),
    });
    let output = invoke(
        &server,
        &["flow", "source", "write", "main", "--file", "-"],
        Some("pass"),
    );
    assert_eq!(payload(&output)["error"]["code"], "MCP_SESSION_EXPIRED");
    assert_eq!(payload(&output)["data"]["outcome"], "unknown");
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.body["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.body["params"]["arguments"]["command"]
                .as_str()
                .is_some_and(|c| c.contains("flow write")))
            .count(),
        1
    );
}

#[test]
fn initialization_requires_supported_protocol_and_session_before_business_calls() {
    for (protocol, header, expected) in [
        ("2025-03-26", "", "MCP_BAD_RESPONSE"),
        (
            "1900-01-01",
            "Mcp-Session-Id: test-session\r\n",
            "MCP_PROTOCOL_UNSUPPORTED",
        ),
    ] {
        let server = Server::new(move |r| {
            if r.path == "/api/v1/mcp" {
                return (404, json!({}), "");
            }
            assert_eq!(r.body["method"], "initialize");
            (
                200,
                json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":protocol}}),
                header,
            )
        });
        let output = invoke(&server, &["app", "current"], None);
        assert_eq!(payload(&output)["error"]["code"], expected);
        assert_eq!(server.log().len(), 2);
    }
}

#[test]
fn explicit_modes_and_custom_endpoints_are_respected() {
    let legacy = Server::new(|r| {
        assert_eq!(r.body["method"], "tools/call");
        (200, json!({"jsonrpc":"2.0","id":1,"result":app()}), "")
    });
    assert!(
        invoke_mode(&legacy, &["app", "current"], None, Some("legacy"))
            .status
            .success()
    );
    let modern = Server::modern(|_| panic!("list must not call tools"));
    assert!(
        invoke_mode(&modern, &["tool", "studio", "list"], None, Some("copilot"))
            .status
            .success()
    );
    assert!(modern
        .log()
        .iter()
        .all(|r| r.path == "/api/v1/copilot-mcp/mcp"));
    let mut custom = Server::new(|r| {
        assert_eq!(r.path, "/custom");
        (404, json!({}), "")
    });
    custom.endpoint = custom.endpoint.replace("/api/v1/mcp", "/custom");
    assert!(!invoke(&custom, &["app", "current"], None).status.success());
    assert_eq!(custom.log().len(), 1);
}

#[test]
fn run_start_returns_immediately_and_wait_keeps_the_original_run_and_typed_outputs() {
    let server = Server::modern(|a| match a["command"].as_str().unwrap() {
        "xbot-bash app info" => app(),
        "xbot-bash flow list" => flows(),
        "xbot-bash run start --flow \"main\" --timeout 0" => {
            tool(json!({"run_id":"run-1","terminal_status":"running"}))
        }
        command if command.starts_with("xbot-bash run wait --run-id \"run-1\" --timeout ") => tool(
            json!({"run_id":"run-1","terminal_status":"completed","result":{"outputs":{"items":[1,true,{"name":"甲"}]}}}),
        ),
        other => panic!("unexpected command {other}"),
    });
    let output = invoke(&server, &["flow", "run", "main", "--timeout", "10"], None);
    assert!(output.status.success(), "{}", payload(&output));
    assert_eq!(payload(&output)["data"]["run_id"], "run-1");
    assert_eq!(
        payload(&output)["data"]["result"]["outputs"]["items"],
        json!([1,true,{"name":"甲"}])
    );
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.body["params"]["arguments"]["command"]
                .as_str()
                .is_some_and(|c| c.starts_with("xbot-bash run start")))
            .count(),
        1
    );
}

#[test]
fn copilot_runtime_failure_preserves_traceback_and_is_not_a_wait_transport_error() {
    let server = Server::modern(|a| {
        assert!(a["command"]
            .as_str()
            .unwrap()
            .starts_with("xbot-bash run wait --run-id \"run-1\" --timeout "));
        json!({"isError":true,"structuredContent":{
            "run_id":"run-1","terminal_status":"failed",
            "latest_runtime_error":{"message":"EXPECTED_FAILURE","flow":"code","line_no":7,
                "python_failure_context":{"raw_traceback":"Traceback: EXPECTED_FAILURE"}},
            "logs":"runtime failed"
        }})
    });
    let output = invoke(&server, &["flow", "wait", "run-1", "--timeout", "5"], None);
    let value = payload(&output);
    assert_eq!(value["error"]["code"], "FLOW_RUN_FAILED");
    assert_eq!(value["data"]["run_id"], "run-1");
    assert_eq!(value["data"]["run"]["run_succeeded"], false);
    assert_eq!(value["data"]["run"]["latest_runtime_error"]["line_no"], 7);
    assert_eq!(
        value["data"]["run"]["latest_runtime_error"]["python_failure_context"]["raw_traceback"],
        "Traceback: EXPECTED_FAILURE"
    );
    assert_eq!(value["data"]["run"]["mcp_error"]["code"], "MCP_TOOL_FAILED");
}

#[test]
fn copilot_wait_timeout_keeps_running_state_without_starting_another_run() {
    let server = Server::modern(|a| {
        assert!(a["command"]
            .as_str()
            .unwrap()
            .starts_with("xbot-bash run wait --run-id \"run-1\" --timeout "));
        tool(json!({"run_id":"run-1","terminal_status":"running","code":"wait_timeout"}))
    });
    let output = invoke(&server, &["flow", "wait", "run-1", "--timeout", "1"], None);
    let value = payload(&output);
    assert_eq!(value["error"]["code"], "FLOW_RUN_TIMEOUT");
    assert_eq!(value["data"]["run"]["active"], true);
    assert!(value["data"]["resume_command"]
        .as_str()
        .unwrap()
        .contains("flow wait run-1"));
}

#[test]
fn copilot_canceled_and_paused_runs_require_intervention() {
    for (status, is_error, expected) in [
        ("canceled", false, "FLOW_RUN_STOPPED"),
        ("user_stopped", true, "FLOW_RUN_STOPPED"),
        ("user_paused", true, "FLOW_RUN_PAUSED"),
    ] {
        let server = Server::modern(
            move |_| json!({"isError":is_error,"structuredContent":{"run_id":"run-1","terminal_status":status}}),
        );
        let output = invoke(&server, &["flow", "wait", "run-1", "--timeout", "5"], None);
        assert_eq!(payload(&output)["error"]["code"], expected);
    }
}

#[test]
fn copilot_unknown_or_conflicting_completion_does_not_verify_success() {
    for state in [
        json!({"run_id":"run-1","terminal_status":"future_terminal"}),
        json!({"run_id":"run-1","terminal_status":"completed","run_succeeded":null}),
        json!({"run_id":"run-1","terminal_status":"completed","verification_error":"not retained"}),
        json!({"run_id":"run-1","terminal_status":"completed","latest_runtime_error":{"message":"contradiction"}}),
    ] {
        let server = Server::modern(move |_| tool(state.clone()));
        let output = invoke(&server, &["flow", "wait", "run-1", "--timeout", "5"], None);
        assert_eq!(payload(&output)["error"]["code"], "FLOW_RUN_UNKNOWN");
    }
}

#[test]
fn replaced_copilot_run_is_not_reported_as_a_retryable_wait() {
    let server = Server::modern(|_| {
        json!({"isError":true,"structuredContent":{
            "run_id":"old-run","code":"run_id_mismatch",
            "message":"run_id does not match the retained Studio run."
        }})
    });
    let output = invoke(
        &server,
        &["flow", "wait", "old-run", "--timeout", "5"],
        None,
    );
    let value = payload(&output);
    assert_eq!(value["error"]["code"], "FLOW_RUN_MISMATCH");
    assert_eq!(value["error"]["retryable"], false);
    assert!(value["data"].get("resume_command").is_none());
    assert_eq!(value["data"]["run_id"], "old-run");
}

#[test]
fn legacy_completed_still_requires_canonical_success_and_raw_copilot_keeps_tool_error() {
    let legacy = Server::new(|r| {
        let result = if r.body["method"] == "tools/list" {
            json!({"tools":[{"name":"app.wait_run_finish"}]})
        } else {
            assert_eq!(r.body["params"]["name"], "app.wait_run_finish");
            tool(json!({"run_id":"run-1","terminal_status":"completed"}))
        };
        (200, json!({"jsonrpc":"2.0","id":1,"result":result}), "")
    });
    let output = invoke(&legacy, &["flow", "wait", "run-1", "--timeout", "5"], None);
    assert_eq!(payload(&output)["error"]["code"], "FLOW_RUN_UNKNOWN");

    let raw = Server::modern(
        |_| json!({"isError":true,"structuredContent":{"run_id":"run-1","terminal_status":"failed"}}),
    );
    let output = invoke(
        &raw,
        &["tool", "studio", "call", "xbot-bash", "-"],
        Some(
            r#"{"command":"xbot-bash run result --run-id run-1","explanation":"read retained failure"}"#,
        ),
    );
    assert_eq!(payload(&output)["error"]["code"], "MCP_TOOL_FAILED");
}

#[test]
fn native_parameter_updates_keep_omitted_fields_and_report_completed_prefix_on_failure() {
    let _guard = PARAMETER_TEST_LOCK.lock().unwrap();
    let server = Server::modern(|a| {
        match a["command"].as_str().unwrap() {
        "xbot-bash app info" => app(),
        "xbot-bash flow list" => flows(),
        "xbot-bash flow parameter list --flow \"main\"" => tool(
            json!({"parameters":[{"name":"keyword","direction":"In","type":"str","kind":"Text","value":"keep me","description":"original"}]}),
        ),
        "xbot-bash flow parameter set --flow \"main\" --name \"keyword\" --direction \"In\" --type \"str\" --value-kind \"Text\" --value \"keep me\" --description \"original\"" => {
            assert!(a.get("stdin").is_none());
            tool(json!({"success":true,"parameters":[{"name":"keyword","value":"keep me"}]}))
        }
        "xbot-bash flow parameter remove --flow \"main\" --name \"obsolete\"" => {
            tool(json!({"success":false,"code":"REMOVE_FAILED"}))
        }
        other => panic!("unexpected {other}"),
    }
    });
    let output = invoke(
        &server,
        &["flow", "params", "set", "main", "--operations", "-"],
        Some(
            r#"[{"op":"upsert","name":"keyword","direction":"In","type":"str"},{"op":"remove","name":"obsolete"}]"#,
        ),
    );
    let value = payload(&output);
    assert_eq!(value["error"]["code"], "PARAMETER_UPDATE_FAILED");
    assert_eq!(value["data"]["completed_count"], 1);
    assert_eq!(value["data"]["results"].as_array().unwrap().len(), 1);
    assert_eq!(value["data"]["cause_code"], "REMOVE_FAILED");
}

#[test]
fn legacy_source_commands_keep_codeflow_transport_and_reject_visual_source() {
    let server = Server::new(|r| {
        let result = match r.body["method"].as_str().unwrap() {
            "tools/list" => json!({"tools":[{"name":"codeflow.read"}]}),
            _ => match r.body["params"]["name"].as_str().unwrap() {
                "app.list_flows" => tool(
                    json!({"flows":[{"flow_id":"code","flow_name":"code.py","flow_kind":"code"},{"flow_id":"main","flow_name":"main.flow","flow_kind":"visual"}]}),
                ),
                "codeflow.read" => {
                    assert_eq!(
                        r.body["params"]["arguments"],
                        json!({"flow_id":"code","offset":2,"limit":3})
                    );
                    tool(json!({"content":"print('hello')"}))
                }
                other => panic!("unexpected {other}"),
            },
        };
        (200, json!({"jsonrpc":"2.0","id":1,"result":result}), "")
    });
    assert!(invoke(
        &server,
        &["flow", "source", "read", "code", "--offset", "2", "--limit", "3"],
        None
    )
    .status
    .success());
    assert!(!invoke(&server, &["flow", "source", "read", "main"], None)
        .status
        .success());
    assert_eq!(
        server
            .log()
            .iter()
            .filter(|r| r.body["params"]["name"] == "codeflow.read")
            .count(),
        1
    );
}

#[test]
fn multiline_edit_preserves_literal_newlines_quotes_and_backslashes() {
    let server = Server::modern(|a| match a["command"].as_str().unwrap() {
        "xbot-bash app info" => app(),
        "xbot-bash flow list" => flows(),
        command => {
            assert_eq!(command,"xbot-bash flow edit --flow \"main\" --old-string \"a\n\\\"b\\\\c\" --new-string \"x\ny\" --replace-all");
            tool(json!({"static_check":{"errors":[]},"diff":[{"before":"a","after":"x"}]}))
        }
    });
    let output = invoke(
        &server,
        &[
            "flow",
            "source",
            "edit",
            "main",
            "--old",
            "a\n\"b\\c",
            "--new",
            "x\ny",
            "--replace-all",
        ],
        None,
    );
    assert!(output.status.success(), "{}", payload(&output));
    assert_eq!(payload(&output)["data"]["result"]["diff"][0]["after"], "x");
}

#[test]
fn raw_execute_cell_preserves_typed_results_and_nontext_content() {
    let server = Server::modern(|a| {
        assert_eq!(a["code"], "return 1");
        json!({"structuredContent":[1,true],"content":[{"type":"image","mimeType":"image/png","data":"test-image"}]})
    });
    let output = invoke(
        &server,
        &["tool", "studio", "call", "execute_cell", "-"],
        Some(r#"{"code":"return 1"}"#),
    );
    assert!(output.status.success(), "{}", payload(&output));
    assert_eq!(payload(&output)["data"]["result"], json!([1, true]));
    assert_eq!(
        payload(&output)["data"]["mcp_content"][0]["data"],
        "test-image"
    );
    assert!(server
        .log()
        .iter()
        .any(|r| r.body["params"]["name"] == "execute_cell"));
}

#[test]
fn resource_and_selector_compatibility_preserves_array_and_rename_arguments() {
    for (tool_name, input, expected) in [
        (
            "selector.details",
            r#"{"selector_names":["甲","b"]}"#,
            "xbot-bash selector get --name \"甲\" \"b\"",
        ),
        (
            "app.resource.add",
            r#"{"file":"D:\\files\\a.txt"}"#,
            "xbot-bash resource add --file \"D:\\\\files\\\\a.txt\"",
        ),
        (
            "app.resource.remove",
            r#"{"files":["a.txt","b.txt"]}"#,
            "xbot-bash resource remove --files \"a.txt\" \"b.txt\"",
        ),
        (
            "app.resource.rename",
            r#"{"name":"a.txt","new_name":"b.txt"}"#,
            "xbot-bash resource rename --name \"a.txt\" --new-name \"b.txt\"",
        ),
        (
            "codeflow.read",
            r#"{"flow_id":"code"}"#,
            "xbot-bash flow read --flow \"code\" --limit 2000",
        ),
    ] {
        let server = Server::modern(move |a| {
            assert_eq!(a["command"], expected);
            tool(json!({"success":true}))
        });
        let output = invoke(
            &server,
            &["tool", "studio", "call", tool_name, "-"],
            Some(input),
        );
        assert!(output.status.success(), "{}", payload(&output));
    }
}
