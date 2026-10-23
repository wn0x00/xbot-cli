//! Public API contract checks against a local HTTP server; no enterprise credentials.
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

#[derive(Clone, Debug)]
struct Request {
    method: String,
    target: String,
    headers: String,
    body: Vec<u8>,
}

struct MockApi {
    origin: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MockApi {
    fn new(handler: impl Fn(&Request, usize) -> (u16, String) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let records = requests.clone();
        let stop = stopped.clone();
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Winsock sockets inherit the listener's nonblocking mode.
                        stream.set_nonblocking(false).unwrap();
                        let Some(request) = read_request(&mut stream) else {
                            continue;
                        };
                        let index = {
                            let mut records = records.lock().unwrap();
                            records.push(request.clone());
                            records.len() - 1
                        };
                        let (status, body) = handler(&request, index);
                        let reply = format!(
                            "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(reply.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("mock accept: {error}"),
                }
            }
        });
        Self {
            origin,
            requests,
            stopped,
            worker: Some(worker),
        }
    }
}

impl Drop for MockApi {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        let size = match stream.read(&mut buffer) {
            Ok(0) if bytes.is_empty() => return None,
            Err(error)
                if bytes.is_empty()
                    && matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
            {
                return None
            }
            result => result.unwrap(),
        };
        assert!(size > 0, "truncated HTTP headers");
        bytes.extend_from_slice(&buffer[..size]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() < end + length {
        let size = stream.read(&mut buffer).unwrap();
        assert!(size > 0, "truncated HTTP body");
        bytes.extend_from_slice(&buffer[..size]);
    }
    let mut first = headers.lines().next().unwrap().split_whitespace();
    Some(Request {
        method: first.next().unwrap().into(),
        target: first.next().unwrap().into(),
        body: bytes[end..end + length].to_vec(),
        headers,
    })
}

fn invoke(args: &[&str], server: Option<&MockApi>, keys: bool, stdin: Option<&str>) -> Output {
    invoke_env(args, server, keys, stdin, &[])
}

fn invoke_env(
    args: &[&str],
    server: Option<&MockApi>,
    keys: bool,
    stdin: Option<&str>,
    extra: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xbot-cli"));
    // 隔离真实凭据:本地凭据文件指向不存在的临时路径,环境变量全部清除。
    let isolated_auth = std::env::temp_dir().join(format!(
        "xbot-oapi-test-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    command
        .args(args)
        .arg("--json")
        .env(
            "XBOT_PLATFORM_BASE_URL",
            server
                .map(|s| s.origin.as_str())
                .unwrap_or("http://127.0.0.1:1"),
        )
        .env("XBOT_OAPI_AUTH_FILE", &isolated_auth)
        .env_remove("XBOT_OAPI_BASE_URL")
        .env_remove("XBOT_OAPI_ACCESS_TOKEN")
        .env_remove("XBOT_OAPI_ACCESS_KEY_ID")
        .env_remove("XBOT_OAPI_ACCESS_KEY_SECRET")
        .env_remove("XBOT_PLATFORM_ACCESS_TOKEN")
        .env_remove("XBOT_PLATFORM_ACCESS_KEY_ID")
        .env_remove("XBOT_PLATFORM_ACCESS_KEY_SECRET")
        .env("XBOT_SHELL_CLI", "__missing_shell_cli__.exe")
        .env("XBOT_MCP_ENDPOINT", "http://127.0.0.1:1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if keys {
        command
            .env("XBOT_PLATFORM_ACCESS_KEY_ID", "test@key&+")
            .env("XBOT_PLATFORM_ACCESS_KEY_SECRET", "test/secret?&+");
    } else if server.is_some() && !extra.iter().any(|(name, _)| *name == "XBOT_OAPI_AUTH_FILE") {
        // 指定了独立凭据文件时不再注入环境 token,以便测试本地配置路径。
        command.env("XBOT_PLATFORM_ACCESS_TOKEN", "private-token");
    }
    for (name, value) in extra {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    if let Some(stdin) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
    }
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

fn envelope(output: Output, exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let (bytes, empty) = if exit == 0 {
        (&output.stdout, &output.stderr)
    } else {
        (&output.stderr, &output.stdout)
    };
    assert!(empty.is_empty());
    let value: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(value["ok"], exit == 0);
    value
}

fn success(data: Value) -> (u16, String) {
    (
        200,
        json!({"code":200,"success":true,"data":data,"requestId":"server-request"}).to_string(),
    )
}

#[test]
fn ordinary_tasks_never_fall_back_to_the_enterprise_api() {
    let server = MockApi::new(|_, _| panic!("ordinary task used the enterprise API"));
    for args in [
        vec![
            "task",
            "run",
            "11111111-1111-1111-1111-111111111111",
            "--timeout",
            "1",
        ],
        vec![
            "app",
            "run",
            "11111111-1111-1111-1111-111111111111",
            "--timeout",
            "1",
        ],
        vec!["task", "status", "22222222-2222-2222-2222-222222222222"],
        vec!["task", "logs", "22222222-2222-2222-2222-222222222222"],
    ] {
        let value = envelope(invoke(&args, Some(&server), false, None), 3);
        assert_eq!(value["error"]["code"], "SHELL_CLI_MISSING");
    }
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn discovery_and_config_work_offline_and_have_platform_semantics() {
    let list = envelope(invoke(&["platform", "api", "list"], None, false, None), 0);
    assert_eq!(list["data"]["endpoints"].as_array().unwrap().len(), 44);
    let docs = envelope(invoke(&["platform", "docs"], None, false, None), 0);
    assert_eq!(docs["data"]["documents"].as_array().unwrap().len(), 68);
    let config = envelope(
        invoke(&["platform", "auth", "status"], None, false, None),
        0,
    );
    assert_eq!(config["data"]["access_token_configured"], false);
    assert_eq!(config["data"]["verified"], false);
    let schema = envelope(
        invoke(&["schema", "platform", "job", "logs"], None, false, None),
        0,
    );
    assert_eq!(schema["data"]["context"], "platform");
    assert_eq!(schema["data"]["requires_studio"], false);
    assert_eq!(schema["data"]["supports_dry_run"], true);
    let result = envelope(
        invoke(&["platform", "job", "status", "job-1"], None, false, None),
        3,
    );
    assert_eq!(result["error"]["code"], "PLATFORM_AUTH");
}

#[test]
fn authentication_encodes_keys_uses_bearer_and_never_prints_credentials() {
    let server = MockApi::new(|request, index| {
        if index == 0 {
            assert_eq!(request.method, "GET");
            let url = url::Url::parse(&format!("http://localhost{}", request.target)).unwrap();
            let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(query["accessKeyId"], "test@key&+");
            assert_eq!(query["accessKeySecret"], "test/secret?&+");
            success(json!({"accessToken":"fresh-token","expiresIn":7199}))
        } else {
            assert!(request
                .headers
                .contains("Authorization: Bearer fresh-token"));
            assert_eq!(
                serde_json::from_slice::<Value>(&request.body).unwrap()["jobUuid"],
                "job-1"
            );
            success(
                json!({"jobUuid":"job-1","status":"error","remark":"failed","screenshotUrl":"https://example.test/error.png"}),
            )
        }
    });
    let value = envelope(
        invoke(
            &["platform", "job", "status", "job-1"],
            Some(&server),
            true,
            None,
        ),
        0,
    );
    assert_eq!(value["data"]["response"]["data"]["remark"], "failed");
    for secret in ["test@key&+", "test/secret?&+", "fresh-token"] {
        assert!(!value.to_string().contains(secret));
    }
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[test]
fn auth_check_returns_expiry_and_static_token_is_not_claimed_verified() {
    let server =
        MockApi::new(|_, _| success(json!({"accessToken":"fresh-token","expiresIn":7199})));
    let value = envelope(
        invoke(&["platform", "auth", "check"], Some(&server), true, None),
        0,
    );
    assert_eq!(value["data"]["verified"], true);
    assert_eq!(value["data"]["expires_in"], 7199);
    assert!(!value.to_string().contains("fresh-token"));
    let value = envelope(
        invoke(&["platform", "auth", "check"], Some(&server), false, None),
        0,
    );
    assert_eq!(value["data"]["verified"], false);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn job_list_preserves_cursor_precision_and_maps_filters() {
    let server = MockApi::new(|request, _| {
        assert_eq!(request.target, "/oapi/dispatch/v2/job/list");
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["cursorId"], json!(284065875470133295_i64));
        assert_eq!(body["statusList"], json!(["error", "stopped"]));
        assert_eq!(body["scheduleUuid"], "schedule-1");
        assert_eq!(body["robotUuid"], "app-1");
        assert_eq!(body["robotClientUuid"], "robot-1");
        success(json!({"nextId":284065875470133276_i64,"hasData":true,"dataList":[]}))
    });
    let value = envelope(
        invoke(
            &[
                "platform",
                "job",
                "list",
                "--app",
                "app-1",
                "--schedule",
                "schedule-1",
                "--robot",
                "robot-1",
                "--status",
                "error,stopped",
                "--cursor",
                "284065875470133295",
            ],
            Some(&server),
            false,
            None,
        ),
        0,
    );
    assert_eq!(
        value["data"]["response"]["data"]["nextId"],
        json!(284065875470133276_i64)
    );
}

#[test]
fn logs_search_maps_filters_and_keeps_pagination() {
    let server = MockApi::new(|request, _| {
        assert_eq!(request.target, "/oapi/dispatch/v2/job/log/search");
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["queryFilter"]["searchKey"], "traceback");
        assert_eq!(body["queryFilter"]["sort"]["sortOrder"], "asc");
        assert_eq!(body["page"], 2);
        success(
            json!({"requestId":"logs-id","page":{"total":42,"page":2,"size":20},"logs":[{"text":"traceback","level":"error"}]}),
        )
    });
    let value = envelope(
        invoke(
            &[
                "platform",
                "job",
                "logs",
                "job-1",
                "--search",
                "traceback",
                "--page",
                "2",
                "--order",
                "asc",
            ],
            Some(&server),
            false,
            None,
        ),
        0,
    );
    assert_eq!(value["data"]["response"]["data"]["page"]["total"], 42);
}

#[test]
fn asynchronous_logs_notify_once_then_poll_same_id() {
    let server = MockApi::new(|request, index| match index {
        0 => {
            assert_eq!(request.target, "/oapi/dispatch/v2/job/log/notify");
            success(json!("logs/id?+&"))
        }
        1 => {
            assert_eq!(request.method, "GET");
            assert!(request.target.contains("requestId=logs%2Fid%3F%2B%26"));
            (
                200,
                json!({"code":80204002,"success":false,"msg":"processing"}).to_string(),
            )
        }
        _ => {
            success(json!({"requestId":"logs/id?+&","page":{},"logs":[{"text":"failure detail"}]}))
        }
    });
    let value = envelope(
        invoke(
            &[
                "platform",
                "job",
                "logs",
                "job-1",
                "--async",
                "--timeout",
                "5",
            ],
            Some(&server),
            false,
            None,
        ),
        0,
    );
    assert_eq!(value["data"]["request_id"], "logs/id?+&");
    assert_eq!(value["data"]["job_uuid"], "job-1");
    assert_eq!(server.requests.lock().unwrap().len(), 3);
}

#[test]
fn async_timeout_retains_request_id_for_continuing_without_notify() {
    let server = MockApi::new(|_, index| {
        if index == 0 {
            success(json!("logs-id"))
        } else {
            (200, json!({"code":80204002,"success":false}).to_string())
        }
    });
    let value = envelope(
        invoke(
            &[
                "platform",
                "job",
                "logs",
                "job-1",
                "--async",
                "--timeout",
                "1",
            ],
            Some(&server),
            false,
            None,
        ),
        4,
    );
    assert_eq!(value["data"]["request_id"], "logs-id");
    assert_eq!(value["data"]["job_uuid"], "job-1");
    assert!(server.requests.lock().unwrap().len() <= 5);
}

#[test]
fn errors_keep_server_evidence_redact_secrets_and_do_not_retry_mutations() {
    let server = MockApi::new(|_, _| {
        (200,
        json!({"success":false,"code":429,"requestId":"trace-id","msg":"echo private-token","data":{"accessToken":"private-token"}}).to_string())
    });
    let value = envelope(
        invoke(
            &["platform", "job", "status", "job-1"],
            Some(&server),
            false,
            None,
        ),
        4,
    );
    assert_eq!(value["error"]["retryable"], true);
    assert_eq!(value["data"]["response"]["requestId"], "trace-id");
    assert!(!value.to_string().contains("private-token"));
    let value = envelope(
        invoke(
            &[
                "platform",
                "api",
                "call",
                "job.stop",
                "--body",
                r#"{"jobUuid":"job-1"}"#,
                "--yes",
            ],
            Some(&server),
            false,
            None,
        ),
        4,
    );
    assert_eq!(value["error"]["retryable"], false);
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[test]
fn http_errors_and_malformed_responses_cannot_claim_success() {
    for (http, body, exit, code) in [
        (
            401,
            r#"{"code":401,"success":false,"requestId":"auth-failure"}"#,
            3,
            "PLATFORM_AUTH",
        ),
        (503, r#"{"code":200,"success":true}"#, 1, "PLATFORM_API"),
        (200, "not json private-token", 6, "PLATFORM_BAD_RESPONSE"),
        (200, r#"{"data":[]}"#, 6, "PLATFORM_BAD_RESPONSE"),
    ] {
        let server = MockApi::new(move |_, _| (http, body.into()));
        let value = envelope(
            invoke(
                &["platform", "job", "status", "job-1"],
                Some(&server),
                false,
                None,
            ),
            exit,
        );
        assert_eq!(value["error"]["code"], code);
        assert!(!value.to_string().contains("private-token"));
    }
}

#[test]
fn writes_are_confirmed_only_after_input_validation_and_preview_is_offline() {
    let server = MockApi::new(|_, _| success(json!({})));
    let args = [
        "platform",
        "api",
        "call",
        "job.stop",
        "--body",
        r#"{"jobUuid":"job-1"}"#,
    ];
    let value = envelope(invoke(&args, Some(&server), false, None), 10);
    assert_eq!(value["data"]["target"]["endpoint"], "job.stop");
    let mut preview = args.to_vec();
    preview.push("--dry-run");
    let value = envelope(invoke(&preview, Some(&server), false, None), 0);
    assert_eq!(value["data"]["confirmation_required"], true);
    assert!(server.requests.lock().unwrap().is_empty());
    let value = envelope(
        invoke(
            &[
                "platform",
                "api",
                "call",
                "job.stop",
                "--body",
                "{}",
                "--dry-run",
            ],
            None,
            false,
            None,
        ),
        2,
    );
    assert_eq!(value["error"]["code"], "BAD_INPUT");
}

#[test]
fn raw_get_encodes_path_and_query_and_reads_stdin_once() {
    let server = MockApi::new(|request, _| {
        assert_eq!(request.method, "GET");
        assert!(
            request
                .target
                .starts_with("/oapi/tool/queue/v1/queues/a%2Fb%20%3F&/queueItems?"),
            "{}",
            request.target
        );
        assert!(request.target.contains("status=on+hold"));
        success(json!([]))
    });
    envelope(
        invoke(
            &[
                "platform",
                "api",
                "call",
                "queue.items",
                "--path",
                r#"{"queueUuid":"a/b ?&"}"#,
                "--query",
                "-",
            ],
            Some(&server),
            false,
            Some(r#"{"status":"on hold","page":1,"size":10}"#),
        ),
        0,
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    envelope(
        invoke(
            &[
                "platform",
                "api",
                "call",
                "queue.items",
                "--path",
                "-",
                "--query",
                "-",
                "--dry-run",
            ],
            None,
            false,
            Some("{}"),
        ),
        2,
    );
}

#[test]
fn raw_patch_and_multipart_upload_use_documented_wire_formats() {
    let server = MockApi::new(|request, index| {
        if index == 0 {
            assert_eq!(request.method, "PATCH");
            assert_eq!(
                request.target,
                "/oapi/tool/queue/v1/queueitems/item-1/reenqueue"
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&request.body).unwrap()["effectiveTime"],
                1792713600
            );
        } else {
            assert!(request
                .headers
                .contains("Content-Type: multipart/form-data; boundary="));
            let body = String::from_utf8_lossy(&request.body);
            assert!(body.contains("name=\"file\""));
            assert!(body.contains("platform upload fixture"));
        }
        success(json!({"fileKey":"uploaded-key"}))
    });
    envelope(
        invoke(
            &[
                "platform",
                "api",
                "call",
                "queue.reenqueue",
                "--path",
                r#"{"itemUuid":"item-1"}"#,
                "--body",
                r#"{"effectiveTime":1792713600}"#,
                "--yes",
            ],
            Some(&server),
            false,
            None,
        ),
        0,
    );
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/platform-upload.txt");
    envelope(
        invoke(
            &[
                "platform",
                "api",
                "call",
                "file.upload",
                "--file",
                path.to_str().unwrap(),
                "--yes",
            ],
            Some(&server),
            false,
            None,
        ),
        0,
    );
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[test]
fn records_keep_nested_page_and_validate_invalid_times_before_network() {
    let server = MockApi::new(|request, _| {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["appId"], "app-1");
        assert_eq!(body["startModelList"], json!(["schedule", "Assistant"]));
        success(json!({"pageDTO":{"total":8},"result":[{"runStatusDesc":"failed"}]}))
    });
    let value = envelope(
        invoke(
            &[
                "platform",
                "app",
                "records",
                "--app",
                "app-1",
                "--from",
                "2026-10-16 00:00:00",
                "--to",
                "2026-10-23 00:00:00",
                "--mode",
                "schedule,Assistant",
            ],
            Some(&server),
            false,
            None,
        ),
        0,
    );
    assert_eq!(value["data"]["response"]["data"]["pageDTO"]["total"], 8);
    envelope(
        invoke(
            &[
                "platform",
                "app",
                "records",
                "--from",
                "2026-10-16 00:00:00",
                "--to",
                "2026-10-23 00:00:01",
            ],
            Some(&server),
            false,
            None,
        ),
        2,
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn oapi_is_canonical_and_platform_remains_a_compatibility_alias() {
    for args in [vec!["oapi", "api", "list"], vec!["platform", "api", "list"]] {
        let value = envelope(invoke(&args, None, false, None), 0);
        assert_eq!(value["data"]["endpoints"].as_array().unwrap().len(), 44);
    }
    let schema = envelope(
        invoke(&["schema", "platform", "job", "start"], None, false, None),
        0,
    );
    assert_eq!(schema["data"]["name"], "oapi job start");
    assert_eq!(schema["data"]["confirmation"], "always");
    let help = invoke(&["oapi", "--help"], None, false, None);
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    for group in [
        "auth", "app", "robot", "job", "schedule", "task", "user", "queue",
    ] {
        assert!(text.contains(group), "oapi help missing {group}");
    }
}

#[test]
fn auth_login_stores_credentials_and_logout_removes_them() {
    let dir = std::env::temp_dir().join(format!("xbot-oapi-login-{}", std::process::id()));
    let auth_file = dir.join("auth.json");
    let auth_env = [("XBOT_OAPI_AUTH_FILE", auth_file.to_str().unwrap())];
    let server = MockApi::new(|request, _| {
        let url = url::Url::parse(&format!("http://localhost{}", request.target)).unwrap();
        match (request.method.as_str(), url.path()) {
            ("GET", "/oapi/token/v2/token/create") => {
                let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
                assert_eq!(query["accessKeyId"], "saved@platform");
                assert_eq!(query["accessKeySecret"], "saved-secret");
                success(json!({"accessToken":"stored-token","expiresIn":7199}))
            }
            ("POST", "/oapi/dispatch/v2/client/list") => {
                assert!(request
                    .headers
                    .contains("Authorization: Bearer stored-token"));
                success(json!({"data":[],"page":{"total":0}}))
            }
            other => panic!("unexpected request: {other:?}"),
        }
    });
    // 位置参数形式:oapi auth <ak> <sk> 等价于 auth login。
    let login = envelope(
        invoke_env(
            &["oapi", "auth", "saved@platform", "saved-secret"],
            Some(&server),
            false,
            None,
            &auth_env,
        ),
        0,
    );
    assert_eq!(login["data"]["saved"], true);
    assert_eq!(login["data"]["verified"], true);
    assert!(!login.to_string().contains("saved-secret"));
    let stored: Value =
        serde_json::from_slice(&std::fs::read(&auth_file).expect("auth file saved")).unwrap();
    assert_eq!(stored["access_key_id"], "saved@platform");
    assert_eq!(stored["access_token"], "stored-token");
    assert!(stored["token_expires_at"].as_u64().unwrap() > 0);

    // 后续命令不带环境变量凭据,直接复用本地缓存的 token,不再请求鉴权接口。
    let list = envelope(
        invoke_env(
            &["oapi", "robot", "list", "--search", "bot"],
            Some(&server),
            false,
            None,
            &auth_env,
        ),
        0,
    );
    assert_eq!(list["data"]["endpoint"], "robot.list");
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    let status = envelope(
        invoke_env(
            &["oapi", "auth", "status"],
            Some(&server),
            false,
            None,
            &auth_env,
        ),
        0,
    );
    assert_eq!(status["data"]["credential_source"], "config_file");
    assert_eq!(status["data"]["access_key_id"], "save***");
    assert_eq!(status["data"]["token_cached"], true);

    // logout 清除当前授权:凭据与 token 缓存一并删除。
    let logout = envelope(
        invoke_env(
            &["oapi", "auth", "logout"],
            Some(&server),
            false,
            None,
            &auth_env,
        ),
        0,
    );
    assert_eq!(logout["data"]["removed"], true);
    assert!(!auth_file.exists());
    let denied = envelope(
        invoke_env(
            &["oapi", "robot", "list"],
            Some(&server),
            false,
            None,
            &auth_env,
        ),
        3,
    );
    assert_eq!(denied["error"]["code"], "PLATFORM_AUTH");
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expired_token_is_refreshed_with_the_stored_keys() {
    let dir = std::env::temp_dir().join(format!("xbot-oapi-refresh-{}", std::process::id()));
    let auth_file = dir.join("auth.json");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &auth_file,
        serde_json::to_vec(&json!({
            "access_key_id": "saved@platform",
            "access_key_secret": "saved-secret",
            "access_token": "expired-token",
            "token_expires_at": 1,
        }))
        .unwrap(),
    )
    .unwrap();
    let auth_env = [("XBOT_OAPI_AUTH_FILE", auth_file.to_str().unwrap())];
    let server = MockApi::new(|request, _| {
        let url = url::Url::parse(&format!("http://localhost{}", request.target)).unwrap();
        match (request.method.as_str(), url.path()) {
            ("GET", "/oapi/token/v2/token/create") => {
                success(json!({"accessToken":"fresh-token","expiresIn":7199}))
            }
            ("POST", "/oapi/dispatch/v2/client/list") => {
                assert!(request
                    .headers
                    .contains("Authorization: Bearer fresh-token"));
                success(json!({"data":[]}))
            }
            other => panic!("unexpected request: {other:?}"),
        }
    });
    envelope(
        invoke_env(
            &["oapi", "robot", "list"],
            Some(&server),
            false,
            None,
            &auth_env,
        ),
        0,
    );
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    let stored: Value = serde_json::from_slice(&std::fs::read(&auth_file).unwrap()).unwrap();
    assert_eq!(stored["access_token"], "fresh-token");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn friendly_commands_map_to_documented_requests() {
    let server = MockApi::new(|request, _| {
        let url = url::Url::parse(&format!("http://localhost{}", request.target)).unwrap();
        match (request.method.as_str(), url.path()) {
            ("GET", "/oapi/token/v2/token/create") => {
                success(json!({"accessToken":"fresh-token","expiresIn":7199}))
            }
            ("POST", "/oapi/dispatch/v2/client/list") => {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body, json!({"page":1,"size":20,"key":"bot"}));
                success(json!({"data":[]}))
            }
            ("POST", "/oapi/app/open/query/list") => {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body, json!({"page":"1","size":"20","appName":"报表"}));
                success(json!({"data":[]}))
            }
            ("GET", "/oapi/app/open/query/appOnlineDetailWithParam") => {
                let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
                assert_eq!(query["appId"], "app-1");
                success(json!({"data":{"appId":"app-1"}}))
            }
            ("POST", "/oapi/dispatch/v2/job/start") => {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["robotUuid"], "app-1");
                assert_eq!(body["accountName"], "robot-1");
                assert_eq!(body["runTimeout"], 600);
                assert_eq!(body["idempotentUuid"], "idem-1");
                assert_eq!(body["params"][0]["name"], "month");
                success(json!({"jobUuid":"job-1"}))
            }
            ("POST", "/oapi/tool/queue/v1/queues/q-1/enqueue") => {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["priority"], 0);
                assert_eq!(body["source"], "OpenAPI");
                success(json!({"data":{"uuid":"q-1_item"}}))
            }
            ("PATCH", "/oapi/tool/queue/v1/queueitems/item-1") => {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["status"], "processed");
                success(json!({}))
            }
            other => panic!("unexpected request: {other:?}"),
        }
    });
    let keys = true;
    let jobs = [
        vec!["oapi", "robot", "list", "--search", "bot"],
        vec!["oapi", "app", "list", "--search", "报表"],
        vec!["oapi", "app", "detail", "app-1"],
        vec![
            "oapi",
            "job",
            "start",
            "--app",
            "app-1",
            "--robot",
            "robot-1",
            "--run-timeout",
            "600",
            "--idempotent",
            "idem-1",
            "--params",
            r#"[{"name":"month","value":"2026-10","type":"str"}]"#,
            "--yes",
        ],
        vec![
            "oapi",
            "queue",
            "enqueue",
            "q-1",
            "--name",
            "工单",
            "--priority",
            "0",
            "--biz-info",
            "biz-1",
            "--yes",
        ],
        vec![
            "oapi",
            "queue",
            "modify",
            "item-1",
            "--status",
            "processed",
            "--yes",
        ],
    ];
    for args in &jobs {
        envelope(invoke(args, Some(&server), keys, None), 0);
    }
    // 环境变量凭据不复用 token:每条命令各 1 次鉴权与 1 次业务请求。
    assert_eq!(server.requests.lock().unwrap().len(), jobs.len() * 2);
}

#[test]
fn job_start_is_confirmed_and_previews_offline() {
    let server = MockApi::new(|_, _| panic!("preview must not call the API"));
    let args = [
        "oapi",
        "job",
        "start",
        "--app",
        "app-1",
        "--robot",
        "robot-1",
        "--params",
        r#"[{"name":"month","value":"2026-10","type":"str"}]"#,
    ];
    let value = envelope(invoke(&args, Some(&server), false, None), 10);
    assert_eq!(value["data"]["target"]["endpoint"], "job.start");
    let mut preview = args.to_vec();
    preview.push("--dry-run");
    let value = envelope(invoke(&preview, Some(&server), false, None), 0);
    assert_eq!(value["data"]["confirmation_required"], true);
    assert_eq!(value["data"]["arguments"]["body"]["robotUuid"], "app-1");
    assert_eq!(
        value["data"]["arguments"]["body"]["params"][0]["name"],
        "month"
    );
    assert!(server.requests.lock().unwrap().is_empty());
    for bad in [
        vec!["oapi", "job", "start", "--app", "app-1", "--dry-run"],
        vec![
            "oapi",
            "job",
            "start",
            "--app",
            "app-1",
            "--robot",
            "r",
            "--params",
            "{}",
            "--dry-run",
        ],
        vec![
            "oapi",
            "queue",
            "enqueue",
            "q",
            "--name",
            "n",
            "--priority",
            "1",
            "--biz-info",
            "b",
            "--dry-run",
        ],
    ] {
        envelope(invoke(&bad, None, false, None), 2);
    }
}
