//! Official Open API transport and offline catalog. No desktop session is required.
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;
use serde_json::{json, Value};
use url::Url;

use crate::app_cmds::CmdOut;
use crate::core::{self, BackendError, Res};
use crate::oapi_cli::*;

const BASE_URL: &str = "https://api.yingdao.com";
const MAX_RESPONSE: u64 = 20 * 1024 * 1024;

// Classification is explicit: several read APIs use POST, while log.notify
// requests log collection without starting or changing a business task.
const ENDPOINTS: &[(&str, &str, bool)] = &[
    ("auth.token", "710499792859115520", false),
    ("user.list", "710498885910380544", false),
    ("user.create", "710497757007953920", true),
    ("user.modify", "710496549620625408", true),
    ("user.delete", "710495670001700864", true),
    ("user.reset-password", "710494844581036032", true),
    ("app.list", "710474957390741504", false),
    ("app.online-detail", "971288732102688768", false),
    ("app.version-detail", "971292986109808640", false),
    ("app.versions", "971293347837771776", false),
    ("app.flows", "971293556891668480", false),
    ("app.transfer", "710472302045138944", true),
    ("app.params-legacy", "710473073333092352", false),
    ("robot.groups", "710477790566137856", false),
    ("robot.list", "710476864272334848", false),
    ("robot.detail", "710475766593617920", false),
    ("job.list", "710485133474119680", false),
    ("job.status", "710487379305533440", false),
    ("job.start", "710488569666060288", true),
    ("job.stop", "710486342769258496", true),
    ("job.retry", "710483153807343616", true),
    ("schedule.list", "710465801475309568", false),
    ("schedule.detail", "710466731559485440", false),
    ("task.start", "710492920473436160", true),
    ("task.stop", "710490754371760128", true),
    ("task.status", "710491717794512896", false),
    ("task.latest", "710468955701456896", false),
    ("task.jobs", "710469888978161664", false),
    ("task.runs", "710468004680773632", false),
    ("log.search", "710481967730900992", false),
    ("log.notify", "710481123051515904", false),
    ("log.query", "710479934544805888", false),
    ("queue.list", "971304783746039808", false),
    ("queue.items", "971305176334413824", false),
    ("queue.enqueue", "710494036079095808", true),
    ("queue.modify", "717268028778450944", true),
    ("queue.reenqueue", "717268756412026880", true),
    ("queue.dequeue", "717267384361996288", true),
    ("queue.count", "971305566201839616", false),
    ("queue.statuses", "971305676046467072", false),
    ("queue.batch-status", "971305358878912512", true),
    ("file.upload", "710470879923273728", true),
    ("app.records", "971307981009453056", false),
    ("app.records-cursor", "710474186124374016", false),
];

fn documents() -> &'static [Value] {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../docs/platform-api-reference.json"))
            .expect("bundled public documentation")
    })["documents"]
        .as_array()
        .expect("bundled document list")
}

#[derive(Clone)]
struct Endpoint {
    name: &'static str,
    id: &'static str,
    method: String,
    path: String,
    mutating: bool,
}

impl Endpoint {
    fn document(&self) -> &'static Value {
        documents().iter().find(|d| d["id"] == self.id).unwrap()
    }

    fn summary(&self) -> Value {
        json!({
            "name":self.name, "document_id":self.id, "title":self.document()["title"],
            "method":self.method, "path":self.path, "mutating":self.mutating,
            "confirmation":if self.mutating {"always"} else {"none"},
            "documentation":self.document()["url"],
            "deprecated":self.document()["title"].as_str().unwrap_or("").contains("废弃"),
        })
    }
}

fn endpoints() -> Vec<Endpoint> {
    ENDPOINTS
        .iter()
        .map(|&(name, id, mutating)| {
            let doc = documents().iter().find(|doc| doc["id"] == id).unwrap();
            let rows = doc["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|b| b["rows"].as_array())
                .flatten();
            let mut method = None;
            let mut path = None;
            for row in rows {
                if row[0] == "HTTP Method" {
                    method = row[1].as_str().map(str::to_owned);
                }
                if row[0] == "HTTP URL" {
                    // The docs contain both the current and the legacy public host.
                    // All calls use the configured origin and the documented /oapi path.
                    path = row[1].as_str().map(|s| {
                        s[s.find("/oapi/").expect("documented API path")..]
                            .replace("{{", "{")
                            .replace("}}", "}")
                    });
                }
            }
            Endpoint {
                name,
                id,
                mutating,
                method: method.expect("documented HTTP method"),
                path: path.expect("documented API path"),
            }
        })
        .collect()
}

fn endpoint(name: &str) -> Res<Endpoint> {
    endpoints()
        .into_iter()
        .find(|e| e.name == name || e.id == name || e.path == name)
        .ok_or_else(|| {
            BackendError::new("BAD_ARGS", format!("未知平台接口: {name}"))
                .with_hint("xbot-cli oapi api list --json")
        })
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

/// XBOT_OAPI_* 优先,XBOT_PLATFORM_* 保持兼容。
fn env_any(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| env_value(name))
}

fn auth_file() -> Option<PathBuf> {
    if let Some(path) = env_value("XBOT_OAPI_AUTH_FILE") {
        return Some(PathBuf::from(path));
    }
    env_value("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("xbot-cli").join("oapi-auth.json"))
}

fn read_stored_auth() -> Option<Value> {
    let content = fs::read_to_string(auth_file()?).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    Some(value)
}

fn stored_string(auth: &Value, key: &str) -> Option<String> {
    auth.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 缓存 token 提前 60 秒视为过期,避免边界时刻使用将过期 token。
const TOKEN_SKEW_SECS: u64 = 60;

fn stored_token(auth: &Value) -> Option<(String, u64)> {
    let token = stored_string(auth, "access_token")?;
    let expires_at = auth.get("token_expires_at")?.as_u64()?;
    Some((token, expires_at))
}

fn write_stored_auth(value: &Value) -> Res<PathBuf> {
    let path = auth_file()
        .ok_or_else(|| BackendError::new("PLATFORM_CONFIG", "无法确定本地凭据存储路径"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| BackendError::new("FILE_WRITE", e.to_string()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(value).unwrap())
        .map_err(|e| BackendError::new("FILE_WRITE", e.to_string()))?;
    Ok(path)
}

fn mask(value: &str) -> String {
    let prefix: String = value.chars().take(4).collect();
    format!("{prefix}***")
}

/// 凭据来源:环境变量优先,其次 oapi auth login 保存的本地配置。
#[derive(Default, Clone)]
struct Credentials {
    token: Option<String>,
    key_id: Option<String>,
    key_secret: Option<String>,
    base_url: Option<String>,
    stored_token: Option<(String, u64)>,
    source: &'static str,
}

impl Credentials {
    fn resolve() -> Self {
        let token = env_any(&["XBOT_OAPI_ACCESS_TOKEN", "XBOT_PLATFORM_ACCESS_TOKEN"]);
        let key_id = env_any(&["XBOT_OAPI_ACCESS_KEY_ID", "XBOT_PLATFORM_ACCESS_KEY_ID"]);
        let key_secret = env_any(&[
            "XBOT_OAPI_ACCESS_KEY_SECRET",
            "XBOT_PLATFORM_ACCESS_KEY_SECRET",
        ]);
        let env_base = env_any(&["XBOT_OAPI_BASE_URL", "XBOT_PLATFORM_BASE_URL"]);
        if token.is_some() || key_id.is_some() || key_secret.is_some() {
            return Self {
                token,
                key_id,
                key_secret,
                base_url: env_base,
                stored_token: None,
                source: "environment",
            };
        }
        let stored = read_stored_auth();
        Self {
            token: None,
            key_id: stored
                .as_ref()
                .and_then(|a| stored_string(a, "access_key_id")),
            key_secret: stored
                .as_ref()
                .and_then(|a| stored_string(a, "access_key_secret")),
            base_url: env_base
                .or_else(|| stored.as_ref().and_then(|a| stored_string(a, "base_url"))),
            stored_token: stored.as_ref().and_then(stored_token),
            source: if stored.is_some() {
                "config_file"
            } else {
                "none"
            },
        }
    }

    fn secrets(&self) -> Secrets {
        Secrets(
            [&self.token, &self.key_id, &self.key_secret]
                .into_iter()
                .flatten()
                .cloned()
                .collect(),
        )
    }
}

#[derive(Default)]
struct Secrets(Vec<String>);

impl Secrets {
    fn scrub(&self, value: &mut Value) {
        match value {
            Value::String(text) => {
                for secret in &self.0 {
                    *text = text.replace(secret, "[REDACTED]");
                    let encoded = url::form_urlencoded::Serializer::new(String::new())
                        .append_pair("value", secret)
                        .finish();
                    *text = text.replace(&encoded[6..], "[REDACTED]");
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.scrub(v)),
            Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(
                        key.to_ascii_lowercase().as_str(),
                        "accesstoken"
                            | "accesskeyid"
                            | "accesskeysecret"
                            | "authorization"
                            | "password"
                            | "oldpassword"
                            | "newpassword"
                            | "pwd"
                            | "oldpwd"
                            | "newpwd"
                            | "secret"
                            | "token"
                    ) {
                        *value = json!("[REDACTED]");
                    } else {
                        self.scrub(value);
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
struct Request {
    endpoint: Endpoint,
    body: Value,
    query: Value,
    path: Value,
    upload: Option<(String, Vec<u8>)>,
}

fn object(raw: Option<&str>) -> Res<Value> {
    let value = raw.map(core::json_arg).transpose()?.unwrap_or(json!({}));
    if !value.is_object() {
        return Err(BackendError::new(
            "BAD_INPUT",
            "平台接口参数必须是 JSON 对象",
        ));
    }
    Ok(value)
}

fn present(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::String(value)) => !value.trim().is_empty(),
        _ => true,
    }
}

fn parse_time(value: &str) -> Res<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .ok()
        .filter(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string() == value)
        .ok_or_else(|| BackendError::new("BAD_INPUT", "时间必须是 yyyy-MM-dd HH:mm:ss"))
}

fn validate_times(body: &Value, begin: &str, end: &str, seven_days: bool) -> Res<()> {
    let from = body
        .get(begin)
        .and_then(Value::as_str)
        .map(parse_time)
        .transpose()?;
    let to = body
        .get(end)
        .and_then(Value::as_str)
        .map(parse_time)
        .transpose()?;
    if seven_days && (from.is_none() || to.is_none()) {
        return Err(BackendError::new("BAD_INPUT", "必须同时提供起止时间"));
    }
    if let (Some(from), Some(to)) = (from, to) {
        let span = to - from;
        if span < chrono::Duration::zero() || (seven_days && span > chrono::Duration::days(7)) {
            return Err(BackendError::new(
                "BAD_INPUT",
                if seven_days {
                    "时间范围必须为0至7天"
                } else {
                    "结束时间不能早于开始时间"
                },
            ));
        }
    }
    Ok(())
}

impl Request {
    fn new(name: &str, body: Value, query: Value, path: Value) -> Res<Self> {
        let request = Self {
            endpoint: endpoint(name)?,
            body,
            query,
            path,
            upload: None,
        };
        if request.endpoint.name == "auth.token" {
            return Err(
                BackendError::new("BAD_ARGS", "请使用 oapi auth check 验证密钥").with_hint(
                    "运行 xbot-cli oapi auth login --ak <id> --sk <secret> 保存凭据,或设置 XBOT_OAPI_ACCESS_KEY_ID / XBOT_OAPI_ACCESS_KEY_SECRET",
                ),
            );
        }
        request.validate()?;
        Ok(request)
    }

    fn validate(&self) -> Res<()> {
        if self
            .query
            .as_object()
            .unwrap()
            .values()
            .any(|v| v.is_array() || v.is_object())
        {
            return Err(BackendError::new(
                "BAD_INPUT",
                "URL 查询参数值必须是字符串、数字或布尔值",
            ));
        }
        if self.endpoint.method == "GET" && self.body.as_object().is_some_and(|b| !b.is_empty()) {
            return Err(BackendError::new(
                "BAD_INPUT",
                "GET 接口请使用 --query 传参",
            ));
        }
        for key in self.path.as_object().unwrap().keys() {
            if !self.endpoint.path.contains(&format!("{{{key}}}")) {
                return Err(BackendError::new(
                    "BAD_INPUT",
                    format!("未知路径参数: {key}"),
                ));
            }
        }
        for segment in self.endpoint.path.split('/') {
            if let Some(key) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                if !self.path.get(key).is_some_and(|v| {
                    v.as_str()
                        .is_some_and(|s| !s.is_empty() && s != "." && s != "..")
                }) {
                    return Err(BackendError::new(
                        "BAD_INPUT",
                        format!("缺少非空字符串路径参数: {key}"),
                    ));
                }
            }
        }
        // Preserve the source tables, including nested fields and descriptions.
        // Validate top-level required fields only; backend-specific nested models
        // remain the server's contract rather than guessed client-side schemas.
        let mut section = "";
        for block in self.endpoint.document()["blocks"].as_array().unwrap() {
            if block["type"] == "heading" {
                section = block["text"].as_str().unwrap_or("");
            }
            if !matches!(section, "请求体" | "请求参数") {
                continue;
            }
            let Some(rows) = block["rows"].as_array() else {
                continue;
            };
            let Some(header) = rows.first().and_then(Value::as_array) else {
                continue;
            };
            let required_column = header.iter().position(|v| v == "是否必填");
            for row in rows.iter().skip(1) {
                let name = row[0].as_str().unwrap_or("");
                if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    continue;
                }
                // file is supplied by the multipart encoder after this validation.
                if self.endpoint.name == "file.upload" && name == "file" {
                    continue;
                }
                let values = if self.endpoint.method == "GET" {
                    &self.query
                } else {
                    &self.body
                };
                if required_column.is_some_and(|i| row[i].as_str().unwrap_or("").starts_with('是'))
                    && !present(values.get(name))
                    && !present(self.path.get(name))
                {
                    return Err(BackendError::new(
                        "BAD_INPUT",
                        format!("缺少必填参数: {name}"),
                    ));
                }
            }
        }
        if self.endpoint.name == "app.records" {
            validate_times(&self.body, "minTime", "maxTime", true)?;
        }
        if self.endpoint.name == "app.records-cursor"
            && !present(self.body.get("appId"))
            && !(present(self.body.get("beginDate")) && present(self.body.get("endDate")))
        {
            return Err(BackendError::new(
                "BAD_INPUT",
                "必须提供 appId 或 beginDate/endDate",
            ));
        }
        if self.endpoint.name == "job.list" {
            validate_times(&self.body, "triggerTimeBegin", "triggerTimeEnd", false)?;
        }
        if matches!(self.endpoint.name, "task.runs" | "task.latest") {
            validate_times(&self.body, "startTime", "endTime", false)?;
        }
        if matches!(self.endpoint.name, "log.search" | "log.notify") {
            if let Some(filter) = self.body.get("queryFilter") {
                validate_times(filter, "beginTime", "endTime", false)?;
            }
        }
        for values in [&self.body, &self.query] {
            for key in ["page", "size"] {
                if let Some(value) = values.get(key) {
                    let integer = value.as_u64().or_else(|| value.as_str()?.parse().ok());
                    if integer.is_none_or(|v| v == 0) {
                        return Err(BackendError::new(
                            "BAD_INPUT",
                            format!("{key} 必须是正整数"),
                        ));
                    }
                    if self.endpoint.name == "job.list"
                        && key == "size"
                        && integer.is_some_and(|v| v > 100)
                    {
                        return Err(BackendError::new("BAD_INPUT", "job.list size 最大100"));
                    }
                }
            }
        }
        Ok(())
    }

    fn add_file(&mut self, path: Option<PathBuf>) -> Res<()> {
        if self.endpoint.name != "file.upload" {
            return if path.is_some() {
                Err(BackendError::new("BAD_ARGS", "--file 仅适用于 file.upload"))
            } else {
                Ok(())
            };
        }
        let path = path.ok_or_else(|| BackendError::new("BAD_INPUT", "file.upload 需要 --file"))?;
        if self.body != json!({}) || self.query != json!({}) {
            return Err(BackendError::new(
                "BAD_INPUT",
                "file.upload 不接受 --body 或 --query",
            ));
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| BackendError::new("BAD_INPUT", "文件名无效"))?;
        if name.chars().count() > 100 || name.contains(['"', '\r', '\n', '\\']) {
            return Err(BackendError::new("BAD_INPUT", "文件名过长或包含非法字符"));
        }
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "txt" | "csv" | "xlsx") {
            return Err(BackendError::new("BAD_INPUT", "上传支持 txt/csv/xlsx 文件"));
        }
        let mut data = Vec::new();
        fs::File::open(&path)
            .map_err(|e| BackendError::new("FILE_READ", e.to_string()))?
            .take(10 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(|e| BackendError::new("FILE_READ", e.to_string()))?;
        if data.len() > 10 * 1024 * 1024 {
            return Err(BackendError::new("BAD_INPUT", "上传文件不能超过10MB"));
        }
        self.upload = Some((name.to_owned(), data));
        Ok(())
    }

    fn preview(&self) -> Value {
        let mut value = json!({
            "action":format!("oapi.{}",self.endpoint.name),
            "target":{"endpoint":self.endpoint.name,"method":self.endpoint.method,"path":self.endpoint.path},
            "arguments":{"body":self.body,"query":self.query,"path":self.path},
        });
        if let Some((name, bytes)) = &self.upload {
            value["arguments"]["file"] = json!({"name":name,"bytes":bytes.len()});
        }
        Credentials::resolve().secrets().scrub(&mut value);
        value
    }
}

pub struct PreparedPlatform {
    operation: Operation,
}

enum Operation {
    Offline(Value),
    Auth {
        check: bool,
        timeout: u64,
    },
    Login {
        key_id: String,
        key_secret: String,
        base_url: Option<String>,
        timeout: u64,
    },
    Logout,
    Call {
        request: Box<Request>,
        timeout: u64,
        asynchronous: bool,
    },
}

impl PreparedPlatform {
    pub fn preview(&self) -> Option<Value> {
        match &self.operation {
            Operation::Call {
                request,
                asynchronous,
                timeout,
            } => {
                let mut value = request.preview();
                value["arguments"]["timeout"] = json!(timeout);
                value["arguments"]["asynchronous"] = json!(asynchronous);
                Some(value)
            }
            _ => None,
        }
    }

    pub fn confirmation_required(&self) -> bool {
        matches!(&self.operation, Operation::Call { request, .. } if request.endpoint.mutating)
    }

    pub fn execute(self) -> Res<CmdOut> {
        let data = match self.operation {
            Operation::Offline(data) => data,
            Operation::Auth { check, timeout } => {
                let mut client = Client::new(timeout)?;
                let mut status = client.status();
                if check {
                    client.authenticate()?;
                    status["verified"] = json!(client.expires_in.is_some());
                    status["expires_in"] = json!(client.expires_in);
                }
                status
            }
            Operation::Login {
                key_id,
                key_secret,
                base_url,
                timeout,
            } => auth_login(key_id, key_secret, base_url, timeout)?,
            Operation::Logout => auth_logout(),
            Operation::Call {
                request,
                timeout,
                asynchronous,
            } => {
                let mut client = Client::new(timeout)?;
                client.authenticate()?;
                if asynchronous {
                    client.logs(*request)?
                } else {
                    client.call(&request)?
                }
            }
        };
        Ok((data, vec![]))
    }
}

/// 先用给定密钥联网换取 token 验证有效,再把密钥保存到本地配置。
fn auth_login(
    key_id: String,
    key_secret: String,
    base_url: Option<String>,
    timeout: u64,
) -> Res<Value> {
    for (name, value) in [("ak", &key_id), ("sk", &key_secret)] {
        if value.trim().is_empty() {
            return Err(BackendError::new("BAD_INPUT", format!("--{name} 不能为空")));
        }
    }
    let credentials = Credentials {
        key_id: Some(key_id.clone()),
        key_secret: Some(key_secret.clone()),
        base_url: base_url.or_else(|| env_any(&["XBOT_OAPI_BASE_URL", "XBOT_PLATFORM_BASE_URL"])),
        ..Default::default()
    };
    let mut client = Client::with_credentials(timeout, credentials)?;
    client.authenticate()?;
    let expires_at = client.expires_in.map(|secs| now_epoch() + secs);
    let stored = json!({
        "access_key_id": key_id,
        "access_key_secret": key_secret,
        "base_url": client.base.as_str(),
        "access_token": client.token,
        "token_expires_at": expires_at,
    });
    let path = write_stored_auth(&stored)?;
    let env_override = env_any(&[
        "XBOT_OAPI_ACCESS_KEY_ID",
        "XBOT_PLATFORM_ACCESS_KEY_ID",
        "XBOT_OAPI_ACCESS_TOKEN",
        "XBOT_PLATFORM_ACCESS_TOKEN",
    ])
    .is_some();
    Ok(json!({
        "saved": true,
        "auth_file": path.display().to_string(),
        "base_url": client.base.as_str(),
        "access_key_id": mask(&key_id),
        "verified": true,
        "expires_in": client.expires_in,
        "token_expires_at": expires_at,
        "note": if env_override {
            "环境变量已设置凭据,优先于本地配置;auth status 可查看生效来源"
        } else {
            "凭据与 token 已保存,过期后自动用密钥刷新;oapi auth logout 可清除授权"
        },
    }))
}

fn auth_logout() -> Value {
    let path = auth_file();
    let removed = path
        .as_ref()
        .filter(|p| p.exists())
        .and_then(|p| fs::remove_file(p).ok())
        .is_some();
    json!({
        "removed": removed,
        "auth_file": path.map(|p| p.display().to_string()),
        "note": "环境变量中的凭据不受影响;auth status 可查看生效来源",
    })
}

fn insert_optional(body: &mut Value, key: &str, value: Option<String>) {
    if let Some(value) = value {
        body[key] = json!(value);
    }
}

fn cursor_body(page: CursorArgs, begin: &str, end: &str) -> (Value, u64) {
    let mut body = json!({"cursorDirection":page.direction,"size":page.size});
    if let Some(cursor) = page.cursor {
        body["cursorId"] = json!(cursor);
    }
    if !page.statuses.is_empty() {
        body["statusList"] = json!(page.statuses);
    }
    insert_optional(&mut body, begin, page.from);
    insert_optional(&mut body, end, page.to);
    (body, page.http.timeout)
}

fn call_request(
    name: &str,
    body: Value,
    query: Value,
    path: Value,
    timeout: u64,
    asynchronous: bool,
) -> Res<PreparedPlatform> {
    Ok(PreparedPlatform {
        operation: Operation::Call {
            request: Box::new(Request::new(name, body, query, path)?),
            timeout,
            asynchronous,
        },
    })
}

fn call(name: &str, body: Value, timeout: u64, asynchronous: bool) -> Res<PreparedPlatform> {
    call_request(name, body, json!({}), json!({}), timeout, asynchronous)
}

pub fn prepare(sub: OapiSub) -> Res<PreparedPlatform> {
    let operation = match sub {
        OapiSub::Api(ApiSub::List { search }) => {
            let search = search.unwrap_or_default().to_lowercase();
            let items: Vec<Value> = endpoints()
                .iter()
                .map(Endpoint::summary)
                .filter(|item| item.to_string().to_lowercase().contains(&search))
                .collect();
            Operation::Offline(json!({"endpoints":items}))
        }
        OapiSub::Api(ApiSub::Schema { endpoint: name }) => {
            let endpoint = endpoint(&name)?;
            Operation::Offline(
                json!({"endpoint":endpoint.summary(),"document":endpoint.document()}),
            )
        }
        OapiSub::Docs { id } => Operation::Offline(if let Some(id) = id {
            documents()
                .iter()
                .find(|doc| doc["id"] == id)
                .cloned()
                .ok_or_else(|| BackendError::new("BAD_ARGS", "未知平台文档ID"))?
        } else {
            json!({"documents":documents().iter().map(|d| json!({"id":d["id"],"title":d["title"],"url":d["url"]})).collect::<Vec<_>>()})
        }),
        OapiSub::Auth(auth) => {
            let AuthArgs {
                credentials,
                command,
            } = auth;
            match command {
                Some(AuthSub::Status) => Operation::Auth {
                    check: false,
                    timeout: 30,
                },
                Some(AuthSub::Check { http }) => Operation::Auth {
                    check: true,
                    timeout: http.timeout,
                },
                Some(AuthSub::Login {
                    ak,
                    sk,
                    base_url,
                    http,
                }) => Operation::Login {
                    key_id: ak,
                    key_secret: sk,
                    base_url,
                    timeout: http.timeout,
                },
                Some(AuthSub::Logout) => Operation::Logout,
                None => {
                    let [ak, sk]: &[String] = &credentials else {
                        return Err(BackendError::new(
                            "BAD_ARGS",
                            "oapi auth 需要 <AccessKeyId> <AccessKeySecret> 两个参数",
                        )
                        .with_hint("或使用 oapi auth status / check / login / logout 子命令"));
                    };
                    Operation::Login {
                        key_id: ak.clone(),
                        key_secret: sk.clone(),
                        base_url: None,
                        timeout: 30,
                    }
                }
            }
        }
        OapiSub::App(AppSub::List {
            search,
            owner,
            page,
            size,
            http,
        }) => {
            let mut body = json!({"page":page.to_string(),"size":size.to_string()});
            insert_optional(&mut body, "appName", search);
            insert_optional(&mut body, "ownerUserSearchKey", owner);
            return call("app.list", body, http.timeout, false);
        }
        OapiSub::App(AppSub::Detail { app_id, dev, http }) => {
            let (name, query) = if dev {
                ("app.version-detail", json!({"appId":app_id}))
            } else {
                ("app.online-detail", json!({"appId":app_id}))
            };
            return call_request(name, json!({}), query, json!({}), http.timeout, false);
        }
        OapiSub::App(AppSub::Flows { app_id, http }) => {
            return call_request(
                "app.flows",
                json!({}),
                json!({"appId":app_id}),
                json!({}),
                http.timeout,
                false,
            );
        }
        OapiSub::App(AppSub::Transfer {
            app_id,
            receive,
            http,
        }) => {
            return call(
                "app.transfer",
                json!({"appId":app_id,"receiveUserAccount":receive}),
                http.timeout,
                false,
            );
        }
        OapiSub::User(UserSub::List {
            search,
            phone,
            page,
            size,
            http,
        }) => {
            let mut query = json!({"page":page,"size":size});
            insert_optional(&mut query, "accountKeyword", search);
            insert_optional(&mut query, "phone", phone);
            return call_request(
                "user.list",
                json!({}),
                query,
                json!({}),
                http.timeout,
                false,
            );
        }
        OapiSub::User(UserSub::Create {
            name,
            account,
            phone,
            email,
            account_type,
            role,
            password,
            http,
        }) => {
            let mut body = json!({
                "name":name, "account":account, "phone":phone,
                "accountType":account_type, "userRole":role, "password":password,
            });
            insert_optional(&mut body, "email", email);
            return call("user.create", body, http.timeout, false);
        }
        OapiSub::User(UserSub::Modify {
            account,
            name,
            phone,
            email,
            role,
            http,
        }) => {
            let mut body = json!({"account":account});
            insert_optional(&mut body, "name", name);
            insert_optional(&mut body, "phone", phone);
            insert_optional(&mut body, "email", email);
            insert_optional(&mut body, "userRole", role);
            return call("user.modify", body, http.timeout, false);
        }
        OapiSub::User(UserSub::Delete {
            account,
            receive,
            http,
        }) => {
            return call(
                "user.delete",
                json!({"account":account,"receiveAccount":receive}),
                http.timeout,
                false,
            );
        }
        OapiSub::User(UserSub::ResetPassword {
            account,
            old,
            new,
            http,
        }) => {
            return call(
                "user.reset-password",
                json!({"loginAccount":account,"oldPwd":old,"pwd":new}),
                http.timeout,
                false,
            );
        }
        OapiSub::Queue(QueueSub::List {
            search,
            page,
            size,
            http,
        }) => {
            let mut query = json!({"page":page,"size":size});
            insert_optional(&mut query, "keyword", search);
            return call_request(
                "queue.list",
                json!({}),
                query,
                json!({}),
                http.timeout,
                false,
            );
        }
        OapiSub::Queue(QueueSub::Items {
            queue_uuid,
            status,
            page,
            size,
            http,
        }) => {
            let mut query = json!({"page":page,"size":size});
            insert_optional(&mut query, "status", status);
            return call_request(
                "queue.items",
                json!({}),
                query,
                json!({"queueUuid":queue_uuid}),
                http.timeout,
                false,
            );
        }
        OapiSub::Queue(QueueSub::Enqueue {
            queue_uuid,
            name,
            priority,
            biz_info,
            description,
            effective_time,
            expire_time,
            http,
        }) => {
            if !(1..=100).contains(&name.chars().count()) {
                return Err(BackendError::new("BAD_INPUT", "--name 长度必须在1~100之间"));
            }
            if !(1..=1000).contains(&biz_info.chars().count()) {
                return Err(BackendError::new(
                    "BAD_INPUT",
                    "--biz-info 长度必须在1~1000之间",
                ));
            }
            let mut body = json!({
                "name":name,
                "priority":priority.parse::<u64>().unwrap_or(100),
                "bizInfo":biz_info,
                "source":"OpenAPI",
            });
            insert_optional(&mut body, "description", description);
            if let Some(value) = effective_time {
                body["effectiveTime"] = json!(value);
            }
            if let Some(value) = expire_time {
                body["expireTime"] = json!(value);
            }
            return call_request(
                "queue.enqueue",
                body,
                json!({}),
                json!({"queueUuid":queue_uuid}),
                http.timeout,
                false,
            );
        }
        OapiSub::Queue(QueueSub::Dequeue { queue_uuid, http }) => {
            return call_request(
                "queue.dequeue",
                json!({}),
                json!({}),
                json!({"queueUuid":queue_uuid}),
                http.timeout,
                false,
            );
        }
        OapiSub::Queue(QueueSub::Modify {
            item_uuid,
            status,
            description,
            http,
        }) => {
            let mut body = json!({"status":status});
            insert_optional(&mut body, "description", description);
            return call_request(
                "queue.modify",
                body,
                json!({}),
                json!({"itemUuid":item_uuid}),
                http.timeout,
                false,
            );
        }
        OapiSub::Queue(QueueSub::Reenqueue { item_uuid, http }) => {
            return call_request(
                "queue.reenqueue",
                json!({}),
                json!({}),
                json!({"itemUuid":item_uuid}),
                http.timeout,
                false,
            );
        }
        OapiSub::Queue(QueueSub::Count { queue_uuid, http }) => {
            return call_request(
                "queue.count",
                json!({}),
                json!({}),
                json!({"queueUuid":queue_uuid}),
                http.timeout,
                false,
            );
        }
        OapiSub::Queue(QueueSub::Statuses { http }) => {
            return call("queue.statuses", json!({}), http.timeout, false);
        }
        OapiSub::Queue(QueueSub::BatchStatus {
            queue_uuid,
            from,
            to,
            http,
        }) => {
            return call_request(
                "queue.batch-status",
                json!({"sourceStatus":from,"targetStatus":to}),
                json!({}),
                json!({"queueUuid":queue_uuid}),
                http.timeout,
                false,
            );
        }
        OapiSub::App(AppSub::Versions {
            app_id,
            page,
            size,
            http,
        }) => {
            return call(
                "app.versions",
                json!({"appId":app_id,"pageDTO":{"page":page,"size":size}}),
                http.timeout,
                false,
            );
        }
        OapiSub::Robot(RobotSub::List {
            search,
            status,
            group,
            page,
            size,
            http,
        }) => {
            let mut body = json!({"page":page,"size":size});
            insert_optional(&mut body, "key", search);
            insert_optional(&mut body, "status", status);
            insert_optional(&mut body, "robotClientGroupUuid", group);
            return call("robot.list", body, http.timeout, false);
        }
        OapiSub::Robot(RobotSub::Groups {
            search,
            page,
            size,
            http,
        }) => {
            let mut body = json!({"page":page,"size":size});
            insert_optional(&mut body, "key", search);
            return call("robot.groups", body, http.timeout, false);
        }
        OapiSub::Robot(RobotSub::Detail { robot, uuid, http }) => {
            if robot.is_none() && uuid.is_none() {
                return Err(BackendError::new(
                    "BAD_INPUT",
                    "--robot 或 --uuid 必须提供一个",
                ));
            }
            let mut body = json!({});
            insert_optional(&mut body, "accountName", robot);
            insert_optional(&mut body, "robotClientUuid", uuid);
            return call("robot.detail", body, http.timeout, false);
        }
        OapiSub::Job(JobSub::Start {
            app,
            robot,
            group,
            scope,
            params,
            run_timeout,
            idempotent,
            callback,
            summary,
            http,
        }) => {
            if robot.is_none() && group.is_none() {
                return Err(BackendError::new(
                    "BAD_INPUT",
                    "--robot 或 --group 必须提供一个",
                ));
            }
            let mut body = json!({"robotUuid":app});
            insert_optional(&mut body, "accountName", robot);
            let grouped = group.is_some();
            insert_optional(&mut body, "robotClientGroupUuid", group);
            if grouped {
                body["executeScope"] = json!(scope);
            }
            if let Some(raw) = params {
                let value = core::json_arg(&raw)?;
                if !value.is_array() {
                    return Err(BackendError::new(
                        "BAD_INPUT",
                        "--params 必须是 JSON 数组,如 [{\"name\":\"x\",\"value\":\"1\",\"type\":\"str\"}]",
                    ));
                }
                body["params"] = value;
            }
            if let Some(seconds) = run_timeout {
                body["runTimeout"] = json!(seconds);
            }
            if let Some(value) = idempotent {
                if value.chars().count() > 36 {
                    return Err(BackendError::new(
                        "BAD_INPUT",
                        "--idempotent 不能超过36字符",
                    ));
                }
                body["idempotentUuid"] = json!(value);
            }
            insert_optional(&mut body, "callbackUrl", callback);
            if let Some(value) = summary {
                if value.chars().count() > 128 {
                    return Err(BackendError::new("BAD_INPUT", "--summary 不能超过128字符"));
                }
                body["summary"] = json!(value);
            }
            return call("job.start", body, http.timeout, false);
        }
        OapiSub::Job(JobSub::Stop { job_uuid, http }) => {
            return call("job.stop", json!({"jobUuid":job_uuid}), http.timeout, false)
        }
        OapiSub::Job(JobSub::Retry { job_uuid, http }) => {
            return call(
                "job.retry",
                json!({"jobUuid":job_uuid}),
                http.timeout,
                false,
            )
        }
        OapiSub::Api(ApiSub::Call {
            endpoint,
            body,
            query,
            path,
            file,
            http,
        }) => {
            if [&body, &query, &path]
                .iter()
                .filter(|v| v.as_deref() == Some("-"))
                .count()
                > 1
            {
                return Err(BackendError::new("BAD_INPUT", "仅一个参数可以读取 stdin"));
            }
            let mut request = Request::new(
                &endpoint,
                object(body.as_deref())?,
                object(query.as_deref())?,
                object(path.as_deref())?,
            )?;
            request.add_file(file)?;
            Operation::Call {
                request: Box::new(request),
                timeout: http.timeout,
                asynchronous: false,
            }
        }
        OapiSub::Job(JobSub::List {
            app,
            schedule,
            robot,
            page,
        }) => {
            let (mut body, timeout) = cursor_body(page, "triggerTimeBegin", "triggerTimeEnd");
            insert_optional(&mut body, "robotUuid", app);
            insert_optional(&mut body, "scheduleUuid", schedule);
            insert_optional(&mut body, "robotClientUuid", robot);
            return call("job.list", body, timeout, false);
        }
        OapiSub::Job(JobSub::Status { job_uuid, http }) => {
            return call(
                "job.status",
                json!({"jobUuid":job_uuid}),
                http.timeout,
                false,
            )
        }
        OapiSub::Job(JobSub::Logs {
            job_uuid,
            page,
            size,
            search,
            from,
            to,
            order,
            asynchronous,
            http,
        }) => {
            let mut filter = json!({"sort":{"sortKey":"time","sortOrder":order}});
            insert_optional(&mut filter, "beginTime", from);
            insert_optional(&mut filter, "endTime", to);
            insert_optional(&mut filter, "searchKey", search);
            return call(
                if asynchronous {
                    "log.notify"
                } else {
                    "log.search"
                },
                json!({"jobUuid":job_uuid,"page":page,"size":size,"queryFilter":filter}),
                http.timeout,
                asynchronous,
            );
        }
        OapiSub::Schedule(ScheduleSub::List {
            search,
            page,
            size,
            http,
        }) => {
            let mut body = json!({"page":page,"size":size});
            insert_optional(&mut body, "key", search);
            return call("schedule.list", body, http.timeout, false);
        }
        OapiSub::Schedule(ScheduleSub::Detail {
            schedule_uuid,
            http,
        }) => {
            return call(
                "schedule.detail",
                json!({"scheduleUuid":schedule_uuid}),
                http.timeout,
                false,
            )
        }
        OapiSub::Schedule(ScheduleSub::Runs {
            schedule_uuid,
            page,
        }) => {
            let (mut body, timeout) = cursor_body(page, "startTime", "endTime");
            body["sourceUuid"] = json!(schedule_uuid);
            return call("task.runs", body, timeout, false);
        }
        OapiSub::Task(TaskSub::Start {
            schedule_uuid,
            idempotent,
            params,
            http,
        }) => {
            let mut body = json!({"scheduleUuid":schedule_uuid});
            if let Some(value) = idempotent {
                if value.chars().count() > 36 {
                    return Err(BackendError::new(
                        "BAD_INPUT",
                        "--idempotent 不能超过36字符",
                    ));
                }
                body["idempotentUuid"] = json!(value);
            }
            if let Some(raw) = params {
                let value = core::json_arg(&raw)?;
                if !value.is_array() {
                    return Err(BackendError::new(
                        "BAD_INPUT",
                        "--params 必须是 JSON 数组,每项含 robotUuid 及对应的 params",
                    ));
                }
                body["scheduleRelaParams"] = value;
            }
            return call("task.start", body, http.timeout, false);
        }
        OapiSub::Task(TaskSub::Stop { task_uuid, http }) => {
            return call(
                "task.stop",
                json!({"taskUuid":task_uuid}),
                http.timeout,
                false,
            )
        }
        OapiSub::Task(TaskSub::Latest { page, size, http }) => {
            return call(
                "task.latest",
                json!({"page":page,"size":size}),
                http.timeout,
                false,
            )
        }
        OapiSub::Task(TaskSub::Status { task_uuid, http }) => {
            return call(
                "task.status",
                json!({"taskUuid":task_uuid}),
                http.timeout,
                false,
            )
        }
        OapiSub::Task(TaskSub::Jobs {
            task_uuid,
            robot,
            http,
        }) => {
            return call(
                "task.jobs",
                json!({"taskUuid":task_uuid,"robotClientUuid":robot}),
                http.timeout,
                false,
            )
        }
        OapiSub::App(AppSub::Records {
            app,
            from,
            to,
            user,
            mode,
            page,
            size,
            http,
        }) => {
            let mut body = json!({"minTime":from,"maxTime":to,"page":page,"size":size});
            insert_optional(&mut body, "appId", app);
            insert_optional(&mut body, "userId", user);
            if !mode.is_empty() {
                body["startModelList"] = json!(mode);
            }
            return call("app.records", body, http.timeout, false);
        }
    };
    Ok(PreparedPlatform { operation })
}

struct Client {
    base: Url,
    token: Option<String>,
    key_id: Option<String>,
    key_secret: Option<String>,
    stored_token: Option<(String, u64)>,
    source: &'static str,
    secrets: Secrets,
    expires_in: Option<u64>,
    deadline: Instant,
    agent: ureq::Agent,
}

impl Client {
    fn new(timeout: u64) -> Res<Self> {
        Self::with_credentials(timeout, Credentials::resolve())
    }

    fn with_credentials(timeout: u64, credentials: Credentials) -> Res<Self> {
        let raw_base = credentials
            .base_url
            .clone()
            .unwrap_or_else(|| BASE_URL.into());
        let base = Url::parse(&raw_base)
            .map_err(|_| BackendError::new("PLATFORM_CONFIG", "平台地址无效"))?;
        let loopback = matches!(base.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
        if (base.scheme() != "https" && !(base.scheme() == "http" && loopback))
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !matches!(base.path(), "" | "/")
        {
            return Err(BackendError::new(
                "PLATFORM_CONFIG",
                "平台地址必须是 HTTPS origin;本地测试可用 HTTP loopback",
            ));
        }
        let secrets = credentials.secrets();
        Ok(Self {
            base,
            token: credentials.token,
            key_id: credentials.key_id,
            key_secret: credentials.key_secret,
            stored_token: credentials.stored_token,
            source: credentials.source,
            secrets,
            expires_in: None,
            deadline: Instant::now() + Duration::from_secs(timeout),
            agent: ureq::AgentBuilder::new().redirects(0).build(),
        })
    }

    fn status(&self) -> Value {
        let credentials = Credentials::resolve();
        let file = auth_file();
        let cached = self
            .stored_token
            .as_ref()
            .map(|(_, expires_at)| *expires_at > now_epoch() + TOKEN_SKEW_SECS)
            .unwrap_or(false);
        json!({
            "base_url":self.base.as_str(),
            "access_token_configured":self.token.is_some(),
            "access_key_id_configured":self.key_id.is_some(),
            "access_key_secret_configured":self.key_secret.is_some(),
            "access_key_id":self.key_id.as_deref().map(mask),
            "credential_source":credentials.source,
            "token_cached":cached,
            "token_expires_at":self.stored_token.as_ref().map(|(_, e)| *e),
            "auth_file":file.as_ref().map(|p| p.display().to_string()),
            "auth_file_exists":file.is_some_and(|p| p.exists()),
            "verified":false,
            "configuration":"XBOT_OAPI_BASE_URL / XBOT_OAPI_ACCESS_TOKEN / XBOT_OAPI_ACCESS_KEY_ID / XBOT_OAPI_ACCESS_KEY_SECRET(兼容 XBOT_PLATFORM_*),或 xbot-cli oapi auth <ak> <sk>",
        })
    }

    fn remaining(&self) -> Res<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(BackendError::new("PLATFORM_TIMEOUT", "平台 API 查询超时"))
        } else {
            Ok(remaining)
        }
    }

    fn authenticate(&mut self) -> Res<()> {
        if self.token.is_some() {
            return Ok(());
        }
        // 本地配置保存的未过期 token 直接复用;过期后走密钥自动刷新。
        if self.source == "config_file" {
            if let Some((token, expires_at)) = &self.stored_token {
                if *expires_at > now_epoch() + TOKEN_SKEW_SECS {
                    self.token = Some(token.clone());
                    self.secrets.0.push(token.clone());
                    return Ok(());
                }
            }
        }
        let (Some(key_id), Some(key_secret)) = (&self.key_id, &self.key_secret) else {
            return Err(BackendError::new("PLATFORM_AUTH", "尚未配置平台 API 凭据")
                .with_hint("运行 xbot-cli oapi auth <AccessKeyId> <AccessKeySecret> 保存凭据;或设置 XBOT_OAPI_ACCESS_KEY_ID / XBOT_OAPI_ACCESS_KEY_SECRET(兼容 XBOT_PLATFORM_*)"));
        };
        let request = Request {
            endpoint: endpoint("auth.token")?,
            body: json!({}),
            query: json!({"accessKeyId":key_id,"accessKeySecret":key_secret}),
            path: json!({}),
            upload: None,
        };
        let response = self.exchange(&request)?;
        self.check_response(&request.endpoint, &response)?;
        let token = response
            .pointer("/data/accessToken")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                BackendError::new("PLATFORM_BAD_RESPONSE", "鉴权响应缺少 accessToken")
            })?;
        self.expires_in = response
            .pointer("/data/expiresIn")
            .and_then(Value::as_u64)
            .filter(|v| *v > 0);
        if self.expires_in.is_none() {
            return Err(BackendError::new(
                "PLATFORM_BAD_RESPONSE",
                "鉴权响应缺少有效 expiresIn",
            ));
        }
        self.token = Some(token.to_owned());
        self.secrets.0.push(token.to_owned());
        // 密钥登录的 token 持久化到本地配置,过期前直接复用;写失败不影响本次调用。
        if self.source == "config_file" {
            if let Some(mut stored) = read_stored_auth() {
                stored["access_token"] = json!(token);
                stored["token_expires_at"] = json!(now_epoch() + self.expires_in.unwrap_or(0));
                let _ = write_stored_auth(&stored);
            }
        }
        Ok(())
    }

    fn request_url(&self, request: &Request) -> Res<Url> {
        let mut url = self.base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| BackendError::new("PLATFORM_CONFIG", "平台地址不能附加路径"))?;
            segments.clear();
            for segment in request.endpoint.path.split('/').filter(|s| !s.is_empty()) {
                if let Some(key) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    let value = request.path[key].as_str().ok_or_else(|| {
                        BackendError::new("BAD_INPUT", format!("缺少路径参数: {key}"))
                    })?;
                    segments.push(value);
                } else {
                    segments.push(segment);
                }
            }
        }
        for (key, value) in request.query.as_object().unwrap() {
            let value = match value {
                Value::Null => continue,
                Value::String(value) => value.clone(),
                Value::Bool(_) | Value::Number(_) => value.to_string(),
                _ => {
                    return Err(BackendError::new(
                        "BAD_INPUT",
                        "URL 查询参数值必须是字符串、数字或布尔值",
                    ))
                }
            };
            url.query_pairs_mut().append_pair(key, &value);
        }
        Ok(url)
    }

    fn exchange(&self, request: &Request) -> Res<Value> {
        let url = self.request_url(request)?;
        let mut http = self
            .agent
            .request(&request.endpoint.method, url.as_str())
            .timeout(self.remaining()?)
            .set("Accept", "application/json");
        if request.endpoint.name != "auth.token" {
            let token = self
                .token
                .as_deref()
                .ok_or_else(|| BackendError::new("PLATFORM_AUTH", "尚未获取平台 token"))?;
            http = http.set("Authorization", &format!("Bearer {token}"));
        }
        let response = if let Some((name, bytes)) = &request.upload {
            let boundary = format!(
                "xbot-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let mut multipart = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
            multipart.extend_from_slice(bytes);
            multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            http.set(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send_bytes(&multipart)
        } else if request.endpoint.method == "GET" {
            http.call()
        } else {
            http.send_json(request.body.clone())
        };
        let response = match response {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(error)) => {
                // ureq error Display includes the full URL; auth URLs contain secrets.
                return Err(BackendError::new(
                    "PLATFORM_NETWORK",
                    format!("平台 API 网络请求失败 ({:?})", error.kind()),
                )
                .with_data(json!({"endpoint":request.endpoint.name,"outcome":"unknown"}))
                .with_retryable(!request.endpoint.mutating));
            }
        };
        let status = response.status();
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                BackendError::new("PLATFORM_NETWORK", "读取平台响应失败")
                    .with_data(json!({"endpoint":request.endpoint.name,"outcome":"unknown"}))
            })?;
        if bytes.len() as u64 > MAX_RESPONSE {
            return Err(BackendError::new(
                "PLATFORM_BAD_RESPONSE",
                "平台响应超过20MB限制",
            ));
        }
        let parsed = serde_json::from_slice::<Value>(&bytes);
        if !(200..300).contains(&status) {
            return Err(self.api_error(
                &request.endpoint,
                status as i64,
                parsed.ok(),
                Some(status),
            ));
        }
        parsed.map_err(|_| {
            BackendError::new("PLATFORM_BAD_RESPONSE", "平台返回非 JSON 响应")
                .with_data(json!({"endpoint":request.endpoint.name,"http_status":status}))
        })
    }

    fn api_error(
        &self,
        endpoint: &Endpoint,
        api_code: i64,
        response: Option<Value>,
        http_status: Option<u16>,
    ) -> BackendError {
        let (code, hint) = match api_code {
            401 | 403 => (
                "PLATFORM_AUTH",
                "检查平台 API 密钥、token、企业权限与平台地址",
            ),
            400 => ("BAD_INPUT", "使用 oapi api schema 查看此接口参数"),
            429 => ("PLATFORM_RATE_LIMIT", "已触发平台限流,稍后重试"),
            80204002 => (
                "PLATFORM_LOG_PENDING",
                "日志正在上传;稍后用 log.query 和同一 requestId 查询",
            ),
            80204001 => (
                "PLATFORM_API",
                "检查目标机器人是否在线、是否已删除以及日志可用性",
            ),
            80204004 => ("PLATFORM_TIMEOUT", "日志查询超时;检查机器人连接状态"),
            _ => (
                "PLATFORM_API",
                "保留 api_code 和 requestId 排查;写操作失败后先确认执行结果",
            ),
        };
        let mut data = json!({"endpoint":endpoint.name,"api_code":api_code,"response":response,"http_status":http_status});
        let message = data
            .pointer("/response/msg")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("平台 API 请求失败")
            .to_owned();
        let mut message = json!(message);
        self.secrets.scrub(&mut message);
        self.secrets.scrub(&mut data);
        BackendError::new(code, message.as_str().unwrap())
            .with_hint(hint)
            .with_data(data)
            .with_retryable(!endpoint.mutating && matches!(api_code, 429 | 80204002))
    }

    fn check_response(&self, endpoint: &Endpoint, response: &Value) -> Res<()> {
        let code = response
            .get("code")
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()));
        if response.get("success").and_then(Value::as_bool) == Some(true) && code == Some(200) {
            return Ok(());
        }
        // Log documentation also shows the bare data object as its response.
        if matches!(endpoint.name, "log.search" | "log.query")
            && response.get("success").is_none()
            && code.is_none()
            && response["requestId"].is_string()
            && response["page"].is_object()
            && (response["logs"].is_array() || response.get("logs") == Some(&Value::Null))
        {
            return Ok(());
        }
        if let Some(code) = code {
            return Err(self.api_error(endpoint, code, Some(response.clone()), None));
        }
        let mut response = response.clone();
        self.secrets.scrub(&mut response);
        Err(
            BackendError::new("PLATFORM_BAD_RESPONSE", "平台响应缺少有效 code/success")
                .with_data(json!({"endpoint":endpoint.name,"response":response})),
        )
    }

    fn call(&self, request: &Request) -> Res<Value> {
        let mut response = self.exchange(request)?;
        self.check_response(&request.endpoint, &response)?;
        self.secrets.scrub(&mut response);
        Ok(json!({"endpoint":request.endpoint.name,"response":response}))
    }

    fn logs(&self, request: Request) -> Res<Value> {
        let notify = self.exchange(&request)?;
        self.check_response(&request.endpoint, &notify)?;
        let id = notify["data"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                BackendError::new("PLATFORM_BAD_RESPONSE", "日志通知响应缺少 requestId")
            })?;
        let query = Request::new("log.query", json!({}), json!({"requestId":id}), json!({}))?;
        let polling = || -> Res<Value> {
            loop {
                let response = self.exchange(&query)?;
                if response["code"]
                    .as_i64()
                    .or_else(|| response["code"].as_str()?.parse().ok())
                    != Some(80204002)
                {
                    self.check_response(&query.endpoint, &response)?;
                    let mut response = response;
                    self.secrets.scrub(&mut response);
                    return Ok(
                        json!({"endpoint":"log.query","job_uuid":request.body["jobUuid"],"request_id":id,"response":response}),
                    );
                }
                std::thread::sleep(Duration::from_millis(250).min(self.remaining()?));
            }
        };
        polling().map_err(|mut error| {
            let mut data = error.data.take().unwrap_or_else(|| json!({}));
            data["job_uuid"] = request.body["jobUuid"].clone();
            data["request_id"] = json!(id);
            self.secrets.scrub(&mut data);
            error.data = Some(data);
            error
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_covers_all_supplied_documents_and_has_unique_endpoints() {
        assert_eq!(documents().len(), 68);
        let endpoints = endpoints();
        assert_eq!(endpoints.len(), 44);
        let names: std::collections::HashSet<_> = endpoints.iter().map(|e| e.name).collect();
        assert_eq!(names.len(), endpoints.len());
        for endpoint in endpoints {
            assert!(endpoint.path.starts_with("/oapi/"));
            assert!(matches!(endpoint.method.as_str(), "GET" | "POST" | "PATCH"));
        }
    }

    #[test]
    fn records_validate_exact_seven_day_boundary_and_time_order() {
        for (to, valid) in [
            ("2026-10-23 00:00:00", true),
            ("2026-10-23 00:00:01", false),
            ("2026-10-15 23:59:59", false),
        ] {
            assert_eq!(
                Request::new(
                    "app.records",
                    json!({"minTime":"2026-10-16 00:00:00","maxTime":to}),
                    json!({}),
                    json!({})
                )
                .is_ok(),
                valid
            );
        }
    }

    #[test]
    fn required_fields_and_path_parameters_are_validated_offline() {
        assert!(Request::new("job.status", json!({}), json!({}), json!({})).is_err());
        assert!(Request::new("queue.items", json!({}), json!({}), json!({})).is_err());
        assert!(Request::new(
            "queue.items",
            json!({}),
            json!({}),
            json!({"queueUuid":"a/b ?&"})
        )
        .is_ok());
        assert!(Request::new(
            "queue.items",
            json!({}),
            json!({}),
            json!({"queueUuid":".."})
        )
        .is_err());
    }

    #[test]
    fn secrets_are_removed_from_nested_fields_and_echoed_messages() {
        let secrets = Secrets(vec!["private-value".into(), "id@x&".into()]);
        let mut value = json!({"msg":"echo private-value","data":[{"password":"password","text":"private-value"}]});
        value["url"] = json!("https://example.test/?key=id%40x%26");
        value["oldPwd"] = json!("previous-password");
        value["newPassword"] = json!("replacement-password");
        secrets.scrub(&mut value);
        assert!(!value.to_string().contains("private-value"));
        assert!(!value.to_string().contains("id%40x%26"));
        assert_eq!(value["data"][0]["password"], "[REDACTED]");
        assert_eq!(value["oldPwd"], "[REDACTED]");
        assert_eq!(value["newPassword"], "[REDACTED]");
    }
}
